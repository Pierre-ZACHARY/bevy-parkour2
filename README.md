# bevy-parkour2

Fresh restart of bevy-parkour on Bevy 0.20.1, Avian 0.8, Tnua 0.33 and the
Jackdaw 0.20 editor. The Ninja (T-pose for now) walks on a ground slab with
a Tnua controller, both authored in the Jackdaw scene `assets/scene.bsn`.

## Toolchain

- Rust 1.97.1 (pinned in `rust-toolchain.toml`, required by Bevy 0.20 and Jackdaw 0.20)
- C/C++ build tools: MSVC on Windows, Xcode command line tools on macOS
- VS Code extensions: CodeLLDB and rust-analyzer (`.vscode/extensions.json`)
- Jackdaw editor + CLI: `cargo +1.97.1 install jackdaw jd --version 0.20.1-rc.1 --locked`
- Dioxus CLI for hotpatching, matching `subsecond` in `Cargo.lock` (0.7.2):
  `cargo binstall dioxus-cli@0.7.2`

Initialize the default compiler cache once after cloning:
`pwsh -File tools/setup_cache.ps1` (or `sh tools/setup_cache.sh` on Unix).
VS Code launch configurations do this automatically. Cargo then uses sccache
for builds, tests, examples and independent crates, including Jackdaw builds.
Install Infisical CLI 0.45.2+, then run `infisical login --domain https://eu.infisical.com`.
Project tools fetch their own credentials; no `.env` is needed. Access is described
in [Infisical onboarding and permissions](docs/INFISICAL.md). Without remote cache
access the project uses local sccache. See
[cache setup, measured gains and clone limitations](docs/REMOTE-CACHE.md).

## Iteration loops

VS Code launch configurations (`.vscode/launch.json`, Windows and macOS)
cover each loop; the equivalent commands, run from the project root:

| VS Code config | Goal | Command |
| --- | --- | --- |
| Jeu (dev, hotpatch) | Hotpatch system code while the game runs | `BEVY_ASSET_ROOT=<project root> pwsh -File tools/cached_command.ps1 dx serve --hot-patch --features hotpatch` |
| Éditeur (Jackdaw) | Jackdaw editor | `JACKDAW_OPEN_PROJECT=<project root> jackdaw` |
| Jeu (debug) | Game under CodeLLDB, breakpoints | `cargo run` (add `--features dynamic` for Bevy as a DLL) |
| Jeu (release) | Release build | `cargo run --release --no-default-features` |

`BEVY_ASSET_ROOT` makes the game under dx read and watch `assets/` in place
instead of the copy dx bundles next to the executable.

Features:

- `asset_watch` (default): `bevy/file_watcher`. Editing a file in `assets/`
  reloads it in the running game. Default because the editor builds the
  package without features.
- `dynamic`: `bevy/dynamic_linking`, faster `cargo` rebuilds. Not a default:
  dx always re-enables default features, and its hotpatches fail to link
  against a Bevy DLL (`undefined symbol: __declspec(dllimport) ...`).
- `hotpatch`: `bevy/hotpatching`, used under `dx serve --hot-patch`.
- `perf_ui` (default): the performance overlay, toggled with F3. Builds
  with `--no-default-features` (distribution, web) leave it out.

Profiles follow the Bevy quick-start: `opt-level = 1` for this crate,
`opt-level = 3` for dependencies.

### Hotpatching

Under `dx serve --hot-patch --features hotpatch`, saving a `.rs` file
patches system bodies into the running game. Changes to system
signatures, components, resources or plugin setup still need a restart
(dx rebuilds and relaunches by itself in that case).

Measured on this machine: first build ~11-14 min (dx uses its own target
directory, `target/x86_64-pc-windows-msvc`), relaunch ~1 min, save to
applied patch ~10 s.

What it depends on, established on Windows while setting this up
(2026-11-02):

- Only code in the binary crate (`src/main.rs` + modules) is patched: dx
  0.7.2 never patches a library. The character's crates
  (`crates/character_controller`, `crates/character_animation`,
  `crates/ninja_character`, see below) need
  a rebuild after a change; their graphs and assets still reload live.
- No custom linker in `.cargo/config.toml`. dx then links with rustup's
  `lld-link`. With `rust-lld.exe` the dx build fails, and with MSVC
  `link.exe` patches arrive but systems keep their old code.
