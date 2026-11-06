# WebGPU runtime (2026-11-05)

The game in the isolated `tooling/rust-ci` worktree now runs through HTTP in
Paseo's integrated Chromium WebGPU browser. This validates this worktree's game
(ground, Ninja, physics/controller and movement graph), not unrelated concurrent
changes in the main checkout. No source assets, global settings or remote refs
were changed. No public upload, commit or deployment was made.

## Build and run

From this worktree:

```text
cargo xtask build --targets webgpu --profile ci --jobs 4 --offline
cargo xtask prototype serve --dir artifacts/<run>/packages/webgpu/bevy_parkour2 --port 8790 --evidence-dir artifacts/<run>/browser
```

Open `http://127.0.0.1:8790/`. Use physical WASD/ZQSD positions; Tab toggles the
existing movement-graph overlay. `?diagnostics` additionally exposes the read-only
`window.__parkour` snapshot (frames, player positions, catalog/graph readiness).
The page keeps its loading status until the scene and graph are ready and shows
catalog or JavaScript bootstrap failures explicitly.

`ci` is an opt-in Cargo profile for iteration, reusing `target/ci`. Distribution
builds still default to the existing `wasm-release` profile when no profile is
supplied. The browser-tested artifact below is a CI-profile build, subsequently
processed by the pinned wasm-bindgen and wasm-opt; it is not a production-size
benchmark or an official tagged release.

## Compatibility changes

- Jackdaw's native filesystem catalog walk does not run on WASM. The Rust asset
  exporter writes `assets/jackdaw.jdcatalog`, a deterministic index of exported
  BSB documents. The new opt-in `JackdawWebCatalogPlugin` fetches these through
  Bevy's AssetServer/HTTP, applies their reflected assets and blocks scene spawn
  until the catalog is ready. The native discovery path is preserved.
- Both catalog timing and live-LOD timing use `bevy::platform::time::Instant`.
  The old `std::time::Instant::now` call panicked on wasm32-unknown-unknown.
