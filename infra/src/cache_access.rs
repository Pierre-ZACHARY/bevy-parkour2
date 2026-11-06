//! Collaborators can read compiled artifacts, never administer the bucket.
use crate::{OVH_VERSION, ovh_package, value};
use pulumi::{Context, Output, PropertyValue, RegisterRequest, Resource, ResourceOptions};

pub struct Reader {
    pub credentials: Resource,
    pub policy: Resource,
}

pub fn register(ctx: &Context, provider: &Resource, project: &str, bucket: &str) -> Reader {
    let user = ctx.register_resource(RegisterRequest {
        type_: "ovh:index/cloudProjectUser:CloudProjectUser".into(),
        name: "cache-collaborators".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs: vec![
            ("serviceName".into(), value(project)),
            (
                "description".into(),
                value("bevy-parkour2 cache readers (Infisical)"),
            ),
            (
                "roleNames".into(),
                Output::known(PropertyValue::Array(vec![PropertyValue::String(
                    "objectstore_operator".into(),
                )])),
            ),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            additional_secret_outputs: vec!["password".into(), "openstackRc".into()],
            ..Default::default()
        },
        package: Some(ovh_package()),
        ..Default::default()
    });
    let credentials = ctx.register_resource(RegisterRequest {
        type_: "ovh:index/cloudProjectUserS3Credential:CloudProjectUserS3Credential".into(),
        name: "cache-collaborators".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs: vec![
            ("serviceName".into(), value(project)),
            ("userId".into(), user.id().cast::<PropertyValue>()),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            additional_secret_outputs: vec!["accessKeyId".into(), "secretAccessKey".into()],
            ..Default::default()
        },
        package: Some(ovh_package()),
        ..Default::default()
    });
    let prefix = ctx
        .config()
        .get("cachePrefix")
        .unwrap_or_else(|| "bevy-parkour2/v1/".into());
    // OVH removes the optional Version field on read. Match its canonical form
    // so every preview does not propose reapplying the same policy.
    let policy = serde_json::json!({"Statement":[{
        "Sid":"ReadCompilerCacheOnly", "Effect":"Allow", "Action":["s3:GetObject"],
        "Resource":[format!("arn:aws:s3:::{bucket}/{prefix}*")]
    }]})
    .to_string();
    let policy = ctx.register_resource(RegisterRequest {
        type_: "ovh:index/cloudProjectUserS3Policy:CloudProjectUserS3Policy".into(),
        name: "cache-collaborators".into(),
        custom: true,
        version: OVH_VERSION.into(),
        inputs: vec![
            ("serviceName".into(), value(project)),
            ("userId".into(), user.id().cast::<PropertyValue>()),
            ("policy".into(), value(policy)),
        ],
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            depends_on: vec![credentials.clone()],
            ..Default::default()
        },
        package: Some(ovh_package()),
        ..Default::default()
    });
    ctx.export(
        "cacheReaderAccessKey",
        credentials.output("accessKeyId").as_secret(),
    );
    ctx.export(
        "cacheReaderSecretKey",
        credentials.output("secretAccessKey").as_secret(),
    );
    ctx.export("cacheReaderUserId", user.id().cast::<PropertyValue>());
    Reader {
        credentials,
        policy,
    }
}
