use crate::model::Common;
use anyhow::{Context, Result, bail};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Pass,
    Fail,
    Timeout,
    Skipped,
    Unavailable,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub tests: u64,
    pub passed: u64,
    pub failures: u64,
    pub errors: u64,
    pub skipped: u64,
    #[serde(default)]
    pub timeouts: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    pub name: String,
    pub status: Status,
    pub seconds: f64,
    pub exit_code: Option<i32>,
    pub log: Option<String>,
    pub detail: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TestReport {
    pub suite: String,
    pub path: String,
    pub status: Status,
    pub counts: Option<Counts>,
    pub detail: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Run {
    pub schema: u32,
    #[serde(default)]
    pub completed: bool,
    pub id: String,
    pub commit: String,
    pub host: String,
    pub toolchain: String,
    pub steps: Vec<Step>,
    pub tests: Vec<TestReport>,
    pub runtime_validation: Status,
    pub coverage_note: String,
    #[serde(default)]
    pub configuration: serde_json::Value,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
    #[serde(skip)]
    pub root: PathBuf,
    #[serde(skip)]
    pub dir: PathBuf,
}
pub fn unique_id() -> String {
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        std::process::id()
    )
}
pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(root).output()?;
    anyhow::ensure!(
        out.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(String::from_utf8(out.stdout)?.trim().into())
}
impl Run {
    pub fn new(root: &Path, opts: &Common, host: &str) -> Result<Self> {
        let id = unique_id();
        let dir = opts.out_dir.join(&id);
        fs::create_dir_all(&opts.out_dir)?;
        fs::create_dir(&dir)?;
        let toolchain = Command::new("rustc").arg("-V").output()?;
        let run = Self {
            schema: 1, completed: false, id, commit: git(root, &["rev-parse", "HEAD"])?,
            host: host.into(), toolchain: String::from_utf8(toolchain.stdout)?.trim().into(),
            steps: vec![], tests: vec![], runtime_validation: Status::Unavailable,
            coverage_note: "Coverage is reported per suite; no combined percentage or threshold. Doctests and rendered checks are not included.".into(),
            configuration: serde_json::json!({
                "arguments": std::env::args().skip(1).collect::<Vec<_>>(),
                "suites": crate::model::SUITES,
                "tool_pins": crate::cache::specs()?,
                "tag": std::env::var("XTASK_TAG").ok(),
            }),
            required_artifacts: vec![],
            root: root.into(), dir,
        };
        run.save()?;
        Ok(run)
    }
    pub fn failed(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s.status,
                Status::Fail | Status::Timeout | Status::Unavailable
            )
        }) || self.tests.iter().any(|s| {
            matches!(
                s.status,
                Status::Fail | Status::Timeout | Status::Unavailable
            )
        })
    }
    pub fn error(&mut self, name: &str, error: &anyhow::Error) {
        self.steps.push(Step {
            name: name.into(),
            status: Status::Fail,
            seconds: 0.,
            exit_code: None,
            log: None,
            detail: Some(format!("{error:#}")),
        });
    }
    pub fn junit(&mut self, suite: &str, path: &Path) {
        let relative = path
            .strip_prefix(&self.dir)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let (status, counts, detail) = match fs::read(path)
            .with_context(|| format!("missing report {}", path.display()))
            .and_then(|x| parse_junit(&x))
        {
            Ok(c) => (
                if c.timeouts > 0 {
                    Status::Timeout
                } else if c.errors + c.failures > 0 {
                    Status::Fail
                } else {
                    Status::Pass
                },
                Some(c),
                None,
            ),
            Err(e) => (Status::Unavailable, None, Some(format!("{e:#}"))),
        };
        self.tests.push(TestReport {
            suite: suite.into(),
            path: relative,
            status,
            counts,
            detail,
        });
    }
    pub fn save(&self) -> Result<()> {
        fs::write(
            self.dir.join("summary.json"),
            serde_json::to_vec_pretty(self)?,
        )?;
        let mut md = format!(
            "# CI report\n\nCommit: `{}`\n\nHost: `{}`\n\nResult: **{}**\n\n",
            self.commit,
            self.host,
            if !self.completed {
                "INCOMPLETE"
            } else if self.failed() {
                "FAIL"
            } else {
                "PASS"
            }
        );
        md.push_str("| Stage | Status | Seconds | Detail |\n|---|---|---:|---|\n");
        for s in &self.steps {
            md.push_str(&format!(
                "| {} | {:?} | {:.2} | {} |\n",
                escape(&s.name),
                s.status,
                s.seconds,
                escape(s.detail.as_deref().unwrap_or(""))
            ));
        }
        md.push_str("\n| Suite | Report | Tests | Passed | Failed | Errors | Skipped | Timeouts |\n|---|---|---:|---:|---:|---:|---:|---:|\n");
        for t in &self.tests {
            if let Some(c) = &t.counts {
                md.push_str(&format!(
                    "| {} | {:?} | {} | {} | {} | {} | {} | {} |\n",
                    t.suite,
                    t.status,
                    c.tests,
                    c.passed,
                    c.failures,
                    c.errors,
                    c.skipped,
                    c.timeouts
                ));
            } else {
                md.push_str(&format!(
                    "| {} | UNAVAILABLE | - | - | - | - | - | - |\n",
                    t.suite
                ));
            }
        }
        md.push_str(&format!("\nRendered runtime validation: **{:?}**. Compilation and headless tests do not validate GPU rendering.\n\n{}\n", self.runtime_validation, self.coverage_note));
        fs::write(self.dir.join("summary.md"), md)?;
        Ok(())
    }
}
fn escape(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}
pub fn regenerate(dir: &Path) -> Result<()> {
    let mut run: Run = serde_json::from_slice(&fs::read(dir.join("summary.json"))?)?;
    run.dir = dir.into();
    let reports = std::mem::take(&mut run.tests);
    for old in reports {
        let path = crate::cache::safe_relative(Path::new(&old.path))?;
        run.junit(&old.suite, &dir.join(path));
    }
    for artifact in run.required_artifacts.clone() {
        let relative = crate::cache::safe_relative(Path::new(&artifact))?;
        let path = dir.join(relative);
        if !path.is_file() || path.metadata()?.len() == 0 {
            run.error(
                "required-artifact",
                &anyhow::anyhow!("missing or empty artifact {artifact}"),
            );
        }
    }
    run.save()?;
    anyhow::ensure!(
        run.completed && !run.failed(),
        "recorded run is incomplete or has failures or unavailable reports"
    );
    Ok(())
}
fn failure_kind(event: &quick_xml::events::BytesStart<'_>) -> Result<u8> {
    for attribute in event.attributes() {
        let attribute = attribute?;
        if attribute.key.as_ref() == b"type"
            && attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                .contains("timeout")
        {
            return Ok(4);
        }
    }
    Ok(1)
}
/// Parse XML structure rather than formatting. Nested reruns never count as new tests.
pub fn parse_junit(bytes: &[u8]) -> Result<Counts> {
    let mut reader = Reader::from_reader(bytes);
    let mut counts = Counts::default();
    let mut active: Option<u8> = None; // pass, failure, error, skipped
    let mut seen_suite = false;
    let mut declared = 0_u64;
    let mut depth = 0;
    let mut declared_any = false;
    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                depth += 1;
                match e.name().as_ref() {
                    b"testsuite" => {
                        seen_suite = true;
                        for a in e.attributes() {
                            let a = a?;
                            if a.key.as_ref() == b"tests" {
                                declared += a
                                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                                    .parse::<u64>()?;
                                declared_any = true;
                            }
                        }
                    }
                    b"testcase" => {
                        anyhow::ensure!(active.is_none(), "nested testcase");
                        active = Some(0);
                    }
                    b"failure" if active.is_some() => active = Some(failure_kind(&e)?),
                    b"error" if active.is_some() => active = Some(2),
                    b"skipped" if active.is_some() => active = Some(3),
                    _ => {}
                }
            }
            Event::Empty(e) => match e.name().as_ref() {
                b"testcase" => {
                    counts.tests += 1;
                    counts.passed += 1;
                }
                b"failure" if active.is_some() => active = Some(failure_kind(&e)?),
                b"error" if active.is_some() => active = Some(2),
                b"skipped" if active.is_some() => active = Some(3),
                _ => {}
            },
            Event::End(e) => {
                anyhow::ensure!(depth > 0, "unbalanced XML");
                depth -= 1;
                if e.name().as_ref() == b"testcase" {
                    match active
                        .take()
                        .context("testcase closing tag without opening tag")?
                    {
                        0 => counts.passed += 1,
                        1 => counts.failures += 1,
                        2 => counts.errors += 1,
                        4 => {
                            counts.failures += 1;
                            counts.timeouts += 1;
                        }
                        _ => counts.skipped += 1,
                    }
                    counts.tests += 1;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    anyhow::ensure!(depth == 0 && active.is_none(), "truncated XML report");
    anyhow::ensure!(seen_suite && counts.tests > 0, "report has no test results");
    if declared_any && declared != counts.tests {
        bail!(
            "report declares {declared} tests but contains {}",
            counts.tests
        );
    }
    Ok(counts)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_multisuite_cdata_entities_and_statuses() {
        let xml = br#"<testsuites><testsuite tests="3"><testcase name="a&amp;b"/><testcase name="failure"><failure><![CDATA[<panic>]]></failure><rerunFailure>again</rerunFailure></testcase><testcase name="ignored"><skipped/></testcase></testsuite><testsuite tests="1"><testcase name="crash"><error message="a &quot;b&quot;"/></testcase></testsuite></testsuites>"#;
        assert_eq!(
            parse_junit(xml).unwrap(),
            Counts {
                tests: 4,
                passed: 1,
                failures: 1,
                errors: 1,
                skipped: 1,
                timeouts: 0,
            }
        );
    }
    #[test]
    fn malformed_empty_and_incomplete_are_not_success() {
        for xml in [
            b"".as_slice(),
            b"<testsuites/>",
            b"<testsuite><testcase>",
            b"<testsuite tests=\"2\"><testcase/></testsuite>",
            b"<testsuite><testcase></testsuite>",
        ] {
            assert!(parse_junit(xml).is_err());
        }
    }
    #[test]
    fn missing_report_is_unavailable_not_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut run = Run {
            schema: 1,
            completed: true,
            id: "fixture".into(),
            commit: "fixture".into(),
            host: "fixture".into(),
            toolchain: "fixture".into(),
            steps: vec![],
            tests: vec![],
            runtime_validation: Status::Unavailable,
            coverage_note: "".into(),
            configuration: serde_json::Value::Null,
            required_artifacts: vec![],
            root: dir.path().into(),
            dir: dir.path().into(),
        };
        run.junit("missing", &dir.path().join("missing.xml"));
        assert_eq!(run.tests[0].status, Status::Unavailable);
        assert!(run.tests[0].counts.is_none());
        assert!(run.failed());
        run.save().unwrap();
        assert!(regenerate(dir.path()).is_err());
        run.tests.clear();
        run.required_artifacts.push("coverage.json".into());
        fs::write(dir.path().join("coverage.json"), "{}").unwrap();
        run.save().unwrap();
        regenerate(dir.path()).unwrap();
        fs::remove_file(dir.path().join("coverage.json")).unwrap();
        assert!(regenerate(dir.path()).is_err());
    }
}
