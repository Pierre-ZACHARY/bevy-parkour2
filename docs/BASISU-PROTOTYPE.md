# Basis / WebGPU prototype

This experiment lives in the isolated `tooling/rust-ci` worktree. Source textures
and the owner's main worktree are not changed. It is not a game-WebGPU release.

Browser access was restored on 2026-11-04. The latest browser findings are in
"Browser run after access repair" below; they supersede the historical NOT_RUN
access attempts, without establishing visual correctness.

## Run

```
cargo xtask prototype basisu --jobs 4
cargo xtask prototype serve --dir artifacts/<run>/basisu/standalone --port 8787
cargo xtask prototype serve --dir artifacts/<run>/basisu/jackdaw --port 8788
```

Open the printed localhost URL in a WebGPU browser. The missing/corrupt links
exercise real AssetServer failures. Browser JSON is saved beside the package as
`web-valid.json`, `web-missing.json` and `web-corrupt.json`; failures stay failures.
Review the four large panels (source on left, Basis on right, sRGB/alpha above,
linear data below). The Jackdaw variant adds a small panel with a material loaded
from `scene.bsb`. Loading statuses do not prove that pixels were rendered correctly.

`--offline`, `--jobs`, `--cache-dir` and `--out-dir` use the common xtask behavior.
The command creates a new run directory, prepares assets, builds/runs both native
variants including error cases, links both WASM variants and runs wasm-bindgen,
then compiles the game in WebGPU and native distribution configurations.
Native startup failures are recorded without preventing browser compilation.
The server binds only to localhost and is stopped with Ctrl+C.

## Dependency and format boundaries

Bevy stays at 0.20.1. The loader/encoder repository is pinned to
`14f2186f59b5e41a6cb1f9272b8175e7862d5b05`. Its format is **`.basisu.ktx2`**, not
legacy `.basis`. Native preparation enables the encoder; browser builds select
only the transcoder. UASTC LDR 4x4 + Zstd with nine mip levels is used for the two
256x256 fixtures. Source PNGs are retained beside the encoded versions.

The new loader's glTF limitation remains: this experiment does not convert the
Ninja's glTF textures. The game registers the new loader on its WebGPU build,
but a successful game compilation does not establish browser playability.
Jackdaw's filesystem catalog/prefab paths still require separate work.

The native Jackdaw importer is preserved under `texture-import`. The local copy
of jackdaw_runtime exposes `basisu` to use the new loader's typed settings for
material slots. See `vendor/jackdaw_runtime/LOCAL-PATCH.md` for provenance.

## Windows toolchain

Clang/libclang and the wasm32-unknown-unknown Rust target are prerequisites.
The upstream vendored C++ standard library requires recent Clang; this machine
has 22.1.8. No global PATH or compiler configuration is changed by xtask.

Cargo can check out upstream directory symlinks as short text files on Windows.
The preparation step materializes those directories from the same pinned git
checkout, inside Cargo's download cache. It verifies that targets stay inside
that checkout, stages copies before replacing link placeholders and leaves
registry source and actual C++/Rust source contents untouched. This avoids
requiring Windows symbolic-link privileges. The operations are tested and can
be repeated. The native encoder enables upstream's full default transcode set:
a restricted UASTC-only configuration failed in upstream C++ headers.

## Evidence interpretation

`preparation.json` records encoded/source sizes, encoding time, a CPU RGBA
transcode time, mip/color-space assertions and roundtrip errors. The synthetic
PNG fixtures compress particularly well; Basis files can be larger on disk.
This experiment does not claim a download reduction for these images.

Runtime JSON records actual GPU texture format, mip count and payload size.
An uncompressed RGBA fallback is explicitly flagged. `load_and_transcode_ms`
includes async scheduling and image loading, not just transcoding; debug loader
logs supply the loader's narrower transcode measurement. Native and browser
measurements are separate. Visual inspection and the full game are separate
validation levels and are never inferred from compile or load success.

The two experiment crates are excluded from the ordinary game lint/test/coverage
suite. Run `cargo xtask prototype basisu` explicitly for their SDK preparation and
runtime checks; this avoids silently adding experimental encoder requirements to
native game CI. The native valid case also saves `native-valid.png` using Bevy's
GPU readback after assets load. Inspect that image before claiming visual success.

## Local observations (2026-11-04)

Native runs in `artifacts/1793794740132219500-54128` passed for both standalone
and Jackdaw: valid textures and the expected missing/corrupt failures (exit 1,
explicit failure JSON). Both `native-valid.png` GPU captures were inspected:
source/Basis color and linear panels agree visually, including alpha; the small
Jackdaw panel renders the material from the binary scene. This is Windows/Vulkan
on an RTX 4070, not browser evidence. Additional runs with `WGPU_BACKEND=dx12`
also passed (`native-dx12.json`/`.log`/`.png` in each variant). Their 1000x800 GPU
captures are pixel-identical to the corresponding Vulkan captures.

