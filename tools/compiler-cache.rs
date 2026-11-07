//! Dependency-free project compiler/cache frontends. Built directly with rustc
//! so a fresh clone can bootstrap before Cargo's configured compiler exists.
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    env,
    ffi::OsString,
    fs,
    hash::{Hash, Hasher},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{Duration, Instant},
};

const VERSION: &str = "0.19.0";
#[path = "cache-config.rs"]
mod cache_config;
#[path = "cache-infisical.rs"]
mod cache_infisical;
const PINS: &str = include_str!("compiler-cache-pins.txt");
const KEYS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "AWS_SHARED_CREDENTIALS_FILE",
    "AWS_CONFIG_FILE",
    "AWS_EC2_METADATA_DISABLED",
    "SCCACHE_BUCKET",
    "SCCACHE_REGION",
    "SCCACHE_ENDPOINT",
    "SCCACHE_S3_USE_SSL",
    "SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE",
    "SCCACHE_S3_KEY_PREFIX",
    "SCCACHE_S3_RW_MODE",
    "SCCACHE_SERVER_PORT",
    "SCCACHE_BASEDIRS",
];
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("project compiler cache: {error}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> Result<PathBuf> {
    env::current_exe()?
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == ".compiler-cache"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            "frontend must be installed inside the project's .compiler-cache directory".into()
        })
}

fn exe(name: &str) -> String {
    format!("{name}{}", env::consts::EXE_SUFFIX)
}

fn active_compiler() -> Result<String> {
    if let (Some(requested), Some(version), Some(target), Some(rustc)) = (
        env::var_os("RUSTUP_TOOLCHAIN"),
        env::var_os("PARKOUR_NIX_EDITOR_TOOLCHAIN"),
        env::var_os("PARKOUR_NIX_EDITOR_TARGET"),
        env::var_os("PARKOUR_NIX_EDITOR_RUSTC"),
    ) {
        let qualified = format!("{}-{}", version.to_string_lossy(), target.to_string_lossy());
        if requested == version || requested == OsString::from(qualified) {
            return rustc
                .into_string()
                .map_err(|_| "non-UTF8 Nix editor rustc path".into());
        }
    }
    if let Some(rustc) =
        env::var_os("PARKOUR_NIX_RUSTC").filter(|_| env::var_os("RUSTUP_TOOLCHAIN").is_none())
    {
        return rustc
            .into_string()
            .map_err(|_| "non-UTF8 Nix rustc path".into());
    }
    let actual = Command::new("rustup").args(["which", "rustc"]).output()?;
    if !actual.status.success() {
        return Err("cannot resolve active rustc through rustup".into());
    }
    Ok(String::from_utf8(actual.stdout)?.trim().to_owned())
}

fn checked(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!(
            "{} failed with {status}",
            command.get_program().to_string_lossy()
        )
        .into());
    }
    Ok(())
}

fn install_executable(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        // A running facade (notably `sccache --run dx serve`) keeps its inode
        // executable. Replace the directory entry instead of overwriting it.
        let temporary = destination.with_extension(format!("{}.install", std::process::id()));
        fs::copy(source, &temporary)?;
        if let Err(error) = fs::rename(&temporary, destination) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
    }
    #[cfg(not(unix))]
    fs::copy(source, destination)?;
    Ok(())
}

fn host() -> Result<String> {
    let output = Command::new("rustc").arg("-vV").output()?;
    if !output.status.success() {
        return Err("rustc -vV failed".into());
    }
    String::from_utf8(output.stdout)?
        .lines()
        .find_map(|l| {
            l.strip_prefix("host: ")
                .map(|s| s.replace("unknown-linux-gnu", "unknown-linux-musl"))
        })
        .ok_or_else(|| "rustc did not report a host".into())
}