- Bevy is linked statically (see the `dynamic` feature above).
- Each dx session leaves a ~1-5 GB `libdeps-*.a` in
  `target/x86_64-pc-windows-msvc/desktop-dev/`; old ones can be deleted.

Same setup as the earlier BevyTests project, where hotpatching worked.

## Jackdaw editor

`jd import` wired `jackdaw_runtime` (with `pie`), `jackdaw_avian_integration`,
`JackdawPlugin` and `maybe_windowless` into the game. `jackdaw.toml` has no
`plugin` key: `jd doctor` only looks for it in `src/lib.rs`, and the editor
build does not use it.

The editor builds into `.jackdaw/target/game` (separate from `target/`) and
does not rebuild on save: after a code change press the restart button next
to Play, which rebuilds changed units and relaunches the game. Compiler results
are cached; rustc incremental code generation is disabled for sccache.

## Assets

- `asset_sources/ida_faber/` – Ida Faber Cyber Ninja Girl "Vex" (original
  downloads, copied from bevy-parkour; FBX/Unity, not loadable by Bevy).
- `asset_sources/yummy_games/` – Yummy Games Core Locomotion Animations (FBX).
- `assets/animations/yummy/locomotion.glb` – its 84 in-place clips on their
  UE5 Manny skeleton (no mesh, 9 MB, Git LFS), written with the clip list
  `crates/ninja_character/src/yummy_clips.rs` by `tools/convert_yummy.py`:
  `"C:/Program Files/Blender Foundation/Blender 5.0/blender.exe" --background
  --factory-startup --python tools/convert_yummy.py`.
- `assets/character/ninja/outfit_01.gltf` – Ninja already converted to glTF
  by the bevy-parkour pipeline (copied as-is, binaries in Git LFS). At load,
  `crates/ninja_character` builds the hair textures' mip chain and switches the
  hair to alpha-to-coverage (fix ported from bevy-parkour). Jackdaw loads
  scene models with GPU-only textures, so the hair PNGs are decoded again
  from their files for that.
- `assets/scene.bsn` – the scene (ground brush with its collider, Ninja,
  sun, cameras), edited in Jackdaw and spawned by `src/game.rs`. The ground
  colour is `base_color` in `assets/materials/ground.material.bsn`.
  Saving `scene.bsn` respawns the scene in the running game; material files
  are read once at startup (by `jackdaw_runtime`), so a material change
  shows in the game after a restart.

Licensed runtime binaries and authoring sources live in the authenticated
`private-assets` submodule. Hydrate its runtime LFS objects, then run
`pwsh -File tools/setup_assets.ps1`; game/editor VS Code tasks also prepare them.
See [GitHub access and clone instructions](docs/GITHUB.md). The original authoring
downloads stay private and are not included in game packages.

## Controls

| Action | Keyboard | Gamepad |
| --- | --- | --- |
| Move (relative to the aim) | ZQSD on AZERTY, WASD on QWERTY | Left stick |
| Jog (held) | Left Shift | Left stick press |
| Sprint (double tap, then hold) | Left Shift twice | Left stick press twice |
| Aim (the character faces it) | Mouse, once the window has the cursor | Right stick |
| Look around, keeping the aim (held) | Middle button | Right stick press |
| Switch to the fly camera and back | F | Select |
| Fly: move where it looks / up / down | ZQSD / Space / Left Shift | Left stick / RT / LT |
| Fly: faster, slower | Wheel up, down | |
| Performance overlay | F3 | |

Letting go of Run walks again; letting go of Move eases to a stop. A left
click gives the window the cursor, Escape takes it back; Tab (the graph
panel) takes it back too. While flying, the player stands still.

Bindings live in `crates/character_controller/src/input.rs` and
`crates/camera_controller/src/input.rs` (leafwing-input-manager). Keys are bound by physical position, so the same
binding follows the keyboard layout.

## The character's crates

The character is split from the game into three workspace crates:

