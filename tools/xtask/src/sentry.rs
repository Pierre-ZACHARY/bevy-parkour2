#[cfg(test)]
use crate::cache;
use crate::{
    credentials,
    model::{BuildArgs, Channel, Common, Target},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[cfg(test)]
pub const DEFAULT_STACK: &str = "Pierre-ZACHARY/bevy-parkour2-infra/dev";
const BUILD_ENV: &[&str] = &["SENTRY_DSN", "SENTRY_RELEASE", "SENTRY_ENVIRONMENT"];
const ADMIN_ENV: &[&str] = &[
    "SENTRY_AUTH_TOKEN",
    "SENTRY_RELEASE_TOKEN",
    "SENTRY_API_KEY",
    "PULUMI_ACCESS_TOKEN",
    "OVH_APPLICATION_KEY",
    "OVH_APPLICATION_SECRET",
    "OVH_CONSUMER_KEY",
    "SONAR_TOKEN",
    "SONARQUBE_TOKEN",
    "INFISICAL_TOKEN",
    "INFISICAL_UNIVERSAL_AUTH_CLIENT_SECRET",
];

pub struct Client {
    dsn: String,
    pub release: String,
    pub environment: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageInfo {
    pub release: String,
    pub environment: String,
    pub transport: String,
}

pub fn validate_dsn(dsn: &str) -> Result<()> {
    let url = url::Url::parse(dsn).context("invalid public Sentry DSN (value redacted)")?;
    ensure!(
        url.scheme() == "https" && url.host_str().is_some(),
        "Sentry DSN must use HTTPS"
    );
    ensure!(
        !url.username().is_empty()
            && url.username().bytes().all(|b| b.is_ascii_hexdigit())
            && url.password().is_none(),
        "Sentry requires a public ingestion DSN; secret DSNs/admin tokens are forbidden"
    );
    ensure!(
        url.query().is_none() && url.fragment().is_none() && !dsn.chars().any(char::is_whitespace),
        "invalid Sentry DSN"
    );
    ensure!(
        url.path_segments()
            .and_then(|mut s| s.next_back())
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())),
        "Sentry DSN must identify a project"
    );
    Ok(())
}

pub fn resolve(root: &Path, opts: &Common, args: &BuildArgs) -> Result<Option<Client>> {
    // Inherited CI secrets/configuration must never enable telemetry in locals.
    if args.channel != Channel::Official {
        return Ok(None);
    }
    let dsn = match std::env::var("SENTRY_DSN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        Some(dsn) => dsn,
        None => {
            ensure!(
                !opts.offline,
                "offline official builds require SENTRY_DSN in the process environment"
            );
            credentials::read(root, "SENTRY_DSN")?
        }
    };
    validate_dsn(&dsn)?;
    let tag = args
        .tag
        .as_deref()
        .context("official Sentry release requires a verified tag")?;
    ensure!(
        !tag.is_empty() && !tag.chars().any(char::is_control),
        "invalid Sentry release tag"
    );
    Ok(Some(Client {
        dsn,
        release: format!("bevy-parkour2@{tag}"),
        environment: "production".into(),
    }))
}

pub fn configure(command: &mut Command, client: Option<&Client>) {
    for name in BUILD_ENV.iter().chain(ADMIN_ENV) {
        command.env_remove(name);
    }
    if let Some(client) = client {
        command
            .env("SENTRY_DSN", &client.dsn)
            .env("SENTRY_RELEASE", &client.release)
            .env("SENTRY_ENVIRONMENT", &client.environment);
    }
}

pub fn features(target: Target, client: Option<&Client>) -> String {
    if client.is_some() {
        format!("{},sentry", target.features())
    } else {
        target.features().into()
    }
}

