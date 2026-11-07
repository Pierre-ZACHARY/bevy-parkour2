//! Source/asset repositories and CI capabilities. The provisioning PAT is env-only.
use crate::value;
use pulumi::{Context, Output, PropertyValue, RegisterRequest, Resource, ResourceOptions};

const VERSION: &str = "6.15.1";
pub struct Repositories {
    pub source: Resource,
    pub provider: Resource,
    pub owner: String,
    pub name: String,
}
pub fn json(v: serde_json::Value) -> Output<PropertyValue> {
    fn convert(v: serde_json::Value) -> PropertyValue {
        match v {
            serde_json::Value::Null => PropertyValue::Null,
            serde_json::Value::Bool(v) => PropertyValue::Bool(v),
            serde_json::Value::Number(v) => PropertyValue::Number(v.as_f64().unwrap()),
            serde_json::Value::String(v) => PropertyValue::String(v),
            serde_json::Value::Array(v) => {
                PropertyValue::Array(v.into_iter().map(convert).collect())
            }
            serde_json::Value::Object(v) => {
                PropertyValue::Object(v.into_iter().map(|(k, v)| (k, convert(v))).collect())
            }
        }
    }
    Output::known(convert(v))
}
pub fn resource(
    ctx: &Context,
    provider: &Resource,
    kind: &str,
    name: &str,
    inputs: Vec<(String, Output<PropertyValue>)>,
) -> Resource {
    ctx.register_resource(RegisterRequest {
        type_: format!("github:index/{kind}"),
        name: name.into(),
        custom: true,
        version: VERSION.into(),
        inputs,
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            ..Default::default()
        },
        ..Default::default()
    })
}
pub fn variable(ctx: &Context, repos: &Repositories, name: &str, input: Output<PropertyValue>) {
    resource(
        ctx,
        &repos.provider,
        "actionsVariable:ActionsVariable",
        name,
        vec![
            ("repository".into(), repos.source.output("name")),
            ("variableName".into(), value(name)),
            ("value".into(), input),
        ],
    );
}
pub fn register(
    ctx: &Context,
    cache_writer: Option<&crate::cache_access::Writer>,
) -> pulumi::Result<Option<Repositories>> {
    if ctx.config().get("enableGitHub").as_deref() != Some("true") {
        return Ok(None);
    }
    let owner = ctx
        .config()
        .get("githubOwner")
        .unwrap_or_else(|| "Pierre-ZACHARY".into());
    let name = ctx
        .config()
        .get("githubSourceRepository")
        .unwrap_or_else(|| "bevy-parkour2".into());
    let assets_name = ctx
        .config()
        .get("githubAssetsRepository")
        .unwrap_or_else(|| "bevy-parkour2-assets".into());
    let provider = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:github".into(),
        name: "github".into(),
        custom: true,
        version: VERSION.into(),
        inputs: vec![("owner".into(), value(&owner))],
        ..Default::default()
    });
    let mut repositories = Vec::new();
    for (logical, repo, visibility, description) in [
        (
            "game-source",
            &name,
            "public",
            "Bevy parkour game source. Licensed third-party assets require separate private access.",
        ),
        (
            "game-assets",
            &assets_name,
            "private",
            "Restricted third-party game assets and license records. No public redistribution.",
        ),
    ] {
        let repository = resource(
            ctx,
            &provider,
            "repository:Repository",
            logical,
            vec![
                ("name".into(), value(repo)),
                ("visibility".into(), value(visibility)),
                ("description".into(), value(description)),
                ("autoInit".into(), json(true.into())),
                ("archiveOnDestroy".into(), json(true.into())),
                ("deleteBranchOnMerge".into(), json(true.into())),
                ("hasIssues".into(), json((visibility == "public").into())),
                ("hasWiki".into(), json(false.into())),
                ("hasProjects".into(), json(false.into())),
            ],
        );
        let mut permissions = vec![
            ("repository".into(), repository.output("name")),
            ("enabled".into(), json((visibility == "public").into())),
        ];
        if visibility == "public" {
            permissions.push(("allowedActions".into(), value("all")));
        }
        resource(
            ctx,
            &provider,
            "actionsRepositoryPermissions:ActionsRepositoryPermissions",
            &format!("{logical}-actions"),
            permissions,
        );
        let config_key = if visibility == "public" {
            "githubSourceCollaborators"
        } else {
            "githubAssetsCollaborators"
        };
        let users: serde_json::Value =
            serde_json::from_str(&ctx.config().get(config_key).unwrap_or_else(|| "[]".into()))
                .map_err(|_| {
                    pulumi::Error::new(format!("{config_key} must be a JSON array of usernames"))
                })?;
        let users = users
            .as_array()
            .ok_or_else(|| pulumi::Error::new("collaborators must be an array"))?;
        let mut grants = Vec::new();
        for user in users {
            let user = user
                .as_str()
                .ok_or_else(|| pulumi::Error::new("collaborator must be a username"))?;
            if user.eq_ignore_ascii_case(&owner) {
                continue;
            }
            grants.push(serde_json::json!({"username":user, "permission":"push"}));
        }
        // Exclusive access list: removing a configured user revokes their grant.
        resource(
            ctx,
            &provider,
            "repositoryCollaborators:RepositoryCollaborators",
            &format!("{logical}-collaborators"),
            vec![
                ("repository".into(), repository.output("name")),
                ("users".into(), json(grants.into())),
            ],
        );
        repositories.push(repository);
    }
    let assets = repositories.pop().unwrap();
    let source = repositories.pop().unwrap();
    let tls = ctx.register_resource(RegisterRequest {
        type_: "pulumi:providers:tls".into(),
        name: "tls".into(),
        custom: true,
        version: "5.6.3".into(),
        ..Default::default()
    });
    let key = ctx.register_resource(RegisterRequest {
        type_: "tls:index/privateKey:PrivateKey".into(),
        name: "assets-ci-reader".into(),
        custom: true,
        version: "5.6.3".into(),
        inputs: vec![("algorithm".into(), value("ED25519"))],
        options: ResourceOptions {
            provider: Some(tls),
            protect: Some(true),
            additional_secret_outputs: vec!["privateKeyOpenssh".into(), "privateKeyPem".into()],
            ..Default::default()
        },
        ..Default::default()
    });
    resource(
        ctx,
        &provider,
        "repositoryDeployKey:RepositoryDeployKey",
        "assets-ci-reader",
        vec![
            ("repository".into(), assets.output("name")),
            (
                "title".into(),
                value("Public game CI: restricted assets read-only"),
            ),
            ("key".into(), key.output("publicKeyOpenssh")),
            ("readOnly".into(), json(true.into())),
        ],
    );
    let environment = resource(
        ctx,
        &provider,
        "repositoryEnvironment:RepositoryEnvironment",
        "infrastructure-environment",
        vec![
            ("repository".into(), source.output("name")),
            ("environment".into(), value("infrastructure")),
            (
                "deploymentBranchPolicy".into(),
                json(serde_json::json!({"protectedBranches":false,"customBranchPolicies":true})),
            ),
        ],
    );
    resource(
        ctx,
        &provider,
        "repositoryEnvironmentDeploymentPolicy:RepositoryEnvironmentDeploymentPolicy",
        "infrastructure-main-only",
        vec![
            ("repository".into(), source.output("name")),
            ("environment".into(), environment.output("environment")),
            ("branchPattern".into(), value("main")),
        ],
    );
    let repos = Repositories {
        source,
        provider,
        owner,
        name,
    };
    variable(
        ctx,
        &repos,
        "ASSETS_REPOSITORY",
        value(format!("{}/{}", repos.owner, assets_name)),
    );
    variable(ctx, &repos, "INFISICAL_SONAR_ENABLED", value("false"));
    crate::github_credentials::register(ctx, &repos, &environment, &key, cache_writer)?;
    resource(
        ctx,
        &repos.provider,
        "repositoryRuleset:RepositoryRuleset",
        "source-main-protection",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("name".into(), value("Protected main")),
            ("target".into(), value("branch")),
            ("enforcement".into(), value("active")),
            (
                "conditions".into(),
                json(serde_json::json!({"refName":{"includes":["refs/heads/main"],"excludes":[]}})),
            ),
            (
                "rules".into(),
                json(
                    serde_json::json!({"deletion":true,"nonFastForward":true,"requiredSignatures":true,
                    "requiredStatusChecks":{"strictRequiredStatusChecksPolicy":true,"requiredChecks":[{"context":"All required checks","integrationId":15368}]},
                    "pullRequest":{"dismissStaleReviewsOnPush":true,"requireCodeOwnerReview":true,"requiredApprovingReviewCount":1,"requiredReviewThreadResolution":true}}),
                ),
            ),
            (
                "bypassActors".into(),
                json(
                    serde_json::json!([{"actorId":5,"actorType":"RepositoryRole","bypassMode":"always"}]),
                ),
            ),
        ],
    );
    resource(
        ctx,
        &repos.provider,
        "repositoryRuleset:RepositoryRuleset",
        "official-tags-protection",
        vec![
            ("repository".into(), repos.source.output("name")),
            ("name".into(), value("Official version tags")),
            ("target".into(), value("tag")),
            ("enforcement".into(), value("active")),
            (
                "conditions".into(),
                json(serde_json::json!({"refName":{"includes":["refs/tags/v*"],"excludes":[]}})),
            ),
            (
                "rules".into(),
                json(serde_json::json!({"creation":true,"update":true,"deletion":true})),
            ),
            (
                "bypassActors".into(),
                json(
                    serde_json::json!([{"actorId":5,"actorType":"RepositoryRole","bypassMode":"always"}]),
                ),
            ),
        ],
    );
    ctx.export("githubSourceUrl", repos.source.output("htmlUrl"));
    ctx.export("githubAssetsUrl", assets.output("htmlUrl"));
    ctx.export("githubRepositoryId", repos.source.output("repoId"));
    Ok(Some(repos))
}
