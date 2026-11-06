//! Reproducible Basis experiment. Encoding is native; browser builds only transcode.
use crate::{
    cache,
    model::Common,
    pipeline, process,
    report::{Run, Status},
};
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
const REV: &str = "14f2186f59b5e41a6cb1f9272b8175e7862d5b05";
#[derive(Subcommand)]
pub enum Action {
    Basisu,
    /// Serve a generated prototype on localhost and collect browser evidence.
    Serve {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Keep captures and reports outside an immutable verified package.
        #[arg(long)]
        evidence_dir: Option<PathBuf>,
    },
}
fn cargo(run: &Run, opts: &Common, verb: &str, package: &str) -> Command {
    cargo_in(run, opts, verb, package, "target/ci")
}
fn cargo_in(run: &Run, opts: &Common, verb: &str, package: &str, target_dir: &str) -> Command {
    let mut c = Command::new("cargo");
    c.current_dir(&run.root)
        .args([
            verb,
            "--locked",
            "-p",
            package,
            "--profile",
            "ci",
            "--jobs",
            &opts.jobs.to_string(),
        ])
        .arg("--target-dir")
        .arg(run.root.join(target_dir));
    if opts.offline {
        c.arg("--offline");
    }
    c
}
fn step(run: &mut Run, name: &str, c: &mut Command) -> Result<()> {
    ensure!(process::run(run, name, c)?, "{name} failed");
    Ok(())
}
pub fn prepare_dependencies(root: &Path, offline: bool) -> Result<Value> {
    // Cargo's Windows git checkout can represent upstream directory symlinks as text.
    // Materialize only these exact vendored directories from the pinned checkout.
    let mut meta = Command::new("cargo");
    meta.current_dir(root)
        .args(["metadata", "--locked", "--format-version", "1"]);
    if offline {
        meta.arg("--offline");
    }
    let metadata: Value = serde_json::from_str(&process::output(&mut meta)?)?;
    let package = metadata["packages"]
        .as_array()
        .context("cargo packages")?
        .iter()
        .find(|p| p["name"] == "basisu_c_sys")
        .context("basisu dependency")?;
    ensure!(
        package["source"].as_str().is_some_and(|s| s.ends_with(REV)),
        "unexpected Basis source revision"
    );
    let manifest = PathBuf::from(package["manifest_path"].as_str().context("manifest path")?);
    let source = manifest
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .context("source root")?;
    materialize_links(source)?;
    Ok(metadata)
}
pub fn basisu(run: &mut Run, opts: &Common) -> Result<()> {
    let metadata = prepare_dependencies(&run.root, opts.offline)?;
    let stage = run.dir.join("basisu");
    fs::create_dir(&stage)?;
    run.required_artifacts
        .push("basisu/assets/preparation.json".into());
    run.configuration["prototype"] = json!({"name":"basisu","upstream_revision":REV,"package":stage,"browser_validation":"NOT_RUN","game_runtime":"NOT_RUN"});
    run.save()?;
    let mut prepare = cargo(run, opts, "run", "basisu-prepare");
    prepare.arg("--").arg(stage.join("assets"));
    step(run, "basisu-encode-and-roundtrip", &mut prepare)?;
    let bindgen = pipeline::ensure_tools(opts, &run.host, &["wasm-bindgen"])?.remove(0);
    let packages = metadata["packages"].as_array().unwrap();
    let version = packages
        .iter()
        .find(|p| p["name"] == "wasm-bindgen")
        .context("wasm-bindgen dependency")?["version"]
        .as_str()
        .unwrap();
    ensure!(
        process::output(Command::new(&bindgen).arg("--version"))?
            .split_whitespace()
            .nth(1)
            == Some(version),
        "wasm-bindgen CLI does not match lockfile"
    );
    for (name, with_jackdaw) in [("standalone", false), ("jackdaw", true)] {
        let package_dir = stage.join(name);
        fs::create_dir(&package_dir)?;
        for file in [
            "native-valid.json",
            "native-valid.png",
            "native-missing.json",
            "native-corrupt.json",
            "wasm-dependencies.txt",
            "wasm-basis-features.txt",
            "prototype.js",
            "prototype_bg.wasm",
            "index.html",
            "manifest.json",
        ] {
            run.required_artifacts.push(format!("basisu/{name}/{file}"));
        }
        copy_tree(&stage.join("assets"), &package_dir.join("assets"))?;
        let mut native = cargo(run, opts, "build", "basisu-prototype");
        if with_jackdaw {
            native.args(["--features", "jackdaw"]);
        }
        step(run, &format!("{name}-native-build"), &mut native)?;
        let exe = format!("basisu-prototype{}", std::env::consts::EXE_SUFFIX);
        fs::copy(
            run.root.join("target/ci/ci").join(&exe),
            package_dir.join(&exe),
        )?;
        for case in ["valid", "missing", "corrupt"] {
            let report = package_dir.join(format!("native-{case}.json"));
            let mut c = Command::new(package_dir.join(&exe));
            c.env("BASISU_ASSETS", package_dir.join("assets"))
                .env("BASISU_EXIT", "1")
                .env("BASISU_REPORT", &report)
                .env("BASISU_CASE", if case == "valid" { "" } else { case })
                .current_dir(&package_dir);
            let log = run.dir.join(format!("logs/{name}-{case}.log"));
            let mut result = process::execute(&mut c, &log, Duration::from_secs(120))?;
            result.name = format!("{name}-native-{case}");
            result.log = Some(log.strip_prefix(&run.dir)?.to_string_lossy().into());
            let data = fs::read(&report)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            let valid = if case == "valid" {
                result.status == Status::Pass
                    && report.with_extension("png").is_file()
                    && data.as_ref().is_some_and(|d| d["status"] == "PASS")
            } else {
                result.status == Status::Fail
                    && result.exit_code == Some(1)
                    && data
                        .as_ref()
                        .is_some_and(|d| d["status"] == "FAIL" && d["expected_failure"] == true)
            };
            result.detail = Some(
                if valid {
                    "expected outcome observed"
                } else {
                    "unexpected or missing runtime result"
                }
                .into(),
            );
            result.status = if valid { Status::Pass } else { Status::Fail };
            run.steps.push(result);
            run.save()?;
            // Preserve diagnostics and continue to the browser build even if native rendering failed.
        }
        let mut tree = Command::new("cargo");
        tree.current_dir(&run.root).args([
            "tree",
            "--locked",
            "-p",
            "basisu-prototype",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            "normal,build",
        ]);
        if with_jackdaw {
            tree.args(["--features", "jackdaw"]);
        }
        if opts.offline {
            tree.arg("--offline");
        }
        let graph = process::output(&mut tree)?;
        fs::write(package_dir.join("wasm-dependencies.txt"), &graph)?;
        ensure!(
            !graph.contains("basis-universal-sys") && graph.contains("basisu_c_sys"),
            "incorrect wasm Basis dependency graph"
        );
        let mut tree = Command::new("cargo");
        tree.current_dir(&run.root).args([
            "tree",
            "--locked",
            "-p",
            "basisu-prototype",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            "features",
            "-i",
            "basisu_c_sys",
        ]);
        if with_jackdaw {
            tree.args(["--features", "jackdaw"]);
        }
        if opts.offline {
            tree.arg("--offline");
        }
        let feature_graph = process::output(&mut tree)?;
        fs::write(package_dir.join("wasm-basis-features.txt"), &feature_graph)?;
        ensure!(
            !feature_graph.contains("feature \"encoder\""),
            "encoder leaked into browser"
        );
        let mut wasm = cargo(run, opts, "build", "basisu-prototype");
        wasm.args(["--target", "wasm32-unknown-unknown"]);
        if with_jackdaw {
            wasm.args(["--features", "jackdaw"]);
        }
        step(run, &format!("{name}-wasm-build"), &mut wasm)?;
        let mut bind = Command::new(&bindgen);
        bind.arg(
            run.root
                .join("target/ci/wasm32-unknown-unknown/ci/basisu-prototype.wasm"),
        )
        .args(["--target", "web", "--out-name", "prototype", "--out-dir"])
        .arg(&package_dir);
        step(run, &format!("{name}-wasm-bindgen"), &mut bind)?;
        fs::write(
            package_dir.join("index.html"),
            include_str!("../templates/basisu.html"),
        )?;
        fs::write(
            package_dir.join("manifest.json"),
            serde_json::to_vec_pretty(
                &json!({"basisu_revision":REV,"bevy":"0.20.1","variant":name,"files":cache::inventory(&package_dir)?,"browser_validation":"NOT_RUN","game_validation":"NOT_RUN"}),
            )?,
        )?;
    }
    for (edges, file) in [
        ("normal,build", "game-wasm-dependencies.txt"),
        ("features", "game-wasm-basis-features.txt"),
    ] {
        let mut tree = Command::new("cargo");
        tree.current_dir(&run.root).args([
            "tree",
            "--locked",
            "-p",
            "bevy_parkour2",
            "--no-default-features",
            "--features",
            "distribution,webgpu",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            edges,
        ]);
        if edges == "features" {
            tree.args(["-i", "basisu_c_sys"]);
        }
        if opts.offline {
            tree.arg("--offline");
        }
        let graph = process::output(&mut tree)?;
        fs::write(stage.join(file), &graph)?;
        ensure!(
            !graph.contains("basis-universal-sys")
                && graph.contains("basisu_c_sys")
                && !graph.contains("feature \"encoder\""),
            "incorrect game WASM Basis graph"
        );
    }
    let mut game = cargo(run, opts, "build", "bevy_parkour2");
    game.args([
        "--no-default-features",
        "--features",
        "distribution,webgpu",
        "--target",
        "wasm32-unknown-unknown",
    ]);
    let web_game = step(run, "game-wasm-build", &mut game);
    // Match the ordinary native CI test cache and its incremental setting.
    let mut native_game = cargo_in(run, opts, "build", "bevy_parkour2", "target/ci/shared");
    native_game.env("CARGO_INCREMENTAL", "0");
    native_game.args(["--no-default-features", "--features", "distribution"]);
    let native_game = step(run, "game-native-build", &mut native_game);
    web_game?;
    native_game?;
    run.configuration["prototype"]["builds_finished"] = json!(true);
    Ok(())
}
fn copy_tree(source: &Path, dest: &Path) -> Result<()> {
    fs::create_dir(dest)?;
    for e in walkdir::WalkDir::new(source)
        .follow_links(true)
        .min_depth(1)
    {
        let e = e?;
        let to = dest.join(e.path().strip_prefix(source)?);
        if e.file_type().is_dir() {
            fs::create_dir_all(to)?;
        } else {
            fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}
fn materialize_links(source: &Path) -> Result<()> {
    let root = source.canonicalize()?;
    for relative in [
        "crates/basisu_c_sys/vendored",
        "crates/basisu_wasm_libcxx/vendored",
    ] {
        let links: Vec<_> = walkdir::WalkDir::new(source.join(relative))
            .follow_links(false)
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for e in links {
            if !e.file_type().is_file() || e.metadata()?.len() > 200 {
                continue;
            }
            let Ok(text) = fs::read_to_string(e.path()) else {
                continue;
            };
            let target = text.trim();
            if !target.starts_with("../") || target.contains('\n') {
                continue;
            }
            let target = e.path().parent().unwrap().join(target).canonicalize()?;
            ensure!(
                target.starts_with(&root),
                "vendored link escapes pinned checkout"
            );
            if !target.is_dir() {
                continue;
            }
            let temp = tempfile::Builder::new()
                .prefix(".basisu-materialize-")
                .tempdir_in(e.path().parent().unwrap())?;
            let copied = temp.path().join("contents");
            copy_tree(&target, &copied)?;
            // A backup makes interruption after replacement recoverable, without deleting a directory.
            let backup = e.path().with_extension("xtask-link");
            ensure!(
                !backup.exists(),
                "incomplete prior materialization: {}",
                backup.display()
            );
            fs::rename(e.path(), &backup)?;
            if let Err(err) = fs::rename(&copied, e.path()) {
                fs::rename(&backup, e.path())?;
                return Err(err.into());
            }
            fs::remove_file(backup)?;
            temp.close()?;
        }
    }
    Ok(())
}
pub fn serve(dir: &Path, port: u16, evidence_dir: Option<&Path>) -> Result<()> {
    let dir = dir.canonicalize()?;
    ensure!(dir.join("index.html").is_file(), "not a prototype package");
    let evidence_dir = evidence_dir.unwrap_or(&dir);
    fs::create_dir_all(evidence_dir)?;
    let evidence_dir = evidence_dir.canonicalize()?;
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!("Serving {} at http://127.0.0.1:{port}", dir.display());
    for stream in listener.incoming() {
        let stream = stream?;
        let root = dir.clone();
        let evidence = evidence_dir.clone();
        std::thread::spawn(move || {
            if let Err(e) = request(stream, &root, &evidence) {
                eprintln!("HTTP: {e:#}");
            }
        });
    }
    Ok(())
}
fn request(mut stream: TcpStream, root: &Path, evidence: &Path) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut data = vec![];
    let mut buffer = [0u8; 4096];
    let end = loop {
        let n = stream.read(&mut buffer)?;
        ensure!(n > 0, "incomplete request");
        data.extend_from_slice(&buffer[..n]);
        ensure!(data.len() < 131072, "request too large");
        if let Some(i) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let header = std::str::from_utf8(&data[..end])?;
    let first = header.lines().next().context("request line")?;
    let parts: Vec<_> = first.split_whitespace().collect();
    ensure!(parts.len() == 3, "request line");
    let method = parts[0].to_owned();
    let path = parts[1].split('?').next().unwrap().to_owned();
    if method == "POST" && path == "/__capture" {
        let length: usize = header
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|s| s.trim().parse().ok())
            })
            .context("capture content length")?;
        ensure!(length <= 16 * 1024 * 1024, "capture too large");
        while data.len() < end + length {
            let n = stream.read(&mut buffer)?;
            ensure!(n > 0, "incomplete capture");
            data.extend_from_slice(&buffer[..n]);
        }
        let png = &data[end..end + length];
        ensure!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG capture");
        fs::write(evidence.join("web-capture.png"), png)?;
        stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    if method == "POST" && path == "/__result" {
        let length: usize = header
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .and_then(|s| s.parse().ok())
            })
            .context("content length")?;
        ensure!(length < 65536, "result too large");
        while data.len() < end + length {
            let n = stream.read(&mut buffer)?;
            ensure!(n > 0, "incomplete body");
            data.extend_from_slice(&buffer[..n]);
        }
        let value: Value = serde_json::from_slice(&data[end..end + length])?;
        let case = value["case"].as_str().context("case")?;
        ensure!(["", "missing", "corrupt"].contains(&case), "unknown case");
        let output = evidence.join(format!(
            "web-{}.json",
            if case.is_empty() { "valid" } else { case }
        ));
        fs::write(output, serde_json::to_vec_pretty(&value)?)?;
        stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    let safe = cache::safe_relative(Path::new(relative)).is_ok() && !relative.contains('%');
    let file = root.join(relative);
    let contained = file.canonicalize().is_ok_and(|p| p.starts_with(root));
    let bytes = if method == "GET" && safe && contained {
        fs::read(&file).ok()
    } else {
        None
    };
    let (status, body) = if let Some(bytes) = bytes {
        ("200 OK", bytes)
    } else {
        ("404 Not Found", b"Not found".to_vec())
    };
    let mime = match file.extension().and_then(|x| x.to_str()) {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    println!("{status} {path}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exchange(root: &Path, request_text: &str) -> String {
        let root = root.canonicalize().unwrap();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            request(listener.accept().unwrap().0, &root, &root).unwrap()
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(request_text.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        handle.join().unwrap();
        response
    }
    #[test]
    fn serving_missing_assets_does_not_return_html_and_traversal_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("package");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("index.html"), "prototype").unwrap();
        fs::write(temp.path().join("private"), "secret").unwrap();
        assert!(exchange(&root, "GET / HTTP/1.1\r\n\r\n").contains("200 OK"));
        for path in ["/missing.basisu.ktx2", "/../private", "/%2e%2e/private"] {
            let response = exchange(&root, &format!("GET {path} HTTP/1.1\r\n\r\n"));
            assert!(response.contains("404 Not Found"));
            assert!(!response.contains("secret"));
            assert!(!response.contains("prototype"));
        }
    }
    #[test]
    fn browser_failure_evidence_remains_a_failure() {
        let root = tempfile::tempdir().unwrap();
        let data = r#"{"case":"missing","status":"FAIL","expected_failure":true}"#;
        let response = exchange(
            root.path(),
            &format!(
                "POST /__result HTTP/1.1\r\nContent-Length: {}\r\n\r\n{data}",
                data.len()
            ),
        );
        assert!(response.contains("204 No Content"));
        let saved: Value =
            serde_json::from_slice(&fs::read(root.path().join("web-missing.json")).unwrap())
                .unwrap();
        assert_eq!(saved["status"], "FAIL");
    }
    #[test]
    fn windows_link_materialization_is_bounded_to_checkout_and_repeatable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("checkout");
        let vendor = root.join("crates/basisu_c_sys/vendored");
        fs::create_dir_all(&vendor).unwrap();
        fs::create_dir_all(root.join("crates/basisu_wasm_libcxx/vendored")).unwrap();
        let upstream = root.join("upstream");
        fs::create_dir(&upstream).unwrap();
        fs::write(upstream.join("test.h"), "original").unwrap();
        fs::write(vendor.join("link"), "../../../upstream").unwrap();
        materialize_links(&root).unwrap();
        materialize_links(&root).unwrap();
        assert_eq!(
            fs::read_to_string(vendor.join("link/test.h")).unwrap(),
            "original"
        );
        fs::create_dir(temp.path().join("outside")).unwrap();
        fs::write(vendor.join("escape"), "../../../../outside").unwrap();
        assert!(materialize_links(&root).is_err());
        assert!(vendor.join("escape").is_file());
    }
}
