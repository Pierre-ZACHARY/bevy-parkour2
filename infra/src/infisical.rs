//! Infisical project/secret lifecycle uses its Terraform provider through the bridge.
use crate::value;
use pulumi::{Context, Output, PackageDescriptor, PropertyValue, RegisterRequest, ResourceOptions};

const VERSION: &str = "0.19.36";
pub(crate) fn package() -> PackageDescriptor {
    PackageDescriptor {
        base_name: "terraform-provider".into(), base_version: "1.1.0".into(),
        name: "infisical".into(), version: VERSION.into(),
        base64_parameter: "eyJyZW1vdGUiOnsidXJsIjoicmVnaXN0cnkub3BlbnRvZnUub3JnL2luZmlzaWNhbC9pbmZpc2ljYWwiLCJ2ZXJzaW9uIjoiMC4xOS4zNiJ9fQ==".into(),
        ..Default::default()
    }
}

pub fn register(
    ctx: &Context,
    reader: Option<&crate::cache_access::Reader>,
    repositories: Option<&crate::github::Repositories>,
) -> pulumi::Result<()> {
    // Authentication is process-only (Infisical login locally, OIDC in CI).
    // No login JWT is recorded as a provider input or stack variable.
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:infisical".into(),
        name: "infisical".into(),
        custom: true,
        version: VERSION.into(),
        inputs: vec![("host".into(), value("https://eu.infisical.com"))],
        package: Some(package()),
        ..Default::default()
    });
    let project = ctx.register_resource(RegisterRequest {
        type_: "infisical:index/project:Project".into(), name: "release-secrets".into(),
        custom: true, version: VERSION.into(),
        inputs: vec![
            ("name".into(), value("bevy-parkour2-release")),
            ("slug".into(), value("bevy-parkour2-release")),
            ("description".into(), value("Official release maintainers and runners only; no infrastructure API credentials.")),
            ("type".into(), value("secret-manager")),
            ("hasDeleteProtection".into(), Output::known(PropertyValue::Bool(true))),
            ("shouldCreateDefaultEnvs".into(), Output::known(PropertyValue::Bool(true))),
        ],
        options: ResourceOptions { provider: Some(provider.clone()), protect: Some(true), retain_on_delete: Some(true), ..Default::default() },
        package: Some(package()), ..Default::default()
    });
    ctx.export(
        "infisicalReleaseProjectId",
        project.id().cast::<PropertyValue>(),
    );
    if let Some(reader) = reader {
        let metadata: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/infisical-config.json"))
                .map_err(|_| pulumi::Error::new("invalid public Infisical configuration"))?;
        let build = metadata["buildProjectId"]
            .as_str()
            .ok_or_else(|| pulumi::Error::new("missing build project ID"))?;
        for (logical, name, output) in [
            ("cache-reader-access", "AWS_ACCESS_KEY_ID", "accessKeyId"),
            (
                "cache-reader-secret",
                "AWS_SECRET_ACCESS_KEY",
                "secretAccessKey",
            ),
        ] {
            ctx.register_resource(RegisterRequest {
                type_: "infisical:index/secret:Secret".into(),
                name: logical.into(),
                custom: true,
                version: VERSION.into(),
                inputs: vec![
                    ("workspaceId".into(), value(build)),
                    ("envSlug".into(), value("dev")),
                    ("folderPath".into(), value("/cache")),
                    ("name".into(), value(name)),
                    (
                        "value".into(),
                        reader.credentials.output(output).as_secret(),
                    ),
                ],
                options: ResourceOptions {
                    provider: Some(provider.clone()),
                    protect: Some(true),
                    depends_on: vec![reader.policy.clone()],
                    import_id: format!("{build}:dev:/cache:{name}"),
                    ..Default::default()
                },
                package: Some(package()),
                ..Default::default()
            });
        }
    }
    if ctx.config().get("enableReleaseToken").as_deref() == Some("true") {
        let token = std::env::var("SENTRY_RELEASE_TOKEN").map_err(|_| pulumi::Error::new(
            "upload SENTRY_RELEASE_TOKEN to Infisical infrastructure/dev/root before enabling release token publication"))?;
        if token.trim().is_empty() || token.chars().any(char::is_control) {
            return Err(pulumi::Error::new("invalid release token (redacted)"));
        }
        ctx.register_resource(RegisterRequest {
            type_: "infisical:index/secret:Secret".into(),
            name: "sentry-release-token".into(),
            custom: true,
            version: VERSION.into(),
            inputs: vec![
                ("workspaceId".into(), project.id().cast::<PropertyValue>()),
                ("envSlug".into(), value("dev")),
                ("folderPath".into(), value("/")),
                ("name".into(), value("SENTRY_AUTH_TOKEN")),
                ("value".into(), Output::secret(PropertyValue::String(token))),
            ],
            options: ResourceOptions {
                provider: Some(provider.clone()),
                protect: Some(true),
                ..Default::default()
            },
            package: Some(package()),
            ..Default::default()
        });
    }
    if let Some(repositories) = repositories {
        crate::github_ci::register(ctx, &provider, &project, repositories)?;
    }
    Ok(())
}
