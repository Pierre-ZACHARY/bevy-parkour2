# Infisical access

Install Infisical CLI **0.45.2 or later**, then authenticate once:

```powershell
infisical login --domain https://eu.infisical.com
pwsh -File tools/setup_cache.ps1
cargo build --release --no-default-features
```

Unix bootstrap: `sh tools/setup_cache.sh`. Reopen VS Code/terminals after installing
the CLI so they inherit its PATH. `.infisical.json` identifies the build project;
`tools/infisical-config.json` contains public project IDs, paths and region only.
No `.env` is read or generated. A login authenticates the user; an administrator
must still add that person to the appropriate project.

## Access boundaries

| Capability | Project / environment / path | Membership |
| --- | --- | --- |
| Compile with shared cache | `bevy-parkour2-build` / `dev` / `/cache` | Viewer |
| Embed Sentry DSN and identify the managed game project | same / `/app` | Viewer |
| Upload Sonar analysis | same / `/analysis` | Viewer; requires dedicated analysis token |
| Provision OVH, Sentry and Sonar | original `bevy-parkour2` / `dev` / `/` | Restricted maintainers; runners get Viewer |
| Publish official symbols/releases | `bevy-parkour2-release` / `dev` / `/` | Release maintainers/runners only |

Build project ID: `14c6d66a-46eb-4da7-ace1-701461b5951a`.
Infrastructure project ID: `e53997b0-d446-4c26-988c-b0b5910f0efd`.
Release project ID: `9da32398-aa8e-4faf-9216-dfd6157c6dad`.
Organization ID: `bed55d7a-b476-4db2-8a3e-6fcf5a76741b`.

The original seven credentials remain in the infrastructure project. The build
project contains the Sentry ingestion DSN/project identifiers and two dedicated
read-only S3 keys. Its `/analysis` folder still needs an analysis credential.
Do not grant collaborators Viewer/Member on the infrastructure project: those
built-in roles can read secrets across all of its environments and paths.
Use organization Member and project Viewer for collaborators; reserve Admin
for the owner. No collaborator invitations have been sent.

