//! Public compiler-cache settings, exported from Pulumi without runtime CLI calls.
use std::{collections::BTreeMap, fs, path::Path};

pub const SNAPSHOT: &str = ".compiler-cache/public.env";
const PUBLIC_KEYS: &[&str] = &[
    "SCCACHE_BUCKET",
    "SCCACHE_REGION",
    "SCCACHE_ENDPOINT",
    "SCCACHE_S3_USE_SSL",
    "SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE",
    "SCCACHE_S3_KEY_PREFIX",
    "SCCACHE_S3_RW_MODE",
    "AWS_EC2_METADATA_DISABLED",
];

pub fn load(root: &Path) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    // Bootstrap fallback for new clones without Pulumi credentials. A local
    // snapshot overrides these public defaults; no secret is stored here.
    let mut config: BTreeMap<String, String> = [
        ("SCCACHE_BUCKET", "yummy-blagg"),
        ("SCCACHE_REGION", "eu-west-par"),
        (
            "SCCACHE_ENDPOINT",
            "https://s3.eu-west-par.io.cloud.ovh.net",
        ),
        ("SCCACHE_S3_USE_SSL", "true"),
        ("SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE", "true"),
        ("SCCACHE_S3_KEY_PREFIX", "bevy-parkour2/v1/"),
        ("SCCACHE_S3_RW_MODE", "READ_WRITE"),
        ("AWS_EC2_METADATA_DISABLED", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    match fs::read_to_string(root.join(SNAPSHOT)) {
        Ok(text) => config.extend(parse(&text)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(config)
}
fn parse(text: &str) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut config = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("invalid public cache assignment on line {}", i + 1))?;
        if !PUBLIC_KEYS.contains(&key) || value.chars().any(char::is_control) {
            return Err(format!("unexpected public cache setting on line {}", i + 1).into());
        }
        config.insert(key.into(), value.into());
    }
    Ok(config)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_allows_only_public_values_without_echoing_credentials() {
        let config = parse(
            "# Pulumi export\nSCCACHE_BUCKET=another-bucket\nSCCACHE_S3_KEY_PREFIX=project/v2/\n",
        )
        .unwrap();
        assert_eq!(config["SCCACHE_BUCKET"], "another-bucket");
        let error = parse("AWS_SECRET_ACCESS_KEY=private-value")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "unexpected public cache setting on line 1");
        assert!(!error.contains("private-value"));
        assert!(
            parse("SCCACHE_BUCKET=ok\nmalformed-private-value")
                .unwrap_err()
                .to_string()
                .ends_with("line 2")
        );
    }
}