- `crates/character_controller`, headless and meant to be reused: a Tnua
  character on an Avian capsule (walk only), its leafwing actions and its
  movement graph, with no model or animation. The scene's `#Player` entity
  carries `character_controller::controller::CharacterController`, whose
  `graph` field names the movement graph to run (default
  `flow/movement.flowgraph.bsn`; empty walks from the Move action without
  one). Adding it adds the capsule body, the Tnua controller and the input
  map. The `view` feature adds the graph over the game on Tab.
- `crates/camera_controller`, portable: cameras driven by node graphs
  run every frame. `CameraController` on a camera names its graph: the
  scene's `#Camera` runs `flow/third_person_camera.flowgraph.bsn` (Third
  Person: over the shoulder of its `target`, `#Player`), its
  `#FlyCamera` `flow/fly_camera.flowgraph.bsn` (Fly), starting inactive.
  Their nodes' inputs hold the settings (the pivot's offset from the
  target, or from its `bone`, shoulder offset, distance, sensitivity, stick
  speed, pitch limits, recenter time; fly speed, wheel factor, speed
  range). The target and the bone are wired in: an Entity node picks
  `Player` by name, a Skeleton node its visible skinned mesh, a Bone node
  one of its joints (both names chosen in a list). An unwired target
  follows the first `CameraTarget`, an unwired bone the target's origin. The third-person camera leaves its aim
  on the player (`CameraAim`), which the movement graph's Camera Aim node
  reads.
  `CullingCamera` tags the cameras the player sees through (`#Camera`):
  culling, once there is some, keeps what they see; the fly camera has no
  tag and shows the scene as culled for them. The third-person camera
  keeps following the player while the fly camera renders. The crate ships
  both graphs and writes copies to the game's assets when there are none.
  Its `reticle` feature (on in the game) puts a dot at the middle of the
  screen while a third-person camera renders, hidden while it looks
  around freely.
- `crates/character_animation`, portable: animation graphs (a state
  machine of clips) playing on a source skeleton. See below.
- `crates/skeleton_retarget`, portable, Bevy only: runtime retargeting of
  a source skeleton's pose onto a character's, so tools can preview clips
  on a character without the animation graphs.
- `crates/ninja_character`, private (third-party assets, not
  redistributed): what the Ninja looks like (the hair fix) and the Yummy
  clips, registered as graph nodes. Its files stay in
  `assets/character/ninja/`, where the scene references them.

The game (`src/game.rs`) adds Avian, Tnua's Avian backend, Jackdaw and the
crates' plugins, and spawns the scene.

### Animation graph and retargeting

The scene's `#Ninja` (the model under `#Player`) carries
`character_animation::AnimationController`, which runs
`assets/flow/animation.flowgraph.bsn`, a hierarchical state machine:

```text
Entry ─▶ Grounded ▣ ── takeoff (airborne) ─▶ In the Air ▣ (Entry ─▶ Fall)
            ▲                                      │ touchdown (not airborne)
            └── land_done (finished) / land_walking (speed > 0.1) ── Land
Grounded ▣:  Entry ─▶ Idle ── start (Move held) ─▶ Locomotion ◆
             Locomotion ── Move let go ─▶ Stop ◇
             Stop ── finished ─▶ Idle, or ── Move held ─▶ Locomotion
Locomotion ◆ (Blend Space 1D on speed): Walk F 1.4 · Jog F 3.5 · Sprint 6 m/s
Stop ◇ (Select 2D on speed and Locomotion's phase): Walk stop · Jog stop LF / RF
       (by the foot ahead) · Sprint stop; distance ◀─ Stop Distance
```