impl Client {
    pub fn package_info(&self, target: Target) -> PackageInfo {
        PackageInfo {
            release: self.release.clone(),
            environment: self.environment.clone(),
            transport: if target == Target::Webgpu {
                "browser"
            } else {
                "rust"
            }
            .into(),
        }
    }
    pub fn stage_browser(&self, stage: &Path, opts: &Common) -> Result<String> {
        let sdk = crate::sentry_web::bundle(opts)?;
        fs::copy(sdk, stage.join("sentry-browser.js"))?;
        let config = serde_json::json!({"dsn": self.dsn, "release": self.release, "environment": self.environment,
            "sendDefaultPii": false, "tracesSampleRate": 0.0, "sendClientReports": false, "defaultIntegrations": false});
        fs::write(
            stage.join("sentry-config.js"),
            format!(
                "if (globalThis.Sentry) {{\nconst config = {config};\ntry {{ globalThis.__parkourTelemetryEnabled = localStorage.getItem('parkour.telemetry') !== 'off'; }} catch {{ globalThis.__parkourTelemetryEnabled = true; }}\nconfig.beforeSend = event => globalThis.__parkourTelemetryEnabled ? event : null;\nconfig.integrations = [Sentry.wasmIntegration(), Sentry.globalHandlersIntegration(), Sentry.breadcrumbsIntegration({{console:true,fetch:false,xhr:false,history:false,dom:false}}), Sentry.dedupeIntegration(), Sentry.linkedErrorsIntegration()];\nSentry.init(config);\nglobalThis.__parkourCaptureError = error => {{ if(globalThis.__parkourTelemetryEnabled) Sentry.captureException(error); }};\n}}\n"
            ),
        )?;
        let html_path = stage.join("index.html");
        let html = fs::read_to_string(&html_path)?;
        ensure!(
            html.contains("</head>"),
            "browser template has no head insertion point"
        );
        fs::write(html_path, html.replacen("</head>", "<script src=\"./sentry-browser.js\"></script><script src=\"./sentry-config.js\"></script></head>", 1))?;
        Ok("11.11.0".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Uploads validation probe debug symbols and creates a marked Sentry release"]
    fn live_cli_symbols() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let opts = Common {
            jobs: 2,
            cache_dir: root.join("target/xtask-cache"),
            out_dir: root.join("artifacts/telemetry-research/sentry-cli"),
            offline: false,
        };
        let mut run = crate::report::Run::new(root, &opts, &crate::model::host()?)?;
        let args = BuildArgs {
            targets: vec![Target::WindowsX64],
            profile: crate::model::Profile::Ci,
            channel: Channel::Official,
            tag: Some("integration-probe".into()),
            sentry_stack: None,
            asset_quality: "original".into(),
            asset_presets: None,
        };
        let mut client = resolve(root, &opts, &args)?.context("official configuration")?;
        client.environment = "validation".into();
        let publisher = crate::sentry_cli::Publisher::new(&run, &opts, &args, &client)?;
        publisher.begin(&mut run)?;
        let fixture = run.dir.join("probe");
        fs::create_dir_all(&fixture)?;
        for (source, destination) in [
            ("report_probe.exe", "bevy_parkour2.exe"),
            ("report_probe.pdb", "bevy_parkour2.pdb"),
        ] {
            fs::copy(
                root.join("target/sentry-validation/ci/examples")
                    .join(source),
                fixture.join(destination),
            )?;
        }
        publisher.upload(&mut run, Target::WindowsX64, &fixture, &fixture)?;
        let web = root.join("artifacts/sentry-integration/browser-package");
        if web.join("probe_bg.wasm").is_file() {
            let stage = run.dir.join("web-symbols");
            fs::create_dir_all(&stage)?;
            fs::copy(web.join("probe_bg.wasm"), stage.join("game_bg.wasm"))?;
            fs::copy(web.join("probe.js"), stage.join("game.js"))?;
            publisher.upload(&mut run, Target::Webgpu, &fixture, &stage)?;
        }
        publisher.finalize(&mut run)?;
        run.completed = true;
        run.save()?;
        Ok(())
    }
    #[test]
    fn only_public_ingestion_dsns_are_accepted() {
        validate_dsn("https://0123abcd@o123.ingest.de.sentry.io/456").unwrap();
        for bad in [
            "sntryu_admin",
            "http://abcd@example.com/1",
            "https://abcd:secret@example.com/1",
            "https://example.com/1",
            "https://abcd@example.com/no-project",
            "https://abcd@example.com/1?token=secret",
        ] {
            assert!(validate_dsn(bad).is_err());
        }
    }
    #[test]
    fn build_environment_is_explicit_and_excludes_admin_credentials() {
        let mut local = Command::new("cargo");
        configure(&mut local, None);
        assert!(local.get_envs().all(|(_, value)| value.is_none()));
        let client = Client {
            dsn: "https://abcd@example.com/1".into(),
            release: "game@v1".into(),
            environment: "production".into(),
        };
        configure(&mut local, Some(&client));
        for name in ADMIN_ENV {
            assert!(local.get_envs().any(|(k, v)| k == *name && v.is_none()));
        }
        let info = serde_json::to_string(&client.package_info(Target::WindowsX64)).unwrap();
        assert!(!info.contains("abcd"));
        assert!(!features(Target::WindowsX64, None).contains("sentry"));
        assert!(features(Target::Webgpu, Some(&client)).ends_with(",sentry"));
    }
    #[test]
    fn local_build_needs_no_pulumi_or_client_configuration() {
        let args = BuildArgs {
            targets: vec![Target::WindowsX64],
            profile: crate::model::Profile::Release,
            channel: Channel::Local,
            tag: None,
            sentry_stack: Some("unreachable/stack".into()),
            asset_quality: "original".into(),
            asset_presets: None,
        };
        let opts = Common {
            jobs: 2,
            cache_dir: "absent".into(),
            out_dir: "absent".into(),
            offline: true,
        };
        assert!(
            resolve(Path::new("nonexistent"), &opts, &args)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn browser_package_embeds_only_public_client_configuration() {
        use sha2::{Digest, Sha256};
        let tmp = tempfile::tempdir().unwrap();
        let opts = Common {
            jobs: 2,
            cache_dir: tmp.path().join("cache"),
            out_dir: tmp.path().join("out"),
            offline: true,
        };
        let spec = cache::spec("sentry-browser", "universal").unwrap();
        let base = opts
            .cache_dir
            .join(&spec.id)
            .join(&spec.version)
            .join(&spec.host);
        fs::create_dir_all(base.join("content")).unwrap();
        let sdk = b"globalThis.Sentry = {};";
        fs::write(base.join("content").join(&spec.file), sdk).unwrap();
        fs::write(base.join("receipt.json"),serde_json::to_vec(&serde_json::json!({"archive_sha256":spec.sha256,"files":{spec.file.clone():format!("{:x}",Sha256::digest(sdk))}})).unwrap()).unwrap();
        let stage = tmp.path().join("stage");
        fs::create_dir(&stage).unwrap();
        fs::write(
            stage.join("index.html"),
            include_str!("../templates/index.html"),
        )
        .unwrap();
        let client = Client {
            dsn: "https://abcd@example.com/1".into(),
            release: "game@v1".into(),
            environment: "production".into(),
        };
        crate::sentry_web::fixture(&opts);
        client.stage_browser(&stage, &opts).unwrap();
        let html = fs::read_to_string(stage.join("index.html")).unwrap();
        assert!(html.find("sentry-config.js").unwrap() < html.find("import('./game.js')").unwrap());
        assert!(!html.contains("abcd"));
        let config = fs::read_to_string(stage.join("sentry-config.js")).unwrap();
        assert!(config.contains("game@v1"));
        assert!(config.contains("sendDefaultPii\":false"));
        assert!(!config.contains("SENTRY_AUTH_TOKEN"));
    }
    #[test]
    #[ignore = "Needs authenticated Pulumi Cloud; sends one marked validation report to Sentry"]
    fn live_stack_output_and_native_report() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let directory = root.join("artifacts/sentry-integration/browser-package");
        fs::create_dir_all(&directory).unwrap();
        let opts = Common {
            jobs: 2,
            cache_dir: root.join("target/xtask-cache"),
            out_dir: directory.clone(),
            offline: false,
        };
        let args = BuildArgs {
            targets: vec![Target::WindowsX64],
            profile: crate::model::Profile::Ci,
            channel: Channel::Official,
            tag: Some("integration-probe".into()),
            sentry_stack: Some(DEFAULT_STACK.into()),
            asset_quality: "original".into(),
            asset_presets: None,
        };
        let mut client = resolve(root, &opts, &args).unwrap().unwrap();
        client.environment = "validation".into();
        fs::write(
            directory.join("index.html"),
            include_str!("../templates/index.html"),
        )
        .unwrap();
        client.stage_browser(&directory, &opts).unwrap();
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .args([
                "run",
                "--locked",
                "-p",
                "game_telemetry",
                "--features",
                "official",
                "--example",
                "report_probe",
                "--profile",
                "ci",
                "--target-dir",
            ])
            .arg(root.join("target/sentry-validation"));
        configure(&mut command, Some(&client));
        assert!(
            command.status().unwrap().success(),
            "native Sentry probe failed"
        );
        fs::write(
            root.join("artifacts/sentry-integration/live-client-validation.json"),
            serde_json::to_vec_pretty(&client.package_info(Target::WindowsX64)).unwrap(),
        )
        .unwrap();
    }
    #[test]
    #[ignore = "Needs Pulumi, wasm target and pinned wasm-bindgen; prepares an isolated browser probe"]
    fn prepare_live_browser_probe() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let stage = root.join("artifacts/sentry-integration/browser-package");
        fs::create_dir_all(&stage).unwrap();
        let opts = Common {
            jobs: 2,
            cache_dir: root.join("target/xtask-cache"),
            out_dir: stage.clone(),
            offline: false,
        };
        let args = BuildArgs {
            targets: vec![Target::Webgpu],
            profile: crate::model::Profile::Ci,
            channel: Channel::Official,
            tag: Some("integration-probe".into()),
            sentry_stack: Some(DEFAULT_STACK.into()),
            asset_quality: "original".into(),
            asset_presets: None,
        };
        let mut client = resolve(root, &opts, &args).unwrap().unwrap();
        client.environment = "validation".into();
        fs::write(
            stage.join("index.html"),
            include_str!("../templates/index.html"),
        )
        .unwrap();
        client.stage_browser(&stage, &opts).unwrap();
        let mut command = Command::new("cargo");
        command
            .current_dir(root)
            .args([
                "build",
                "--locked",
                "-p",
                "game_telemetry",
                "--features",
                "official",
                "--example",
                "browser_probe",
                "--target",
                "wasm32-unknown-unknown",
                "--profile",
                "ci",
                "--target-dir",
            ])
            .arg(root.join("target/sentry-validation"));
        configure(&mut command, Some(&client));
        assert!(command.status().unwrap().success());
        let host = crate::model::host().unwrap();
        let bindgen = cache::ensure(
            &opts.cache_dir,
            &cache::spec("wasm-bindgen", &host).unwrap(),
            false,
        )
        .unwrap();
        assert!(
            Command::new(bindgen)
                .arg(root.join(
                    "target/sentry-validation/wasm32-unknown-unknown/ci/examples/browser_probe.wasm"
                ))
                .args([
                    "--target",
                    "web",
                    "--keep-debug",
                    "--out-name",
                    "probe",
                    "--out-dir"
                ])
                .arg(&stage)
                .status()
                .unwrap()
                .success()
        );
        fs::write(stage.join("probe.html"),"<!doctype html><html><head><script src='./sentry-browser.js'></script><script src='./sentry-config.js'></script></head><body><script type='module'>import init,{panic_probe,reporting_enabled,manual_probe} from './probe.js';await init();window.__probePanic=panic_probe;window.__probeEnabled=reporting_enabled;window.__probeManual=manual_probe;window.__probeReady=true;</script></body></html>").unwrap();
    }
}