Both Basis textures use BC7 (sRGB for color, linear UNORM for data), nine mip
levels, and 87,408 bytes of GPU payload each. No RGBA fallback was observed.
The PNGs have one level and 262,144 bytes each. Color PNG/Basis sizes are
2,763/27,003 bytes; linear PNG/Basis sizes are 2,565/20,721 bytes. These synthetic
images favor PNG on disk. CPU RGBA roundtrip mean channel error is 0.612/0.335
on the 0..255 scale, with maximum error 7/4. Timing is recorded per run rather
than treated as a stable benchmark.

Native game regression with the patched runtime passed in
`artifacts/1793795062839248600-95804`: seven tests passed, one authoring test
remained ignored, doctests passed with their ignored example preserved.
The xtask unit suite passed 18 tests (one opt-in acceptance test ignored), and
xtask and the Jackdaw prototype Clippy checks passed with warnings denied.

Browser execution is **NOT_RUN**: the browser access host was unavailable, and
native Chrome access approval timed out. No browser result was fabricated from
HTTP, compilation or native rendering. Use the two `prototype serve` commands
above to finish visual and negative-case browser validation. Browser transcode
time and format remain unknown until that run. The external catalog and Ninja
glTF Basis integration remain outside this texture experiment.

A fresh delegated session on 2026-11-04 confirmed the access blocker without
restarting Paseo: its tool catalog contained no Paseo `browser_*` tools, and both
`cua.createBrowserTab("iab", "http://127.0.0.1:8787/", {visible: true})` and
`cua.listBrowsers()` returned `The browser access host is unavailable`. These
calls failed before page access; they provide no evidence about the prototype
server or WebGPU support. All six standalone/Jackdaw valid, missing and corrupt
browser cases remain NOT_RUN. The attempt is recorded in
`artifacts/1793796852752545600-35344/basisu/browser-access-attempt.json`;
it is an access report, not a page-generated `web-*.json` result. Browser GPU
formats, alpha, mip rendering and negative loader behavior remain unverified.

A second session with the browser/desktop tool available made one targeted
attempt at 2026-11-04 18:55 CET: `cua.getState()` returned the native application
inventory, but `browsers: []` and
`Browsers: Error: The browser access host is unavailable`. No page was opened.
This independently confirms an access blocker, not a WebGPU or prototype failure.
All six cases remain NOT_RUN; browser version/backend, GPU formats, RGBA fallback,
transcode timing, color/alpha comparison and mip rendering are unknown. There are
no page-generated reports or browser captures from this attempt. No server was
started, no compilation was run, Paseo was not restarted and private Swarm Chrome
was not accessed. The additional access-only report is
`artifacts/1793796852752545600-35344/basisu/browser-access-attempt-root.json`;
the previous access report is preserved.

User-authorized troubleshooting later on 2026-11-04 aligned the PATH-installed
Paseo CLI from 0.11.4 to the running desktop daemon's 0.15.0 without restarting
the daemon. The official extension check confirmed Chrome's ChatGPT extension
installed and enabled. The official window-opening helper opened a separate
`about:blank` Chrome window, verified by native window inventory. Browser access
still returned `The browser access host is unavailable`; native capture/launch
also failed, and Windows reported the user session disconnected. This did not
establish WebGPU support or run any of the six cases. See
`artifacts/1793796852752545600-35344/basisu/browser-access-diagnostic.json`.

After the user-authorized reboot, Chrome launch was confirmed by an
`about:blank - Google Chrome` window and its ChatGPT extension remained enabled.
Desktop logs confirm the browser/native services started at about 20:35 CET,
but this Paseo agent's CUA MCP server started at 20:28 and still reports a
missing native pipe. This indicates a stale inherited native connection.
Separately, browser access fails in its authentication prerequisite before
discovery: the required access-pipe/verifying-identity settings are absent from
the plugin configuration and agent shell. Resetting the JS kernel did not fix
either connection; no reconnect tool is exposed in this session. All six cases
remain NOT_RUN. See
`artifacts/1793796852752545600-35344/basisu/browser-access-after-reboot.json`.

The standalone and Jackdaw WASM links and wasm-bindgen packaging also passed in
`artifacts/1793794740132219500-54128`. Their dependency and inverse-feature trees
are saved in each package: the new transcoder is present, the legacy
`basis-universal-sys` and encoder feature are absent. HTTP delivery checks verified
200 responses, matching SHA-256 bytes and MIME types for HTML/JS/WASM/textures/BSB,
and a real 404 for the missing fixture (`basisu/jackdaw-http-smoke.json`; the earlier
standalone HTTP check is in `artifacts/1793792338690313500-62360/basisu/standalone-http-smoke.json`).
The CI-profile WASM outputs are large experimental builds (about 96 MB standalone,
181 MB with Jackdaw), not optimized production download-size measurements.

## Final run

`artifacts/1793796852752545600-35344/summary.md` is the final **PASS** run of
`cargo xtask prototype basisu --jobs 4 --offline`: all 15 stages passed, including
actual compilation/linking of the complete game for WASM and native distribution.
The final WASM game build has no warnings. Native game compilation shares the
ordinary native CI test cache (`target/ci/shared`, incremental disabled).
The earlier run `1793794740132219500-54128` ends in FAIL because its redundant
native game rebuild was deliberately stopped before switching caches; the
successful individual prototype stages and captures from it remain evidence.

