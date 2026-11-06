use crate::{cache, model, process};
#[path = "../../cache-config.rs"]
mod cache_config;
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Subcommand)]
pub enum Action {
    /// Forward a command after --. Only cache credentials are fetched from Infisical.
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Compile an isolated rlib, restart the daemon, and require an S3 hit.
    Smoke {},
}

// Do not pass OVH management credentials to arbitrary build tools.
const KEYS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_EC2_METADATA_DISABLED",
    "SCCACHE_BUCKET",
    "SCCACHE_REGION",
    "SCCACHE_ENDPOINT",
    "SCCACHE_S3_USE_SSL",
    "SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE",
    "SCCACHE_S3_KEY_PREFIX",
    "SCCACHE_S3_RW_MODE",
];

struct Session {
    executable: PathBuf,
    env: BTreeMap<String, String>,
}
impl Session {
    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.envs(&self.env);
        // An empty variable still forces recaching in upstream sccache.
        if std::env::var_os("SCCACHE_RECACHE").is_some_and(|s| s.is_empty()) {
            cmd.env_remove("SCCACHE_RECACHE");
        }
        cmd
    }
    fn start(&self) -> Result<()> {
        process::output(self.command().arg("--start-server"))?;
        Ok(())
    }
    fn stats(&self) -> Result<String> {
        process::output(self.command().args(["--show-stats", "--stats-format=json"]))
    }
    fn stop(&self) -> Result<()> {
        process::output(self.command().arg("--stop-server"))?;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn run(root: &Path, opts: &model::Common, action: &Action) -> Result<()> {
    let mut env = cache_config::load(root).map_err(|e| anyhow::anyhow!("{e}"))?;
    let is_smoke = matches!(action, Action::Smoke {});
    for key in ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"] {
        let source = if is_smoke {
            format!("INFRA_{key}")
        } else {
            key.into()
        };
        env.insert(key.into(), crate::credentials::read(root, &source)?);
    }
    env.insert(
        "SCCACHE_S3_RW_MODE".into(),
        if is_smoke { "READ_WRITE" } else { "READ_ONLY" }.into(),
    );
    for key in KEYS {
        if let Ok(value) = std::env::var(key) {
            env.insert((*key).into(), value);
        }
    }
    for key in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "SCCACHE_BUCKET",
        "SCCACHE_REGION",
        "SCCACHE_ENDPOINT",
    ] {
        ensure!(
            env.get(key).is_some_and(|v| !v.is_empty()),
            "missing {key}; check Infisical membership or the process environment"
        );
    }
    ensure!(
        env["SCCACHE_ENDPOINT"].starts_with("https://"),
        "S3 endpoint must use HTTPS"
    );
    env.insert("SCCACHE_S3_USE_SSL".into(), "true".into());
    env.insert("AWS_EC2_METADATA_DISABLED".into(), "true".into());
    env.insert("CARGO_INCREMENTAL".into(), "0".into());
    env.insert(
        "SCCACHE_BASEDIRS".into(),
        root.to_string_lossy().into_owned(),
    );
    // Each invocation owns its daemon. Never stop or reconfigure a user's daemon.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    env.insert(
        "SCCACHE_SERVER_PORT".into(),
        listener.local_addr()?.port().to_string(),
    );
    drop(listener);
    let host = model::host()?;
    let spec_host = host.replace("unknown-linux-gnu", "unknown-linux-musl");
    let executable = cache::ensure(
        &opts.cache_dir,
        &cache::spec("sccache", &spec_host)?,
        opts.offline,
    )?;
    let session = Session { executable, env };
    session.start()?;
    match action {
        Action::Run { command, .. } => {
            let frontend = root.join(if cfg!(windows) {
                ".compiler-cache/bin/sccache.exe"
            } else {
                ".compiler-cache/bin/sccache"
            });
            ensure!(
                frontend.is_file(),
                "run pwsh -File tools/setup_cache.ps1 before building"
            );
            let mut child = Command::new(&command[0]);
            child
                .args(&command[1..])
                .current_dir(root)
                .envs(&session.env)
                .env("RUSTC_WRAPPER", frontend);
            // Changing RUSTC_WRAPPER can rebuild xtask's dependencies. On
            // Windows the child must not try to replace this running exe.
            let target = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .map(|p| root.join(p))
                .unwrap_or_else(|| root.join("target"));
            let active_target = std::env::current_exe()?
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf);
            let is_active = active_target.as_ref().is_some_and(|active| {
                fs::canonicalize(active).ok() == fs::canonicalize(&target).ok()
            });
            if is_active {
                child.env("CARGO_TARGET_DIR", target.join("remote-cache"));
            }
            let status = child.status().context("launch cached command")?;
            println!("{}", session.stats()?);
            ensure!(status.success(), "cached command failed: {status}");
        }
        Action::Smoke { .. } => smoke(root, &session)?,
    }
    Ok(())
}

fn smoke(root: &Path, session: &Session) -> Result<()> {
    ensure!(
        session
            .env
            .get("SCCACHE_S3_RW_MODE")
            .is_none_or(|m| m == "READ_WRITE"),
        "smoke requires READ_WRITE cache access"
    );
    let tmp = tempfile::tempdir_in(root.join("target"))?;
    let source = tmp.path().join("probe.rs");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    fs::write(
        &source,
        format!("pub fn cache_probe() -> u128 {{ {nonce} }}\n"),
    )?;
    let output = tmp.path().join("libremote_cache_probe.rlib");
    let rustc = process::output(Command::new("rustup").args(["which", "rustc"]))?;
    let compile = || -> Result<()> {
        let status = session
            .command()
            .arg(rustc.trim())
            .arg(&source)
            .args([
                "--crate-name",
                "remote_cache_probe",
                "--crate-type",
                "rlib",
                "--emit=link",
                "--out-dir",
            ])
            .arg(tmp.path())
            .status()?;
        ensure!(
            status.success() && output.is_file(),
            "cache probe compilation failed"
        );
        Ok(())
    };
    compile()?;
    let first_hash = cache::hash_file(&output)?;
    let first = session.stats()?;
    session.stop()?;
    fs::remove_file(&output)?;
    session.start()?;
    compile()?;
    ensure!(
        cache::hash_file(&output)? == first_hash,
        "cached output differs from the first compilation"
    );
    let second = session.stats()?;
    let stats: serde_json::Value = serde_json::from_str(&second)?;
    let hits = stats
        .pointer("/stats/cache_hits/counts")
        .and_then(|v| v.as_object())
        .map(|m| m.values().filter_map(|v| v.as_u64()).sum::<u64>())
        .unwrap_or(0);
    let dir = root.join("artifacts/remote-cache-smoke");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("first.json"), first)?;
    fs::write(dir.join("second.json"), &second)?;
    fs::write(
        dir.join("evidence.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "output_sha256": first_hash,
            "identical_output": true,
            "output_deleted_between_compiles": true,
            "daemon_restarted_between_compiles": true,
            "cache_hits_after_restart": hits,
            "scope": "isolated Rust rlib; full game build speedup not measured"
        }))?,
    )?;
    println!("{second}");
    ensure!(
        hits > 0,
        "no remote hit after daemon restart; see artifacts/remote-cache-smoke"
    );
    println!("PASS: S3 cache hit after deleting the output and restarting sccache");
    Ok(())
}
