# Default compiler cache: OVH S3 and sccache

Cargo now uses sccache 0.19.0 by default through `.cargo/config.toml`.
The shared backend is the private `yummy-blagg` OVH S3 bucket in Paris. This
caches compiler results, independently of Cargo sources, targets and assets.

## New checkout

```powershell
pwsh -File tools/setup_cache.ps1
infisical login --domain https://eu.infisical.com
cargo build --release --no-default-features
cargo test -p character_controller
```

Unix without PowerShell: `sh tools/setup_cache.sh`. A developer needs Infisical
CLI 0.45.2+ and membership in the build project. See [access status and onboarding](INFISICAL.md).

The bootstrap compiles a dependency-free Rust frontend directly with rustc,
then installs a SHA-256 verified sccache release. It does not compile Bevy or
xtask. Binaries, daemon port records and optional disk cache live in ignored
`.compiler-cache/`, outside Cargo targets. Windows x64, Linux x64/ARM64 and
macOS x64/ARM64 have pinned releases.

Infisical holds S3 credentials and management tokens. Public bucket/endpoint/
region/prefix/cache options are configured in the ignored Pulumi stack YAML.
After `infra/pulumi.ps1 up`, the public `compilerCache` output is exported to
ignored `.compiler-cache/public.env`. To refresh it separately, run
`pwsh -File infra/export-cache-config.ps1 -Stack Pierre-ZACHARY/bevy-parkour2-infra/dev`.
The compiler and `xtask remote-cache` both read this local snapshot; no Pulumi
CLI call is made per compilation. New clones without a snapshot retain the
public bootstrap defaults, so ordinary development does not require a Pulumi
login. Process values take precedence over the snapshot. No tool reads `.env`.
The frontend fetches only the two S3 keys, starts an authenticated sccache daemon,
and retains no secret file. It reuses that daemon across compilation units,
including IDE/Jackdaw launches, so there is no network fetch per crate. Daemon
profiles rotate hourly; idle daemons exit after ten minutes. Logout/revocation
does not erase credentials already held by a running daemon; stop the project
daemon if immediate local revocation is needed. Unavailable sessions retry after
60 seconds; remove `.compiler-cache/infisical/unavailable` to retry immediately.
Build-project credentials use READ_ONLY. Infrastructure members use their private
writer to populate the shared cache; this reads only those two keys and never
grants collaborators infra access. Set `PROJECT_CACHE_ACCESS=read-only` to choose
reader credentials explicitly. A build-only Viewer identity was verified to
obtain an S3 hit with zero cache errors using this default frontend.
Reader cache misses compile locally; infrastructure-member builds populate new
entries in the shared cache.
OVH management keys are not loaded for compilation. AWS_PROFILE and standard AWS credential files are also
supported. Without credentials, a local disk cache is selected with a one-time
notice. An incomplete key pair fails explicitly. S3 uses HTTPS; an S3 outage is
not silently reported as a working remote cache.

## Covered entry points

| Entry point | Integration |
| --- | --- |
| Cargo builds, tests, runs, examples and workspace packages | Root compiler and wrapper hooks |
| Independent crate workspaces under this checkout | Ancestor Cargo configuration discovery |
| Native/WASM targets and profiles | Same hooks, separate compatible cache keys |
| VS Code debug/release | Setup prelaunch task, Cargo cwd and incremental=0 |
| VS Code hotpatch | Cached launcher supplies the compiler on PATH for Dioxus |
| VS Code Jackdaw | Setup before extensions, cached editor launcher |
| Jackdaw Play/restart | Cargo runs in the project and rediscovers root configuration |
| Jackdaw extensions | SDK ABI wrapper preserved; caching happens after its rewrites |
| VS Code terminals | Frontend directory on workspace PATH; reopen existing terminals |
| GitHub Actions | Bootstrap per job; S3 for trusted jobs with credentials, local otherwise |

Cargo must run from inside this checkout or a descendant. A --manifest-path
argument alone does not load its configuration when the caller is elsewhere.

The two hooks are deliberate: the outer sccache frontend covers ordinary Cargo
and cc-rs native compilation; the configured rustc frontend still runs when
Jackdaw overrides RUSTC_WRAPPER. Toolchain selection remains with rustup and the
SDK. No editor installation, global Rust setup or global PATH is changed.

Dioxus 0.7.2 calls rustc by PATH instead of forwarding the configured compiler
path. VS Code uses the cached launcher to preserve caching after its wrapper.
Outside VS Code use:

