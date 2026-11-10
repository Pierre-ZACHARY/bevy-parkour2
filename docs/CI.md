# Rust build and test tooling

All project-owned CI orchestration is in `tools/xtask`. GitHub Actions supplies
runners, checkout, caches, credentials and artifact storage. The same commands
run locally. Third-party compilers, nextest, LLVM and packaging tools still do
their specialized work.

Linux and Apple Silicon jobs enter the pinned `nix develop .#ci` environment; the release job also
builds the Crane game and dependency derivations. Nix supplies native libraries,
Rust, LLVM coverage and Clang 22 for wasm C/C++. Basis's vendored libc++ needs
newer builtins than the runner's default Clang. The Nix store cache complements
the existing sccache backend. Windows retains its native setup. See the
[Nix / Crane guide](NIX.md), including native package closure requirements.
Main builds queue behind the current run so a new push does not discard a cold
Bevy compilation before its cache is saved. Superseded pull request runs cancel.
The vendored `iyes_perf_ui` dependency is excluded from automatic workspace
membership; its incomplete upstream documentation snippets are not game doctests.

Cargo uses the project compiler cache by default, including independent crates
and editor builds. Bootstrap it once with `pwsh -File tools/setup_cache.ps1`;
the workflow and VS Code launch tasks do this automatically. Trusted CI jobs
exchange GitHub OIDC tokens for scoped Infisical access at runtime.
Without credentials, including pull requests, sccache uses a local backend.
See [remote compilation cache](REMOTE-CACHE.md) for measurements and the limits
of Rust cache reuse between different checkout paths.

The WebGPU dependency/runtime fixes and observed browser validation are described
in [the runtime guide](WEBGPU-RUNTIME.md). The earlier
[Jackdaw investigation](WEBGPU-JACKDAW.md) is retained as history.

## Commands

```
cargo xtask doctor --targets windows-x64,webgpu
cargo xtask plan --targets windows-x64,webgpu
cargo xtask lint
cargo xtask test
cargo xtask coverage
cargo xtask report --input artifacts/<run-id>
cargo xtask build --targets windows-x64,webgpu
cargo xtask build --targets webgpu --asset-quality optimized
cargo xtask ci --targets linux-x64 --coverage
```

Common options: `--jobs 2`, `--cache-dir target/xtask-cache`,
`--out-dir artifacts`, `--offline`. Paths are relative to the repository.
`XTASK_CACHE_DIR` changes the default download cache; an explicit `--cache-dir`
takes precedence. CI uses `.xtask-cache`, outside Cargo's `target/` directory,
because `rust-cache` removes non-Cargo files below `target/` before saving.
Every invocation creates a unique run directory; reports are never overwritten.
`report` is the explicit exception: it regenerates the summary of an existing
run from its recorded statuses and JUnit files without executing tests.

Build asset quality is selected independently with `--asset-quality original`
(default) or `optimized`. The named preset resolves per target, and
`--asset-presets <path>` supports a custom versioned catalog. See
[asset quality presets](ASSET-EXPORT.md) for the explicit Web 2K/RGB list,
desktop recipes and source-preservation guarantees.

`lint`, `test` and `coverage` accept `--suites game,xtask,asset-export,graph,noodle`.
The default includes every suite. Native builds require a matching native runner;
cross-compilation to Linux is intentionally unsupported. WebGPU can be built on
any supported host. Mobile targets are recognized but fail explicitly until the
mobile milestone is implemented.

