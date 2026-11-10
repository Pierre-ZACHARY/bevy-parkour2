use crate::{
    assets, cache,
    model::{BuildArgs, Channel, Common, Target},
    pipeline, process,
    report::{self, Run},
    sentry,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::Path,
    process::Command,
    time::Instant,
};

#[derive(Serialize, Deserialize)]
pub struct Manifest {
    schema: u32,
    commit: String,
    source_dirty: bool,
    tag: Option<String>,
    target: String,
    triple: String,
    profile: String,
    backends: Vec<String>,
    features: String,
    toolchain: String,
    tools: BTreeMap<String, String>,
    asset_format: String,
    #[serde(default)]
    asset_quality: String,
    #[serde(default)]
    asset_recipe: String,
    #[serde(default)]
    asset_presets_sha256: String,
    #[serde(default)]
    compiler_options: Vec<String>,
    files: BTreeMap<String, String>,
    capabilities: BTreeMap<String, bool>,
    runtime_validation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sentry: Option<sentry::PackageInfo>,
}
pub fn build_all(run: &mut Run, opts: &Common, args: &BuildArgs) -> Result<()> {
    if args.channel == Channel::Official {
        verify_tag(&run.root, args.tag.as_deref().context("official tag")?)?;
    }
    let client = sentry::resolve(&run.root, opts, args)?;
    let publisher = client
        .as_ref()
        .map(|c| crate::sentry_cli::Publisher::new(run, opts, args, c))
        .transpose()?;
    if let Some(publisher) = &publisher {
        publisher.begin(run)?;
    }
    let mut success = true;
    for target in &args.targets {
        if let Err(err) = build_one(
            run,
            opts,
            args,
            *target,
            client.as_ref(),
            publisher.as_ref(),
        ) {
            success = false;
            run.error(&format!("{}-build", target.name()), &err);
            run.save()?;
        }
    }
    if success
        && std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        && let Some(publisher) = &publisher
    {
        publisher.finalize(run)?;
    }
    Ok(())
}
pub(crate) fn verify_tag(root: &Path, tag: &str) -> Result<()> {
    let reference = format!("refs/tags/{tag}");
    report::git(root, &["check-ref-format", &reference])?;
    let tagged = report::git(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?;
    let head = report::git(root, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(tagged == head, "tag {tag} does not identify HEAD");
    anyhow::ensure!(
        report::git(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty(),
        "official builds require a clean checkout"
    );
    Ok(())
}
fn build_one(
    run: &mut Run,
    opts: &Common,
    args: &BuildArgs,
    target: Target,
    client: Option<&sentry::Client>,
    publisher: Option<&crate::sentry_cli::Publisher>,
) -> Result<()> {
    target.validate(&run.host)?;
    assets::validate(&run.root)?;
    let preset_path = args
        .asset_presets
        .as_deref()
        .unwrap_or(Path::new("tools/asset-export/presets.json"));
    let preset_path = run.root.join(preset_path);
    let presets: serde_json::Value = serde_json::from_slice(&fs::read(&preset_path)?)?;
    anyhow::ensure!(
        presets["presets"][&args.asset_quality][target.name()].is_string(),
        "no asset quality {} for {} in {}",
        args.asset_quality,
        target.name(),
        preset_path.display()
    );
    if target == Target::Webgpu {
        crate::prototype::prepare_dependencies(&run.root, opts.offline)?;
    }
    crate::toolchain::require_target(&run.root, target.triple(), opts.offline)?;
    let mut tool_versions = BTreeMap::new();
    let web_tools = if target == Target::Webgpu {
        check_bindgen_version(&run.root, &run.host)?;
        for id in ["wasm-bindgen", "wasm-opt"] {
            tool_versions.insert(id.into(), cache::spec(id, &run.host)?.version);
        }
        pipeline::ensure_tools(opts, &run.host, &["wasm-bindgen", "wasm-opt"])?
    } else {
        vec![]
    };
    let moltenvk = if target == Target::MacosArm64 {
        let s = cache::spec("moltenvk", &run.host)?;
        tool_versions.insert("moltenvk".into(), s.version.clone());
        Some(cache::ensure(&opts.cache_dir, &s, opts.offline)?)
    } else {
        None
    };
    let target_dir = run.root.join(if args.profile == crate::model::Profile::Ci {
        "target/ci"
    } else {
        "target/distribution"
    });
    let mut cargo = Command::new("cargo");
    let features = sentry::features(target, client);
    sentry::configure(&mut cargo, client);
    cargo
        .current_dir(&run.root)
        .args([
            "build",
            "--timings",
            "--locked",
            "-p",
            "bevy_parkour2",
            "--bin",
            "bevy_parkour2",
            "--no-default-features",
            "--features",
            &features,
            "--target",
            target.triple(),
            "--profile",
            args.profile.cargo(target),
            "--jobs",
            &opts.jobs.to_string(),
        ])
        .env("CARGO_TARGET_DIR", &target_dir);
    if target == Target::WindowsX64 {
        // A distributable executable must not depend on an unbundled MSVC runtime.
        cargo.args([
            "--config",
            r#"target.x86_64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]"#,
        ]);
    }
    if client.is_some() {
        if target == Target::Webgpu {
            cargo.args([
                "--config",
                r#"target.wasm32-unknown-unknown.rustflags=["-C","link-arg=--build-id"]"#,
            ]);
        }
        cargo.args([
            "--config",
            &format!("profile.{}.debug=2", args.profile.cargo(target)),
            "--config",
            &format!("profile.{}.strip=\"none\"", args.profile.cargo(target)),
        ]);
        tool_versions.insert(
            "sentry-cli".into(),
            cache::spec("sentry-cli", &run.host)?.version,
        );
    }
    if opts.offline {
        cargo.arg("--offline");
    }
    if target == Target::MacosArm64 {
        cargo.env("MACOSX_DEPLOYMENT_TARGET", "13.0");
    }
    let timing = target_dir.join("cargo-timings/cargo-timing.html");
    if timing.is_file() {
        fs::remove_file(&timing)?;
    }
    let compiled = process::run(run, &format!("{}-compile", target.name()), &mut cargo)?;
    // Keep Cargo's per-crate timing report even on compilation failure. This
    // measures the final crate separately from dependency cache restoration.
    if timing.is_file() {
        let timings = run.dir.join("timings");
        fs::create_dir_all(&timings)?;
        fs::copy(timing, timings.join(format!("{}.html", target.name())))?;
    }
    anyhow::ensure!(compiled, "compilation failed");
    let staging_started = Instant::now();
    let package_dir = run.dir.join("packages").join(target.name());
    fs::create_dir_all(&package_dir)?;
    let stage = package_dir.join(if target == Target::MacosArm64 {
        "BevyParkour.app"
    } else {
        "bevy_parkour2"
    });
    fs::create_dir(&stage)?;
    let built = target_dir
        .join(target.triple())
        .join(args.profile.directory(target));
    if target == Target::Webgpu {
        let mut bindgen = Command::new(&web_tools[0]);
        sentry::configure(&mut bindgen, None);
        bindgen
            .arg(built.join("bevy_parkour2.wasm"))
            .args(["--target", "web", "--out-name", "game", "--out-dir"])
            .arg(&stage);
        if client.is_some() {
            bindgen.arg("--keep-debug");
        }
        anyhow::ensure!(
            process::run(run, "webgpu-bindgen", &mut bindgen)?,
            "wasm-bindgen failed"
        );
        // Binaryen currently discards/invalidates Rust DWARF. Official builds keep
        // the exact bindgen module and upload its DWARF for symbolication.
        if client.is_none() {
            let optimized = stage.join("game_bg.optimized.wasm");
            let mut optimize = Command::new(&web_tools[1]);
            sentry::configure(&mut optimize, None);
            optimize
                .arg("-Oz")
                .arg(stage.join("game_bg.wasm"))
                .arg("-o")
                .arg(&optimized);
            anyhow::ensure!(
                process::run(run, "webgpu-optimize", &mut optimize)?,
                "wasm-opt failed"
            );
            fs::copy(&optimized, stage.join("game_bg.wasm"))?;
            fs::remove_file(optimized)?;
        }
        fs::write(
            stage.join("index.html"),
            include_str!("../templates/index.html"),
        )?;
        if let Some(client) = client {
            tool_versions.insert("sentry-browser".into(), client.stage_browser(&stage, opts)?);
        }
        assets::copy(&run.root, &stage.join("assets"))?;
    } else if target == Target::MacosArm64 {
        let macos = stage.join("Contents/MacOS");
        let resources = stage.join("Contents/Resources");
        let frameworks = stage.join("Contents/Frameworks");
        fs::create_dir_all(&macos)?;
        fs::create_dir_all(&frameworks)?;
        fs::copy(built.join("bevy_parkour2"), macos.join("game"))?;
        assets::copy(&run.root, &resources.join("assets"))?;
        fs::copy(
            moltenvk.as_ref().unwrap(),
            frameworks.join("libvulkan.dylib"),
        )?;
        let version = package_version(&run.root)?;
        fs::write(
            stage.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>bevy_parkour2</string><key>CFBundleIdentifier</key><string>org.pierre-zachary.bevy-parkour2</string><key>CFBundleName</key><string>Bevy Parkour</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundleVersion</key><string>{version}</string><key>LSMinimumSystemVersion</key><string>13.0</string><key>NSHighResolutionCapable</key><true/></dict></plist>"#
            ),
        )?;
        let mut launcher = Command::new("rustc");
        sentry::configure(&mut launcher, None);
        launcher
            .arg(run.root.join("tools/xtask/templates/macos_launcher.rs"))
            .args(["--edition=2024", "-O", "--target", target.triple(), "-o"])
            .arg(macos.join("bevy_parkour2"));
        anyhow::ensure!(
            process::run(run, "macos-launcher", &mut launcher)?,
            "launcher failed"
        );
        let sdk_root = opts
            .cache_dir
            .join("moltenvk")
            .join(cache::spec("moltenvk", &run.host)?.version)
            .join(&run.host)
            .join("content");
        for entry in walkdir::WalkDir::new(&sdk_root) {
            let entry = entry?;
            if entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains("license")
            {
                let destination = resources
                    .join("licenses")
                    .join(entry.path().strip_prefix(&sdk_root)?);
                fs::create_dir_all(destination.parent().unwrap())?;
                fs::copy(entry.path(), destination)?;
            }
        }
        // Ad-hoc signing permits local ARM execution; distribution signing/notarization is a later stage.
        for file in [
            frameworks.join("libvulkan.dylib"),
            macos.join("game"),
            macos.join("bevy_parkour2"),
        ] {
            anyhow::ensure!(
                process::run(
                    run,
                    "macos-adhoc-sign",
                    Command::new("codesign")
                        .args(["--force", "--sign", "-"])
                        .arg(file)
                )?,
                "ad-hoc signing failed"
            );
        }
    } else {
        let file = if target == Target::WindowsX64 {
            "bevy_parkour2.exe"
        } else {
            "bevy_parkour2"
        };
        fs::copy(built.join(file), stage.join(file))?;
        assets::copy(&run.root, &stage.join("assets"))?;
    }
    if let Some(publisher) = publisher {
        publisher.upload(run, target, &built, &stage)?;
    }
    let asset_directory = if target == Target::MacosArm64 {
        stage.join("Contents/Resources/assets")
    } else {
        stage.join("assets")
    };
    record_duration(run, target, "stage", staging_started);
    let mut export = Command::new("cargo");
    sentry::configure(&mut export, None);
    export
        .current_dir(&run.root)
        .args([
            "run",
            "--locked",
            "-p",
            "asset-export",
            "--profile",
            "ci",
            "--jobs",
            &opts.jobs.to_string(),
        ])
        .env("CARGO_TARGET_DIR", run.root.join("target/asset-export"));
    if opts.offline {
        export.arg("--offline");
    }
    export
        .arg("--")
        .arg(&asset_directory)
        .arg("--target")
        .arg(target.name())
        .arg("--quality")
        .arg(&args.asset_quality)
        .arg("--presets")
        .arg(&preset_path);
    anyhow::ensure!(
        process::run(
            run,
            &format!("{}-binary-assets", target.name()),
            &mut export
        )?,
        "binary asset export failed"
    );
    let packaging_started = Instant::now();
    let texture_report: serde_json::Value =
        serde_json::from_slice(&fs::read(asset_directory.join("asset-export.json"))?)?;
    let manifest = Manifest {
        schema: 1,
        commit: run.commit.clone(),
        source_dirty: !report::git(
            &run.root,
            &["status", "--porcelain", "--untracked-files=normal"],
        )?
        .is_empty(),
        tag: args.tag.clone(),
        target: target.name().into(),
        triple: target.triple().into(),
        profile: args.profile.cargo(target).into(),
        backends: target.backends().iter().map(|x| x.to_string()).collect(),
        features,
        toolchain: run.toolchain.clone(),
        tools: tool_versions,
        asset_format: "jackdaw-bsb-0.20.1-rc.1".into(),
        asset_quality: args.asset_quality.clone(),
        asset_recipe: texture_report["recipe"]
            .as_str()
            .context("asset recipe")?
            .into(),
        asset_presets_sha256: texture_report["presets_sha256"]
            .as_str()
            .context("preset fingerprint")?
            .into(),
        compiler_options: if target == Target::WindowsX64 {
            vec!["-C target-feature=+crt-static".into()]
        } else {
            vec![]
        },
        files: cache::inventory(&stage)?,
        capabilities: BTreeMap::from([
            ("dlss".into(), false),
            ("ray_tracing".into(), false),
            ("sentry".into(), client.is_some()),
        ]),
        runtime_validation: "UNAVAILABLE: requires independent GPU/browser validation".into(),
        sentry: client.map(|c| c.package_info(target)),
    };
    fs::write(
        stage.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    verify_package(&stage)?;
    let archive = package_dir.join(format!("bevy_parkour2-{}.zip", target.name()));
    zip_package(&stage, &archive)?;
    fs::write(
        archive.with_extension("zip.sha256"),
        format!(
            "{}  {}\n",
            cache::hash_file(&archive)?,
            archive.file_name().unwrap().to_string_lossy()
        ),
    )?;
    record_duration(run, target, "package", packaging_started);
    run.steps.last_mut().unwrap().detail =
        Some(archive.strip_prefix(&run.dir)?.to_string_lossy().into());
    run.save()?;
    Ok(())
}
fn record_duration(run: &mut Run, target: Target, phase: &str, started: Instant) {
    run.steps.push(crate::report::Step {
        name: format!("{}-{phase}", target.name()),
        status: crate::report::Status::Pass,
        seconds: started.elapsed().as_secs_f64(),
        exit_code: None,
        log: None,
        detail: None,
    });
}
fn package_version(root: &Path) -> Result<String> {
    let value: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    Ok(value["package"]["version"]
        .as_str()
        .context("package version")?
        .into())
}
fn check_bindgen_version(root: &Path, host: &str) -> Result<()> {
    let lock: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.lock"))?)?;
    let versions: Vec<_> = lock["package"]
        .as_array()
        .context("Cargo.lock packages")?
        .iter()
        .filter(|p| p["name"].as_str() == Some("wasm-bindgen"))
        .filter_map(|p| p["version"].as_str())
        .collect();
    let s = cache::spec("wasm-bindgen", host)?;
    anyhow::ensure!(
        versions == [s.version.as_str()],
        "Cargo.lock wasm-bindgen {versions:?} differs from pinned CLI {}; update tools.lock.json with verified release checksums",
        s.version
    );
    Ok(())
}
pub fn verify_package(dir: &Path) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
    let mut actual = cache::inventory(dir)?;
    actual.remove("manifest.json");
    anyhow::ensure!(
        actual == manifest.files,
        "package inventory/checksums differ from manifest"
    );
    for name in actual.keys() {
        cache::safe_relative(Path::new(name))?;
    }
    Ok(())
}
fn zip_package(dir: &Path, output: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(File::create(output)?);
    let prefix = dir
        .file_name()
        .context("package directory name")?
        .to_string_lossy();
    for (name, _) in cache::inventory(dir)? {
        let path = dir.join(&name);
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()?.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = 0o644;
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(mode);
        zip.start_file(format!("{prefix}/{name}"), options)?;
        std::io::copy(&mut File::open(path)?, &mut zip)?;
    }
    zip.finish()?.flush()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_tampering_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("game"), "binary").unwrap();
        let m = Manifest {
            schema: 1,
            commit: "a".into(),
            source_dirty: false,
            tag: None,
            target: "fixture".into(),
            triple: "fixture".into(),
            profile: "release".into(),
            backends: vec![],
            features: "".into(),
            toolchain: "test".into(),
            tools: BTreeMap::new(),
            asset_format: "fixture".into(),
            asset_quality: "original".into(),
            asset_recipe: "source".into(),
            asset_presets_sha256: String::new(),
            compiler_options: vec![],
            files: cache::inventory(tmp.path()).unwrap(),
            capabilities: BTreeMap::new(),
            runtime_validation: "UNAVAILABLE".into(),
            sentry: None,
        };
        fs::write(
            tmp.path().join("manifest.json"),
            serde_json::to_vec(&m).unwrap(),
        )
        .unwrap();
        verify_package(tmp.path()).unwrap();
        fs::write(tmp.path().join("game"), "modified").unwrap();
        assert!(verify_package(tmp.path()).is_err());
    }
}