fn sha256(path: &Path) -> Result<String> {
    let output = if cfg!(windows) {
        Command::new("certutil.exe")
            .args(["-hashfile"])
            .arg(path)
            .arg("SHA256")
            .output()?
    } else if cfg!(target_os = "macos") {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()?
    } else {
        Command::new("sha256sum").arg(path).output()?
    };
    if !output.status.success() {
        return Err("archive SHA-256 verification command failed".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| "SHA-256 was absent from checksum output".into())
}

fn setup(root: &Path) -> Result<()> {
    let home = root.join(".compiler-cache");
    let bin = home.join("bin");
    fs::create_dir_all(&bin)?;
    let host = host()?;
    let hash = PINS
        .lines()
        .filter_map(|l| l.split_once(' '))
        .find_map(|(h, sum)| (h == host).then_some(sum.trim()))
        .ok_or_else(|| format!("no pinned sccache release for {host}"))?;
    // Keep the upstream basename: sccache treats other executable names as
    // compiler-masquerade mode. A renamed binary can recursively invoke itself.
    let real_dir = home.join("real");
    fs::create_dir_all(&real_dir)?;
    let real = real_dir.join(exe("sccache"));
    let receipt = home.join("release.sha256");
    if let Some(nix_sccache) = env::var_os("PARKOUR_NIX_SCCACHE") {
        let nix_receipt = format!("nix:{}", Path::new(&nix_sccache).display());
        if !real.is_file() || fs::read_to_string(&receipt).unwrap_or_default() != nix_receipt {
            install_executable(Path::new(&nix_sccache), &real)?;
            fs::write(&receipt, nix_receipt)?;
        }
    } else if !real.is_file() || fs::read_to_string(&receipt).unwrap_or_default() != hash {
        let archive = home.join("sccache.tar.gz");
        let url = format!(
            "https://github.com/mozilla/sccache/releases/download/v{VERSION}/sccache-v{VERSION}-{host}.tar.gz"
        );
        eprintln!("Installing verified sccache {VERSION} for {host}...");
        checked(
            Command::new("curl")
                .args([
                    "--fail",
                    "--location",
                    "--silent",
                    "--show-error",
                    "--retry",
                    "2",
                    "--connect-timeout",
                    "20",
                    "--max-time",
                    "300",
                ])
                .arg(url)
                .arg("--output")
                .arg(&archive),
        )?;
        if sha256(&archive)? != hash {
            return Err("sccache archive checksum mismatch".into());
        }
        let member = format!("sccache-v{VERSION}-{host}/{}", exe("sccache"));
        checked(
            Command::new("tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&home)
                .arg(&member),
        )?;
        install_executable(&home.join(member), &real)?;
        fs::write(&receipt, hash)?;
    }
    let current = env::current_exe()?;
    for name in ["sccache", "rustc-cache", "rustc"] {
        let destination = bin.join(exe(name));
        if !destination.is_file() || fs::read(&current)? != fs::read(&destination)? {
            install_executable(&current, &destination)?;
        }
    }
    fs::write(
        home.join("config.toml"),
        "# Project cache settings are supplied by the frontend environment.\n",
    )?;
    println!(
        "Project compiler cache ready (sccache {VERSION}). Remote access uses your Infisical login."
    );
    Ok(())
}

fn daemon_port(root: &Path, identity: u64) -> Result<u16> {
    let directory = root.join(".compiler-cache/ports");
    fs::create_dir_all(&directory)?;
    let file = directory.join(format!("{identity:016x}.port"));
    let read = || -> Option<u16> {
        let port = fs::read_to_string(&file).ok()?.trim().parse::<u16>().ok()?;
        match TcpListener::bind(("127.0.0.1", port)) {
            // An existing daemon may already own this profile's port.
            Ok(_) => Some(port),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Some(port),
            Err(_) => None,
        }
    };
    let lock = file.with_extension("lock");
    struct Guard(PathBuf);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let started = Instant::now();
    loop {
        if let Some(port) = read() {
            return Ok(port);
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => break,
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists
                    && started.elapsed() < Duration::from_secs(30) =>
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(e.into()),
        }
    }
    let _guard = Guard(lock);
    if let Some(port) = read() {
        return Ok(port);
    }
    for offset in 0..40000 {
        let port = 20000 + (identity.wrapping_add(offset) % 40000) as u16;
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) {
            drop(listener);
            let temporary = file.with_extension(format!("{}.tmp", std::process::id()));
            fs::write(&temporary, port.to_string())?;
            if file.exists() {
                fs::remove_file(&file)?;
            }
            fs::rename(temporary, &file)?;
            return Ok(port);
        }
    }
    Err("no available local port for the project sccache daemon".into())
}

fn configuration(root: &Path) -> Result<BTreeMap<String, OsString>> {
    let mut config = cache_config::load(root)?;
    for key in KEYS {
        if let Ok(value) = env::var(key)
            && !value.is_empty()
        {
            config.insert((*key).into(), value);
        }
    }
    if !config.contains_key("SCCACHE_BASEDIRS") {
        let mut bases = vec![root.to_path_buf()];
        if let Some(home) = env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
            env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|p| PathBuf::from(p).join(".cargo"))
        }) {
            bases.push(home);
        }
        config.insert(
            "SCCACHE_BASEDIRS".into(),
            env::join_paths(bases)?.to_string_lossy().into_owned(),
        );
    }
    let has = |key: &str| config.get(key).is_some_and(|s| !s.is_empty());
    if has("AWS_ACCESS_KEY_ID") != has("AWS_SECRET_ACCESS_KEY") {
        return Err(
            "incomplete S3 credentials: configure both AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY"
                .into(),
        );
    }
    let mut remote = has("AWS_ACCESS_KEY_ID") || has("AWS_PROFILE");
    if !remote {
        config.insert("SCCACHE_S3_RW_MODE".into(), "READ_ONLY".into());
        if let Some(port) = cache_infisical::daemon(root, &config) {
            config.insert("SCCACHE_SERVER_PORT".into(), port.to_string());
            remote = true;
        }
    }
    if remote
        && !config
            .get("SCCACHE_ENDPOINT")
            .is_some_and(|s| s.starts_with("https://"))
    {
        return Err("S3 endpoint must use HTTPS".into());
    }
    if !remote {
        config.retain(|key, _| {
            !key.starts_with("SCCACHE_S3_")
                && !["SCCACHE_BUCKET", "SCCACHE_REGION", "SCCACHE_ENDPOINT"].contains(&key.as_str())
        });
        if fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(".compiler-cache/local-only-warning"))
            .is_ok()
        {
            eprintln!(
                "project compiler cache: using local sccache; install Infisical CLI and run infisical login --domain https://eu.infisical.com for the shared cache."
            );
        }
    }
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    // A different endpoint, credential or READ_ONLY policy must not reuse a
    // daemon configured for another profile. Only this digest is stored locally.
    config.hash(&mut hasher);
    // Cargo's rustup proxy sets RUSTUP_TOOLCHAIN, while a direct stats command
    // may not. Resolve the compiler so both reach the same project daemon.
    active_compiler()?.hash(&mut hasher);
    if !config.contains_key("SCCACHE_SERVER_PORT") {
        config.insert(
            "SCCACHE_SERVER_PORT".into(),
            daemon_port(root, hasher.finish())?.to_string(),
        );
    }
    let mut config: BTreeMap<String, OsString> =
        config.into_iter().map(|(k, v)| (k, v.into())).collect();
    config.insert(
        "SCCACHE_CONF".into(),
        root.join(".compiler-cache/config.toml").into(),
    );
    if !remote {
        config.insert(
            "SCCACHE_DIR".into(),
            root.join(".compiler-cache/objects").into(),
        );
    }
    if !config.contains_key("SCCACHE_BASEDIRS") {
        let mut bases = vec![root.to_path_buf()];
        if let Some(home) = env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
            env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|p| PathBuf::from(p).join(".cargo"))
        }) {
            bases.push(home);
        }
        config.insert("SCCACHE_BASEDIRS".into(), env::join_paths(bases)?);
    }
    Ok(config)
}

