# SonarCloud

The Rust Pulumi stack uses **rewe-digital/sonarcloud 0.5.2** through Terraform
Bridge 1.1.0. It manages protected project `Pierre-ZACHARY_bevy-parkour2` in
organization `pierre-zachary`, with main branch `main`. No patched provider or
additional Go build is needed. The public visibility matches the existing
BevyTests reference and the organization's free subscription.

The administrator `SONAR_TOKEN` remains in the restricted Infisical infrastructure
project. The Pulumi wrapper loads it for provisioning as a Pulumi secret.
xtask uploads read a separate `SONAR_TOKEN` from build project `/analysis`, and
pass it only to the scanner. That analysis credential still needs provisioning;
the administrator token is not shared with collaborators. Report preparation
requires no Sonar credential. See [access setup](INFISICAL.md). Stack configuration files remain
ignored even when encrypted. Outputs: `sonarProjectKey`, `sonarOrganization`,
`sonarHostUrl`. Configure `sonarOrganization`, optional `sonarProject`,
`sonarVisibility`, and `sonarImportId` for adopting an existing project, through
the ignored stack configuration.

```powershell
# All lint/tests/coverage, prepare Sonar reports, then package:
cargo xtask ci --targets windows-x64
# Also upload an analysis to SonarCloud:
cargo xtask ci --targets windows-x64 --sonar
# Reuse an existing completed coverage run (no tests rerun):
cargo xtask sonar --input artifacts/<coverage-run>
cargo xtask sonar --input artifacts/<coverage-run> --upload
```

CI always collects coverage; the previous `--coverage` switch remains accepted.
`cargo llvm-cov` exports LCOV for all five suites (including independent graph
workspaces). Sonar's Rust analyzer accepts LCOV from LLVM or Tarpaulin, so a
second Linux-only Tarpaulin test pass is unnecessary. The merger normalizes paths
to this checkout, filters dependencies/generated/vendor files, preserves hits
across overlapping suite reports and rejects empty or missing coverage. Suite
JUnit files and commit identity are checked before import. JUnit remains in the
xtask report; no guessed Rust-module-to-test-file mapping is sent to Sonar.

Lint exports Clippy compiler-message JSON for each suite; Sonar imports it without
rerunning Cargo. Sources/configuration are in `sonar-project.properties` and each
run's `sonar/` directory. The pinned scanner 8.0.1.6346 includes Java for Windows,
Linux x64/ARM64 and macOS ARM64. Downloads/cache contents are checksum verified.
Uploading requires network access. `--offline` can prepare local reports.

GitHub's trusted Sonar job imports Linux coverage plus lint artifacts after both
jobs succeed. It needs a repository secret `SONAR_TOKEN`. Fork/PR jobs do not
receive this token. An analysis upload failure fails CI. Quality gate evaluation
is distinct from publishing the report; the scanner does not wait for a verdict.
The initial probe was processed with `NONE` (no verdict), not a passing gate.

Observed 2026-11-06: the real xtask suite produced LCOV consumed by Sonar's Rust
sensor. The initial partial analysis reports **33.7%** coverage over 3212 lines
to cover; this is **only the xtask validation run**, not coverage of all suites.
The full CI pipeline and non-Windows scanner distributions have not run here.
Evidence is under ignored `artifacts/telemetry-research/`.

[Project dashboard](https://sonarcloud.io/dashboard?id=Pierre-ZACHARY_bevy-parkour2),
[provider](https://registry.terraform.io/providers/rewe-digital/sonarcloud/0.5.2/docs),
[BevyTests reference](https://github.com/Pierre-ZACHARY/BevyTests/blob/main/sonar-project.properties).
