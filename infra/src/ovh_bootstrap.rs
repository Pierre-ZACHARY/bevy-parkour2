//! Owner-only bootstrap. Runners never receive the classic, IP-restricted key.
use crate::{OVH_VERSION, infisical, ovh_package, value};
use pulumi::{Context, Output, PropertyValue, RegisterRequest, ResourceOptions};

pub fn register(ctx: &Context) -> pulumi::Result<()> {
    let config = ctx.config();
    let project = config
        .get("projectId")
        .ok_or_else(|| pulumi::Error::new("set projectId in the ignored bootstrap stack config"))?;
    let mut inputs = vec![(
        "endpoint".into(),
        value(config.get("ovhEndpoint").unwrap_or_else(|| "ovh-eu".into())),
    )];
    for (key, variable) in [
        ("applicationKey", "OVH_APPLICATION_KEY"),
        ("applicationSecret", "OVH_APPLICATION_SECRET"),
        ("consumerKey", "OVH_CONSUMER_KEY"),
    ] {
        let secret = std::env::var(variable).map_err(|_| {
            pulumi::Error::new(format!("missing {variable}; use pulumi.ps1 -OvhBootstrap"))
        })?;
        inputs.push((key.into(), Output::secret(PropertyValue::String(secret))));
    }
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:ovh".into(),
        name: "ovh-owner".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs,
        package: Some(ovh_package()),
        ..Default::default()
    });
    let client = ctx.register_resource(RegisterRequest {
        type_: "ovh:index/meApiOauth2Client:MeApiOauth2Client".into(),
        name: "github-infrastructure".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs: vec![
            ("name".into(), value("bevy-parkour2-github-infrastructure")),
            ("description".into(), value("Pulumi infrastructure runners: existing compiler cache and S3 credentials only")),
            ("flow".into(), value("CLIENT_CREDENTIALS")),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            additional_secret_outputs: vec!["clientSecret".into()],
            ..Default::default()
        },
        package: Some(ovh_package()),
        ..Default::default()
    });
    // These are the IAM action names exposed by OVH's /1.0/cloud.json.
    // User creation/role changes would allow project-admin escalation. Keep
    // them, account/IAM administration, compute and object deletion excluded.
    let allows = [
        "region/storage/get",
        "region/storage/edit",
        "user/get",
        "user/openrc/get",
        "user/s3Credentials/get",
        "user/s3Credentials/create",
        "user/s3Credentials/delete",
        "user/s3Credentials/secret/display",
        "user/policy/get",
        "user/policy/create",
    ];
    let policy = ctx.register_resource(RegisterRequest {
        type_: "ovh:index/iamPolicy:IamPolicy".into(),
        name: "github-compiler-cache".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs: vec![
            ("name".into(), value("bevy-parkour2-github-compiler-cache")),
            ("description".into(), value("Storage administration in one Public Cloud project; no account, IAM, compute or user-role administration")),
            ("identities".into(), client.output("identity").map(|identity| PropertyValue::Array(vec![identity]))),
            ("resources".into(), Output::known(PropertyValue::Array(vec![PropertyValue::String(format!("urn:v1:eu:resource:publicCloudProject:{project}"))]))),
            ("allows".into(), Output::known(PropertyValue::Array(allows.iter().map(|action| PropertyValue::String(format!("publicCloudProject:apiovh:{action}"))).collect()))),
        ],
        options: ResourceOptions {
            provider: Some(provider),
            protect: Some(true),
            ..Default::default()
        },
        package: Some(ovh_package()),
        ..Default::default()
    });
    let secrets_provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:infisical".into(),
        name: "infisical".into(),
        custom: true,
        version: "0.19.36".into(),
        inputs: vec![("host".into(), value("https://eu.infisical.com"))],
        package: Some(infisical::package()),
        ..Default::default()
    });
    let metadata: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/infisical-config.json"))
            .map_err(|_| pulumi::Error::new("invalid public Infisical configuration"))?;
    let infra_project = metadata["infraProjectId"]
        .as_str()
        .ok_or_else(|| pulumi::Error::new("missing infrastructure project ID"))?;
    for (name, output) in [
        ("OVH_CLIENT_ID", "clientId"),
        ("OVH_CLIENT_SECRET", "clientSecret"),
    ] {
        ctx.register_resource(RegisterRequest {
            type_: "infisical:index/secret:Secret".into(),
            name: name.to_ascii_lowercase().replace('_', "-"),
            custom: true,
            version: "0.19.36".into(),
            inputs: vec![
                ("workspaceId".into(), value(infra_project)),
                ("envSlug".into(), value("dev")),
                ("folderPath".into(), value("/")),
                ("name".into(), value(name)),
                ("value".into(), client.output(output).as_secret()),
            ],
            options: ResourceOptions {
                provider: Some(secrets_provider.clone()),
                protect: Some(true),
                depends_on: vec![policy.clone()],
                ..Default::default()
            },
            package: Some(infisical::package()),
            ..Default::default()
        });
    }
    ctx.export("ovhRunnerIdentity", client.output("identity"));
    ctx.export("ovhRunnerPolicyId", policy.id().cast::<PropertyValue>());
    Ok(())
}