fn run() -> Result<i32> {
    let root = root()?;
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|s| s == "--setup") {
        setup(&root)?;
        return Ok(0);
    }
    let bin = root.join(".compiler-cache/bin");
    if args.first().is_some_and(|s| s == "--run") {
        let program = args.get(1).ok_or("--run requires a command")?;
        let mut paths = vec![bin.clone()];
        if let Some(path) = env::var_os("PATH") {
            paths.extend(env::split_paths(&path));
        }
        // Dioxus 0.7.2 invokes `rustc` by PATH after processing its wrapper
        // arguments, so its launcher also needs the compiler facade on PATH.
        return Ok(Command::new(program)
            .args(&args[2..])
            .env("PATH", env::join_paths(paths)?)
            .env("CARGO_INCREMENTAL", "0")
            .status()?
            .code()
            .unwrap_or(1));
    }
    let actual_sccache = root.join(".compiler-cache/real").join(exe("sccache"));
    if !actual_sccache.is_file() {
        return Err("run pwsh -File tools/setup_cache.ps1 once before building".into());
    }
    // argv[0] keeps the installed name if another shell atomically replaces
    // this inode; /proc/self/exe may then end in " (deleted)" on Linux.
    let compiler_mode = env::args_os()
        .next()
        .as_deref()
        .and_then(|name| Path::new(name).file_stem())
        .is_some_and(|s| s == "rustc-cache" || s == "rustc");
    // The outer frontend also has the standard sccache name so cc-rs can use
    // it for C/C++. Avoid double caching when Cargo selects our compiler facade.
    if !compiler_mode
        && args.first().is_some_and(|p| {
            let path = Path::new(p);
            path.file_stem()
                .is_some_and(|s| s == "rustc-cache" || s == "rustc")
                && path.parent().and_then(|p| fs::canonicalize(p).ok())
                    == fs::canonicalize(&bin).ok()
        })
    {
        return Ok(Command::new(&args[0])
            .args(&args[1..])
            .status()?
            .code()
            .unwrap_or(1));
    }
    let mut command = Command::new(actual_sccache);
    if compiler_mode {
        // Honor cargo +toolchain / Jackdaw's SDK toolchain, never hardcode the
        // game's compiler for extensions built with the editor's pinned rustc.
        let compiler = active_compiler()?;
        let compiler = compiler.trim();
        // sccache 0.19 expands Rust response files into a Windows command
        // line. Large web-sys feature/check-cfg lists then fail with OS 206,
        // including metadata-only checks that cannot be cached anyway.
        // Preserve the build by using rustc's own response-file support for
        // these unsupported invocations; other units still use sccache.
        if cfg!(windows) && long_rust_arguments(&args) {
            eprintln!(
                "project compiler cache: long Windows rustc command uses a response file (sccache limit)"
            );
            return rustc_response_command(&root, compiler, &args);
        }
        command.arg(compiler);
    }
    // Clear the inherited S3 selectors first: a missing credential must select
    // the project-local backend even if a parent had configured another bucket.
    for key in KEYS.iter().filter(|k| {
        k.starts_with("SCCACHE_S3_")
            || ["SCCACHE_BUCKET", "SCCACHE_REGION", "SCCACHE_ENDPOINT"].contains(k)
    }) {
        command.env_remove(key);
    }
    command.args(&args).envs(configuration(&root)?);
    if env::var_os("SCCACHE_RECACHE").is_some_and(|s| s.is_empty()) {
        command.env_remove("SCCACHE_RECACHE");
    }
    strip_management(&mut command);
    Ok(command.status()?.code().unwrap_or(1))
}

