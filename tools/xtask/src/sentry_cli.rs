//! Official publishing only. Management credentials never enter a Cargo process.
use crate::{
    cache, credentials,
    model::{BuildArgs, Common, Target},
    process,
    report::Run,
    sentry,
};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
pub struct Publisher {
    binary: PathBuf,
    token: String,
    organization: String,
    project: String,
    release: String,
    environment: String,
}
impl Publisher {
    pub fn new(
        run: &Run,
        opts: &Common,
        _args: &BuildArgs,
        client: &sentry::Client,
    ) -> Result<Self> {
        ensure!(
            !opts.offline,
            "official Sentry symbol publication requires network access"
        );
        Ok(Self {
            binary: cache::ensure(
                &opts.cache_dir,
                &cache::spec("sentry-cli", &run.host)?,
                opts.offline,
            )?,
            token: credentials::read(&run.root, "SENTRY_AUTH_TOKEN")?,
            organization: credentials::read(&run.root, "SENTRY_ORG")?,
            project: credentials::read(&run.root, "SENTRY_PROJECT")?,
            release: client.release.clone(),
            environment: client.environment.clone(),
        })
    }
    fn command(&self, root: &Path) -> Command {
        let mut command = Command::new(&self.binary);
        sentry::configure(&mut command, None);
        command
            .current_dir(root)
            .env("SENTRY_AUTH_TOKEN", &self.token)
            .env("SENTRY_ORG", &self.organization)
            .env("SENTRY_PROJECT", &self.project)
            .env("SENTRY_DISABLE_UPDATE_CHECK", "true")
            .env("SENTRY_LOG_LEVEL", "info");
        command
    }
    fn run(&self, run: &mut Run, name: &str, args: &[&str]) -> Result<()> {
        ensure!(
            process::run(run, name, self.command(&run.root).args(args))?,
            "Sentry publication failed; see management log"
        );
        Ok(())
    }
    pub fn begin(&self, run: &mut Run) -> Result<()> {
        self.run(
            run,
            "sentry-release-create",
            &["releases", "new", &self.release],
        )?;
        self.run(
            run,
            "sentry-release-commits",
            &[
                "releases",
                "set-commits",
                &self.release,
                "--auto",
                "--ignore-missing",
                "--ignore-empty",
            ],
        )
    }
    pub fn finalize(&self, run: &mut Run) -> Result<()> {
        self.run(
            run,
            "sentry-release-finalize",
            &["releases", "finalize", &self.release],
        )?;
        self.run(
            run,
            "sentry-release-deploy",
            &[
                "deploys",
                "new",
                "--release",
                &self.release,
                "--env",
                &self.environment,
            ],
        )
    }
    pub fn upload(&self, run: &mut Run, target: Target, built: &Path, stage: &Path) -> Result<()> {
        let mut files = Vec::new();
        match target {
            Target::WindowsX64 => {
                files.push(built.join("bevy_parkour2.exe"));
                files.push(built.join("bevy_parkour2.pdb"));
            }
            Target::MacosArm64 => {
                let dsym = built.join("bevy_parkour2.dSYM");
                ensure!(
                    process::run(
                        run,
                        "macos-symbols",
                        Command::new("dsymutil")
                            .arg(built.join("bevy_parkour2"))
                            .arg("-o")
                            .arg(&dsym)
                    )?,
                    "dsymutil failed"
                );
                files.push(dsym);
            }
            Target::Webgpu => {
                files.push(stage.join("game_bg.wasm"));
            }
            _ => files.push(built.join("bevy_parkour2")),
        }
        ensure!(
            files.iter().all(|p| p.exists()),
            "missing debug information for {}",
            target.name()
        );
        // debug-files check rejects a binary with no usable debug information.
        for file in &files {
            ensure!(
                process::run(
                    run,
                    &format!(
                        "{}-symbols-check-{}",
                        target.name(),
                        files
                            .iter()
                            .position(|p| p == file)
                            .context("symbol index")?
                    ),
                    self.command(&run.root)
                        .args(["debug-files", "check"])
                        .arg(file)
                )?,
                "unusable debug information"
            );
        }
        ensure!(
            process::run(
                run,
                &format!("{}-symbols-upload", target.name()),
                self.command(&run.root)
                    .args(["debug-files", "upload", "--wait"])
                    .args(&files)
            )?,
            "debug symbol upload failed"
        );
        if target == Target::Webgpu {
            ensure!(
                process::run(
                    run,
                    "webgpu-js-debug-ids",
                    self.command(&run.root)
                        .args(["sourcemaps", "inject"])
                        .arg(stage)
                )?,
                "JavaScript debug ID injection failed"
            );
            ensure!(
                process::run(
                    run,
                    "webgpu-js-upload",
                    self.command(&run.root)
                        .args(["sourcemaps", "upload", "--release", &self.release])
                        .arg(stage)
                )?,
                "JavaScript upload failed"
            );
        }
        Ok(())
    }
}
