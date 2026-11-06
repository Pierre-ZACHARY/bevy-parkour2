use pulumi::{Context, Output, PackageDescriptor, PropertyValue, RegisterRequest, ResourceOptions};
fn value(s: impl Into<String>) -> Output<PropertyValue> {
    Output::known(PropertyValue::String(s.into()))
}
fn package() -> PackageDescriptor {
    PackageDescriptor {
        base_name: "terraform-provider".into(), base_version: "1.1.0".into(),
        name: "sonarcloud".into(), version: "0.5.2".into(),
        base64_parameter: "eyJyZW1vdGUiOnsidXJsIjoicmVnaXN0cnkub3BlbnRvZnUub3JnL3Jld2UtZGlnaXRhbC9zb25hcmNsb3VkIiwidmVyc2lvbiI6IjAuNS4yIn19".into(),
        ..Default::default()
    }
}
pub fn register(ctx: &Context) -> pulumi::Result<()> {
    let config = ctx.config();
    let organization = config.get("sonarOrganization").ok_or_else(|| {
        pulumi::Error::new("set sonarOrganization in ignored stack configuration")
    })?;
    let key = config
        .get("sonarProject")
        .unwrap_or_else(|| "Pierre-ZACHARY_bevy-parkour2".into());
    let token = std::env::var("SONAR_TOKEN")
        .map_err(|_| pulumi::Error::new("missing SONAR_TOKEN; use infra/pulumi.ps1"))?;
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:sonarcloud".into(),
        name: "sonarcloud".into(),
        custom: true,
        version: "0.5.2".into(),
        package: Some(package()),
        inputs: vec![
            ("organization".into(), value(&organization)),
            ("token".into(), Output::secret(PropertyValue::String(token))),
        ],
        ..Default::default()
    });
    let project = ctx.register_resource(RegisterRequest {
        type_: "sonarcloud:index/project:Project".into(),
        name: "game-quality".into(),
        custom: true,
        version: "0.5.2".into(),
        package: Some(package()),
        inputs: vec![
            ("name".into(), value("bevy-parkour2")),
            ("key".into(), value(&key)),
            (
                "visibility".into(),
                value(
                    config
                        .get("sonarVisibility")
                        .unwrap_or_else(|| "public".into()),
                ),
            ),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            retain_on_delete: Some(true),
            import_id: config.get("sonarImportId").unwrap_or_default(),
            ..Default::default()
        },
        required: &["key"],
        ..Default::default()
    });
    ctx.register_resource(RegisterRequest {
        type_: "sonarcloud:index/projectMainBranch:ProjectMainBranch".into(),
        name: "game-quality-main".into(),
        custom: true,
        version: "0.5.2".into(),
        package: Some(package()),
        inputs: vec![
            ("name".into(), value("main")),
            ("projectKey".into(), project.output("key")),
        ],
        options: ResourceOptions {
            provider: Some(provider),
            protect: Some(true),
            ..Default::default()
        },
        ..Default::default()
    });
    ctx.export("sonarOrganization", value(organization));
    ctx.export("sonarProjectKey", project.output("key"));
    ctx.export("sonarHostUrl", value("https://sonarcloud.io"));
    Ok(())
}
