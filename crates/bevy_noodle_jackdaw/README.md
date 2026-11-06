# bevy_noodle_jackdaw

Data-flow node graphs saved as Jackdaw BSN (`*.flowgraph.bsn`), run by the
game for an entity every fixed step (or every frame), and drawn with bevy_noodle (vendored in
`../bevy_noodle`), lighting up what flows while it plays.

- `FlowGraphPlugin`: the graph asset, its loader, the runner, and the
  built-in node types. Put a `FlowGraphRunner { graph }` on an entity to run
  that graph for it, with `RunEveryFrame` to run it every frame (in
  `PostUpdate`) rather than every fixed step: a camera's graph.
- Features:
  - `view`: the bevy_noodle canvas (in-game view, editor extension);
  - `leafwing`: one source node per leafwing-input-manager action
    (`LeafwingFlowNodesPlugin::<Action>`);
  - `tnua`: Tnua Walk (sink: motion, and `facing`, zero to face the
    motion) and Tnua State (source) nodes
    (`TnuaFlowNodesPlugin::<Scheme>`).

Tests: `cargo test --all-features` here (the crate is its own workspace).

## Node types

A graph names each node's type by its `kind`, a string such as
`float/add`. The type says what the node's ports are and what it does.
Graphs with a kind nobody registered still load; the node just gives rest
values (zero, false).

The crate registers its built-in types with `FlowGraphPlugin`:

- `core/make_vector2` and `core/camera_relative`;
- `core/double_tap`: `held` while its button is down, `double` while it is
  held after a second press within `window` seconds;
- math for `float`, `vec2` and `vec3`: add, subtract, multiply, divide,
  min, max, abs, negate, lerp, select, scale, length, dot, normalize,
  split/make, cross, clamp, greater, less;
