use crate::{
    assets, cache,
    model::{self, Common, Suite, Target},
    process,
    report::{Run, Status, Step},
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn cargo(root: &Path, opts: &Common) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(root).env("CARGO_INCREMENTAL", "0");
    if opts.offline {
        command.env("CARGO_NET_OFFLINE", "true");
    }
    command
}
fn suite_args(command: &mut Command, suite: &Suite, root: &Path) {
    command
        .arg("--manifest-path")
        .arg(root.join(suite.manifest))
        .arg("--locked");
    if suite.workspace {
        command.args([
            "--workspace",
            "--exclude",
            "xtask",
            "--exclude",
            "asset-export",
            "--exclude",
            "basisu-prepare",
            "--exclude",
            "basisu-prototype",
            "--no-default-features",
        ]);
    }
    if !suite.features.is_empty() {
        command.args(["--features", suite.features]);
    }
}
fn tool_path(root: &Path, opts: &Common, host: &str, ids: &[&str]) -> Result<std::ffi::OsString> {
    let mut dirs = BTreeSet::new();
    for id in ids {
        let path = cache::ensure(&opts.cache_dir, &cache::spec(id, host)?, opts.offline)?;
        dirs.insert(path.parent().context("tool directory")?.to_path_buf());
    }
    let mut all: Vec<_> = dirs.into_iter().collect();
    all.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let _ = root;
    Ok(std::env::join_paths(all)?)
}
pub fn doctor(root: &Path, opts: &Common, host: &str, requested: &[Target]) -> Result<()> {
    println!("Runner: {host}");
    let targets = if requested.is_empty() {
        vec![model::native_target(host)?]
    } else {
        requested.to_vec()
    };
    let mut errors = Vec::new();
    for target in targets {
        if let Err(e) = target.validate(host) {
            errors.push(e.to_string());
        }
        if !crate::toolchain::has_target(root, target.triple())? {
            errors.push(format!(
                "missing Rust target {}; rustup target add {}",
                target.triple(),
                target.triple()
            ));
        }
        if target == Target::Webgpu {
            check_tool(opts, host, "wasm-bindgen");
            check_tool(opts, host, "wasm-opt");
        }
        if target == Target::MacosArm64 {
            check_tool(opts, host, "moltenvk");
        }
    }
    check_tool(opts, host, "nextest");
    check_tool(opts, host, "llvm-cov");
    if let Err(e) = assets::validate(root) {
        errors.push(e.to_string());
    }
    for program in if host.contains("linux") {
        vec!["cc", "pkg-config"]
    } else if host.contains("apple") {
        vec!["xcrun"]
    } else {
        vec!["rustc"]
    } {
        let mut c = Command::new(program);
        c.arg("--version");
        if process::output(&mut c).is_err() {
            errors.push(format!("missing prerequisite: {program}"));
        }
    }
    for error in &errors {
        eprintln!("MISSING: {error}");
    }
    anyhow::ensure!(
        errors.is_empty(),
        "doctor found {} missing prerequisites",
        errors.len()
    );
    Ok(())
}
fn check_tool(opts: &Common, host: &str, id: &str) {
    match cache::spec(id, host).and_then(|s| cache::cached(&opts.cache_dir, &s)) {
        Ok(path) => println!("CACHED: {id}: {}", path.display()),
        Err(e) => println!("NOT CACHED: {id}; execution will prepare its pinned download ({e})"),
    }
}
pub fn lint(run: &mut Run, opts: &Common, suites: &[Suite]) -> Result<()> {
    for suite in suites {
        let mut fmt = cargo(&run.root, opts);
        fmt.args(["fmt", "--manifest-path"])
            .arg(run.root.join(suite.manifest))
            .args(["--", "--check"]);
        process::run(run, &format!("{}-fmt", suite.name), &mut fmt)?;
        let mut clippy = cargo(&run.root, opts);
        clippy.arg("clippy");
        suite_args(&mut clippy, suite, &run.root);
        clippy.args([
            "--all-targets",
            "--message-format=json",
            "--profile",
            "ci",
            "--jobs",
            &opts.jobs.to_string(),
            "--",
            "-D",
            "warnings",
        ]);
        clippy.env("CARGO_TARGET_DIR", run.root.join("target/ci/shared"));
        process::run(run, &format!("{}-clippy", suite.name), &mut clippy)?;
        let step = run.steps.last().context("Clippy step")?;
        let log = fs::read_to_string(run.dir.join(step.log.as_ref().context("Clippy log")?))?;
        let messages = log
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|v| v["reason"] == "compiler-message")
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let path = format!("clippy/{}.json", suite.name);
        fs::create_dir_all(run.dir.join("clippy"))?;
        fs::write(run.dir.join(&path), messages)?;
        run.required_artifacts.push(path);
    }
    Ok(())
}
fn llvm_component(root: &Path, opts: &Common) -> Result<()> {
    if std::env::var_os("PARKOUR_NIX_RUSTC").is_some() {
        for key in ["LLVM_COV", "LLVM_PROFDATA"] {
            let path = std::env::var_os(key).with_context(|| format!("missing Nix {key}"))?;
            anyhow::ensure!(Path::new(&path).is_file(), "missing Nix tool {key}");
        }
        return Ok(());
    }
    let installed = process::output(
        Command::new("rustup")
            .args(["component", "list", "--installed"])
            .current_dir(root),
    )?;
    if !installed.lines().any(|l| l.starts_with("llvm-tools")) {
        anyhow::ensure!(
            !opts.offline,
            "llvm-tools-preview is not installed; offline mode"
        );
        let status = Command::new("rustup")
            .args(["component", "add", "llvm-tools-preview"])
            .current_dir(root)
            .status()?;
        anyhow::ensure!(status.success(), "install llvm-tools-preview failed");
    }
    Ok(())
}
pub fn tests(run: &mut Run, opts: &Common, suites: &[Suite], coverage: bool) -> Result<()> {
    let path = tool_path(
        &run.root,
        opts,
        &run.host,
        if coverage {
            &["nextest", "llvm-cov"]
        } else {
            &["nextest"]
        },
    )?;
    if coverage {
        llvm_component(&run.root, opts)?;
    }
    for suite in suites {
        let result = test_suite(run, opts, suite, coverage, &path);
        if let Err(e) = result {
            run.error(&format!("{}-tests", suite.name), &e);
            if !run.tests.iter().any(|report| report.suite == suite.name) {
                let missing = run.dir.join("tests").join(suite.name).join("junit.xml");
                run.junit(suite.name, &missing);
            }
            run.save()?;
        }
    }
    Ok(())
}
fn test_target(root: &Path, suite: &Suite, coverage: bool) -> PathBuf {
    root.join(if coverage {
        "target/coverage-ci"
    } else {
        "target/ci"
    })
    .join(if coverage { suite.name } else { "shared" })
}
fn coverage_lock(target: &Path) -> Result<fs::File> {
    let parent = target.parent().context("coverage target parent")?;
    fs::create_dir_all(parent)?;
    // Keep the lock outside the disposable build directory. It must remain
    // locked while completed coverage targets are removed on small runners.
    let name = target
        .file_name()
        .context("coverage target name")?
        .to_string_lossy();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(parent.join(format!(".{name}.xtask-coverage.lock")))?;
    lock.try_lock()
        .context("another coverage collection is using this suite's target directory")?;
    Ok(lock)
}
fn test_suite(
    run: &mut Run,
    opts: &Common,
    suite: &Suite,
    coverage: bool,
    path: &std::ffi::OsStr,
) -> Result<()> {
    if suite.name == "game" {
        assets::validate(&run.root)?;
    }
    let out = run.dir.join("tests").join(suite.name);
    fs::create_dir_all(&out)?;
    // Coverage builds are separate from normal builds and from each other.
    let target = test_target(&run.root, suite, coverage);
    // File locks are released by the OS even if this process crashes.
    // Two coverage runs must never clean or merge each other's raw profiles.
    fs::create_dir_all(&target)?;
    let _coverage_lock = if coverage {
        Some(coverage_lock(&target)?)
    } else {
        None
    };
    let profile = if coverage { "coverage" } else { "ci" };
    let configure = |c: &mut Command| {
        c.env("PATH", path)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_LLVM_COV_TARGET_DIR", &target);
    };
    if coverage {
        let mut clean = cargo(&run.root, opts);
        clean
            .args(["llvm-cov", "clean", "--workspace", "--manifest-path"])
            .arg(run.root.join(suite.manifest));
        configure(&mut clean);
        anyhow::ensure!(
            process::run(run, &format!("{}-coverage-clean", suite.name), &mut clean)?,
            "coverage cleanup failed"
        );
    }
    // nextest store.dir is workspace-relative, independent of CARGO_TARGET_DIR.
    // Give every run its own store so concurrent invocations cannot reuse a JUnit file.
    let mut config: toml::Value =
        toml::from_str(&fs::read_to_string(run.root.join(".config/nextest.toml"))?)?;
    config
        .as_table_mut()
        .context("nextest configuration must be a table")?
        .insert(
            "store".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "dir".into(),
                toml::Value::String(out.join("nextest").to_string_lossy().into_owned()),
            )])),
        );
    let config_path = out.join("nextest.toml");
    fs::write(&config_path, toml::to_string(&config)?)?;
    let junit_source = out.join("nextest/ci/junit.xml");
    let mut test = cargo(&run.root, opts);
    if coverage {
        test.args(["llvm-cov", "nextest", "--no-report"]);
    } else {
        test.args(["nextest", "run"]);
    }
    suite_args(&mut test, suite, &run.root);
    test.args([
        "--cargo-profile",
        profile,
        "--profile",
        "ci",
        "--no-fail-fast",
        "--test-threads",
        &opts.jobs.to_string(),
        "--build-jobs",
        &opts.jobs.to_string(),
    ]);
    test.arg("--config-file").arg(&config_path);
    configure(&mut test);
    process::run(run, &format!("{}-nextest", suite.name), &mut test)?;
    let junit = out.join("junit.xml");
    if junit_source.exists() {
        fs::copy(&junit_source, &junit)?;
    }
    run.junit(suite.name, &junit);
    run.save()?;
    if coverage {
        // Package selection and feature flags belong to collection. `report`
        // reads this suite's isolated target; llvm-cov 0.9 rejects --exclude here.
        for format in ["html", "lcov", "json"] {
            let cov = out.join("coverage");
            fs::create_dir_all(&cov)?;
            let mut report = cargo(&run.root, opts);
            report
                .args(["llvm-cov", "report", "--manifest-path"])
                .arg(run.root.join(suite.manifest))
                .args(["--profile", "coverage", "--locked"]);
            report.arg(format!("--{format}"));
            let expected = if format == "html" {
                report.arg("--output-dir").arg(&cov);
                cov.join("html/index.html")
            } else {
                let output = cov.join(if format == "lcov" {
                    "lcov.info"
                } else {
                    "coverage.json"
                });
                report.arg("--output-path").arg(&output);
                output
            };
            run.required_artifacts.push(
                expected
                    .strip_prefix(&run.dir)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            configure(&mut report);
            process::run(
                run,
                &format!("{}-coverage-{format}", suite.name),
                &mut report,
            )?;
            if !expected.is_file() || expected.metadata()?.len() == 0 {
                run.error(
                    &format!("{}-coverage-{format}-artifact", suite.name),
                    &anyhow::anyhow!("missing or empty {}", expected.display()),
                );
            }
        }
    }
    // Stable doctests run separately and are never claimed to be part of JUnit/coverage.
    let mut docs = cargo(&run.root, opts);
    docs.args(["test", "--doc"]);
    suite_args(&mut docs, suite, &run.root);
    docs.args(["--profile", "ci", "--jobs", &opts.jobs.to_string()]);
    docs.env("CARGO_TARGET_DIR", run.root.join("target/ci/shared"));
    if matches!(suite.name, "xtask" | "asset-export") {
        run.steps.push(Step {
            name: format!("{}-doctests", suite.name),
            status: Status::Skipped,
            seconds: 0.,
            exit_code: None,
            log: None,
            detail: Some("binary-only tool; no library doctests".into()),
        });
    } else {
        process::run(run, &format!("{}-doctests", suite.name), &mut docs)?;
    }
    run.save()?;
    if coverage && std::env::var("XTASK_COVERAGE_CLEAN_TARGETS").as_deref() == Ok("1") {
        fs::remove_dir_all(&target)
            .with_context(|| format!("remove completed coverage build {}", target.display()))?;
    }
    Ok(())
}
#[test]
fn coverage_lock_survives_completed_target_cleanup() -> Result<()> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("coverage-ci/game");
    fs::create_dir_all(&target)?;
    let guard = coverage_lock(&target)?;
    fs::remove_dir_all(&target)?;
    assert!(coverage_lock(&target).is_err());
    drop(guard);
    assert!(coverage_lock(&target).is_ok());
    Ok(())
}
pub fn ensure_tools(opts: &Common, host: &str, ids: &[&str]) -> Result<Vec<PathBuf>> {
    ids.iter()
        .map(|id| cache::ensure(&opts.cache_dir, &cache::spec(id, host)?, opts.offline))
        .collect()
}