Separate projects enforce the boundary on the current subscription. Creating a
custom project role was rejected by Infisical's Enterprise entitlement check;
folder access is also a paid feature. Adding folders alone is not isolation.
See the [role documentation](https://infisical.com/docs/documentation/platform/access-controls/role-based-access-controls)
and [folder access documentation](https://infisical.com/docs/documentation/platform/access-controls/folder-rbac).

## Provisioning still required

1. **Cache readers: deployed and validated.** `infra/src/cache_access.rs` manages
   dedicated OVH user `944578`, its S3 credential and a policy permitting only
   `s3:GetObject` under `yummy-blagg/bevy-parkour2/v1/*`. The owner approved OVH's
   coarse `objectstore_operator` role; ignored stack config has
   `enableCacheReaders=true`. Before publishing the keys, live tests verified
   a permitted read and HTTP 403 for writes, deletion, reads outside the prefix,
   bucket listing and bucket ACL reads. All disposable probe objects were removed.
   Pulumi's bridged Infisical provider manages the two protected secrets in build
   `/cache`; values and provider outputs are secret in state. Collaborators use
   READ_ONLY; infrastructure members retain their private writer to populate
   the cache. `PROJECT_CACHE_ACCESS=read-only` explicitly selects reader access.

2. **Sonar upload:** put a dedicated analysis credential in build `/analysis`
   as `SONAR_TOKEN`. The current administrator PAT remains private. SonarCloud's
   Free plan only offers personal tokens; use a separate account granted only
   Execute Analysis on this project, or upgrade to a plan with scoped organization
   tokens. Generating another PAT for the administrator does not narrow its
   permissions. Report preparation works without this credential. Existing
   infrastructure maintainers can still scan with their private admin token;
   build-only members are denied that fallback by Infisical project membership.
   [Sonar token documentation](https://docs.sonarsource.com/sonarqube-cloud/managing-your-projects/managing-your-project-as-developer).

3. **Release publication: provisioned.** Pulumi created and protects
   private project `bevy-parkour2-release`. The owner approved a Sentry `org:ci`
   token, which is organization-wide rather than limited to this game's project.
   The Sentry Terraform provider has no organization-token resource; the actual
   `/org-auth-tokens/` API returns HTTP 403 with the existing personal PAT.
   The owner created the Organization Auth Token in Sentry's UI with only
   `org:ci`, named `bevy-parkour2-release`, and saved it in infrastructure/dev/root
   as `SENTRY_RELEASE_TOKEN`, preserving the existing administrator
   `SENTRY_AUTH_TOKEN`. Keep that bootstrap source for subsequent Pulumi updates
   and token rotations. Ignored stack config has `enableReleaseToken=true`.
   `infra/src/infisical.rs` publishes it as protected secret
   `SENTRY_AUTH_TOKEN` in the private release project. Both local xtask publication
   and CI read that dedicated release credential; the admin PAT is not their
   default. Build-only collaborators cannot publish official releases.
   [Sentry scope documentation](https://docs.sentry.io/api/permissions/).

## GitHub runners

The repositories are now managed by Pulumi. See [repository setup and CI
boundaries](GITHUB.md) for the public/private split, collaboration lists and
environment policies. GitHub identity IDs are public repository variables.

The subscription permits two machine identities in total, including
project-scoped identities. The managed build identity has Viewer on the build
project only. The infrastructure identity has infra Viewer, build Member and
release Admin. Both have organization role no-access. OIDC checks immutable
repository/owner IDs, exact workflow, allowed refs and event names, audience
and issuer; access tokens expire after 15 minutes. Pull requests are excluded.

To keep publishing separate within that quota, Pulumi copies the restricted
Sentry token into GitHub's tag-only release environment. Local publication still
uses the private Infisical release project. Main builds cannot read that
publishing credential. The asset deploy key is similarly confined to the main
build and official tag environments, rather than a general repository secret.

The generated Pulumi Cloud token and provisioning GitHub PAT belong to GitHub's
main-only infrastructure environment. The GitHub PAT is not stored in Infisical.
The OVH/Sentry/Sonar administrator credentials remain in Infisical. Infrastructure
runs only on manual dispatch of main. Its OVH provider now uses the dedicated
`OVH_CLIENT_ID` / `OVH_CLIENT_SECRET`, generated and synchronized by the owner-only
OVH bootstrap stack. The original OVH application/consumer credentials are no
longer exported into runner environments; their IP restrictions remain unchanged.
The infra identity's existing project Viewer membership still permits reading
that project's other secrets; filtering exported variables is not folder RBAC.
The original OVH credential remains protected by its separate IP restriction.

After successful up, infra/pulumi.ps1 synchronizes only the three managed Sentry
client values to build /app. Its sync script can also be called after rotation;
unchanged values are not rewritten. Sonar upload remains disabled until a
dedicated analysis credential is provisioned.

## Validation and secret lifetime

The seven uploaded credential values were checked against the local `.env`
without printing them, then that local file was removed. A temporary Viewer
identity successfully read the build DSN and was denied infrastructure access;
the temporary identity was deleted afterwards. Evidence is under ignored
`artifacts/infisical/` and contains no credentials.

Checks include 27 xtask tests (five integration tests remain opt-in),
four standalone frontend tests, Clippy, workflow actionlint and PowerShell parsing.
The pre-GitHub migration preview reported 17 unchanged resources after provisioning and
adopting the read-only credentials, policy, private release project and its
protected Sentry publishing token. The dedicated token has only `org:ci`:
Sentry CLI authentication, release creation/finalization and a validation
deployment succeeded. The symbol upload command accepted existing probe
PE/PDB files (already present on the server). The disposable releases were
removed with the private administrator credential because `org:ci` cannot
delete releases. A temporary build-project Viewer was denied access to the
release secret and was deleted after validation.
An isolated Rust rlib got
an S3 hit after output deletion and daemon restart with Infisical credentials.
The default frontend also got an S3 hit with identical regenerated output;
these are cache integration checks, not a new full-game benchmark.
With a temporary build-project Viewer identity, the default frontend also
obtained a remote hit in ReadOnly mode with zero read/write errors after deleting
the output and stopping the isolated writer daemon. This identity was deleted.

The compiler fetches only S3 keys and keeps them in a sccache daemon's memory.
Daemon profiles rotate hourly and idle daemons exit after ten minutes; a running
daemon can retain access after logout/revocation until it exits. There is no
secret cache on disk and no Infisical call per crate. The optional
`.compiler-cache/public.env` contains only public Pulumi cache settings.
Stack YAML, state exports, artifacts and legacy `.env*` stay ignored.
The Infisical provider receives the short-lived login/OIDC token through its
process environment; that token is not recorded as a provider input or stack
variable. Machine-authenticated processes use distinct daemon profiles.