The top graph sets two variables from Tnua State, `airborne` and `speed`
(the length of `velocity`, the real horizontal speed in m/s); the
machines' conditions read them through get nodes instead of wires.
Locomotion mixes the two clips around the speed and keeps them in phase:
each sample says how many sync cycles (a left step then a right one) its
clip holds and where the first starts, measured on the clips; its `phase`
output (below 0.5: the left foot ahead) goes with the speed into Stop, a
Select 2D: on entering it, it picks the stop clip nearest to the speed and
phase then (each sits at its gait's speed, phase 0.25 for the left foot
ahead, 0.75 for the right), plays it once, and says when it is `finished`.
Its clip is distance matched: the movement graph's Gait node says how far
the character has left to go before it is at rest (`stop_distance`, kept
on the character by Set Stop Distance and read by Stop Distance), and the
clip plays where it has as far left to go by its feet
(`character_animation::ClipMotions`, measured on the glb's `ball_l` and
`ball_r` when it loads), then on by itself once the character is at rest.
A clip starts where its distance left is the controller's: a walk stops
along the walk stop clip's own deceleration (0.83 m in 0.93 s, its `walk_stop`
easing), so that clip starts as it leaves the walk (0.35 s in) and plays at
its own pace. A sprint slows at once then glides on for 1 s, nearly
logarithmically (1.9 m, its `sprint_stop` easing): its clip starts 0.9 s in.
A jog still stops in 0.3 s, skipping the start of its clip. A stop is
from the pace the speed is nearest, as the Stop select picks its clip: a
sprint let down to a walk just before the stick is let go stops as a
sprint, both the controller and the clip. The easings'
values are saved in the movement graph file: edit them there (in Jackdaw,
select the Gait node), not their defaults in `gait.rs`.
`cargo test -p ninja_character --test animation
write_animation_graph -- --ignored` rewrites it as first authored.

