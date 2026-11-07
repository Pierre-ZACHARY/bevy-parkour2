//! CI credentials are encrypted Pulumi outputs, scoped to GitHub environments.
use crate::{
    github::{self, Repositories, json},
    value,
};
use pulumi::{Context, Output, PropertyValue, RegisterRequest, Resource, ResourceOptions};

fn secret(
    ctx: &Context,
    repos: &Repositories,
    environment: &Resource,
    name: &str,
    value: Output<PropertyValue>,
) {
    github::resource(
        ctx,
        &repos.provider,
        "actionsEnvironmentSecret:ActionsEnvironmentSecret",
        name,
        vec![
            ("repository".into(), repos.source.output("name")),
            ("environment".into(), environment.output("environment")),
            ("secretName".into(), crate::value(name)),
            ("value".into(), value.as_secret()),
        ],
    );
}
pub fn register(
    ctx: &Context,
    repos: &Repositories,
    infrastructure: &Resource,
    key: &Resource,
    cache_writer: Option<&crate::cache_access::Writer>,
) -> pulumi::Result<()> {
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:pulumiservice".into(),
        name: "pulumi-cloud".into(),
        custom: true,
        version: "1.7.0".into(),
        ..Default::default()
    });
    let token = ctx.register_resource(RegisterRequest {
        type_: "pulumiservice:index:AccessToken".into(),
        name: "github-infrastructure-v2".into(),
        custom: true,
        version: "1.7.0".into(),
        inputs: vec![
            (
                "description".into(),
                value("bevy-parkour2 GitHub infrastructure runner; managed by Pulumi"),
            ),
            (
                "expires".into(),
                value(
                    ctx.config()
                        .get("githubPulumiTokenExpires")
                        .unwrap_or_else(|| "2027-02-06T00:00:00Z".into()),
                ),
            ),
        ],
        options: ResourceOptions {
            provider: Some(provider),
            protect: Some(true),
            // Untyped Rust registration must mark generated credentials itself.
            additional_secret_outputs: vec!["value".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    secret(
        ctx,
        repos,
        infrastructure,
        "PULUMI_ACCESS_TOKEN",
        token.output("value"),
    );
    let github_token = std::env::var("GITHUB_TOKEN").map_err(|_| {
        pulumi::Error::new(
            "set GITHUB_TOKEN for GitHub provisioning; it is never loaded from Infisical",
        )
    })?;
    secret(
        ctx,
        repos,
        infrastructure,
        "INFRA_GH_TOKEN",
        Output::secret(PropertyValue::String(github_token)),
    );
    // The assets key belongs only to main/tag environments, never PRs/branches.
    let build = github::resource(
        ctx,
        &repos.provider,
        "repositoryEnvironment:RepositoryEnvironment",
        "build-environment",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("environment".into(), value("build")),
            (
                "deploymentBranchPolicy".into(),
                json(serde_json::json!({"protectedBranches":false,"customBranchPolicies":true})),
            ),
        ],
    );
    github::resource(
        ctx,
        &repos.provider,
        "repositoryEnvironmentDeploymentPolicy:RepositoryEnvironmentDeploymentPolicy",
        "build-main-only",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("environment".into(), build.output("environment")),
            ("branchPattern".into(), value("main")),
        ],
    );
    let release = github::resource(
        ctx,
        &repos.provider,
        "repositoryEnvironment:RepositoryEnvironment",
        "release-environment",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("environment".into(), value("release")),
            (
                "deploymentBranchPolicy".into(),
                json(serde_json::json!({"protectedBranches":false,"customBranchPolicies":true})),
            ),
        ],
    );
    github::resource(
        ctx,
        &repos.provider,
        "repositoryEnvironmentDeploymentPolicy:RepositoryEnvironmentDeploymentPolicy",
        "release-tags-only",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("environment".into(), release.output("environment")),
            ("tagPattern".into(), value("v*")),
        ],
    );
    let sentry = std::env::var("SENTRY_RELEASE_TOKEN")
        .map_err(|_| pulumi::Error::new("missing private SENTRY_RELEASE_TOKEN"))?;
    secret(
        ctx,
        repos,
        &release,
        "SENTRY_AUTH_TOKEN",
        Output::secret(PropertyValue::String(sentry)),
    );
    for (name, environment) in [("build", &build), ("release", &release)] {
        github::resource(
            ctx,
            &repos.provider,
            "actionsEnvironmentSecret:ActionsEnvironmentSecret",
            &format!("{name}-assets-reader"),
            vec![
                ("repository".into(), repos.source.output("name")),
                ("environment".into(), environment.output("environment")),
                ("secretName".into(), value("ASSETS_DEPLOY_KEY")),
                ("value".into(), key.output("privateKeyOpenssh").as_secret()),
            ],
        );
    }
    if let Some(writer) = cache_writer {
        // Same main/tag (and owner-authorized branch) boundary as the assets
        // key. Collaborators keep the Infisical read-only cache credentials.
        let signing = std::env::var("NIX_CACHE_SIGNING_KEY").map_err(|_| {
            pulumi::Error::new("missing NIX_CACHE_SIGNING_KEY from Infisical infrastructure")
        })?;
        let public: String = include_str!("../../nix/cache-public-key").trim().into();
        let key_name = public.split(':').next().unwrap_or_default();
        if !signing.starts_with(&format!("{key_name}:")) || signing.chars().any(char::is_control) {
            return Err(pulumi::Error::new(
                "NIX_CACHE_SIGNING_KEY does not belong to nix/cache-public-key (redacted)",
            ));
        }
        for (name, environment) in [("build", &build), ("release", &release)] {
            for (suffix, secret_name, value) in [
                (
                    "cache-writer-access",
                    "OVH_CACHE_WRITER_ACCESS_KEY_ID",
                    writer.credentials.output("accessKeyId"),
                ),
                (
                    "cache-writer-secret",
                    "OVH_CACHE_WRITER_SECRET_ACCESS_KEY",
                    writer.credentials.output("secretAccessKey"),
                ),
                (
                    "nix-cache-signing",
                    "NIX_CACHE_SIGNING_KEY",
                    Output::secret(PropertyValue::String(signing.clone())),
                ),
            ] {
                github::resource(
                    ctx,
                    &repos.provider,
                    "actionsEnvironmentSecret:ActionsEnvironmentSecret",
                    &format!("{name}-{suffix}"),
                    vec![
                        ("repository".into(), repos.source.output("name")),
                        ("environment".into(), environment.output("environment")),
                        ("secretName".into(), crate::value(secret_name)),
                        ("value".into(), value.as_secret()),
                    ],
                );
            }
        }
        ctx.export(
            "cacheWriterPolicyId",
            writer.policy.id().cast::<PropertyValue>(),
        );
    }
    Ok(())
}
