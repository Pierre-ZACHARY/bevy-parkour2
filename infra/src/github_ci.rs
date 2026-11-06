//! Two identities fit Infisical's quota without mixing build/infra access.
use crate::{
    github::{self, Repositories, json},
    infisical, value,
};
use pulumi::{Alias, Context, Output, PropertyValue, RegisterRequest, Resource, ResourceOptions};

fn resource(
    ctx: &Context,
    provider: &Resource,
    kind: &str,
    name: &str,
    previous: Option<&str>,
    inputs: Vec<(String, Output<PropertyValue>)>,
) -> Resource {
    ctx.register_resource(RegisterRequest {
        type_: format!("infisical:index/{kind}"),
        name: name.into(),
        custom: true,
        version: "0.19.36".into(),
        inputs,
        package: Some(infisical::package()),
        options: ResourceOptions {
            provider: Some(provider.clone()),
            protect: Some(true),
            aliases: previous
                .map(|name| {
                    vec![Alias::Urn(format!(
                        "urn:pulumi:dev::bevy-parkour2-infra::infisical:index/{kind}::{name}"
                    ))]
                })
                .unwrap_or_default(),
            ..Default::default()
        },
        ..Default::default()
    })
}
pub fn register(
    ctx: &Context,
    provider: &Resource,
    release: &Resource,
    repos: &Repositories,
) -> pulumi::Result<()> {
    let metadata: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/infisical-config.json"))
            .map_err(|_| pulumi::Error::new("invalid public Infisical metadata"))?;
    let build = metadata["buildProjectId"].as_str().unwrap();
    let infra = metadata["infraProjectId"].as_str().unwrap();
    let org = metadata["organizationId"].as_str().unwrap();
    let owner_id = ctx
        .config()
        .get("githubOwnerId")
        .ok_or_else(|| pulumi::Error::new("set immutable githubOwnerId"))?;
    let full_name = format!("{}/{}", repos.owner, repos.name);
    for (name, variable) in [
        ("build", "INFISICAL_BUILD_IDENTITY_ID"),
        ("infrastructure", "INFISICAL_INFRA_IDENTITY_ID"),
    ] {
        let identity = resource(
            ctx,
            provider,
            "identity:Identity",
            &format!("github-{name}"),
            (name == "build").then_some("github-release-tags"),
            vec![
                ("orgId".into(), value(org)),
                ("name".into(), value(format!("bevy-parkour2 GitHub {name}"))),
                ("role".into(), value("no-access")),
                ("hasDeleteProtection".into(), json(true.into())),
            ],
        );
        let memberships: Vec<(&str, Output<PropertyValue>, &str)> = if name == "infrastructure" {
            vec![
                ("infra", value(infra), "viewer"),
                ("build", value(build), "member"),
                ("release", release.id().cast(), "admin"),
            ]
        } else {
            vec![("build", value(build), "viewer")]
        };
        for (suffix, project, role) in memberships {
            resource(
                ctx,
                provider,
                "projectIdentity:ProjectIdentity",
                &format!("github-{name}-{suffix}"),
                (name == "build").then_some("github-release-tags-build"),
                vec![
                    ("identityId".into(), identity.id().cast()),
                    ("projectId".into(), project),
                    ("roles".into(), json(serde_json::json!([{"roleSlug":role}]))),
                ],
            );
        }
        let repo = full_name.clone();
        let owner = owner_id.clone();
        let claims=repos.source.output("repoId").map(move|id| {
            let id=match id {PropertyValue::Number(n)=>format!("{n:.0}"),PropertyValue::String(s)=>s,_=>panic!("invalid repository ID")};
            let workflow=if name=="infrastructure"{"infra.yml"}else{"ci.yml"};
            let workflow_ref=if name=="infrastructure"{format!("{repo}/.github/workflows/{workflow}@refs/heads/main")}
                else{format!("{repo}/.github/workflows/{workflow}@refs/heads/main,{repo}/.github/workflows/{workflow}@refs/tags/v*")};
            // Comma-separated entries are alternatives in Infisical OIDC policies.
            let mut claims=std::collections::BTreeMap::from([
                ("repository".into(),PropertyValue::String(repo)),("repository_id".into(),PropertyValue::String(id)),
                ("repository_owner_id".into(),PropertyValue::String(owner)),("workflow_ref".into(),PropertyValue::String(workflow_ref)),
                ("ref".into(),PropertyValue::String(if name=="infrastructure"{"refs/heads/main"}else{"refs/heads/main,refs/tags/v*"}.into())),
                ("event_name".into(),PropertyValue::String(if name=="infrastructure"{"workflow_dispatch"}else{"push,workflow_dispatch"}.into())),
            ]);
            if name=="infrastructure"{claims.insert("environment".into(),PropertyValue::String("infrastructure".into()));}
            PropertyValue::Object(claims)
        });
        resource(
            ctx,
            provider,
            "identityOidcAuth:IdentityOidcAuth",
            &format!("github-{name}-oidc"),
            None,
            vec![
                ("identityId".into(), identity.id().cast()),
                (
                    "oidcDiscoveryUrl".into(),
                    value("https://token.actions.githubusercontent.com"),
                ),
                (
                    "boundIssuer".into(),
                    value("https://token.actions.githubusercontent.com"),
                ),
                (
                    "boundAudiences".into(),
                    json(serde_json::json!(["https://eu.infisical.com"])),
                ),
                ("boundClaims".into(), claims),
                ("accessTokenTtl".into(), json(900.into())),
                ("accessTokenMaxTtl".into(), json(900.into())),
            ],
        );
        github::variable(ctx, repos, variable, identity.id().cast());
    }
    Ok(())
}