- `logic`: and, or, not;
- `core/entity` (the first entity with a name), `core/skeleton` (an
  entity's skinned mesh, the first visible one under it) and `core/bone`
  (the skeleton's joint with a name): `Entity` values, found every frame
  (none while nothing matches), never saved.

### Adding a node

Register it on the app, after `FlowGraphPlugin`:

```rust
use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, FlowValue, PortKind};

#[derive(Component)]
struct Stamina(f32);

fn build(app: &mut App) {
    // A source: reads the world for the entity running the graph.
    app.register_flow_node(FlowNodeType::compute(
        "player/stamina",
        "Stamina",
        &[],
        &[("value", PortKind::Float)],
        |world, entity, _inputs| {
            let stamina = world.get::<Stamina>(entity).map_or(0.0, |s| s.0);
            vec![FlowValue::Float(stamina)]
        },
    ))
    // A sink: acts on the world with its inputs.
    .register_flow_node(FlowNodeType::apply(
        "player/spend_stamina",
        "Spend Stamina",
        &[("amount", PortKind::Float)],
        |world, entity, inputs| {
            if let Some(mut stamina) = world.get_mut::<Stamina>(entity) {
                stamina.0 -= inputs[0].as_f32();
            }
        },
    ));
}
```

- **Kind**: `category/name`. The category, the part before `/`, is the
  node's section in the node library ("player" → "Player"). Graph files
  store the kind, so renaming one breaks the graphs using it.
- **Ports**: `(name, PortKind)` with `PortKind` one of `Float`, `Bool`,
  `Vec2`, `Vec3`, `Text`, `Entity` (and more).
  `with_choices(input, f)` makes a text input a list to pick from: `f`
  gets the world the graph is drawn in and the node's inputs, and returns
  the names. Inputs are handed over in that order. A compute node
  returns one value per output, in order and of the declared kinds (a test
  checks this for every registered type).
- **Running**: `compute` closures get `&World`, `apply` closures
  `&mut World`, both with the entity whose `FlowGraphRunner` runs the graph.
  An unwired input gives the value typed on the node (see below), rest by
  default; wires between mismatched ports and cycles give rest values, so
  a node should accept zeros.
- Registering a kind again replaces the type, built-ins included.

The `leafwing` and `tnua` modules are worked examples of node families
registered by a plugin.

### Making it appear in the Jackdaw editor

The editor extension does not link the game, so it cannot see the game's
registry. It lists the crate's built-in types itself. The game's own types
come from a catalog file the game writes:

```rust,ignore
#[cfg(debug_assertions)]
app.add_plugins(bevy_noodle_jackdaw::FlowNodeCatalogPlugin {
    path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/flow/nodes.flowcatalog.bsn"),
});
```

At startup it writes the registered types that are not built-ins (and
built-ins registered differently) to that file, only when they changed.
Run the game once after adding a node, and the editor's library lists it.
The editor merges every `*.flowcatalog.bsn` under `assets/` (first one wins
per kind), then adds the built-ins those files do not name.

The in-game view needs no file: `spawn_flow_graph_editor` takes the
catalog, and `FlowNodeTypes::catalog()` lists every registered type.

### Nodes that remember

`FlowNodeType::step` makes a node with memory: its function gets, besides
the inputs, a `Vec<FlowValue>` it keeps from one step to the next (empty
at first). The runner holds it in `FlowNodeMemory`, by playback id, on the
entity running the graph. Double Tap and the character's Gait are such
nodes.

### Arrows

`FlowNodeType::arrow` makes a type whose nodes views draw as arrows: from
the node wired into its first `State` input to the one its first `State`
output leads to, with its other ports (and their value fields) on a pill
halfway along. A state machine's transitions are such nodes. The graph file
keeps them as nodes; the catalog names their kinds (`arrows`), and
`spawn_flow_graph_editor` draws them so (a bare view reads
`view::FlowArrows`). The pill sits halfway along the wire the arrow would
be, routed like the others, moved across it off the nodes and the other
pills.

A state (a node with a `State` port in and out) the playback says flows
through (`FlowValue::State(true)` on one of them) is the one its machine
is in: views mark it `view::CurrentState` and run a ring of green dashes
round it, in place of the lit border.

### Easings

An `easing` port (`FlowValue::Easing`) carries a curve and a duration: a
cubic Bézier from (0, 0) to (1, 1) with two handles, like CSS's
`cubic-bezier`. `Easing::after(elapsed)` gives the progress, 0 to 1. A
node with easing inputs that says which one is under way on an `easing`
output (the input's index, -1 for none) and how far on a `progress` output
gets that curve marked on the canvas while it plays.

### Blend spaces

`blend` holds the Blend Space 1D (`blend/space_1d`) and 2D
(`blend/space_2d`) nodes. A blend space holds its samples in
`FlowNode::samples` (`BlendSample`: the kind it plays, a clip's say, its
title, and where it sits). `sample_weights` mixes them by the input: in 1D,
the two samples around `x` linearly, the end one alone beyond; in 2D, the
corners of the Delaunay triangle around `(x, y)` by their barycentric
weights, the nearest hull edge outside. Samples play in phase: a clip
holds `SyncCycles` (how many, where the first starts), what the clip says
of itself rather than the blend, and `synced_duration` gives how long the
blend's cycle lasts. These nodes only describe the blend: a player
(`character_animation`) plays it and sets `BlendSpacePhase`, the node's
`phase` output.

Select 2D (`blend/select_2d`) holds samples on a plane too, but plays one
of them, not a mix: `sample_weights` gives the nearest to `(x, y)` (each
axis measured against its span, as drawn) all the weight. A player picks
on entering the state, by the inputs then, plays the clip once and sets
`BlendSelection`: the node's `time` and `finished` outputs. Its `distance`
input says how far what plays has left to go (a character's stop), for the
player to match the clip with.

## Variables and inner graphs

- A graph declares named values in `variables` (`FlowVariable`: name, kind,
  starting value). A get node (`var/get/<name>`, from
  `FlowVariable::get_node`) reads one, a set node (`var/set/<name>`) writes
  it, without a wire between them. A get node reads what its graph's set
  node is fed the same step; else the value set before (by an outer graph,
  an earlier step, or the game through the runner's `FlowVariables`), else
  the declared one.
- A node may hold a graph (`FlowNode::graph`; a type gives its new nodes
  one with `FlowNodeType::with_graph`). Inner graphs run after the graph
  holding them, every step, and play back under the holder's id
  (`grounded/idle`); `FlowGraph::node_at` and `graph_at` follow such paths.

## Levels: graphs inside nodes on the canvas

- A node holding a graph draws it inside, live and read-only, lit by the
  playback like the canvas (a `GraphPreview` canvas, fitted around the
  inner nodes). Previews do not nest: a preview's own holders show none.
- **Double click** the node (or select it and press **Enter**) to open it:
  the canvas zooms into the preview, then shows the inner graph in its
  place, framed, to edit like any other. **Escape** or **Backspace** (the
  pointer over the canvas) goes back up a level, zooming out onto the node;
  the breadcrumb over the top left corner names the levels and goes back to
  the one clicked.
- `FlowGraphView::graph` is the level shown; `outer` holds the levels above
  it. `FlowGraphView::root_graph()` is the file's graph with the edits of
  every level: save and apply that one. `levels::open_level` and
  `close_level` navigate from code.
- To look at a graph file outside the editor, the game has an example:
  `cargo run --example flow_graph -- assets/flow/animation.flowgraph.bsn`.

## Editing a graph on the canvas

- Drag a type from the library onto the canvas to add a node. Drag from an
  output to an input to wire them; drag a wire off an input to cut it.
  **Delete** removes the selected nodes and ties.
- An unwired input shows its value next to its name: a number field per
  axis, a box to tick for a bool, or for an easing its curve (drag its two
  handles) and duration, or for a name with choices a list (click to open
  it; the wheel scrolls it, Escape or a click elsewhere closes it). Click a field and type (the text is
  selected, so typing replaces it); Enter or a click elsewhere keeps it, a
  number that does not read puts the old one back. `,` works as a decimal
  point. To double `y`, wire it into Multiply's `a` and type 2 in `b`.
  Wiring the input hides the field; the input keeps its value for when it
  is unwired again. Values are saved with the graph (the port's `value`,
  `None` at rest). An easing's fields, and all of an arrow's pill's, show
  only while their node is selected: else it shows its ports alone.
- A blend space shows its samples as points on its axis (1D) or plane
  (2D, with the triangles that mix them; a select's, without). Hover a point for its sample's
  name and value; drag it to move the sample (snapped to round values);
  right-click it to remove it. Click away from the points to probe the
  blend: the points light up by how much each would play, and a label
  gives the mix (drag to move the probe, right-click to take it away).
  Drop a library item on the axis to add a sample playing it. While the
  game plays, the input is marked on the axis. A selected blend space
  holding a probe or a selected point opens a slot under its axis
  (`BlendPreviewSlot`, carrying a `BlendPreview` of the point hovered,
  else probed); an app draws the blend there, from the catalog's
  `BlendPreviewAssets` (see the game's `pose_preview` crate).
- Transitions are arrows (see [Arrows](#arrows)): wire a state's `State`
  output into another's `State` input and one goes in between, its pill
  of ports halfway. Cutting either half of an arrow (dragging it off the
  state, or Delete on it), or deleting a state it links, deletes it.
- **Double click** a node holding no graph to frame it, an easing's curve
  or a blend space's axis to zoom onto it; **Escape** zooms back out.
- The buttons in the corner switch between curved and straight wires, and
  between wires going around nodes or straight through them.
- Zip ties: hold **Z** and drag across wires to tie them through one point.
  - Drag a tie to move it; **Shift**-drag turns it.
  - Click selects it; **Ctrl**-click adds it to the selection, or takes it
    out. Selected ties move and are deleted together.
  - Right-click opens a menu: rotate 90° or 45°, colour, remove.
  - Z-drag across a tie, or double-click it, to cut it.
  - A tie moves with its nodes when every node of its wires moves.
- Ties are saved with the graph (`ties`, with an optional `color`).
