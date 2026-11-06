//! Fetch one capability on demand. No dotenv file or persistent secret export.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{path::Path, process::Command};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    domain: String,
    build_project_id: String,
    infra_project_id: String,
    release_project_id: String,
    environment: String,
    app_path: String,
    analysis_path: String,
    cache_path: String,
}

pub fn read(root: &Path, name: &str) -> Result<String> {
    if let Ok(value) = std::env::var(name)
        && !value.trim().is_empty()
    {
        return validate(name, value);
    }
    let config: Config = serde_json::from_str(include_str!("../../infisical-config.json"))?;
    let (project, path) = match name {
        "INFRA_AWS_ACCESS_KEY_ID" | "INFRA_AWS_SECRET_ACCESS_KEY" => {
            (&config.infra_project_id, "/")
        }
        "AWS_ACCESS_KEY_ID" | "AWS_SECRET_ACCESS_KEY" => {
            (&config.build_project_id, config.cache_path.as_str())
        }
        "SENTRY_DSN" | "SENTRY_ORG" | "SENTRY_PROJECT" => {
            (&config.build_project_id, config.app_path.as_str())
        }
        "SONAR_TOKEN" => (&config.build_project_id, config.analysis_path.as_str()),
        // Publication stays privileged, separate from collaborator builds.
        "SENTRY_AUTH_TOKEN" => (&config.release_project_id, "/"),
        _ => anyhow::bail!("unsupported credential {name}"),
    };
    let secret_name = name.strip_prefix("INFRA_").unwrap_or(name);
    let fetch = |project: &str, path: &str| {
        Command::new("infisical")
        .current_dir(root)
        .args(["sm", "secrets", "get", secret_name, "--plain", "--silent",
            "--domain", &config.domain, "--projectId", project,
            "--env", &config.environment, "--path", path,
            "--include-imports=false", "--secret-overriding=false", "--expand=false"])
        .env("INFISICAL_DISABLE_TELEMETRY", "1")
        .output()
        .context("install Infisical CLI 0.45.2 or later and run infisical login --domain https://eu.infisical.com")
    };
    let output = fetch(project, path)?;
    // Preserve existing maintainer workflows without sharing admin credentials.
    // Build-only users/machines are denied this separate project by Infisical.
    let output = if !output.status.success()
        && matches!(
            name,
            "SONAR_TOKEN" | "AWS_ACCESS_KEY_ID" | "AWS_SECRET_ACCESS_KEY"
        ) {
        fetch(&config.infra_project_id, "/")?
    } else {
        output
    };
    // CLI stderr can contain sensitive API responses: never forward it.
    ensure!(
        output.status.success(),
        "Infisical access unavailable for {name}; login and check project permissions (Sonar upload requires a dedicated analysis token)"
    );
    let value =
        String::from_utf8(output.stdout).context("Infisical returned invalid text (redacted)")?;
    validate(name, value.trim_end_matches(['\r', '\n']).to_owned())
}

fn validate(name: &str, value: String) -> Result<String> {
    ensure!(
        !value.is_empty() && !value.chars().any(char::is_control),
        "invalid {name} (redacted)"
    );
    Ok(value)
}
