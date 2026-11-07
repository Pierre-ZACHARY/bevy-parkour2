mod assets;
mod build;
mod cache;
mod credentials;
mod model;
mod pipeline;
mod process;
mod prototype;
mod remote_cache;
mod report;
mod sentry;
mod sentry_cli;
mod sentry_web;
mod sonar;
mod toolchain;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use model::{BuildArgs, Common};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Portable build, test and report tooling", version)]
struct Cli {
    #[command(flatten)]
    common: Common,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Run a command with OVH S3 sccache, or prove a remote cache hit.
    RemoteCache {
        #[command(subcommand)]
        action: remote_cache::Action,
    },
    /// Build or serve the isolated Basis/WebGPU experiment.
    Prototype {
        #[command(subcommand)]
        action: prototype::Action,
    },
    /// Check the final CI job results supplied through XTASK_JOB_RESULTS.
    Gate,
    /// Inspect prerequisites without downloading, installing or building.
    Doctor {
        #[arg(long, value_delimiter = ',')]
        targets: Vec<model::Target>,
    },
    /// Print the build/test plan without executing it.
    Plan {
        #[command(flatten)]
        build: BuildArgs,
    },
    Lint {
        #[arg(long, value_delimiter = ',')]
        suites: Vec<String>,
    },
    Test {
        #[arg(long, value_delimiter = ',')]
        suites: Vec<String>,
    },
    Coverage {
        #[arg(long, value_delimiter = ',')]
        suites: Vec<String>,
    },
    Report {
        #[arg(long)]
        input: PathBuf,
    },
    /// Merge suite coverage and optionally run the pinned SonarCloud scanner.
    Sonar {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        upload: bool,
    },
    /// Finalize an official Sentry release after every packaging runner succeeds.
    SentryFinalize {
        #[arg(long)]
        tag: String,
        #[arg(long)]
        sentry_stack: Option<String>,
    },
    Build {
        #[command(flatten)]
        build: BuildArgs,
    },
    Ci {
        #[command(flatten)]
        build: BuildArgs,
        #[arg(long)]
        coverage: bool,
        /// Publish SonarCloud analysis; implies --coverage.
        #[arg(long)]
        sonar: bool,
    },
}
fn main() {
    if let Err(err) = run() {
        eprintln!("xtask: {err:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let mut cli = Cli::parse();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Keep ordinary Windows paths: LLVM globbing does not accept verbatim \\?\ prefixes.
    let root = manifest
        .parent()
        .and_then(std::path::Path::parent)
        .context("workspace root")?
        .to_path_buf();
    cli.common.resolve(&root);
    if let Action::RemoteCache { action } = &cli.command {
        return remote_cache::run(&root, &cli.common, action);
    }
    if let Action::Report { input } = &cli.command {
        let input = if input.is_absolute() {
            input.clone()
        } else {
            root.join(input)
        };
        return report::regenerate(&input);
    }
    if let Action::Prototype {
        action:
            prototype::Action::Serve {
                dir,
                port,
                evidence_dir,
            },
    } = &cli.command
    {
        return prototype::serve(dir, *port, evidence_dir.as_deref());
    }
    if matches!(cli.command, Action::Gate) {
        return pipeline::gate(&std::env::var("XTASK_JOB_RESULTS")?);
    }
    let host = model::host()?;
    match cli.command {
        Action::Doctor { targets } => pipeline::doctor(&root, &cli.common, &host, &targets),
        Action::Plan { mut build } => {
            build.resolve(&host)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "host": host, "build": build, "suites": model::SUITES,
                    "sdk_policy": {"dlss": "not integrated", "ray_tracing": "not integrated",
                        "sentry": "official tags only; sentryDsnPublic from Pulumi; native Rust and browser SDKs"},
                    "steps": ["prerequisites", "lint", "nextest + doctests", "reports", "build", "package", "verify"],
                    "runtime_validation": "requires a GPU/browser; never inferred from compilation"
                }))?
            );
            Ok(())
        }
        action => {
            let mut run = report::Run::new(&root, &cli.common, &host)?;
            let result = (|| -> Result<()> {
                match action {
                    Action::Prototype {
                        action: prototype::Action::Basisu,
                    } => prototype::basisu(&mut run, &cli.common),
                    Action::Lint { suites } => {
                        pipeline::lint(&mut run, &cli.common, &model::suites(&suites)?)
                    }
                    Action::Test { suites } => {
                        pipeline::tests(&mut run, &cli.common, &model::suites(&suites)?, false)
                    }
                    Action::Coverage { suites } => {
                        pipeline::tests(&mut run, &cli.common, &model::suites(&suites)?, true)
                    }
                    Action::Build { mut build } => {
                        build.resolve(&host)?;
                        build::build_all(&mut run, &cli.common, &build)
                    }
                    Action::Sonar { input, upload } => {
                        let input = if input.is_absolute() {
                            input
                        } else {
                            root.join(input)
                        };
                        sonar::analyze(&mut run, &cli.common, &input, upload)
                    }
                    Action::SentryFinalize { tag, sentry_stack } => {
                        build::verify_tag(&root, &tag)?;
                        let mut args = BuildArgs {
                            targets: vec![],
                            profile: model::Profile::Release,
                            channel: model::Channel::Official,
                            tag: Some(tag),
                            sentry_stack,
                            asset_quality: "original".into(),
                            asset_presets: None,
                        };
                        args.resolve(&host)?;
                        let client = sentry::resolve(&root, &cli.common, &args)?
                            .context("official Sentry configuration")?;
                        sentry_cli::Publisher::new(&run, &cli.common, &args, &client)?
                            .finalize(&mut run)
                    }
                    Action::Ci {
                        mut build,
                        coverage,
                        sonar,
                    } => {
                        build.resolve(&host)?;
                        // All phases get a chance to produce diagnostics. No phase hides earlier failures.
                        let lint = pipeline::lint(&mut run, &cli.common, model::SUITES);
                        if let Err(e) = lint {
                            run.error("lint", &e);
                        }
                        let _ = coverage; // accepted for compatibility; CI always collects coverage.
                        let tests = pipeline::tests(&mut run, &cli.common, model::SUITES, true);
                        if let Err(e) = tests {
                            run.error("tests", &e);
                        }
                        {
                            run.save()?;
                            let input = run.dir.clone();
                            if let Err(e) = sonar::analyze(&mut run, &cli.common, &input, sonar) {
                                run.error("sonar", &e);
                            }
                        }
                        build::build_all(&mut run, &cli.common, &build)
                    }
                    _ => unreachable!(),
                }
            })();
            if let Err(e) = &result {
                run.error("pipeline", e);
            }
            run.completed = true;
            run.save()?;
            println!("Reports: {}", run.dir.display());
            if run.failed() {
                anyhow::bail!("one or more required stages failed; see summary.md");
            }
            result
        }
    }
}
