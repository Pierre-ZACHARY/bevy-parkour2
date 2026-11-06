# Local validation — 2026-11-04

Worktree: tooling/rust-ci, based on a57a10df848c4ce1c013923b00112698e035e9a4.
This checkout deliberately excludes the concurrent uncommitted changes in the
original worktree. No commit, push, GitHub run, deployment or Sonar upload was made.

## Observed results

| Check | Result | Evidence |
|---|---|---|
| xtask unit tests | PASS: 15 passed, 1 opt-in acceptance test ignored | cargo test -p xtask --locked |
| Real nextest failure/crash/timeout, ignored test, fresh second run, missing report | PASS: explicit acceptance harness | artifacts/acceptance/1793747846588469200-62984 and 1793747850059550300-62984 |
| xtask coverage exports | PASS: JUnit, HTML, LCOV, JSON | artifacts/1793752241293662400-18352 |
| Game/graph/noodle format + Clippy | PASS before the BSB addition | artifacts/1793748073961555500-63308 |
| Final xtask and asset-export Clippy | PASS, warnings denied | cargo clippy -p xtask; cargo clippy -p asset-export --profile ci |
| Distribution configuration Clippy | PASS after the BSB addition, warnings denied | cargo clippy -p bevy_parkour2 --no-default-features --features distribution --all-targets --profile ci |
| Binary asset exporter | PASS: 2 tests, including actual scene/material/graph round trips | cargo test -p asset-export --profile ci |
| Windows release build and packaging | PASS | artifacts/1793749518682970000-16120 |
| Extracted BSB package integrity | PASS: 22 file hashes, 3 BSB, zero BSN; unchanged after both launches | same run, runtime/startup.json |
| DX12 and Vulkan startup outside the repository | PASS: RTX 4070 selected, material catalog and glTF loaded; no logged error/panic | same run, runtime/*.log |
| GitHub workflow syntax | PASS | actionlint 1.7.12 |
| Native game tests | PASS: 7 passed, 1 ignored authoring test | artifacts/1793750709857547900-72824 |
| Game doctests | PASS: 1 ignored example, no failure | same run |
| Graph tests and doctests | PASS: 40 tests; doctests: 1 passed, 1 ignored | same run |
| Noodle tests and doctests | PASS: 70 tests and 3 doctests passed | same run |
| Initial WebGPU build (before Basis prototype) | FAIL: upstream Basis Universal C++ dependency | artifacts/1793747610118328500-57848 |

Paths above name local, ignored artifacts. Their JSON/Markdown reports keep
stage status, duration, logs and configuration. The acceptance fixture's
intentional failed/timeout reports are expected: its harness checks that these
failures remain visible and returns success only when the failure handling works.

Across the selected Windows suites, 134 unit/integration tests passed, plus
4 doctests and the explicit acceptance harness. Ignored authoring/example tests
remain ignored. All required stages in the final native test run passed.

## Limits

Startup logs and successful compilation do not prove rendered correctness or
gameplay. During the earlier release validation, Windows capture failed with CreateForMonitor (0x80070057), then
GetCursorPos access denied (0x80070005); no visual or movement validation is
claimed. Audio hardware was unavailable; the model loader also reports existing
COLOR_1 warnings.

Linux x64/ARM64 and macOS ARM64 builds/tests, Metal/MoltenVK rendering and browser
WebGPU validation have not run on this Windows machine. The workflow declares
those native runners. The initial WebGPU dependency failure is investigated in
BASISU-PROTOTYPE.md, with a versioned local Jackdaw patch. Browser catalog/runtime
validation remains separate from compilation. See that report for newer evidence.

Android/iOS, production signing/notarization, deployment and actual DLSS
integration remain later milestones, as scoped in the approved design.
Sentry was integrated separately on 2026-11-06; see [its validation evidence](SENTRY.md).

## Basis prototype follow-up

See [BASISU-PROTOTYPE.md](BASISU-PROTOTYPE.md) for the new implementation and exact
evidence. Final run `artifacts/1793796852752545600-35344` passes all 15 stages:
native encoder, standalone and Jackdaw native/WASM builds, expected loader failures,
and complete game native/WASM compilation. Native prototype GPU captures validate
BC7 rendering on DX12 and Vulkan. The patched native game also passes its seven
tests and startup checks on both backends outside the repository. Xtask has 18
passing unit tests; the opt-in acceptance test remains excluded from ordinary runs.
Browser rendering remains NOT_RUN because browser access was unavailable. The
prototype does not validate the filesystem catalog or Basis-in-glTF in the browser.
A fresh delegated session on 2026-11-04 reproduced the browser-host error before
navigation and exposed no Paseo `browser_*` tools. Its access-only report is
`artifacts/1793796852752545600-35344/basisu/browser-access-attempt.json`.
The six browser cases remain NOT_RUN; no browser GPU or visual result was obtained.

A second browser-capable session confirmed the same access blocker at
2026-11-04 18:55 CET: `cua.getState()` returned a native app inventory but no
browsers and `Browsers: Error: The browser access host is unavailable`.
Standalone and Jackdaw `/`, `/?missing` and `/?corrupt` are each NOT_RUN.
No page, WebGPU/backend/format, fallback, timing, console/network result or
rendered image was observed. No browser screenshots or `web-*.json` reports
were obtained. Evidence:
`artifacts/1793796852752545600-35344/basisu/browser-access-attempt-root.json`.
The earlier access report and existing work are preserved; this attempt did not
compile, start servers, restart Paseo, or access private Swarm Chrome.

Later user-authorized troubleshooting updated the PATH-installed Paseo CLI to
0.15.0, matching the running daemon, and verified a separate newly opened Chrome
window plus an installed/enabled ChatGPT extension. The browser-access host
remained unavailable; the Windows user session was disconnected. All six
prototype browser cases remain NOT_RUN. Diagnostic evidence:
`artifacts/1793796852752545600-35344/basisu/browser-access-diagnostic.json`.

## Browser validation after access repair (2026-11-04)

The earlier browser NOT_RUN access reports are historical. Paseo browser access
is now PASS: real page navigation, DOM/results inspection, WebGPU initialization,
console and network inspection succeeded after correcting MCP compatibility and
loopback proxy exclusions. The separate Desktop CUA/Chrome access path remains
unrepaired; no authentication was bypassed.

| Existing prototype case | Result |
|---|---|
| Standalone valid | PASS: BC7 sRGB/linear, nine mips, no RGBA fallback |
| Standalone missing | FAIL with `expected_failure=true`; expected-error check PASS |
| Standalone corrupt | FAIL with `expected_failure=true`; expected-error check PASS |
| Jackdaw valid | FAIL: `std::time::Instant::now` WASM panic |
| Jackdaw missing | FAIL: same panic, required result JSON absent |
| Jackdaw corrupt | FAIL: same panic, required result JSON absent |

Runtime: Paseo 0.15.0 / Chromium 152.0.7977.76 / Electron 44.2.0 on Windows;
Bevy backend BrowserWebGpu. Standalone load/transcode elapsed time was 349.1 ms;
loader transcode measurements were 1.7 ms color and 5.1 ms linear. The standalone
server saved all three `web-*.json`; Jackdaw remained on `Loading...` and saved
none. Console errors in Jackdaw do not substitute for its required failure JSON.

Visual validation remains NOT_RUN: browser captures timed out or reported an
unpainted tab; native capture failed while Windows reported a disconnected user
session. No screenshot, pixel/alpha comparison, rendered mip behavior or inline
Jackdaw material was validated. No compilation, runtime-source change, commit or
push was performed during these browser checks.

Evidence: `artifacts/1793796852752545600-35344/basisu/browser-validation.json`,
`browser-observed-*.json`, `browser-capture-errors.json`, and the standalone
package's `web-{valid,missing,corrupt}.json`. See BASISU-PROTOTYPE.md for connection
fixes, backups and exact limitations.


## Web game follow-up (2026-11-05)

The previous clock/catalog/render blockers have been addressed and the game now
runs through HTTP with WebGPU, Ninja rendering, controller input, graph overlay
and GPU screenshots. The older failures above remain historical evidence. See
[WEBGPU-RUNTIME.md](WEBGPU-RUNTIME.md) for the current build, compatibility choices,
exact validation scope, package evidence and capture instructions. The desktop
screenshot API remains distinct from the working Bevy GPU-readback path.