```powershell
pwsh -File tools/cached_command.ps1 dx serve --hot-patch --features hotpatch
pwsh -File tools/cached_command.ps1 jd open
```

Each project/toolchain/cache profile selects an available daemon port and saves
it locally, avoiding Windows excluded port ranges. Cargo incremental codegen
is disabled for cache eligibility. Cargo still skips unchanged units through
normal target fingerprints. Explicit user/tool compiler overrides may alter
this policy. No arbitrary build system is silently forced to use another compiler.

## Release measurement: 2026-11-05

Windows x64, Rust 1.97.1, release with thin LTO and one codegen unit, 12 jobs,
no editor features (`--no-default-features --features distribution`).

| Measurement | Cold compile | S3 warm compile |
| --- | ---: | ---: |
| Wall time | 989.92 s (16 min 30 s) | 661.35 s (11 min 01 s) |
| Rust hits | 0 | 363 |
| Rust misses / compilations | 427 | 64 |
| C/C++ hits | 0 | 36 |
| C/C++ misses | 36 | 0 |
| Cache writes | 463 | 64 |
| S3 read/write errors | 0 / 0 | 0 / 0 |

Observed speedup: **1.50x**, **33.19% less wall time**. Rust hit rate among
cacheable requests: **85.0%**; Rust+C/C++ combined: **86.2%**. Both successful
builds produced a 127,019,520-byte executable.

The complete dedicated Cargo target was deleted and absence verified before
the warm build. It reused the same path, source hashes, compiler and effective
compiler flags, with a new daemon. Statistics identify S3 explicitly; no local
sccache disk tier was configured. The cold prefix was new. Cargo sources and OS
filesystem caches remained, and other builds ran on this shared workstation.

The original measurement script left SCCACHE_RECACHE present but empty because
PowerShell/.NET null assignment retained an empty variable. The cold run thus
forced compilation while writing S3. The retained warm run removed that flag
while retaining the cold run's other compiler environment values. Two interrupted
diagnostic warm attempts are excluded. Empty RUSTFLAGS overrode the intended
static-CRT setting: this is a normal MSVC-CRT release benchmark, not the static-CRT
packaging lane. The controlled flag probes are saved with the evidence.

The game's final compilation/link unit took 267.55 s cold and 247.33 s warm.
Bevy PBR and Avian still compiled in the warm run, taking about 230 s and 206 s
in parallel. These durations must not be added to wall time. Proc macros,
binaries/build scripts and other unsupported invocations remain non-cacheable.
Routing every invocation through the frontend does not make all of them cacheable.

On Windows, exceptionally long Rust argument lists (notably web-sys) can exceed
the OS command-line limit when sccache 0.19 expands them. The frontend sends
those invocations directly to the selected rustc using a response file, preserving
argument order and quoting. Those units do not use S3 caching; other units
continue through sccache. This also permits metadata-only WebGPU checks, which
are not cacheable Rust compilations in this sccache version.

Raw logs, statistics, source/executable hashes and Cargo timing HTML are in
ignored `artifacts/remote-cache-release-20261105-215612/`, with `summary.json`
and `report.md`. The later dev/debug benchmark reused this exact target path;
the release executable is now preserved as
`artifacts/remote-cache-dev-debug-20261105/previous-release-bevy_parkour2.exe`
with its original SHA-256 verified.
This is compilation evidence; no game runtime/GPU validation was performed.

## Dev/debug measurement with the existing remote cache: 2026-11-05

All four Windows x64 builds succeeded with Rust 1.97.1 and 12 jobs. The S3
prefix from the release experiment was retained without deleting or invalidating
remote objects. The complete dedicated local target was deleted before each
build, including Dioxus outputs/session state, and every build used a new daemon.
The recorded inventory of 192 source files remained identical throughout.

| Workflow | First pass | Reconstruction | Rust hits / misses on reconstruction | Time reduction |
| --- | ---: | ---: | ---: | ---: |
| Debug: ordinary Cargo build, default features | 11 min 12 s | 7 min 35 s | 387 / 65 (85.6% hits) | 32.36% |
| Dev: Dioxus initial hotpatch binary | 12 min 03 s | 9 min 10 s | 417 / 69 (85.8% hits) | 23.94% |

Debug uses Cargo's dev profile (project opt-level 1, dependencies 3, full debug
symbols). Its first pass observed zero Rust/native hits from the existing
release entries, with 452 Rust and 36 native misses. Its reconstruction also
recovered all 36 native compilations from S3. The combined hit rate was 86.7%.