- Nodes: **Entry**, **Transition** (`from`, `condition`, `duration` of the
  blend, 0.2 s by default, and its `easing`, linear by default; drawn as an
  arrow from one state to the next, with a pill of its ports halfway, their
  values to edit while it is selected:
  wire a state's `exit` into another's `enter` to add one), **State Machine** (a state holding a machine
  in its node: entering it enters its entry's state) and one node per clip,
  listed by category (Standing idle, Standing walk…): `enter`, `speed` in;
  `exit`, `time` (0 to 1), `finished` out. State ports (grey) take any
  number of wires.
- Each step, machines are stepped from the top down: the first transition
  whose condition holds fires, outer machines' before inner ones', and
  below it machines start from their entry. `AnimationState::path` holds
  the current state of each machine (`["grounded", "locomotion"]`); the
  playback lights them (inner nodes play back under their path,
  `grounded/locomotion`): a ring of green dashes runs round each, and a
  blend space's samples light up with their weight.
- Variables (`FlowGraph::variables`, `var/get/<name>` and `var/set/<name>`
  nodes, `FlowVariables` on the runner) belong to the graph engine,
  `bevy_noodle_jackdaw`.
- Clips are skeleton-agnostic, after Unreal 5's runtime retargeter. The
  canonical skeleton is UE5 Manny's bone naming: a hidden source skeleton
  (the clips' glb) is spawned under the model and plays the clips, and
  every frame its pose is copied onto the model's bones of the same name
  (`skeleton_retarget::RETARGETED`: pelvis, spine, neck, head, arms, legs).
  Each bone turns, in the model's frame, as the source's turned from its
  rest; limbs are first aimed the way the source's point at rest, and the
  pelvis travels as the source's, scaled by the ratio of pelvis heights.
  Fingers, twist bones and hair keep their rest under their parent. A
  character named otherwise gets a `Rig` (canonical name → its own).
- The clips are stored once (one glb for every character); nothing is
  baked per character.
- Not done yet: foot IK, fingers, root motion. The clips' distances are
  measured within about 10 % and in the source skeleton's units (not
  scaled to the character, as the Locomotion speeds). The Tab view shows the
  movement graph only; open the animation graph in Jackdaw's Flow Graph
  window (or `cargo run --example flow_graph`, which draws it without
  running it). Double click a State Machine node to open it, Escape to go
  back up (see the `bevy_noodle_jackdaw` README, "Levels"). Tests:
  `cargo test --workspace`.

### Movement graph (branch `proto/noodle-graph`)

The player's movement is the node graph `assets/flow/movement.flowgraph.bsn`
(Move → Make Vector → Camera Aim → Gait → Tnua Walk and Set Stop
Distance, with Run → Double Tap → Gait for the jog and sprint, and Camera
Aim's `forward` → Tnua Walk's `facing`: the character faces where the
camera aims), run every fixed step by
the crate `crates/bevy_noodle_jackdaw`. `character_controller` ships that
graph read-only (`crates/character_controller/assets/`) and writes a copy
to the game's assets at startup when there is none: that copy is the one to
edit, it is never overwritten.

- Camera Aim (like Camera Relative, which the crate's own graph still
  uses) caps its input's length at 1, so a diagonal on keys is no
  faster than a straight line: scaling before it changes the direction
  only.
- Gait (`crates/character_controller/src/gait.rs`) turns a direction into
  a velocity in m/s: walk 1.4, jog 3.5, sprint 6 (inputs to type in). Each
  change of gait eases the speed along its own curve (start, walk ↔ jog,
  jog → sprint, walk ↔ sprint, and a stop from each), drawn on the node with two handles
  to drag and its duration to type; double-click a curve to zoom onto it.
  While the game plays, the curve under way is lit and marked where it is.
  Tnua Walk's speed is 1, so its motion input is that velocity. Its
  `stop_distance` (m) is how far the character goes before it is at rest:
  what is left of the stop under way, else the stop it would make now; Set
  Stop Distance keeps it on the character (`StopDistance`) for the
  animation graph.

- The crate depends on neither leafwing nor Tnua: its `leafwing` feature
  derives a node per `PlayerAction` variant (outputs by action kind), its
  `tnua` feature adds Tnua Walk and Tnua State nodes.
- Tab shows the graph over the game, drawn with
  [bevy_noodle](https://github.com/gold-silver-copper/bevy_noodle) 0.5
  (MIT), vendored in `crates/bevy_noodle` with routed wires added
  (`EdgeRoute`: waypoints, one curve per stretch; see its CHANGELOG). Nodes
  something flowed through get a bright border and the wires carrying a
  value turn bright and pulse. Edits there (drag a wire off an input to
  cut it) drive the player at once but are not saved.
- The canvas has a dotted background. Each header takes the colour of the
  node's first output type (float green, vec2 yellow, vec3 orange…).
  **Delete** removes the selected nodes and their wires. An unwired input
  shows its value in a field to type in (Enter or a click elsewhere keeps
  it): to double `y`, wire it into Multiply's `a` and type 2 in `b`. The
  buttons in the canvas's top-right corner switch between curved and
  straight wires, and between wires going around the nodes in their way
  (A* on a grid, like react-flow-smart-edge) or straight through (view
  settings, not saved).
- Zip ties, like comfyui-zip-ties: hold **Z** and drag across wires to tie
  them, side by side, through one point. Drag a tie to move it,
  **Shift**-drag to turn it toward the pointer; Z-drag across it or
  double-click it to cut it. Click selects a tie, **Ctrl**-click adds one
  (selected ties move together, **Delete** removes them); right-click
  opens a menu (rotate 90°/45°, colour, remove). A tie follows its nodes
  when they all move together. Ties are saved with the graph (`ties`, with
  an optional `color`; older files may leave them out).
- The node library on the canvas's left (`<` folds it, a category's title
  folds the category) lists every registered node type. Drag one onto the
  canvas to add it. Besides the leafwing/Tnua nodes, the crate registers
  math nodes for float, vec2 and vec3 (add, subtract, multiply, divide,
  min, max, abs, negate, lerp, select, scale, length, dot, normalize,
  split/make, cross, clamp, greater, less), plus logic and/or/not.
- Development builds write the game's own node types (leafwing, Tnua) to
  `assets/flow/nodes.flowcatalog.bsn` at startup (only when it changed).
  The editor extension lists its library from that file, since it does not
  link the game's types, plus the crate's built-in types. How to add a node
  type: `crates/bevy_noodle_jackdaw/README.md`.
- The file is a Jackdaw BSN document, loaded as an asset (saving it
  reloads it in the running game). `cargo test write_movement_graph --
  --ignored` rewrites it as first authored.

Crate tests: `cargo test --all-features` in
`crates/bevy_noodle_jackdaw` (it is its own workspace).

#### In the Jackdaw editor

`crates/bevy_noodle_jackdaw_editor` is a Jackdaw editor extension adding
**Window > Flow Graph**: a toolbar lists the `.flowgraph.bsn` files under
`assets/` (the first one opens), the canvas edits the graph and **Save**
writes it back. Its node library merges every `*.flowcatalog.bsn` under
`assets/`, then the crate's built-in types: run the game once in debug for
it to list new node types of the game.
During Play the nodes and wires light up like in the
game's Tab view: the editor receives the `FlowGraphPlayback` component the
game's runners write on the player and on the Ninja.
Selecting a blend space node and probing its axis (click, or hover)
previews the blend under the axis on the model the catalog names, the
Ninja, retargeted (`crates/pose_preview`; also in `cargo run --example
flow_graph`). `crates/bevy_parkour2_editor` is the project's own extension:
it adds what the editor needs to show the models as the game does, today
the Ninja's hair fix (`crates/ninja_hair`), so Flow Graph stays generic.

Extensions load only into the precompiled editor (`jd editor install
0.20.1-rc.1`, then `jd open` or
`%LOCALAPPDATA%\jackdaw\editors\0.20.1-rc.1\jackdaw.exe` with
`JACKDAW_OPEN_PROJECT` set); the cargo-installed `jackdaw` does not load
them. The VS Code "Éditeur (Jackdaw)" configuration runs `jd open` after
`tools/editor_extension.ps1` (PowerShell 7, `pwsh`), which builds and
installs each extension only when its sources (or those of the crates it
builds on, or the editor's compiler) changed. It builds with the `jd` shipped
with the editor `jackdaw.toml` pins: an editor loads only extensions built
by its own rustc (1.99.0 for 0.20.1-rc.1), and the `jd` on PATH builds
with this repository's toolchain. Each build installs as version
`<version>-dev.<source hash>`, since Jackdaw installs a version only once.
To release by hand, from `crates/bevy_noodle_jackdaw_editor` and with that
`jd` (`%LOCALAPPDATA%\jackdaw\editors\0.20.1-rc.1\jd.exe`):

```
jd build                                  # against the editor's SDK
jd extension pack                         # publisher from Cargo.toml
jd extension install bevy_noodle_jackdaw_editor-0.1.3-x86_64-pc-windows-msvc.jdext --trust-publisher
```

then restart the editor (a version installs once: bump it in
`Cargo.toml` first). `jd extension keygen` creates the signing key
once. The extension's `bevy` crate must match the SDK's exactly (0.20.0
for 0.20.1-rc.1, its subcrates 0.20.1), or `bsn!` fails to compile: the
crates an extension builds on ask for `bevy = "0.20"`, and the script
pins the shim's lock (`.jackdaw/shim/Cargo.lock`) back to the editor's
version when it moved. A standalone `cargo check` resolves Bevy 0.20.1.

## Performance overlay

`src/perf_ui.rs`: in the top right corner, FPS, frame time, entity count,
the process's CPU and RAM, and the player's speed (Avian's
`LinearVelocity`, vertical motion included: green standing, yellow at a
jog, red at a sprint, highlighted past it). F3 hides and shows it; hidden,
it stops updating. CPU and RAM refresh less often and read `N/A` until
known; Bevy has no such diagnostics under `--features dynamic`. The
overlay is not part of the scene: it outlives scene reloads and follows
the active camera.

It is iyes_perf_ui 0.5.0 (Bevy 0.16) ported to Bevy 0.20.1 in
`vendor/iyes_perf_ui` (see its `PORTING.md`). `cargo test --bin
bevy_parkour2 renders_overlay_offscreen -- --ignored` (a GPU, no window)
runs the real scene, walks, toggles F3 and reloads the scene, saving
captures under `target/perf-ui-smoke/<run>/`.

## Build and CI tooling

Run cargo xtask --help for the portable Rust build/test CLI.
Builds, nextest JUnit reports, LLVM coverage and package verification are documented
in [docs/CI.md](docs/CI.md).
Official tagged packages read their Sentry client DSN from Pulumi; local builds
keep reporting disabled. See [Sentry provisioning and packaging](docs/SENTRY.md).
SonarCloud analysis and suite coverage are orchestrated by `cargo xtask ci`;
see [SonarCloud configuration](docs/SONAR.md).

Prototype Basis/WebGPU: `cargo xtask prototype basisu --jobs 4`. See [the prototype guide](docs/BASISU-PROTOTYPE.md) for local serving and validation boundaries.

Functional WebGPU game and GPU screenshot instructions: [docs/WEBGPU-RUNTIME.md](docs/WEBGPU-RUNTIME.md).

Per-target asset quality (`--asset-quality optimized`, Web 2K/RGB with original sources preserved): [docs/ASSET-EXPORT.md](docs/ASSET-EXPORT.md).