`cargo xtask ci` runs lint, instrumented tests, Sonar report preparation, then
packaging and retains failures from all phases. `--sonar` also publishes the
analysis; `--coverage` remains accepted for compatibility. See [SonarCloud](SONAR.md).
The GitHub
workflow runs these phases in independent jobs so failed builds cannot suppress
test results. Le contrôle final vérifie directement les résultats `needs` de GitHub, sans
checkout ni préparation Nix/Cargo. Sa politique correspond à la commande locale
`cargo xtask gate`, qui lit `XTASK_JOB_RESULTS` : tout job obligatoire absent,
en échec ou annulé bloque la validation. Les jobs obligatoires sont `lint`,
`tests`, `build` et `crane` ; `build` et `crane` peuvent être ignorés en PR.
La matrice `crane` construit et publie les releases Nix en parallèle du packaging,
sans réutilisation directe de ces binaires dans les ZIP. La finalisation Sentry
attend les deux matrices. Voir les [mesures de durée](CI-DURATION.md). Packaging may
be skipped only on a public pull request, which receives no licensed assets;
lint and the four asset-independent test suites still have to succeed.
See [repository and asset access](GITHUB.md).

## Tests and coverage

- `game`: root workspace, excluding xtask and asset-export; distribution-compatible features.
  New workspace members are included automatically.
- `xtask`: the build/test tooling, with no Bevy dependency.
- `asset-export`: binary asset encoding and round-trip verification.
- `graph`: the independent Jackdaw graph crate, with view/leafwing/tnua.
- `noodle`: the independent UI graph fork, with default_style/scene.

Nextest runs unit and integration tests. No ignored test is executed: some
ignored tests regenerate source assets. The JUnit configuration records skipped
tests, failure output, no retries, and a five-minute per-test timeout.
Stable doctests run separately through Cargo. Their command status and log are
reported separately; they are not included in nextest JUnit or coverage.

Coverage is collected in isolated per-suite target directories with a fresh
collection, then exported to HTML, LCOV and LLVM JSON. The Linux x64 CI job is the
reference; other native runners execute ordinary tests. There is initially no
percentage threshold. Test failures, missing/malformed required reports and
export failures still fail the command. A failed test run attempts all exports
before exiting. Concurrent coverage collections for the same suite are refused
with an OS-managed lock. An interrupted pipeline stays INCOMPLETE. Missing results are UNAVAILABLE, never zero successful tests.

Hosted Linux jobs remove unused preinstalled .NET, Android, GHC and Boost SDKs
before installing Nix. Coverage CI also sets `XTASK_COVERAGE_CLEAN_TARGETS=1`:
after each suite's reports and doctests finish, its isolated coverage build
directory is removed while the suite lock remains held. Reports and logs remain
in the run artifacts. Local runs retain these directories unless explicitly opted in.

The run directory contains `summary.json`, `summary.md`, `logs/` and
`tests/<suite>/junit.xml`; coverage adds `tests/<suite>/coverage/`.
The opt-in tooling acceptance test creates a temporary crate with a failure,
abrupt exit, timeout and ignored test, then checks a second run and a missing
report. It does not touch game assets:

```
cargo xtask test --suites xtask
cargo test -p xtask real_nextest_reports -- --ignored --nocapture
```

The first command prepares the pinned nextest cache; the second requires that
cache and performs no download. Normal CI never runs ignored authoring tests.

Coverage percentages from different suites are not averaged. Sonar upload,
source-to-test mapping, branch coverage and doctest coverage are future work.

## Prerequisites and downloads

Rust 1.97.1 is pinned in `rust-toolchain.toml`; compilation uses Cargo lockfiles.
The runner needs Git/LFS and its native compiler/linker:
MSVC Build Tools on Windows, Xcode Command Line Tools on macOS, and a C toolchain
plus pkg-config/ALSA/udev/X11/Wayland/xkbcommon development packages on Linux.
The GitHub workflow delegates Linux/macOS provisioning to the locked Nix flake,
with no project-maintained host package installer.

`doctor` never downloads or installs. Execution can add Rust targets and the
matching llvm-tools-preview component via rustup; all other tools are installed
only into the requested cache, never the user's PATH. Versioned release URLs and
SHA-256 values are committed in `tools/xtask/tools.lock.json`. Update that file
deliberately when upgrading tools or Cargo.lock's wasm-bindgen version.