- WebGPU uses standard filtered shadows. Native keeps experimental PCSS. The
  pinned Bevy PCSS shader first failed derivative-uniformity validation, then a
  trial explicit-LOD repair exposed an incompatible Depth/Filtering sampler
  pairing. That shader experiment is evidence only, not shipped. See
  `artifacts/web-20261105-iteration1/pcss-investigation` and the
  [WGSL texture sampling rules](https://www.w3.org/TR/WGSL/#texture-builtin-functions).
- `asset-export --web` converts staged normalized 16-bit PNGs to the matching
  8-bit channel layout, preserving dimensions and alpha. The current asset is
  `T_CYBER_NG_CLOTHES_Normal.png` (4096x4096): 41,441,652 source bytes become
  10,648,177 distributed bytes. This quantizes channel precision for WebGPU;
  the original 16-bit texture and native texture precision are unchanged. The
  pinned wgpu browser backend panicked on `Rgba16Unorm`. The historical package
  below records this in `web-textures.json`; new exports use the comprehensive
  `asset-export.json` report. These historical sizes describe the original
  dimensions and earlier PNG encoding settings.
- The web canvas fits its parent and resizes with the browser viewport.
- Basis remains supported through the pinned new transcoder. The WASM graph
  contains no legacy basis-universal-sys or native encoder. Ninja's current
  glTF textures remain PNG; no glTF Basis conversion is claimed.

The local Jackdaw patch is documented in `vendor/jackdaw_runtime/LOCAL-PATCH.md`.
General filesystem prefab resolution, terrain/navmesh sidecars and mobile are
not added by this change; the current scene does not require them.

## Per-target asset quality

`cargo xtask build --targets webgpu --asset-quality optimized` applies the
versioned `web-2k` recipe on the staged package: eleven 4K textures become 2K,
nine opaque RGBA textures become RGB, hair and eye transparency are preserved.
The `original` default keeps the earlier dimensions. See
[asset export presets](ASSET-EXPORT.md) for the explicit texture list, native
recipes, override catalogs and before/after per-file provenance.

## Captures without a desktop session

Paseo's tab screenshot API still reports an unpainted tab on this disconnected
Windows session. Actual canvas captures work through Bevy's GPU readback:

```javascript
window.__parkourCapture = {state: 'requested'};
window.__parkourCaptureRequested = true;
```

Wait for `window.__parkourCapture.state === 'saved'` (or inspect its error).
`WebCapturePlugin` captures the primary window, PNG-encodes the GPU image in Rust
and POSTs it to the local xtask server's `/__capture` endpoint. The server writes
`web-capture.png` into `--evidence-dir`. Copy that file under a distinct name
before requesting another capture. This captures the rendered game and Bevy UI;
it does not capture browser chrome or HTML overlays. The endpoint belongs to the
local validation server; arbitrary static hosts do not implement it.

Keeping evidence outside the package preserves the package's inventory and
hashes for future promotion/deployment. The server binds only to loopback.

## Verification

Original-quality reference package: `1793868682175410900-35872` (see adjacent
browser evidence). The later optimized-quality package and its validation limits
are recorded in [asset export presets](ASSET-EXPORT.md#observed-validation-2026-11-05).
The earlier complete package `1793868024752637100-12264` also passed real browser
execution with screenshots; the final source additionally groups two read-only
ECS queries to satisfy WASM Clippy without changing behavior.

Observed in Paseo 0.15.0 / Chromium 152.0.7977.76 / Electron 44.2.0 on Windows:
BrowserWebGpu, NVIDIA/Lovelace as reported by the browser (exact GPU/driver hidden).

- Scene: 24 meshes, one Camera3d, loaded catalog and movement graph, Ninja visibly
  rendered with its ground shadow. The current game does not implement locomotion
  animation; this port preserves its authored pose and existing features.
- Controller: DOM KeyboardEvent input held for one second moves the body roughly
  four meters in camera-relative forward/backward directions; release settles
  the position and the floating height remains about 0.95 m. These are automated
  canvas input checks, not a manual physical keyboard session.
- Tab overlay: the graph nodes/wires and UI text are visible in GPU captures.
- Resizing: 1280x800 and 800x600 viewports render and update the canvas, including
  the graph overlay. Physical capture dimensions include the browser's DPI scale.
- A deliberately missing/corrupt catalog in a disposable iteration package
  produces an explicit visible failure with zero spawned scene meshes. The
  original index was restored in `finally` before continuing.
- Basis/Jackdaw prototype: valid BC7 sRGB/linear, nine mips, no RGBA fallback,
  inline BSB material visibly rendered; missing and corrupt textures retain
  FAIL/expected_failure reports. A screen-space comparison of the PNG/Basis
  panel interiors is recorded separately; mip-level metadata is not a proof of
  exact mip selection at every distance.
- Native regression: `1793866994911376900-45804`, seven tests passed, one authoring
  test ignored, doctests passed. Native fmt/Clippy with denied warnings:
  `1793868110845950800-47960`, PASS. WASM Clippy with denied warnings also passed.
- Exporter: three tests passed, including binary documents/index, malformed
  input protection and 16-to-8-bit PNG dimensions/alpha/source preservation.
  xtask: 18 tests passed, one opt-in acceptance test ignored.

The complete package is still large (about 75 MB WASM and 158 MB ZIP in the
first optimized CI run). This establishes a functional build, not download-size
optimization, broad device/browser compatibility, 60 FPS or production readiness.
The automated host showed roughly 30 Hz frame cadence; it is not a clean GPU
benchmark. Existing COLOR_1 warnings and the transient no-camera warning before
scene spawn remain. No rendering validation error or panic is accepted as PASS.


## Final artifact and evidence

`artifacts/1793868682175410900-35872` is PASS for build/bindgen/optimization,
binary asset export and packaging. The separate `browser/runtime.json` is PASS
for actual WebGPU boot, held/released forward/backward input, graph UI, resizing
and the normal URL without diagnostics. `browser/console-network.json` contains
35 final-package console entries with no panic/rendering error and 34 observed
network entries. Existing warnings are retained, not relabelled as errors.

`browser/verification.json` verifies all 30 package files against the immutable
manifest, every archived file against the same hashes and the archive CRC/SHA-256.
Source normal-map hash is unchanged. Exactly three BSB documents and no BSN
sources are distributed. WASM: 75,291,109 bytes; ZIP: 158,406,051 bytes.
ZIP SHA-256: `2f6796657b57d25c4597960cf6a83ab51ae0c4332cd403b91df549873165dc9b`.

GPU captures in that run's `browser/` directory:

- `initial.png`: scene/Ninja/shadow, 1402x876.
- `moved.png`: player after forward input, 1402x876.
- `overlay.png`: visible graph after returning and pressing Tab, 1402x876.
- `resized.png`: visible game/graph at 800x600 CSS pixels, 876x657 physical pixels.

Forward displacement was 3.9554 m for the one-second held input; backward input
returned near the origin and the released body stayed stable. Key input is
explicitly labelled as DOM synthesis in the JSON. The page without diagnostics
also starts and captures successfully. No physical keyboard, other browser/GPU,
public hosting or remote CI execution is inferred from this local run.

The single retained validation server serves this final package at loopback port
8791, with evidence outside the package; its Paseo tab is left on the normal game
URL. Superseded test servers were stopped. The build manifest deliberately retains
its build-time runtime-validation field; the attached browser report supplies
runtime evidence without mutating the verified package.
