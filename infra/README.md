# OVH compilation cache and Sentry infrastructure

Project `bevy-parkour2-infra`, stack `Pierre-ZACHARY/bevy-parkour2-infra/dev`,
backend Pulumi Cloud. The project pins the cloud backend explicitly so an
unrelated inherited local backend does not select a different state store.

The program uses the experimental `pulumi-labs/pulumi-rust` runtime at revision
`d62e9f213321befce6f870971e191c9f3946b967`. It is an independent Cargo workspace:
infrastructure dependencies do not enter the game's lockfile or build.

The approved cache reader, credential and prefix policy are managed by the OVH
provider. Bridged `Infisical/infisical` 0.19.36 manages their protected secret
copies and private project `bevy-parkour2-release`. Infisical login/OIDC tokens
are process-only, not provider inputs. The Sentry organization CI token needs
one UI bootstrap because the Sentry provider has no resource for it and the
existing personal PAT is denied by that API; its storage/distribution is then
managed by Pulumi. See [current provisioning status](../docs/INFISICAL.md).

The OVH Terraform provider is bridged through Pulumi's `terraform-provider`
1.1.0, parameterized with `ovh/ovh` 2.24.0 from registry.opentofu.org. The Rust
program registers the small set of resources directly through the core SDK,
using tokens checked against the bridged schema. This avoids generating a Rust
SDK for the provider's entire API. Both the package declaration and registration
descriptor pin the same versions; update both together if changing versions.

## Bootstrap and preview on Windows

Requirements: Pulumi CLI (validated with 3.274.0), Rust/Cargo, Go with automatic
toolchain downloads, PowerShell and curl.exe. Upstream requires Go 1.25.11; the
bootstrap's process-only `GOTOOLCHAIN=auto` handles this with an older installed Go.

```powershell
.\infra\bootstrap.ps1
infisical login --domain https://eu.infisical.com
.\infra\pulumi.ps1 whoami
.\infra\pulumi.ps1 stack select Pierre-ZACHARY/bevy-parkour2-infra/dev
.\infra\pulumi.ps1 preview --diff
```

The bootstrap downloads a SHA-256 verified source archive, builds the language
plugin in ignored `.tools/bin`, and compiles the infrastructure with its lockfile.
`pulumi.ps1` fetches only OVH/Sentry/Sonar credentials from the restricted Infisical
project (or uses a complete CI process environment), adds the
local plugin to PATH, selects Pulumi Cloud for the child, and restores the caller's
environment afterwards. S3 credentials are unnecessary for bucket management.

## Local configuration

`Pulumi.dev.yaml` is deliberately ignored, even when values are encrypted.
The initial checkout has this configuration populated from a read-only OVH
inventory. On a new checkout, set these keys with `infra/pulumi.ps1 config set`:

| Key | Meaning |
| --- | --- |
| `projectId` | OVH Public Cloud project ID; not the S3 username |
| `bucketName` | `yummy-blagg` |
| `region` | `EU-WEST-PAR` (management API); sccache uses `eu-west-par` |
| `ovhEndpoint` | OVH API endpoint, e.g. `ovh-eu` |
| `sentryBaseUrl` | Sentry API endpoint, e.g. `https://sentry.io/api/` |
| `cacheRegion`, `cacheEndpoint` | S3 cache region and HTTPS endpoint |
| `cachePrefix`, `cacheRwMode` | S3 key prefix and read/write policy |
| `cacheUseSsl`, `cacheVirtualHostStyle` | S3 connection options (`true`/`false`) |
| `cacheDisableEc2Metadata` | Disable EC2 credential discovery (`true`) |

Public values belong to this ignored stack configuration. The old
`OVH_CLOUD_PROJECT_SERVICE` value is represented by `projectId`; it is an
identifier, not a credential. Credentials live in Infisical. See
[project boundaries, onboarding and pending provisioning](../docs/INFISICAL.md).

The `compilerCache` stack output contains only eight explicitly allowed public
cache variables. After a successful `pulumi.ps1 up`, the wrapper exports it to
ignored `.compiler-cache/public.env` atomically. Refresh separately with:

```powershell
.\infra\export-cache-config.ps1 -Stack Pierre-ZACHARY/bevy-parkour2-infra/dev
```

Both compiler frontends and `xtask remote-cache` consume this snapshot locally,
with process environment overrides. Fresh clones still have public bootstrap
defaults without needing Pulumi on every developer machine. Stack YAML, public
exports, state files and `.env` remain ignored by Git.

