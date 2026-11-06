use pulumi::{Context, Output, PropertyValue};
use std::collections::BTreeMap;

/// Only public settings go into this output. Credentials stay in Infisical/provider secrets.
pub fn export(ctx: &Context, bucket: &str) -> pulumi::Result<()> {
    let config = ctx.config();
    let mut values = BTreeMap::from([(
        "SCCACHE_BUCKET".into(),
        PropertyValue::String(bucket.into()),
    )]);
    for (variable, key) in [
        ("SCCACHE_REGION", "cacheRegion"),
        ("SCCACHE_ENDPOINT", "cacheEndpoint"),
        ("SCCACHE_S3_USE_SSL", "cacheUseSsl"),
        (
            "SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE",
            "cacheVirtualHostStyle",
        ),
        ("SCCACHE_S3_KEY_PREFIX", "cachePrefix"),
        ("SCCACHE_S3_RW_MODE", "cacheRwMode"),
        ("AWS_EC2_METADATA_DISABLED", "cacheDisableEc2Metadata"),
    ] {
        let text = config.get(key).ok_or_else(|| {
            pulumi::Error::new(format!("set {key} in the ignored stack configuration"))
        })?;
        if text.chars().any(char::is_control) {
            return Err(pulumi::Error::new(format!("invalid public setting {key}")));
        }
        values.insert(variable.into(), PropertyValue::String(text.clone()));
        match key {
            "cacheEndpoint" => ctx.export(
                "sccacheEndpoint",
                Output::known(PropertyValue::String(text)),
            ),
            "cacheRegion" => {
                ctx.export("sccacheRegion", Output::known(PropertyValue::String(text)))
            }
            "cachePrefix" => ctx.export("cachePrefix", Output::known(PropertyValue::String(text))),
            _ => {}
        }
    }
    ctx.export(
        "compilerCache",
        Output::known(PropertyValue::Object(values)),
    );
    Ok(())
}
