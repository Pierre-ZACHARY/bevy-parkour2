//! SonarCloud consumes the same LCOV collected by xtask (LLVM or Tarpaulin).
use crate::{
    cache, credentials,
    model::Common,
    process,
    report::{Run, Status, Step},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Default)]
struct Coverage {
    lines: BTreeMap<u32, u64>,
    branches: BTreeMap<(u32, String, String), u64>,
}
fn source_path(root: &Path, value: &str) -> Result<String> {
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let absolute = absolute
        .canonicalize()
        .context("coverage source does not exist")?;
    let root = root.canonicalize()?;
    let relative = absolute
        .strip_prefix(root)
        .context("coverage source is outside this checkout")?;
    let name = relative.to_string_lossy().replace('\\', "/");
    ensure!(name.ends_with(".rs"), "coverage source is not Rust");
    ensure!(
        !name
            .split('/')
            .any(|p| matches!(p, "target" | "vendor" | ".tools" | "infra")),
        "excluded coverage source"
    );
    Ok(name)
}
/// Merge repeated workspace coverage with max hits, rather than letting the last
/// suite's zero overwrite a line exercised by another suite.
pub fn merge_lcov(root: &Path, reports: &[PathBuf]) -> Result<String> {
    let mut files: BTreeMap<String, Coverage> = BTreeMap::new();
    for report in reports {
        let mut current = None;
        for line in fs::read_to_string(report)?.lines() {
            if let Some(path) = line.strip_prefix("SF:") {
                current = source_path(root, path).ok();
            } else if line == "end_of_record" {
                current = None;
            } else if let Some(name) = &current {
                let data = files.entry(name.clone()).or_default();
                if let Some(value) = line.strip_prefix("DA:") {
                    let mut fields = value.split(',');
                    let line: u32 = fields.next().context("LCOV line")?.parse()?;
                    ensure!(line > 0, "LCOV lines are one based");
                    let hits: u64 = fields.next().context("LCOV hits")?.parse()?;
                    data.lines
                        .entry(line)
                        .and_modify(|v| *v = (*v).max(hits))
                        .or_insert(hits);
                } else if let Some(value) = line.strip_prefix("BRDA:") {
                    let fields: Vec<_> = value.split(',').collect();
                    ensure!(fields.len() == 4, "invalid LCOV branch");
                    let key = (fields[0].parse()?, fields[1].into(), fields[2].into());
                    let hits = if fields[3] == "-" {
                        0
                    } else {
                        fields[3].parse()?
                    };
                    data.branches
                        .entry(key)
                        .and_modify(|v| *v = (*v).max(hits))
                        .or_insert(hits);
                }
            }
        }
    }
    ensure!(
        !files.is_empty() && files.values().any(|c| !c.lines.is_empty()),
        "no usable Rust coverage in this checkout"
    );
    let mut output = String::new();
    use std::fmt::Write;
    for (path, data) in files {
        writeln!(output, "SF:{path}")?;
        for (line, hits) in &data.lines {
            writeln!(output, "DA:{line},{hits}")?;
        }
        writeln!(
            output,
            "LF:{}\nLH:{}",
            data.lines.len(),
            data.lines.values().filter(|h| **h > 0).count()
        )?;
        for ((line, block, branch), hits) in data.branches {
            writeln!(output, "BRDA:{line},{block},{branch},{hits}")?;
        }
        writeln!(output, "end_of_record")?;
    }
    Ok(output)
}
pub fn analyze(run: &mut Run, opts: &Common, input: &Path, upload: bool) -> Result<()> {
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(input.join("summary.json"))?)?;
    ensure!(
        summary["commit"].as_str() == Some(run.commit.as_str()),
        "Sonar report commit differs from checkout"
    );
    let tests = summary["tests"]
        .as_array()
        .context("coverage run has no test suites")?;
    ensure!(!tests.is_empty(), "coverage run has no test suites");
    let mut reports = Vec::new();
    for suite in tests {
        ensure!(
            suite["status"] == "PASS",
            "Sonar requires passing suites; coverage must not hide failures"
        );
        let path = suite["path"].as_str().context("missing JUnit path")?;
        cache::safe_relative(Path::new(path))?;
        let counts = crate::report::parse_junit(&fs::read(input.join(path))?)?;
        ensure!(
            counts.tests > 0 && counts.failures == 0 && counts.errors == 0 && counts.timeouts == 0,
            "JUnit contains failures or no tests"
        );
        ensure!(
            serde_json::to_value(counts)? == suite["counts"],
            "JUnit counts changed since coverage collection"
        );
        let report = input
            .join(path)
            .parent()
            .context("suite path")?
            .join("coverage/lcov.info");
        ensure!(
            report.is_file(),
            "missing coverage {}; run cargo xtask ci --coverage",
            report.display()
        );
        reports.push(report);
    }
    let dir = run.dir.join("sonar");
    fs::create_dir_all(&dir)?;
    let lcov = dir.join("lcov.info");
    fs::write(&lcov, merge_lcov(&run.root, &reports)?)?;
    run.required_artifacts.push("sonar/lcov.info".into());
    let settings = dir.join("sonar-project.properties");
    let template = fs::read_to_string(run.root.join("sonar-project.properties"))?;
    fs::write(
        &settings,
        format!(
            "{template}\nsonar.rust.lcov.reportPaths={}\nsonar.projectVersion={}\nsonar.scm.revision={}\nsonar.working.directory={}\n",
            lcov.to_string_lossy().replace('\\', "/"),
            run.commit,
            run.commit,
            dir.join("work").to_string_lossy().replace('\\', "/")
        ),
    )?;
    run.required_artifacts
        .push("sonar/sonar-project.properties".into());
    let clippy: Vec<_> = crate::model::SUITES
        .iter()
        .map(|suite| input.join("clippy").join(format!("{}.json", suite.name)))
        .filter(|path| path.is_file())
        .collect();
    if !clippy.is_empty() {
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new().append(true).open(&settings)?,
            "sonar.rust.clippy.reportPaths={}",
            clippy
                .iter()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .collect::<Vec<_>>()
                .join(",")
        )?;
    }
    if !upload {
        run.steps.push(Step {
            name: "sonar-upload".into(),
            status: Status::Skipped,
            seconds: 0.,
            exit_code: None,
            log: None,
            detail: Some("LCOV prepared locally; --upload sends an analysis to SonarCloud".into()),
        });
        return run.save();
    }
    ensure!(!opts.offline, "SonarCloud analysis requires network access");
    let token = credentials::read(&run.root, "SONAR_TOKEN")?;
    let jar = cache::ensure(
        &opts.cache_dir,
        &cache::spec("sonar-scanner", &run.host)?,
        opts.offline,
    )?;
    let java = jar
        .parent()
        .and_then(Path::parent)
        .context("scanner directory")?
        .join(if cfg!(windows) {
            "jre/bin/java.exe"
        } else {
            "jre/bin/java"
        });
    ensure!(java.is_file(), "scanner's bundled Java runtime missing");
    let mut command = Command::new(java);
    crate::sentry::configure(&mut command, None);
    command
        .current_dir(&run.root)
        .env("SONAR_TOKEN", token)
        .args(["-jar"])
        .arg(jar)
        .arg(format!("-Dproject.settings={}", settings.display()))
        .arg(format!("-Dsonar.projectBaseDir={}", run.root.display()))
        .arg("-Dsonar.qualitygate.wait=false");
    ensure!(
        process::run(run, "sonar-analysis", &mut command)?,
        "Sonar analysis failed; see sonar-analysis log"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges_coverage_without_losing_hits_and_excludes_external_sources() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::write(root.join("lib.rs"), "fn a() {}\nfn b() {}")?;
        let a = root.join("a.info");
        let b = root.join("b.info");
        fs::write(
            &a,
            "SF:lib.rs\nDA:1,2\nDA:2,0\nend_of_record\nSF:../external.rs\nDA:1,1\nend_of_record\n",
        )?;
        fs::write(&b, "SF:lib.rs\nDA:1,0\nDA:2,3\nend_of_record\n")?;
        let result = merge_lcov(root, &[a, b])?;
        assert!(result.contains("DA:1,2\nDA:2,3"));
        assert!(result.contains("LH:2"));
        assert!(!result.contains("external"));
        Ok(())
    }
}
