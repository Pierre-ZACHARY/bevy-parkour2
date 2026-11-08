//! Start an authenticated sccache daemon; credentials stay in its memory.
//! Cargo, rustc, Jackdaw and IDE launches need neither dotenv nor per-unit API calls.
use std::{
    collections::BTreeMap,
    env, fs,
    hash::{Hash, Hasher},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CONFIG: &str = include_str!("infisical-config.json");
fn setting(key: &str) -> Option<&'static str> {
    CONFIG
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("\"{key}\"")))?
        .split_once(':')
        .map(|(_, value)| value.trim().trim_end_matches(',').trim_matches('"'))
}

pub fn daemon(root: &Path, config: &BTreeMap<String, String>) -> Option<u16> {
    if env::var_os("PROJECT_CACHE_LOCAL_ONLY").is_some() {
        return None;
    }
    let directory = root.join(".compiler-cache/infisical");
    fs::create_dir_all(&directory).ok()?;
    let failure = directory.join("unavailable");
    if fs::metadata(&failure)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(60))
    {
        return None;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hash);
    config.hash(&mut hash);
    CONFIG.hash(&mut hash);
    // Machine identities in the same worktree use separate daemon profiles.
    // Only the digest is persisted, never the authentication token.
    env::var_os("INFISICAL_TOKEN").hash(&mut hash);
    env::var_os("PROJECT_CACHE_ACCESS").hash(&mut hash);
    // New daemon each hour bounds credential retention; idle daemons exit after
    // ten minutes. Never stop a daemon compiling another crate.
    (SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() / 3600).hash(&mut hash);
    let compiler = Command::new("rustup")
        .args(["which", "rustc"])
        .output()
        .ok()?;
    if !compiler.status.success() {
        return None;
    }
    String::from_utf8_lossy(&compiler.stdout)
        .trim()
        .hash(&mut hash);
    let identity = hash.finish();
    let port = super::daemon_port(root, identity).ok()?;
    let listening = || {
        TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(100),
        )
        .is_ok()
    };
    if listening() {
        return Some(port);
    }
    let lock = directory.join(format!("{identity:016x}.lock"));
    struct Guard(PathBuf);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let started = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => break,
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists
                    && started.elapsed() < Duration::from_secs(60) =>
            {
                std::thread::sleep(Duration::from_millis(50));
                if listening() {
                    return Some(port);
                }
            }
            Err(_) => return None,
        }
    }
    let _guard = Guard(lock);
    if listening() {
        return Some(port);
    }
    let real = root
        .join(".compiler-cache/real")
        .join(super::exe("sccache"));
    let read = |project: &str, path: &str| -> Option<Vec<String>> {
        let output = Command::new("infisical")
            .args([
                "sm",
                "secrets",
                "get",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "--plain",
                "--silent",
                "--domain",
                setting("domain")?,
                "--projectId",
                project,
                "--env",
                setting("environment")?,
                "--path",
                path,
                "--include-imports=false",
                "--secret-overriding=false",
                "--expand=false",
            ])
            .env("INFISICAL_DISABLE_TELEMETRY", "1")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8(output.stdout).ok()?;
        let values: Vec<_> = text.lines().map(str::to_owned).collect();
        (values.len() == 2
            && values
                .iter()
                .all(|v| !v.is_empty() && !v.chars().any(char::is_control)))
        .then_some(values)
    };
    // Infrastructure members populate the shared cache; collaborators use its
    // read-only credentials. Each lookup reads two keys, never admin tokens.
    // A writer whose daemon cannot start (e.g. revoked S3 rights) still gets
    // the read-only cache instead of silently falling back to local disk.
    let maintainer = setting("maintainerCacheFallback") == Some("true")
        && env::var("PROJECT_CACHE_ACCESS").ok().as_deref() != Some("read-only");
    let attempts: [(&str, &str, &str); 2] = [
        (setting("infraProjectId")?, "/", "READ_WRITE"),
        (
            setting("buildProjectId")?,
            setting("cachePath")?,
            "READ_ONLY",
        ),
    ];
    for (project, path, mode) in attempts.into_iter().skip(usize::from(!maintainer)) {
        if let Some(values) = read(project, path)
            && start(root, &real, config, &values, mode, port)
            && listening()
        {
            let _ = fs::remove_file(&failure);
            return Some(port);
        }
    }
    let _ = fs::write(
        failure,
        "Infisical unavailable; retry after 60 seconds or remove this file.\n",
    );
    None
}

fn start(
    root: &Path,
    real: &Path,
    config: &BTreeMap<String, String>,
    values: &[String],
    mode: &str,
    port: u16,
) -> bool {
    let result = Command::new(real)
        .arg("--start-server")
        .envs(config)
        .env("AWS_ACCESS_KEY_ID", &values[0])
        .env("AWS_SECRET_ACCESS_KEY", &values[1])
        .env("SCCACHE_S3_RW_MODE", mode)
        .env("SCCACHE_SERVER_PORT", port.to_string())
        .env("SCCACHE_IDLE_TIMEOUT", "600")
        .env("SCCACHE_CONF", root.join(".compiler-cache/config.toml"))
        .env_remove("OVH_APPLICATION_KEY")
        .env_remove("OVH_APPLICATION_SECRET")
        .env_remove("OVH_CONSUMER_KEY")
        .env_remove("SENTRY_AUTH_TOKEN")
        .env_remove("SENTRY_RELEASE_TOKEN")
        .env_remove("SONAR_TOKEN")
        .env_remove("PULUMI_ACCESS_TOKEN")
        .env_remove("INFISICAL_TOKEN")
        .output();
    // Never echo the secret manager's output/errors.
    result.is_ok_and(|o| o.status.success())
}
