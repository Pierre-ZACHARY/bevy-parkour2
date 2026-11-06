# Official build reporting

Pulumi manages the protected Sentry project `bevy-parkour2`, its stable public
ingestion key, and secret stack outputs `sentryDsnPublic` / `sentryClientKey`.
Provider: `jianyuan/sentry` 0.15.8 through Terraform Bridge 1.1.0. The existing
BevyTests project was consulted as a reference and is unchanged.

## Packaging and releases

From a clean checkout at an existing tag identifying HEAD:

```powershell
cargo xtask build --targets windows-x64 --channel official --tag v1.0.0
cargo xtask build --targets webgpu --channel official --tag v1.0.0
```

The packager reads the public DSN, organization and project from the Infisical
build project `/app`. Pulumi remains the source of the managed ingestion key;
its outputs are synchronized to Infisical after `infra/pulumi.ps1 up`. Builds
need no Pulumi login. `SENTRY_DSN` can supply the exported DSN explicitly.
The legacy `--sentry-stack` argument remains accepted but no longer selects
build configuration. See [permissions and current provisioning status](INFISICAL.md).
The game embeds only the ingestion DSN, release `bevy-parkour2@<tag>` and
environment `production`. The DSN is necessarily readable in a distributed game.

Official packaging uses checksum-pinned **sentry-cli 3.8.2** to create/update the
release, associate Git commits, check and upload debug information, then finalize
the release and record a deployment. An upload failure fails packaging. Native
debug information is uploaded before distributing the executable:

| Packaged target | Debug information |
| --- | --- |
| Windows x64 | executable + PDB |
| Linux x64 / ARM64 | ELF with DWARF |
| macOS ARM64 | dSYM generated with dsymutil |
| WebGPU | bindgen WASM with DWARF/build ID + JavaScript artifacts/debug IDs |

Official builds override debug information to level 2 and disable stripping.
Official Web builds keep the bindgen module's DWARF and skip wasm-opt to preserve
symbol addresses. This increases package size; a future split-DWARF step can
reduce it. Native PDB/dSYM files are uploaded from the build directory and are
not copied into the player archive. Browser SDK bundling requires Node 22+ / npm;
the exact browser, WASM and esbuild versions/integrities are in
`tools/sentry-web/package-lock.json`. xtask builds and caches one offline-capable
SDK bundle, with `wasmIntegration()` installed before loading game WASM.

In GitHub Actions, each native packaging runner uploads its own symbols. A
separate `sentry-release` job finalizes only after **all** build matrix jobs pass.
The equivalent explicit operation is `cargo xtask sentry-finalize --tag v1.0.0`.
Local official invocations finalize after all their requested targets succeed.

`SENTRY_AUTH_TOKEN` is read from the private Infisical release project
locally, or from the tag-restricted GitHub release environment in CI,
only for infrastructure and sentry-cli. It never enters a game compiler child
or package. Official publication requires networking; it is not an offline
operation. Ordinary `--channel local` builds do not enable Sentry.
Official release publication needs the additional publishing permission; access
to the ingestion DSN alone does not grant it. CI expects a separate private
release project containing an `org:ci` token. Pulumi now manages that protected
secret after its one-time UI bootstrap; the administrator token stays private
in the infrastructure project. See [setup](INFISICAL.md).

## Player reports and privacy switch

**F8** requests a voluntary bug report through Leafwing. A separate input entity
keeps this usable while player movement input is disabled. Reports have a
15-second cooldown. **F9** toggles reporting; `ReportingSettings.enabled` is the
resource for a future settings page, with a TODO at its definition.

The preference persists under the user's configuration directory on native
platforms and in browser localStorage. Disabling clears diagnostic buffers and
blocks SDK events, Rust errors/panics, manual reports and native crash uploads.
Already transmitted events cannot be recalled. A failed preference write is
logged; the current process still applies the switch.

| Information | Manual F8 | Error/panic | Native OS crash |
| --- | --- | --- | --- |
| ECS state | current bounded snapshot | latest snapshot | latest synchronized snapshot |
| Recent logs | bounded ring | bounded ring | synchronized ring |
| Screenshot | async primary-window capture | unavailable | unavailable |
| Call stacks | current stack | error/panic stack | minidump threads/stacks |
| Minidump | no | no | Windows/Linux/macOS integration |

ECS JSON contains elapsed game time, entity count, up to 16 player transforms/
velocities and 128 entity transforms/velocities. It does **not** serialize arbitrary
resources, assets, names or the whole reflected world. Automatic snapshots are
at most once per second. Logs keep at most 128 lines of 1024 characters.
Screenshots are resized to 1280×720 and limited to 2 MiB; if readback takes over
five seconds or encoding fails, the report is still submitted without an image.

Native minidumps use Sentry Rust SDK 0.49.5's separate crash reporter, reexecuting
the packaged game binary before Bevy starts. No extra helper installation is
needed. Context is synchronized to that process. The minidump contains OS crash
thread/stack/memory information; this is not a full RAM dump. A browser cannot
produce an OS minidump. Raw minidump retention depends on Sentry's project/plan
policy: processing into a crash event does not imply a downloadable raw dump.
Default PII collection and performance sampling are disabled. No session replay.

## CI and observed evidence

Trusted tag jobs obtain build configuration through Infisical OIDC and the
dedicated publishing token from GitHub's tag-only release environment, managed
by Pulumi. This preserves publishing isolation within Infisical's two-identity
quota. They need no Pulumi access. Pull requests receive neither credential.
See [runner setup](GITHUB.md).

Validation on 2026-11-06:

- Windows and WebGPU game checks pass with official reporting enabled.
- SDK envelope tests cover attachments, size bounds, scoping and disabled sends.
- Chrome 156 executes the real WASM probe and combined Sentry SDK; error/panic
  and manual attachments are intercepted before transmission, including on/off.
- sentry-cli uploaded and processed real Windows probe PDB/PE files and completed
  the release lifecycle. This is a probe, not a full tagged game package.
- The dedicated Infisical release token was separately validated with only
  `org:ci`: authentication, release creation/finalization, validation deployment
  and the symbol upload command succeeded (probe symbols already on the server).
  A build-project Viewer could not read this private token.
- A deliberate isolated Windows crash reached Sentry as event
  `bd1dc9bae78e4e9da61b52d5bdd70fa4`, with six threads and synchronized ECS context.
  The raw dump was processed but not retained as a downloadable attachment.

Evidence: ignored `artifacts/telemetry-research/`. Linux/macOS runtime behavior
and GitHub Actions have not been executed on this Windows workstation. Existing
Android/iOS packaging remains explicitly unimplemented.

References: [Sentry CLI](https://docs.sentry.io/cli/),
[Rust minidump integration](https://github.com/getsentry/sentry-rust/tree/0.49.5/sentry-minidump),
[WASM integration](https://docs.sentry.io/platforms/javascript/guides/wasm/).
