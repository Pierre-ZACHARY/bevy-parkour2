use crate::report::{Run, Status, Step};
use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("launch {:?}", command.get_program()))?;
    anyhow::ensure!(
        result.status.success(),
        "{}: {}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?)
}
pub fn execute(command: &mut Command, log: &Path, timeout: Duration) -> Result<Step> {
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(log)?;
    command
        .stdout(Stdio::from(file.try_clone()?))
        .stderr(Stdio::from(file));
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let start = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("launch {:?}", command.get_program()))?;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Step {
                name: String::new(),
                status: if status.success() {
                    Status::Pass
                } else {
                    Status::Fail
                },
                seconds: start.elapsed().as_secs_f64(),
                exit_code: status.code(),
                log: None,
                detail: if status.success() {
                    None
                } else {
                    Some(format!("process exited with {status}"))
                },
            });
        }
        if start.elapsed() >= timeout {
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Step {
                name: String::new(),
                status: Status::Timeout,
                seconds: start.elapsed().as_secs_f64(),
                exit_code: None,
                log: None,
                detail: Some("process deadline exceeded".into()),
            });
        }
        thread::sleep(Duration::from_millis(50));
    }
}
pub fn run(run: &mut Run, name: &str, command: &mut Command) -> Result<bool> {
    let path = format!(
        "logs/{:03}-{}.log",
        run.steps.len(),
        name.replace(['/', ':', ' '], "-")
    );
    println!("> {name}");
    let mut step = match execute(command, &run.dir.join(&path), Duration::from_secs(7200)) {
        Ok(step) => step,
        Err(err) => Step {
            name: String::new(),
            status: Status::Fail,
            seconds: 0.,
            exit_code: None,
            log: None,
            detail: Some(format!("{err:#}")),
        },
    };
    step.name = name.into();
    step.log = Some(path);
    let passed = step.status == Status::Pass;
    run.steps.push(step);
    run.save()?;
    Ok(passed)
}
#[cfg(test)]
mod tests {
    use super::*;
    // Use the Rust test executable as a portable fixture process; no shell scripts.
    fn fixture(name: &str) -> Command {
        let mut c = Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", name, "--nocapture"])
            .env("XTASK_FIXTURE_CHILD", "1");
        c
    }
    #[test]
    fn child_failure() {
        if std::env::var_os("XTASK_FIXTURE_CHILD").is_some() {
            panic!("deliberate fixture failure");
        }
    }
    #[test]
    fn child_timeout() {
        if std::env::var_os("XTASK_FIXTURE_CHILD").is_some() {
            thread::sleep(Duration::from_secs(30));
        }
    }
    #[test]
    fn child_crash() {
        if std::env::var_os("XTASK_FIXTURE_CHILD").is_some() {
            std::process::exit(7);
        }
    }
    #[test]
    fn failures_and_deadlines_preserve_logs() {
        let d = tempfile::tempdir().unwrap();
        for name in [
            "process::tests::child_failure",
            "process::tests::child_crash",
        ] {
            let log = d.path().join(name.replace(':', "_"));
            let step = execute(&mut fixture(name), &log, Duration::from_secs(10)).unwrap();
            assert_eq!(step.status, Status::Fail);
            assert!(log.exists());
        }
        let step = execute(
            &mut fixture("process::tests::child_timeout"),
            &d.path().join("timeout"),
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(step.status, Status::Timeout);
    }
}