Credentials from Infisical are supplied as secret inputs to
the provider. Pulumi Cloud stores provider secrets encrypted in the state.
Sensitive client/cache outputs are marked secret. Do not add `.env`, stack YAML files, state
exports or populated credential examples to Git, even encrypted stack variables.

## Existing bucket adoption

The program imports the existing bucket using `<projectId>/<region>/<bucketName>`
through `ovh:index/cloudProjectStorage:CloudProjectStorage` (OVH Public Cloud
API v1). `hideObjects=true` keeps the changing cache object inventory out of the
Pulumi state.
It sets `protect=true` and `retain_on_delete=true`. No object-lock, versioning,
tags, policy or expiration change is included in the initial adoption.

OVH recommends its newer v2 resource `cloud_storage_object_bucket`. Live previews
with 2.24.0 found it cannot import this bucket: the read maps disabled Object
Lock to `{mode: "", retentionDays: 0}`, whose removal requests replacement.
Preserving that value with `ignoreChanges` then fails provider validation because
versioning is disabled. No v2 update was applied. This program uses the still
supported v1 resource until the upstream import defect is fixed. Never enable
Object Lock or recreate a bucket merely to make the v2 import pass. A future
v1-to-v2 migration needs an explicit state migration and a clean preview.

Review `preview --diff` before an update. The first update should create the
stack/provider state and import one existing bucket, with no bucket creation,
replacement or deletion. A subsequent preview must report no changes.
The initial `hideObjects=true` setting appears as an update immediately after
import. The v1 provider sends an idempotent PUT carrying the preserved policies
and then stores the state without the object inventory. Encryption, Object Lock,
versioning and owner are adopted with `ignoreChanges`; managing them later needs
a deliberate code change to that list.

```powershell
.\infra\pulumi.ps1 up --yes
.\infra\pulumi.ps1 preview --expect-no-changes
```

The bucket's name/region/project changes would require replacement and are
blocked by protection. Deliberately deleting the resource from code would leave
the physical bucket in place after unprotecting because of retain-on-delete.

The [cache study](../docs/REMOTE-CACHE.md) recommends considering a prefix-scoped
30-day lifecycle after measuring cache growth. The Terraform OVH provider has
`cloud_project_storage_object_bucket_lifecycle_configuration` for that future
change; it replaces the whole lifecycle document, so preserve existing rules.

## Observed validation (2026-11-05)

Sentry was added on 2026-11-06 using bridged `jianyuan/sentry` 0.15.8. The
ignored stack configuration also supplies `sentryOrganization=pierre-zachary`,
`sentryTeam=pierre-zachary` and optionally `sentryProject` (default bevy-parkour2).
The admin token is SENTRY_AUTH_TOKEN in restricted Infisical infrastructure access. The protected project
and named client key export secret `sentryDsnPublic` and `sentryClientKey`, plus
non-secret organization/project slugs. No per-build client-key rotation occurs.
See [provisioning, official builds and validation](../docs/SENTRY.md).

SonarCloud is managed with bridged `rewe-digital/sonarcloud` 0.5.2. The protected
project and `main` branch use ignored configuration `sonarOrganization`,
`sonarProject`, optional `sonarVisibility` and `sonarImportId` for adoption.
`SONAR_TOKEN` is loaded from restricted Infisical infrastructure access and encrypted in provider state.
Public outputs are `sonarOrganization`, `sonarProjectKey`, `sonarHostUrl`.
See [analysis and coverage](../docs/SONAR.md).

[Update 2](https://app.pulumi.com/Pierre-ZACHARY/bevy-parkour2-infra/dev/updates/2)
created three Sentry resources with three existing resources unchanged.
[Stable preview](https://app.pulumi.com/Pierre-ZACHARY/bevy-parkour2-infra/dev/previews/dabb970a-56fc-45de-a37d-0a5d0b0cb5e0)
then reported six unchanged resources.

- [Cloud update 1](https://app.pulumi.com/Pierre-ZACHARY/bevy-parkour2-infra/dev/updates/1):
  stack/provider created, existing bucket imported, `hideObjects` applied.
- [Final preview](https://app.pulumi.com/Pierre-ZACHARY/bevy-parkour2-infra/dev/previews/ce497f75-c7b9-4cb9-883f-bd0fcd123d83):
  `preview --expect-no-changes` passed, three resources unchanged.
- State export confirmed `protect`, `retainOnDelete`, `hideObjects`, null object
  inventory and ciphertext for all three OVH provider credential inputs. The
  export remains under ignored `.tools/`.
- Rust compilation, formatting and Clippy passed. The warning about loading the
  locally built Rust language plugin from PATH is expected with this runtime.
