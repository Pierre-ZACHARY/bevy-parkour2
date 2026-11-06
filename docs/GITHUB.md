# GitHub source and restricted assets

Pulumi manages `Pierre-ZACHARY/bevy-parkour2` (public source) and
`Pierre-ZACHARY/bevy-parkour2-assets` (private assets). The first publication uses
a clean snapshot: the old history contains third-party binaries and must never
be pushed to the public remote. The original worktree/history stays intact.

The private repository is pinned as the `private-assets` Git submodule. GitHub
authenticates its clone independently of the public repository. Downloading the
source does not grant access to assets or a license to use them.

```powershell
git clone https://github.com/Pierre-ZACHARY/bevy-parkour2.git
cd bevy-parkour2
$env:GIT_LFS_SKIP_SMUDGE = '1'
git submodule update --init private-assets
Remove-Item Env:GIT_LFS_SKIP_SMUDGE
git -C private-assets lfs pull --include="assets/**"
pwsh -File tools/setup_assets.ps1
infisical login --domain https://eu.infisical.com
pwsh -File tools/setup_cache.ps1
```

Git Credential Manager or an SSH credential must belong to a user invited to
the private repository. For SSH, change only the local submodule URL; no PAT
belongs in `.gitmodules`. VS Code's game/editor tasks prepare hydrated assets
automatically. Plain Cargo compilation does not require the binary assets;
running the game and its asset integration tests does.

The private repository holds runtime assets under `assets/`, original authoring
downloads under `asset_sources/` and vendor documents under `licenses/`.
Runtime packaging reads only tracked runtime paths, checks hydration and refuses
copies that differ from the private checkout. Authoring files, receipts and
licenses are excluded from packages. Edit licensed files in the private checkout,
commit there, then update the public repository's submodule revision.

## Permissions and credentials

Ignored stack configuration controls:

- `enableGitHub=true`, `githubOwner`, immutable `githubOwnerId`.
- `githubSourceRepository` and `githubAssetsRepository` (defaults above).
- `githubSourceCollaborators` / `githubAssetsCollaborators`: JSON username arrays;
  default `[]` leaves only the owner. Pulumi manages these lists exclusively.
  GitHub personal repositories grant invited collaborators write access; use an
  organization if human read-only repository roles are needed.
- `githubPulumiTokenExpires`: expiry of the generated personal Cloud token.

`GITHUB_TOKEN` supplies the provisioning PAT through the process environment.
It is never placed in Infisical or a versioned file. Pulumi copies it as
`INFRA_GH_TOKEN` into GitHub's **infrastructure** environment and encrypts
its value in state. The generated Pulumi Cloud token is also an environment
secret; it acts with the current Pulumi user's permissions and expires on
2027-02-06 by default. Rotate both before expiry. Stack YAML stays ignored,
including encrypted values. The provisioning PAT needs repository Administration,
Contents, Workflows, Actions, Secrets, Variables and Environments write access
and access to all repositories to create new ones.

Pulumi generates an ED25519 key. Its public half is a **read-only** deploy key on
the assets repository; its private half is `ASSETS_DEPLOY_KEY` in the source
repository's build/release environment secrets. It grants no access to another repository or to
infrastructure. Authorized main/tag jobs hydrate only `assets/`; sparse checkout
avoids downloading the multi-GB authoring sources.

Main requires signed commits, the CI gate and owner review through CODEOWNERS;
the owner retains administrator bypass. Only repository administrators can
create, replace or delete official `v*` tags. The build environment allows main
only, while release allows version tags only. Other branches cannot obtain the
asset key by manually dispatching a modified workflow.

Infisical's current quota allows two identities, so the managed roles are:

| Capability | Credential and enforcement |
| --- | --- |
| Build/cache/analysis | Infisical build Viewer only; OIDC checks immutable repository/owner IDs, exact CI workflow, main or `v*` tags, push/manual events; 15-minute tokens |
| Official release | Sentry `org:ci` copied from the private Infisical bootstrap source into GitHub's `release` environment; only `v*` tag deployment policies |
| Infrastructure | Separate Infisical identity: infra Viewer, build Member, release Admin; exact main/manual infrastructure workflow and environment |

Local publication still reads the private Infisical release project. The build
identity has no membership there. GitHub's release environment has a separately
managed copy of the same restricted Sentry token; rotating the bootstrap token
and applying Pulumi updates both copies.

## CI boundaries

`ci.yml` runs on main pushes, version tags, PRs and manual dispatch. Main jobs
use private assets and the shared cache; official tag jobs additionally publish
Sentry symbols/releases. PR jobs receive no private assets or secrets: lint runs,
tests select the four suites independent of licensed game assets, and packaging
is skipped. The gate includes those explicit job results.

Public Actions artifacts contain allowlisted reports, logs, JUnit, coverage and
Clippy data. Game archives, staged raw assets, screenshots and source downloads
are never uploaded by this workflow. Distribution of built games requires a
separate publication policy consistent with the vendor licenses.

Infrastructure is manual, limited to main and the `infrastructure` environment.
Its OVH API calls also depend on the API key's IP allowlist: GitHub-hosted runners
have changing addresses. Use an allowed fixed-egress runner or update that
allowlist deliberately; this setup does not relax the existing restriction.
Sonar upload remains disabled until a dedicated analysis credential is provided.