Downloads are verified before extraction. Cache installation is atomic and
locked. Every reuse checks the installed file inventory and hashes. A corrupted
cache is rejected; remove only the reported SDK/version/host directory to rebuild
it. Offline mode fails if required artifacts are absent. No SDK downloads occur
inside a Cargo build script.

## Packages

| Target | Runner | Backends |
|---|---|---|
| windows-x64 | Windows x64 | DX12, Vulkan |
| linux-x64 | Linux x64 | Vulkan |
| linux-arm64 | Linux ARM64 | Vulkan |
| macos-arm64 | macOS ARM64 | Metal, Vulkan through MoltenVK |
| webgpu | supported Windows/Linux/macOS host | browser WebGPU |

Packages live in `artifacts/<run>/packages/<target>/`, with a staged directory,
a ZIP archive and its SHA-256. Unix executable permissions are preserved in ZIP. Windows distribution builds
link the MSVC runtime statically.
The manifest records the exact commit, dirty-checkout status, tag, target,
features, toolchain, external tool versions and every runtime file hash.

The packager converts staged BSN documents to binary BSB with the pinned
Jackdaw encoder, verifies their decoded contents and preserves repository BSN
sources. The application resolves existing BSN references to their BSB twins.
Scene, material and movement graph documents use the same conversion; the
manifest records the format version and hashes the resulting binaries.
See [binary format requirements](WEBGPU-JACKDAW.md#bsn-versus-bsb).

Assets come only from tracked runtime files under `assets/`; generated node
catalogs are excluded. Missing/empty files and LFS pointers fail preflight.
External glTF/GLB buffer and image references must resolve to tracked files.
Hydrate LFS assets before building/testing; source asset archives are never used.

Native assets resolve relative to the executable or macOS bundle, independent of
the working directory. The macOS Rust launcher supplies its bundled MoltenVK
loader path to the child process and defaults to Metal. Set `WGPU_BACKEND=vulkan`
to test Vulkan, or `WGPU_BACKEND=dx12` on Windows. Ad-hoc signing supports local
macOS execution; distribution signing/notarization is not implemented.
Linux packages require the runtime system libraries and graphics driver.

Serve Web output through HTTPS or localhost. It contains the HTML entry point,
JS bindings, optimized WASM and assets; absence of WebGPU is shown to the user.

## Official builds and later deployment

`--channel official --tag vX.Y.Z` requires an exact tag at HEAD and a clean
checkout. The GitHub tag job supplies the tag through `XTASK_TAG`.
The Cargo profile (`--profile dev|release`) is independent of this channel.
Local packaging defaults to release and records when its sources are dirty.

Editor IPC is a default development feature and is disabled for packages.
DLSS and ray tracing are explicitly recorded as not integrated.
Official builds read the managed ingestion DSN from Infisical, enable error/panic
reporting and embed the public client configuration. Local builds keep it off.
The release is `bevy-parkour2@<tag>`. Build jobs need the Infisical build identity;
symbol publication needs a separate restricted release identity. Infrastructure
uses a protected manual workflow. See [Infisical access and pending setup](INFISICAL.md)
and [Sentry provisioning and packaging](SENTRY.md).
Future SDK adapters use the same verified cache.
Future deployment consumes and verifies an existing package manifest instead of
rebuilding. No publish, signing service, store or Sonar connection is configured.

## Evidence

Compilation, packaging, headless tests and rendered runtime checks are separate.
A successful build never marks GPU/browser validation as passed. Run every
desktop backend on suitable hardware and open the Web package in a WebGPU
browser before claiming rendered validation for that platform.

See [local validation results](VALIDATION.md) for observed checks and remaining limits.

## Mesures de durée CI

Le [relevé des durées et des optimisations](CI-DURATION.md) décrit les mesures
par job, les gains estimés, les limites des caches et la validation attendue.