pub fn gate(input: &str) -> Result<()> {
    gate_for_event(
        input,
        &std::env::var("GITHUB_EVENT_NAME").unwrap_or_default(),
    )
}
fn gate_for_event(input: &str, event: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(input)?;
    let jobs = value.as_object().context("job results must be an object")?;
    for required in ["lint", "tests", "build"] {
        anyhow::ensure!(
            jobs.get(required)
                .and_then(|v| v["result"].as_str())
                .is_some_and(|result| result == "success"
                    || (required == "build" && event == "pull_request" && result == "skipped")),
            "required job {required} did not succeed"
        );
    }
    for optional in ["sentry-release", "sonar"] {
        if let Some(job) = jobs.get(optional) {
            anyhow::ensure!(
                matches!(job["result"].as_str(), Some("success" | "skipped")),
                "job {optional} failed or was cancelled"
            );
        }
    }
    println!("All required checks passed for this workflow commit.");
    Ok(())
}
#[cfg(test)]
mod gate_tests {
    #[test]
    fn public_pr_allows_only_the_deliberately_skipped_private_packaging_job() {
        let input = r#"{"lint":{"result":"success"},"tests":{"result":"success"},"build":{"result":"skipped"}}"#;
        assert!(super::gate_for_event(input, "pull_request").is_ok());
        assert!(super::gate_for_event(input, "push").is_err());
        assert!(
            super::gate_for_event(&input.replace("skipped", "failure"), "pull_request").is_err()
        );
        assert!(
            super::gate_for_event(
                &input.replace(
                    "\"tests\":{\"result\":\"success\"}",
                    "\"tests\":{\"result\":\"skipped\"}"
                ),
                "pull_request"
            )
            .is_err()
        );
    }
    #[test]
    fn missing_skipped_and_failed_jobs_block() {
        for input in [
            r#"{}"#,
            r#"{"lint":{"result":"success"},"tests":{"result":"skipped"},"build":{"result":"success"}}"#,
            r#"{"lint":{"result":"success"},"tests":{"result":"failure"},"build":{"result":"success"}}"#,
        ] {
            assert!(super::gate(input).is_err());
        }
        super::gate(r#"{"lint":{"result":"success"},"tests":{"result":"success"},"build":{"result":"success"}}"#).unwrap();
    }
}

#[cfg(test)]
mod acceptance {
    use super::*;
    /// Explicit acceptance run: requires the pinned nextest already in the local cache.
    /// This fixture never edits game assets and is excluded from normal CI tests.
    #[test]
    #[ignore = "requires cached nextest; cargo test -p xtask real_nextest_reports -- --ignored --nocapture"]
    fn real_nextest_reports() -> Result<()> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf();
        let host = model::host()?;
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join(".config"))?;
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "acceptance_fixture"
version = "0.1.0"
edition = "2024"
[workspace]
[profile.ci]
inherits = "dev"
"#,
        )?;
        fs::write(
            root.join("Cargo.lock"),
            "version = 4\n[[package]]\nname = \"acceptance_fixture\"\nversion = \"0.1.0\"\n",
        )?;
        fs::write(
            root.join(".config/nextest.toml"),
            r#"[profile.ci]
fail-fast = false
retries = 0
slow-timeout = { period = "200ms", terminate-after = 1 }
[profile.ci.junit]
path = "junit.xml"
report-skipped = "all"
"#,
        )?;
        fs::write(
            root.join("src/lib.rs"),
            r#"
#[test] fn success() {}
#[test] fn failure() { panic!("expected failure"); }
#[test] fn crash() { std::process::exit(7); }
#[test] fn timeout() { std::thread::sleep(std::time::Duration::from_secs(30)); }
#[test] #[ignore] fn authoring() { panic!("must never run"); }
"#,
        )?;
        let opts = Common {
            jobs: 2,
            cache_dir: repository.join("target/xtask-cache"),
            out_dir: repository.join("artifacts/acceptance"),
            offline: true,
        };
        let path = tool_path(&repository, &opts, &host, &["nextest"])?;
        let suite = Suite {
            name: "fixture",
            manifest: "Cargo.toml",
            features: "",
            workspace: false,
        };
        let mut first = Run::new(&repository, &opts, &host)?;
        first.root = root.into();
        test_suite(&mut first, &opts, &suite, false, &path)?;
        first.completed = true;
        first.save()?;
        assert!(first.failed());
        let counts = first.tests[0]
            .counts
            .as_ref()
            .context("expected complete JUnit after failure")?;
        assert_eq!(counts.tests, 5);
        assert_eq!(counts.passed, 1);
        assert_eq!(counts.failures + counts.errors, 3);
        assert_eq!(counts.skipped, 1);
        assert_eq!(counts.timeouts, 1);
        assert_eq!(first.tests[0].status, Status::Timeout);
        let xml = fs::read_to_string(first.dir.join("tests/fixture/junit.xml"))?;
        assert!(xml.contains("timeout") && xml.contains("crash"));
        assert!(crate::report::regenerate(&first.dir).is_err());
        fs::write(
            root.join("src/lib.rs"),
            "#[test] fn success() {}\n#[test] #[ignore] fn authoring() { panic!(); }",
        )?;
        let mut second = Run::new(&repository, &opts, &host)?;
        second.root = root.into();
        test_suite(&mut second, &opts, &suite, false, &path)?;
        second.completed = true;
        second.save()?;
        assert!(!second.failed(), "{second:?}");
        assert_ne!(first.dir, second.dir);
        assert_eq!(second.tests[0].counts.as_ref().unwrap().tests, 2);
        assert_eq!(
            fs::read_to_string(first.dir.join("tests/fixture/junit.xml"))?,
            xml
        );
        fs::remove_file(second.dir.join("tests/fixture/junit.xml"))?;
        assert!(crate::report::regenerate(&second.dir).is_err());
        println!(
            "Acceptance evidence: {} and {}",
            first.dir.display(),
            second.dir.display()
        );
        Ok(())
    }
}
