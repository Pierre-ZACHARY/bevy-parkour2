use pulumi::{Context, Output, PackageDescriptor, PropertyValue, RegisterRequest, ResourceOptions};

const VERSION: &str = "0.15.8";

fn package() -> PackageDescriptor {
    PackageDescriptor {
        base_name: "terraform-provider".into(),
        base_version: "1.1.0".into(),
        name: "sentry".into(),
        version: VERSION.into(),
        base64_parameter: "eyJyZW1vdGUiOnsidXJsIjoicmVnaXN0cnkub3BlbnRvZnUub3JnL2ppYW55dWFuL3NlbnRyeSIsInZlcnNpb24iOiIwLjE1LjgifX0=".into(),
        ..Default::default()
    }
}

fn value(s: impl Into<String>) -> Output<PropertyValue> {
    Output::known(PropertyValue::String(s.into()))
}

pub fn register(ctx: &Context) -> pulumi::Result<()> {
    let config = ctx.config();
    let organization = config.get("sentryOrganization").ok_or_else(|| {
        pulumi::Error::new("set sentryOrganization in the ignored stack configuration")
    })?;
    let team = config
        .get("sentryTeam")
        .ok_or_else(|| pulumi::Error::new("set sentryTeam in the ignored stack configuration"))?;
    let slug = config
        .get("sentryProject")
        .unwrap_or_else(|| "bevy-parkour2".into());
    let token = std::env::var("SENTRY_AUTH_TOKEN")
        .map_err(|_| pulumi::Error::new("missing SENTRY_AUTH_TOKEN; use infra/pulumi.ps1"))?;
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:sentry".into(),
        name: "sentry".into(),
        custom: true,
        version: VERSION.into(),
        inputs: vec![
            ("token".into(), Output::secret(PropertyValue::String(token))),
            (
                "baseUrl".into(),
                value(
                    config
                        .get("sentryBaseUrl")
                        .unwrap_or_else(|| "https://sentry.io/api/".into()),
                ),
            ),
        ],
        package: Some(package()),
        ..Default::default()
    });
    let project = ctx.register_resource(RegisterRequest {
        type_: "sentry:index/project:Project".into(),
        name: "game-errors".into(),
        custom: true,
        version: VERSION.into(),
        inputs: vec![
            ("organization".into(), value(&organization)),
            ("name".into(), value(&slug)),
            ("slug".into(), value(&slug)),
            ("platform".into(), value("rust")),
            (
                "teams".into(),
                Output::known(PropertyValue::Array(vec![PropertyValue::String(team)])),
            ),
            // Manage exactly one ingestion key below. Disable automatic alerts;
            // collecting game reports must not implicitly subscribe the team.
            (
                "defaultKey".into(),
                Output::known(PropertyValue::Bool(false)),
            ),
            (
                "defaultRules".into(),
                Output::known(PropertyValue::Bool(false)),
            ),
            (
                "digestsMinDelay".into(),
                Output::known(PropertyValue::Number(300.)),
            ),
            (
                "digestsMaxDelay".into(),
                Output::known(PropertyValue::Number(1800.)),
            ),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            ..Default::default()
        },
        package: Some(package()),
        required: &["slug"],
        ..Default::default()
    });
    let key = ctx.register_resource(RegisterRequest {
        type_: "sentry:index/key:Key".into(),
        name: "official-game-client".into(),
        custom: true,
        version: VERSION.into(),
        inputs: vec![
            ("name".into(), value("Official game builds")),
            ("organization".into(), value(&organization)),
            ("project".into(), project.output("slug")),
        ],
        options: ResourceOptions {
            provider: Some(provider),
            protect: Some(true),
            ..Default::default()
        },
        package: Some(package()),
        ..Default::default()
    });
    ctx.export("sentryOrganization", value(organization));
    ctx.export("sentryProject", project.output("slug"));
    // Export only the public ingestion DSN, never the legacy secret DSN or
    // administrative token. Keep it encrypted/masked in stack output as well.
    ctx.export(
        "sentryDsnPublic",
        key.output("dsn").index("public").as_secret(),
    );
    ctx.export("sentryClientKey", key.output("public").as_secret());
    Ok(())
}
