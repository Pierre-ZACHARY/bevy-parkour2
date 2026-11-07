use pulumi::{Output, PackageDescriptor, PropertyValue, RegisterRequest, ResourceOptions};

mod cache_access;
mod cache_config;
mod github;
mod github_ci;
mod github_credentials;
mod infisical;
mod ovh_bootstrap;
mod sentry;
mod sonar;

const OVH_VERSION: &str = "2.24.0";

// This is the parameterized Terraform OVH provider, not a separately maintained
// provider fork. Tokens and parameterization were checked against its schema.
fn ovh_package() -> PackageDescriptor {
    PackageDescriptor {
        base_name: "terraform-provider".into(),
        base_version: "1.1.0".into(),
        name: "ovh".into(),
        version: OVH_VERSION.into(),
        base64_parameter: "eyJyZW1vdGUiOnsidXJsIjoicmVnaXN0cnkub3BlbnRvZnUub3JnL292aC9vdmgiLCJ2ZXJzaW9uIjoiMi4yNC4wIn19".into(),
        ..Default::default()
    }
}

fn value(text: impl Into<String>) -> Output<PropertyValue> {
    Output::known(PropertyValue::String(text.into()))
}

fn main() {
    pulumi::run(|ctx| async move {
        let config = ctx.config();
        if ctx.project() == "bevy-parkour2-ovh-bootstrap" {
            return ovh_bootstrap::register(&ctx);
        }
        let project = config.get("projectId").ok_or_else(|| {
            pulumi::Error::new("set projectId in the ignored stack configuration")
        })?;
        let bucket_name = config
            .get("bucketName")
            .unwrap_or_else(|| "yummy-blagg".into());
        let region = config.get("region").unwrap_or_else(|| "EU-WEST-PAR".into());

        let mut inputs = vec![(
            "endpoint".into(),
            value(config.get("ovhEndpoint").unwrap_or_else(|| "ovh-eu".into())),
        )];
        for (key, variable) in [
            ("clientId", "OVH_CLIENT_ID"),
            ("clientSecret", "OVH_CLIENT_SECRET"),
        ] {
            let secret = std::env::var(variable).map_err(|_| {
                pulumi::Error::new(format!("missing {variable}; use infra/pulumi.ps1"))
            })?;
            inputs.push((key.into(), Output::secret(PropertyValue::String(secret))));
        }
        let provider = ctx.register_resource(RegisterRequest {
            type_: "pulumi:providers:ovh".into(),
            name: "ovh".into(),
            custom: true,
            version: OVH_VERSION.into(),
            inputs,
            package: Some(ovh_package()),
            ..Default::default()
        });
        let bucket = ctx.register_resource(RegisterRequest {
            // OVH 2.24.0's v2 resource cannot import disabled Object Lock:
            // its read produces an invalid empty lock that fails validation.
            // Keep the supported v1 resource until that upstream bug is fixed.
            type_: "ovh:index/cloudProjectStorage:CloudProjectStorage".into(),
            name: "compilation-cache".into(),
            custom: true,
            version: OVH_VERSION.into(),
            inputs: vec![
                ("serviceName".into(), value(&project)),
                ("name".into(), value(&bucket_name)),
                ("regionName".into(), value(&region)),
                (
                    "hideObjects".into(),
                    Output::known(PropertyValue::Bool(true)),
                ),
            ],
            options: ResourceOptions {
                provider: Some(provider.clone()),
                protect: Some(true),
                retain_on_delete: Some(true),
                // Adopt the live policies and owner. Initial management only
                // changes hideObjects, which controls provider state inventory.
                ignore_changes: vec![
                    "encryption".into(),
                    "objectLock".into(),
                    "ownerId".into(),
                    "versioning".into(),
                ],
                import_id: format!("{project}/{region}/{bucket_name}"),
                ..Default::default()
            },
            package: Some(ovh_package()),
            required: &["regionName"],
            ..Default::default()
        });
        ctx.export("bucketId", bucket.id().cast::<PropertyValue>());
        // Activation requires explicit approval of OVH's coarse project role.
        let cache_reader = if config.get("enableCacheReaders").as_deref() == Some("true") {
            Some(cache_access::register(
                &ctx,
                &provider,
                &project,
                &bucket_name,
            ))
        } else {
            None
        };
        // Set from the bootstrap stack output once its owner has created the user.
        let cache_writer = config.get("cacheWriterUserId").map(|user| {
            cache_access::register_writer(&ctx, &provider, &project, &bucket_name, &user)
        });
        cache_config::export(&ctx, &bucket_name)?;
        ctx.export("bucketName", value(bucket_name));
        sentry::register(&ctx)?;
        sonar::register(&ctx)?;
        let repositories = github::register(&ctx, cache_writer.as_ref())?;
        infisical::register(&ctx, cache_reader.as_ref(), repositories.as_ref())?;
        Ok(())
    });
}