fn strip_management(command: &mut Command) {
    for key in [
        "OVH_APPLICATION_KEY",
        "OVH_APPLICATION_SECRET",
        "OVH_CONSUMER_KEY",
        "SENTRY_AUTH_TOKEN",
        "SENTRY_RELEASE_TOKEN",
        "SONAR_TOKEN",
        "PULUMI_ACCESS_TOKEN",
        "INFISICAL_TOKEN",
        "INFISICAL_UNIVERSAL_AUTH_CLIENT_SECRET",
    ] {
        command.env_remove(key);
    }
}

fn long_rust_arguments(args: &[OsString]) -> bool {
    args.iter()
        .map(|arg| {
            let text = arg.to_string_lossy();
            let command_cost = text.encode_utf16().count() + text.matches('"').count() + 3;
            let response_cost = text
                .strip_prefix('@')
                .and_then(|p| fs::read_to_string(p).ok())
                .map_or(0, |s| s.encode_utf16().count());
            command_cost.max(response_cost)
        })
        .sum::<usize>()
        > 24000
}

fn rustc_response_command(root: &Path, compiler: &str, args: &[OsString]) -> Result<i32> {
    // Expand existing top-level Cargo response files once, preserving order.
    // rustc does not expand @file directives nested in response-file contents.
    let mut command = Command::new(compiler);
    strip_management(&mut command);
    let text = rustc_response_text(args.iter())?;
    let directory = root.join(".compiler-cache/responses");
    fs::create_dir_all(&directory)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let file = directory.join(format!("{}-{nonce}.rsp", std::process::id()));
    struct ResponseFile(PathBuf);
    impl Drop for ResponseFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fs::write(&file, text)?;
    let _guard = ResponseFile(file.clone());
    command.arg(format!("@{}", file.display()));
    Ok(command.status()?.code().unwrap_or(1))
}