The first dev hotpatch pass was already partially warm: 277 Rust hits and 209
misses (57.0%), after debug had populated the same prefix. Dioxus selected
desktop-dev inheriting dev, with the hotpatch feature. The measurement used
`dx build --fat-binary --features hotpatch --skip-assets` through the cached
launcher. Like serve --hot-patch, this selects Dioxus BuildMode::Fat, without
starting the game or watcher. It measures initial compilation and binary
preparation, not a live code edit/patch.

Both hotpatch runs reported no native requests in sccache: Dioxus sets its own
RUSTC_WRAPPER=dx, which cc-rs does not recognize as a C/C++ cache wrapper. The
Rust facade remains active. Native caching is therefore verified for ordinary
Cargo here, and is not established for Dioxus's native dependencies.

All builds had zero S3 read/write errors, timeouts and forced recaches. New wall
times exclude target deletion, daemon startup/shutdown and artifact hashing;
Dioxus metadata and bundling are included. Cargo sources and OS filesystem
caches remain. The release pair used distribution features and a slightly
different stopwatch boundary; dev/debug use defaults, and hotpatch's first
pass already had hits. These are observed workflow timings, not an isolated
comparison of a single compiler option.

Raw commands, logs, per-run statistics, source/binary hashes, Cargo timings,
comparison.json and validation.json are in ignored
`artifacts/remote-cache-dev-debug-20261105/`. Its report.md documents the method
and excluded diagnostics. The final hotpatch executable remains under the
dedicated target's `dx/bevy_parkour2/debug/windows/app/` directory.

## Fresh clone limits: measured rather than assumed

| Identical library at two paths, default hooks | Rust hits | Rust misses |
| --- | ---: | ---: |
| First build at A | 0 | 1 |
| Rebuild at A, target deleted, daemon restarted | 1 | 0 |
| First build at B, both roots supplied in SCCACHE_BASEDIRS | 0 | 1 |

The Rust key in sccache 0.19.0 includes the compile cwd and Cargo environment
paths. SCCACHE_BASEDIRS helps native compilation but did not make this Rust
case reusable. This test uses two tiny independent projects, not two full game
checkouts. Its evidence is cross-path-results.json beside the release results.

Keep checkout paths, Cargo homes, compilers, features, profiles and flags stable
for maximum reuse. Different OSes/architectures, coverage flags, editor SDKs and
hotpatch modes need compatible entries of their own. Warming Windows release
does not prewarm every development or SDK profile. The benchmark prefix is
separate; normal builds and CI populate `bevy-parkour2/v1/` over time.

The goal that every arbitrary fresh clone avoids most library compilation is
**not yet established**. It needs relocatable Rust compiler caching or a
controlled build environment. The frontend does not remove path inputs from
keys or falsify Cargo variables, which could reuse artifacts embedding wrong paths.

## Checks and CI

```powershell
.\.compiler-cache\bin\sccache.exe --show-stats
cargo xtask remote-cache smoke
# Optional isolated daemon and per-command statistics:
cargo xtask remote-cache run -- cargo xtask build --targets windows-x64
```

Ordinary builds no longer need a wrapper command. Validation on Windows covered
the default Cargo hooks, target deletion/daemon restart, the real Jackdaw ABI
wrapper with Rust 1.99.0 on a tiny library, native C via cc-rs, Clippy, xtask tests, PATH launcher
and VS Code JSON parsing. A clone fixture without credentials also compiled and
reused its local disk cache after target deletion. Full GUI editor sessions, Linux/macOS builds and a
GitHub Actions run were not repeated. Metadata-only check/Clippy invocations
can be outside sccache's cacheable Rust subset.

Trusted CI jobs reference OVH_S3_ACCESS_KEY_ID and OVH_S3_SECRET_ACCESS_KEY.
Their values have not been uploaded to GitHub by this change. Pull request jobs
receive no private cache credentials. Missing secrets select local caching.
Keep the bucket private: objects contain compiled project code. READ_ONLY mode
does not reduce a key's actual rights. Consider a 30-day prefix-scoped expiration after
observing growth; the initial infrastructure import applied no lifecycle.

Infrastructure: [infra/README.md](../infra/README.md).

## Primary sources

- [Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html)
- [sccache S3](https://github.com/mozilla/sccache/blob/v0.19.0/docs/S3.md)
- [sccache Rust key construction](https://github.com/mozilla/sccache/blob/v0.19.0/src/compiler/rust.rs)
- [CodeLLDB Cargo environment/cwd](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md#cargo-support)
