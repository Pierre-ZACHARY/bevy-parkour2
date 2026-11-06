use std::{env, fs, path::PathBuf};

fn main() {
    for name in ["SENTRY_DSN", "SENTRY_RELEASE", "SENTRY_ENVIRONMENT"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if env::var_os("CARGO_FEATURE_OFFICIAL").is_none() {
        return;
    }
    let dsn =
        env::var("SENTRY_DSN").expect("official telemetry requires the Pulumi SENTRY_DSN output");
    let authority = dsn
        .strip_prefix("https://")
        .and_then(|s| s.split('/').next())
        .expect("SENTRY_DSN must use HTTPS");
    let (key, host) = authority
        .split_once('@')
        .expect("SENTRY_DSN must contain a public client key");
    assert!(
        !key.is_empty() && key.bytes().all(|b| b.is_ascii_hexdigit()) && !host.is_empty(),
        "SENTRY_DSN must be public; secret DSNs and admin tokens are forbidden"
    );
    assert!(!dsn.chars().any(char::is_control), "invalid SENTRY_DSN");
    let release = env::var("SENTRY_RELEASE").expect("official telemetry requires SENTRY_RELEASE");
    let environment = env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "production".into());
    assert!(
        !release.trim().is_empty() && !release.chars().any(char::is_control),
        "invalid SENTRY_RELEASE"
    );
    assert!(
        !environment.trim().is_empty() && !environment.chars().any(char::is_control),
        "invalid SENTRY_ENVIRONMENT"
    );
    let generated = format!(
        "pub const DSN: &str = {dsn:?};\npub const RELEASE: &str = {release:?};\npub const ENVIRONMENT: &str = {environment:?};\n"
    );
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("sentry_config.rs"),
        generated,
    )
    .expect("write Sentry client configuration");
}