fn rustc_response_text<'a>(args: impl Iterator<Item = &'a OsString>) -> Result<String> {
    let mut text = String::new();
    for arg in args {
        let value = arg.to_str().ok_or("non-UTF8 Rust response argument")?;
        if let Some(path) = value.strip_prefix('@') {
            for line in fs::read_to_string(path)?
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                text.push_str(line);
                text.push('\n');
            }
            continue;
        }
        if value.is_empty() || value.trim() != value || value.contains(['\r', '\n']) {
            return Err("argument cannot be represented losslessly in a Rust response file".into());
        }
        // Rust response files split on lines, without shell quoting.
        text.push_str(value);
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rust_response_arguments_preserve_spaces_and_literal_quotes() {
        let args = [
            OsString::from(r#"C:\directory with spaces\source.rs"#),
            OsString::from(r#"cfg(feature,values("one","two"))"#),
        ];
        let text = rustc_response_text(args.iter()).unwrap();
        assert_eq!(text.lines().map(OsString::from).collect::<Vec<_>>(), args);
        assert!(rustc_response_text([OsString::from("line\nbreak")].iter()).is_err());
        assert!(long_rust_arguments(&[OsString::from("x".repeat(25000))]));
    }
    #[test]
    fn existing_response_files_preserve_argument_order_and_do_not_recurse() {
        let path =
            env::temp_dir().join(format!("parkour-rust-response-{}.rsp", std::process::id()));
        fs::write(&path, "--cfg\n first \n@literal-nested\n").unwrap();
        let args = [
            OsString::from("before"),
            OsString::from(format!("@{}", path.display())),
            OsString::from("after"),
        ];
        let result = rustc_response_text(args.iter()).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(result, "before\n--cfg\nfirst\n@literal-nested\nafter\n");
    }
    #[test]
    fn releases_have_valid_hashes() {
        let pins: Vec<_> = PINS
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .collect();
        assert_eq!(pins.len(), 5);
        for pin in pins {
            let (_, hash) = pin.split_once(' ').unwrap();
            assert_eq!(hash.len(), 64);
            assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