The final report was regenerated successfully with all 21 required artifacts
present. `verification.json` records successful SHA-256 verification of 19 files
in each prototype package and hashes of both game binaries. The raw CI-profile
game WASM is about 1.22 GB including debug data, before wasm-bindgen/production
optimization; this is a compile proof, not a distributable game Web package.

`game-native-smoke/result.json` records the new native binary running from a fresh
temporary package outside the repository on both DX12 and Vulkan. Both remained
running for the 18-second observation window, loaded Jackdaw asset files and Ninja
textures, and logged no error/panic. They were stopped explicitly afterwards;
the resulting exit code is not presented as a natural successful termination.
All 21 copied asset file hashes stayed unchanged. Existing glTF COLOR_1 warnings
remain. This startup check does not validate game visuals or gameplay.

The successful build report alone does not establish browser behavior. The
separate browser run below supplies newer runtime evidence. Full-game browser
playability remains untested.

## Browser run after access repair (2026-11-04)

Access is **PASS** through Paseo's own `browser_*` MCP tools. Two distinct
connection problems were corrected: Codex's newer MCP protocol was rejected by
Paseo's server, then localhost traffic went through the external HTTP proxy and
returned a McAfee gateway 502. The explicit Codex setting
`features.mcp_2026_07_28=false` selects compatible negotiation;
`agents.providers.codex.env.NO_PROXY=artifactory,localhost,127.0.0.1,::1` preserves
the previous exclusion and adds loopback only for Codex child processes. Existing
external proxies and authentication were preserved. Both configuration files
have `.browser-fix-20261104.bak` backups. Only this agent was reloaded after each
correction. Successful real navigation, snapshot, evaluation and console/network
inspection establish restored browser access; this does not repair the separate
Desktop CUA/Chrome authentication path.

The existing final-run packages were served with `target/debug/xtask.exe`; no
compilation ran. Browser: Paseo 0.15.0, Electron 44.2.0, Chromium 152.0.7977.76
on Windows. Bevy logs identify `BrowserWebGpu`; the physical GPU and underlying
D3D/Vulkan backend were not identified by browser output.

| Package | Case | Observed outcome |
|---|---|---|
| standalone | `/` | PASS for asset/runtime metadata; visual validation NOT_RUN |
| standalone | `/?missing` | Explicit FAIL, `expected_failure=true`; negative check PASS |
| standalone | `/?corrupt` | Explicit FAIL, `expected_failure=true`; negative check PASS |
| jackdaw | `/` | FAIL: WASM panic, result element remains `Loading...` |
| jackdaw | `/?missing` | FAIL: same panic, missing required failure JSON |
| jackdaw | `/?corrupt` | FAIL: same panic, missing required failure JSON |

Standalone page-generated reports are in
`artifacts/1793796852752545600-35344/basisu/standalone/web-{valid,missing,corrupt}.json`.
Valid Basis textures are `Bc7RgbaUnormSrgb` and `Bc7RgbaUnorm`, nine mip levels
and 87,408 GPU bytes each, with no RGBA fallback. Combined asynchronous
`load_and_transcode_ms` was 349.1 ms; narrower loader console measurements were
1.7 ms for color and 5.1 ms for linear. These are observations, not benchmarks.
Missing reports `Path not found: assets/missing.basisu.ktx2`; corrupt reports
`Failed to load ktx2 data, likely the input data isn't valid`. Both retain FAIL.

All three Jackdaw pages reach BrowserWebGpu initialization and then panic with
`time not implemented on this platform`; the captured stack reaches
`std::time::Instant::now`. They do not POST `__result` and produce no `web-*.json`.
The expected loader error messages still appear in missing/corrupt console logs,
but they do not satisfy the required page failure-report contract. The valid
case logs both BC7 transcodes (color 5.5 ms, linear 1.7 ms); its inline `scene.bsb`
request is present. Neither proves a rendered or successful Jackdaw material.
No WASM/runtime source fix was made within this browser-validation-only scope.

Screenshots returned timeouts or `The tab has not painted yet`, including after
viewport resize/focus. Native Paseo capture failed with CreateForMonitor
`0x80070057`; Windows reported the user session disconnected. Canvas readback
also timed out. No browser PNG, pixel/color/alpha comparison, mip sampling or
Jackdaw small-panel visual validation is claimed. Nine mips are metadata evidence.

The machine-readable summary is
`artifacts/1793796852752545600-35344/basisu/browser-validation.json`.
`browser-observed-*.json` retain DOM/console/network observations (the standalone
valid file is a concise observation summary); `browser-capture-errors.json`
records capture limits. Earlier access-only reports remain preserved.


## Web game follow-up (2026-11-05)

The previous clock/catalog/render blockers have been addressed and the game now
runs through HTTP with WebGPU, Ninja rendering, controller input, graph overlay
and GPU screenshots. The older failures above remain historical evidence. See
[WEBGPU-RUNTIME.md](WEBGPU-RUNTIME.md) for the current build, compatibility choices,
exact validation scope, package evidence and capture instructions. The desktop
screenshot API remains distinct from the working Bevy GPU-readback path.
