//! Character animation as a node graph: a state machine of clips and of
//! state machines inside it, moving between states on conditions (graph
//! variables, a clip's end).
//!
//! Clips are skeleton-agnostic: a library (a glb holding a skeleton and its
//! animations) is registered once, and its clips play on any character
//! through [`skeleton_retarget`]. Put an [`AnimationController`] on the
//! entity above the character's skeleton (its model); the graph runs for
//! that entity, so its Tnua State node reads the controller above it.
//!
//! ```text
//! Entry ─▶ [Grounded ▣] ── Transition (airborne) ─▶ [Airborne ▣]
//!               ▲                                        │
//!               └─ Transition (finished) ─ Land ◀─ Transition (landed)
//!
//! inside Grounded:  Entry ─▶ Idle ⇄ Transitions ⇄ [Locomotion ◆] ─▶ Stops
//! ```
//!
//! - **Entry** (`anim/entry`): the state a machine starts in.
//! - **Facing** (`anim/facing`, [`Facing`]): how the model turns apart
//!   from its body, holding its yaw while the character stands, and
//!   turning back to it as a turn clip turns.
//! - **Root Motion** (`anim/root_motion`): how far the current clip (a
//!   turn, see [`RootMotion`]) turns in all and has turned.
//! - **Aim Twist** (`anim/aim_twist`, [`AimTwist`]): the upper body, from
//!   a bone up, turning toward a direction (the camera's aim).
//! - **Clip** nodes (one kind per registered clip): playing while current;
//!   `speed` scales it, `time` (0 to 1) and `finished` tell how far it is.
//!   A turn, its root motion taken out of its poses ([`RootMotion`]), also
//!   tells how far it `turn`s in all and has `turned` (degrees, left
//!   positive).
//! - **State Machine** (`anim/state_machine`): a state holding a machine
//!   (its node's inner graph). Entering it enters its entry's state; while
//!   it is current, so is one of its states.
//! - **Blend Space 1D / 2D** ([`bevy_noodle_jackdaw::blend`]): a state
//!   playing its samples' clips, mixed by `x` (and `y`), in phase: each clip
//!   plays so that its sync cycles (a left step then a right one, see
//!   [`Clip::sync`]) last the blend of theirs, all starting together. In 1D,
//!   `x` is a speed: below the first sample or beyond the last, the clips
//!   play slower or faster in proportion.
//! - **Select 2D** ([`bevy_noodle_jackdaw::blend`]): a state playing one
//!   of its samples' clips once, the nearest to `x` and `y` as they were on
//!   entering it; `time` and `finished` tell how far it is. The stops: by
//!   the speed and the phase of the gait they stop, the foot ahead. Fed a
//!   `distance` (m) the character has left to go, the clip matches it: it
//!   plays where it has as far left to go by its feet ([`ClipMotions`]),
//!   then on by itself once the character is (all but) at rest, or for a
//!   distance longer than the clip's.
//! - **Transition** (`anim/transition`): from the state wired into `from`,
//!   once `condition` holds, to the state wired from `to`, blending over
//!   `duration` seconds along the `easing` curve (linear by default; the
//!   curve's own duration is unused). A state's transitions are tried in
//!   link order, and
//!   an outer machine's before those inside it: one transition at most per
//!   step, below which machines start from their entry.
//!
//! Conditions read graph variables (`var/get/…` nodes, see
//! [`bevy_noodle_jackdaw::FlowVariable`]), set once in the top graph from
//! Tnua's state, rather than wires running into every machine.

use bevy::gltf::Gltf;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
pub use bevy_noodle_jackdaw::blend::{BlendPreviewAssets, SyncCycles};
use bevy_noodle_jackdaw::blend::{
    BlendSelection, BlendSpacePhase, PreviewClip, SELECT_2D, has_samples, is_blend_space, playrate,
    sample_weights, synced_duration,
};
use bevy_noodle_jackdaw::{
    Easing, FlowGraph, FlowGraphAppExt, FlowGraphPlayback, FlowGraphPlugin, FlowGraphRunner,
    FlowGraphSystems, FlowNode, FlowNodeType, FlowValue, PortKind,
};

mod facing;
mod motion;
mod root;
mod twist;

pub use facing::{FACING, Facing};
pub use motion::{ClipMotion, ClipMotions};
pub use root::{ROOT_MOTION, RootMotion};
pub use skeleton_retarget::{RETARGETED, Retarget, Rig, SkeletonRetargetPlugin};
pub use twist::{AIM_TWIST, AimTwist};

/// Where an app keeps its characters' animation graph, under its assets.
pub const ANIMATION_GRAPH: &str = "flow/animation.flowgraph.bsn";
pub const ENTRY: &str = "anim/entry";
pub const TRANSITION: &str = "anim/transition";
pub const STATE_MACHINE: &str = "anim/state_machine";
/// How deep state machines may nest.
const MAX_DEPTH: usize = 16;
/// A select fed a distance this short (m) or none plays on by itself.
const AT_REST: f32 = 0.02;

/// Animates the character below this entity with an animation graph.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct AnimationController {
    /// The graph's asset path.
    pub graph: String,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self {
            graph: ANIMATION_GRAPH.to_owned(),
        }
    }
}

/// One clip of a [`ClipLibrary`].
#[derive(Clone, Debug)]
pub struct Clip {
    /// Its node kind, `category/name`: the library lists clips by category.
    pub kind: String,
    pub title: String,
    /// The animation's name in the glb.
    pub name: String,
    pub looping: bool,
    /// In seconds.
    pub duration: f32,
    /// Its steps, to play in phase with others in a blend space.
    pub sync: SyncCycles,
    /// A turn's root motion, taken out of its poses.
    pub root: Option<RootMotion>,
}

/// Clips on one source skeleton: a glb holding the skeleton as its first
/// scene, its bones named like UE5 Manny's, and the clips as animations.
#[derive(Clone, Debug)]
pub struct ClipLibrary {
    /// The glb's asset path.
    pub path: String,
    pub clips: Vec<Clip>,
}

#[derive(Clone, Debug)]
struct RegisteredClip {
    library: String,
    name: String,
    looping: bool,
    duration: f32,
    sync: SyncCycles,
    root: Option<RootMotion>,
}

/// Every registered clip, by node kind.
#[derive(Resource, Default)]
struct ClipLibraries(HashMap<String, RegisteredClip>);

pub trait ClipLibraryAppExt {
    /// Make `library`'s clips nodes of animation graphs.
    fn register_clip_library(&mut self, library: ClipLibrary) -> &mut Self;
}

impl ClipLibraryAppExt for App {
    fn register_clip_library(&mut self, library: ClipLibrary) -> &mut Self {
        for clip in library.clips {
            let kind = clip.kind.clone();
            let mut outputs = vec![
                ("exit", PortKind::State),
                ("time", PortKind::Float),
                ("finished", PortKind::Bool),
            ];
            // A turn tells how far it turns, and has turned.
            if clip.root.is_some() {
                outputs.extend([("turn", PortKind::Float), ("turned", PortKind::Float)]);
            }
            let has_root = clip.root.is_some();
            self.register_flow_node(
                FlowNodeType::compute(
                    clip.kind.clone(),
                    clip.title,
                    &[("enter", PortKind::State), ("speed", PortKind::Float)],
                    &outputs,
                    move |world, entity, _| {
                        let Some(state) = world
                            .get::<AnimationState>(entity)
                            .filter(|state| state.kind == kind)
                        else {
                            return Vec::new();
                        };
                        let mut values = vec![
                            FlowValue::State(true),
                            FlowValue::Float(state.time),
                            FlowValue::Bool(state.finished),
                        ];
                        if has_root {
                            let root = world
                                .get_resource::<ClipLibraries>()
                                .and_then(|libraries| libraries.0.get(&kind))
                                .and_then(|clip| clip.root.as_ref());
                            values.extend([
                                FlowValue::Float(
                                    root.map_or(0.0, |root| root.total_turn().to_degrees()),
                                ),
                                FlowValue::Float(
                                    root.map_or(0.0, |root| root.turn_at(state.time).to_degrees()),
                                ),
                            ]);
                        }
                        values
                    },
                )
                .with_default("speed", FlowValue::Float(1.0)),
            );
            // For previews in the editor.
            self.world_mut()
                .get_resource_or_init::<BlendPreviewAssets>()
                .clips
                .push(PreviewClip {
                    kind: clip.kind.clone(),
                    library: library.path.clone(),
                    animation: clip.name.clone(),
                    duration: clip.duration,
                    sync: clip.sync,
                });
            self.world_mut()
                .get_resource_or_init::<ClipLibraries>()
                .0
                .insert(
                    clip.kind,
                    RegisteredClip {
                        library: library.path.clone(),
                        name: clip.name,
                        looping: clip.looping,
                        duration: clip.duration,
                        sync: clip.sync,
                        root: clip.root,
                    },
                );
        }
        self
    }
}

/// Where an animated character's state machine is.
#[derive(Component, Clone, Debug, Default)]
pub struct AnimationState {
    /// The current state of each machine, from the top graph's down to the
    /// current clip's: node ids, joined by `/` in the playback. Empty before
    /// the entry.
    pub path: Vec<String>,
    /// The current clip's kind: in a select, the one it picked; empty in a
    /// blend space.
    pub kind: String,
    /// How far through the clip (or the blend space's cycle), 0 to 1.
    pub time: f32,
    pub finished: bool,
    /// In a blend space, each of its samples' weight.
    pub sample_weights: Vec<f32>,
    /// The blend into the current state.
    blend: Blend,
    /// A select's inputs on entering it.
    selected_at: Vec2,
    /// Seconds in the current state, once its clips started.
    elapsed: f32,
    /// Whether the current state's clips started on the source.
    started: bool,
    /// The weight of every clip playing on the source.
    weights: HashMap<AnimationNodeIndex, f32>,
    /// The weights when the current state was entered, fading out.
    from: HashMap<AnimationNodeIndex, f32>,
}

/// The source skeleton playing a character's clips.
#[derive(Component, Clone, Debug)]
struct AnimationSource {
    holder: Entity,
    library: String,
    gltf: Handle<Gltf>,
    /// How far the library's clips go.
    motions: Handle<ClipMotions>,
    /// Once bound: the player, and its graph's node for each clip kind.
    player: Option<Entity>,
    clips: HashMap<String, AnimationNodeIndex>,
}

/// Animation graphs, their entry, transition and state machine nodes, and
/// the retargeter. Clip nodes come from
/// [`ClipLibraryAppExt::register_clip_library`].
pub struct CharacterAnimationPlugin;

impl Plugin for CharacterAnimationPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FlowGraphPlugin>() {
            app.add_plugins(FlowGraphPlugin);
        }
        if !app.is_plugin_added::<SkeletonRetargetPlugin>() {
            app.add_plugins(SkeletonRetargetPlugin);
        }
        let entry = FlowNodeType::compute(
            ENTRY,
            "Entry",
            &[],
            &[("start", PortKind::State)],
            |_, _, _| vec![FlowValue::State(true)],
        );
        // A new machine starts with its entry, to wire to its first state.
        let machine = FlowGraph {
            nodes: vec![entry.instantiate("entry", Vec2::new(40.0, 40.0))],
            ..default()
        };
        app.register_type::<AnimationController>()
            .init_resource::<ClipLibraries>()
            .init_asset::<ClipMotions>()
            .init_asset_loader::<motion::ClipMotionsLoader>()
            .register_flow_node(entry)
            .register_flow_node(facing::node())
            .register_flow_node(root::node())
            .register_flow_node(twist::node())
            .register_flow_node(
                FlowNodeType::compute(
                    STATE_MACHINE,
                    "State Machine",
                    &[("enter", PortKind::State)],
                    &[("exit", PortKind::State)],
                    // Lit by `step_state_machines` while current.
                    |_, _, _| Vec::new(),
                )
                .with_graph(machine),
            )
            .register_flow_node(
                FlowNodeType::compute(
                    TRANSITION,
                    "Transition",
                    &[
                        ("from", PortKind::State),
                        ("condition", PortKind::Bool),
                        ("duration", PortKind::Float),
                        ("easing", PortKind::Easing),
                    ],
                    &[("to", PortKind::State)],
                    |_, _, inputs| {
                        vec![FlowValue::State(
                            inputs[0].as_state() && inputs[1].as_bool(),
                        )]
                    },
                )
                .with_default("duration", FlowValue::Float(0.2))
                // Drawn as an arrow from one state to the next.
                .arrow(),
            )
            .add_observer(start_controller)
            .add_systems(Update, (spawn_sources, bind_sources).chain())
            .add_systems(
                FixedUpdate,
                step_state_machines.after(FlowGraphSystems::Run),
            )
            // After the body's interpolation (in `RunFixedMainLoop`).
            .add_systems(
                PostUpdate,
                (
                    facing::face,
                    twist::twist.after(skeleton_retarget::RetargetSystems),
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            );
    }
}

fn start_controller(
    add: On<Add<AnimationController>>,
    controllers: Query<&AnimationController>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    let Ok(controller) = controllers.get(add.entity) else {
        return;
    };
    commands.entity(add.entity).insert((
        FlowGraphRunner {
            graph: asset_server.load(&controller.graph),
        },
        AnimationState::default(),
        Facing::default(),
        AimTwist::default(),
        BlendSpacePhase::default(),
        BlendSelection::default(),
    ));
}

/// Animated characters without their source skeleton yet.
type Unsourced = (With<AnimationController>, Without<AnimationSource>);

/// Once a character's graph is loaded, spawn under it a holder for the
/// skeleton of the library its first clip (inner graphs included) comes
/// from, and load the library.
fn spawn_sources(
    controllers: Query<(Entity, &FlowGraphRunner), Unsourced>,
    graphs: Res<Assets<FlowGraph>>,
    libraries: Res<ClipLibraries>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
) {
    for (entity, runner) in &controllers {
        let Some(graph) = graphs.get(&runner.graph) else {
            continue;
        };
        let Some(library) = first_library(graph, &libraries) else {
            continue;
        };
        let holder = commands
            .spawn((
                Name::new("Source skeleton"),
                Transform::IDENTITY,
                ChildOf(entity),
            ))
            .id();
        commands.entity(entity).insert(AnimationSource {
            holder,
            gltf: asset_server.load(library.clone()),
            // By type: the same file, measured.
            motions: asset_server.load::<ClipMotions>(library.clone()),
            library,
            player: None,
            clips: HashMap::default(),
        });
    }
}

/// Once the library is loaded, spawn its scene in the holder. Once both
/// skeletons are spawned, still at rest: give the source's player the
/// library's clips, and bind the character's skeleton to it.
///
/// The scene comes from the loaded library rather than its own load:
/// loading `library#Scene0` alongside `library` can load the file twice,
/// and the second load respawns the scene, player and all.
fn bind_sources(world: &mut World) {
    let unbound: Vec<(Entity, AnimationSource)> = world
        .query::<(Entity, &AnimationSource)>()
        .iter(world)
        .filter(|(_, source)| source.player.is_none())
        .map(|(entity, source)| (entity, source.clone()))
        .collect();
    for (entity, mut source) in unbound {
        // Read licensed sample data through Bevy's asset loader on every platform.
        // Waiting here also prevents a turn starting before its curves are ready.
        let Some(motions) = world.resource::<Assets<ClipMotions>>().get(&source.motions) else {
            continue;
        };
        let roots = motions.roots().clone();
        let missing_root = world.resource::<ClipLibraries>().0.values().any(|clip| {
            clip.library == source.library
                && clip
                    .root
                    .as_ref()
                    .is_some_and(|root| root.turn.is_empty() && !roots.contains_key(&clip.name))
        });
        if missing_root {
            warn_once!("Animation library is missing required root motion metadata");
            continue;
        }
        for clip in world.resource_mut::<ClipLibraries>().0.values_mut() {
            if clip.library == source.library
                && let Some(root) = roots.get(&clip.name)
            {
                clip.root = Some(root.clone());
            }
        }
        let Some(gltf) = world.resource::<Assets<Gltf>>().get(&source.gltf) else {
            continue;
        };
        if world.get::<WorldAssetRoot>(source.holder).is_none() {
            let Some(scene) = gltf.default_scene.clone().or(gltf.scenes.first().cloned()) else {
                continue;
            };
            world
                .entity_mut(source.holder)
                .insert(WorldAssetRoot(scene));
            continue;
        }
        let Some(player) = find_player(world, source.holder) else {
            continue;
        };
        let Some(retarget) = Retarget::bind(world, source.holder, entity, world.get::<Rig>(entity))
        else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        for (kind, clip) in &world.resource::<ClipLibraries>().0 {
            if clip.library != source.library {
                continue;
            }
            match gltf.named_animations.get(clip.name.as_str()) {
                Some(handle) => {
                    let index = graph.add_clip(handle.clone(), 1.0, graph.root);
                    source.clips.insert(kind.clone(), index);
                }
                None => warn!("{}: no animation {}", source.library, clip.name),
            }
        }
        let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
        world.entity_mut(player).insert(AnimationGraphHandle(graph));
        source.player = Some(player);
        world.entity_mut(entity).insert((source, retarget));
    }
}

/// The source skeleton's player: on its scene's top node.
fn find_player(world: &World, holder: Entity) -> Option<Entity> {
    let mut stack = vec![holder];
    while let Some(entity) = stack.pop() {
        if world.get::<AnimationPlayer>(entity).is_some() {
            return Some(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

/// Whether `node` is a state: a clip, a blend space, a select or a state
/// machine.
fn is_state(node: &FlowNode, libraries: &ClipLibraries) -> bool {
    node.kind == STATE_MACHINE || has_samples(&node.kind) || libraries.0.contains_key(&node.kind)
}

/// The library of `graph`'s first clip, blend space samples and inner
/// graphs included.
fn first_library(graph: &FlowGraph, libraries: &ClipLibraries) -> Option<String> {
    graph.nodes.iter().find_map(|node| {
        let library = |kind: &str| libraries.0.get(kind).map(|clip| clip.library.clone());
        library(&node.kind)
            .or_else(|| node.samples.iter().find_map(|s| library(&s.kind)))
            .or_else(|| first_library(node.graph.as_ref()?, libraries))
    })
}

/// A transition's crossfade into its state: over `duration` seconds, along
/// `easing`'s curve (whose own duration is unused).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Blend {
    duration: f32,
    easing: Easing,
}

impl Blend {
    /// Whether the crossfade is over `elapsed` seconds in.
    fn done(&self, elapsed: f32) -> bool {
        elapsed >= self.duration
    }

    /// How far the state faded in, 0 to 1, `elapsed` seconds in.
    fn fade(&self, elapsed: f32) -> f32 {
        if self.done(elapsed) {
            return 1.0;
        }
        self.easing.at(elapsed / self.duration).clamp(0.0, 1.0)
    }
}

/// The current states after this step, from `graph`'s down, and the blend
/// into them if a transition led there. Going down: a machine whose state
/// is still current tries its transitions, unless one above fired this
/// step; a machine just entered (or with no valid current state) enters
/// its entry's state. `inputs` gives a node's inputs by its path.
fn next_path(
    graph: &FlowGraph,
    current: &[String],
    is_state: &dyn Fn(&FlowNode) -> bool,
    inputs: &dyn Fn(&str) -> Vec<FlowValue>,
) -> (Vec<String>, Blend) {
    let mut path = Vec::new();
    let mut blend = Blend::default();
    let mut level = graph;
    let mut prefix = String::new();
    // Once a machine changed state, those below start from their entry.
    let mut entered = false;
    for depth in 0..MAX_DEPTH {
        let stay = current
            .get(depth)
            .filter(|id| !entered && level.node(id).is_some_and(is_state));
        let next = match stay {
            Some(id) => {
                let inputs = |transition: &str| inputs(&format!("{prefix}{transition}"));
                match fired_transition(level, id, &inputs) {
                    Some((to, fired)) => {
                        entered = true;
                        blend = fired;
                        Some(to)
                    }
                    None => Some(id.clone()),
                }
            }
            None => {
                entered = true;
                entry_state(level)
            }
        };
        let Some(node) = next
            .and_then(|id| level.node(&id))
            .filter(|node| is_state(node))
        else {
            break;
        };
        path.push(node.id.clone());
        match node.graph.as_ref().filter(|_| node.kind == STATE_MACHINE) {
            Some(inner) => {
                prefix = format!("{prefix}{}/", node.id);
                level = inner;
            }
            None => break,
        }
    }
    (path, blend)
}

/// An animated character: its graph, where it is, and its playback.
type Animated<'a> = (
    &'a FlowGraphRunner,
    &'a mut AnimationState,
    &'a mut BlendSpacePhase,
    &'a mut BlendSelection,
    Option<&'a mut FlowGraphPlayback>,
    Option<&'a AnimationSource>,
);

/// After the graphs ran: step each machine from the top graph's down (see
/// [`next_path`]), then play the current state's clips, crossfading from
/// what played before, and light the current states up in the playback.
fn step_state_machines(
    graphs: Res<Assets<FlowGraph>>,
    libraries: Res<ClipLibraries>,
    motions: Res<Assets<ClipMotions>>,
    time: Res<Time>,
    mut controllers: Query<Animated>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (runner, mut state, mut phase, mut selection, playback, source) in &mut controllers {
        let Some(graph) = graphs.get(&runner.graph) else {
            continue;
        };
        let is_state = |node: &FlowNode| is_state(node, &libraries);
        let node_inputs = |id: &str| {
            playback
                .as_deref()
                .and_then(|p| p.node(id))
                .map(|n| n.inputs.clone())
                .unwrap_or_default()
        };
        let (path, blend) = next_path(graph, &state.path, &is_state, &node_inputs);
        if path != state.path {
            let leaf = path.join("/");
            let node = graph.node_at(&leaf);
            // A select picks on entering, by its inputs then.
            let inputs = node_inputs(&leaf);
            let input = |index: usize| inputs.get(index).map_or(0.0, |v| v.as_f32());
            let selected_at = Vec2::new(input(1), input(2));
            let kind = match node {
                Some(node) if node.kind == SELECT_2D => {
                    selected(node, selected_at).unwrap_or_default().to_owned()
                }
                Some(node) if libraries.0.contains_key(&node.kind) => node.kind.clone(),
                _ => String::new(),
            };
            let from = std::mem::take(&mut state.weights);
            *state = AnimationState {
                path,
                kind,
                blend,
                selected_at,
                weights: from.clone(),
                from,
                ..default()
            };
        }
        let leaf = state.path.join("/");
        let Some(node) = graph.node_at(&leaf) else {
            continue;
        };
        let inputs = node_inputs(&leaf);
        let input = |index: usize| inputs.get(index).map_or(0.0, |v| v.as_f32());
        let in_blend_space = is_blend_space(&node.kind);
        let in_select = node.kind == SELECT_2D;
        // A blend space's, or what a select picked by.
        let at = if in_select {
            state.selected_at
        } else {
            Vec2::new(input(1), input(2))
        };
        let plays = plays(node, at, &libraries);

        let player = source.and_then(|s| Some((s, players.get_mut(s.player?).ok()?)));
        if let Some((source, mut player)) = player {
            let indexed: Vec<(AnimationNodeIndex, &Played)> = plays
                .iter()
                .filter_map(|played| Some((*source.clips.get(&played.kind)?, played)))
                .collect();
            if !state.started {
                for (index, played) in &indexed {
                    let active = player.start(*index);
                    if played.looping {
                        active.repeat();
                    }
                    // Samples start in phase, at their first cycle.
                    if in_blend_space {
                        active.seek_to(played.sync.start() * played.duration);
                    }
                }
                state.started = true;
            } else {
                state.elapsed += time.delta_secs();
            }
            // Crossfade: what played before fades out as this fades in.
            let fade = state.blend.fade(state.elapsed);
            let mut weights: HashMap<AnimationNodeIndex, f32> = state
                .from
                .iter()
                .map(|(index, weight)| (*index, weight * (1.0 - fade)))
                .collect();
            for (index, played) in &indexed {
                *weights.entry(*index).or_default() += played.weight * fade;
            }
            for (index, weight) in &weights {
                if let Some(active) = player.animation_mut(*index) {
                    active.set_weight(*weight);
                }
            }
            if state.blend.done(state.elapsed) {
                let current: Vec<AnimationNodeIndex> = indexed.iter().map(|(i, _)| *i).collect();
                weights.retain(|index, _| {
                    let kept = current.contains(index);
                    if !kept {
                        player.stop(*index);
                    }
                    kept
                });
                state.from.clear();
            }
            state.weights = weights;

            // Speeds, and how far through the state is.
            let cycle = synced_duration(
                &plays.iter().map(Played::cycle).collect::<Vec<_>>(),
                &plays.iter().map(|p| p.weight).collect::<Vec<_>>(),
            );
            let speed = if in_blend_space {
                playrate(node, at)
            } else if in_select {
                1.0
            } else {
                inputs.get(1).map_or(1.0, |v| v.as_f32())
            };
            for (index, played) in &indexed {
                if let Some(active) = player.animation_mut(*index) {
                    let synced = if in_blend_space {
                        played.cycle() / cycle.max(1e-3)
                    } else {
                        1.0
                    };
                    active.set_speed(speed * synced);
                }
            }
            // A select fed how far the character has left to go plays its
            // clip where the clip has as far left, never back.
            let distance = input(3);
            let motion = motions
                .get(&source.motions)
                .zip(indexed.first())
                .and_then(|(motions, (index, played))| Some((*index, motions.get(&played.name)?)));
            if in_select
                && distance > AT_REST
                && let Some((index, motion)) = motion
                && distance <= motion.total()
                && let Some(active) = player.animation_mut(index)
            {
                let at = motion.time_left(distance);
                active.set_speed(0.0);
                if at > active.seek_time() {
                    active.seek_to(at);
                }
            }
            let heaviest = indexed
                .iter()
                .max_by(|a, b| a.1.weight.total_cmp(&b.1.weight));
            if let Some((index, played)) = heaviest
                && let Some(active) = player.animation(*index)
            {
                let time = active.seek_time() / played.duration.max(1e-3);
                state.time = if in_blend_space {
                    played.sync.phase_at(time)
                } else if played.looping {
                    time.rem_euclid(1.0)
                } else {
                    time.clamp(0.0, 1.0)
                };
                state.finished = active.is_finished();
            }
        }
        state.sample_weights = if has_samples(&node.kind) {
            sample_weights(node, at)
        } else {
            Vec::new()
        };
        phase.set_if_neq(BlendSpacePhase(if in_blend_space {
            state.time
        } else {
            0.0
        }));
        selection.set_if_neq(if in_select {
            BlendSelection {
                time: state.time,
                finished: state.finished,
            }
        } else {
            BlendSelection::default()
        });

        if let Some(mut playback) = playback {
            // The current state of every machine on the way down.
            let current: Vec<String> = (1..=state.path.len())
                .map(|depth| state.path[..depth].join("/"))
                .collect();
            let mut lit = playback.clone();
            for node in &mut lit.nodes {
                if !graph.node_at(&node.id).is_some_and(is_state) {
                    continue;
                }
                let current = current.contains(&node.id);
                if let Some(enter) = node.inputs.first_mut() {
                    *enter = FlowValue::State(current);
                }
                if let Some(exit) = node.outputs.first_mut() {
                    *exit = FlowValue::State(current);
                }
            }
            // A blend space's phase, set after the graph ran.
            if in_blend_space
                && let Some(space) = lit.nodes.iter_mut().find(|state| state.id == leaf)
                && let Some(output) = space.outputs.get_mut(1)
            {
                *output = FlowValue::Float(state.time);
            }
            // A select's: what it picked by, and how far its clip is.
            if in_select && let Some(select) = lit.nodes.iter_mut().find(|state| state.id == leaf) {
                for (index, value) in [(1, state.selected_at.x), (2, state.selected_at.y)] {
                    if let Some(input) = select.inputs.get_mut(index) {
                        *input = FlowValue::Float(value);
                    }
                }
                if let [_, time, finished, ..] = &mut select.outputs[..] {
                    *time = FlowValue::Float(state.time);
                    *finished = FlowValue::Bool(state.finished);
                }
            }
            playback.set_if_neq(lit);
        }
    }
}

/// A clip a state plays, and how much.
struct Played {
    kind: String,
    /// The clip's animation in its library.
    name: String,
    weight: f32,
    looping: bool,
    duration: f32,
    sync: SyncCycles,
}

impl Played {
    /// How long one of its sync cycles lasts.
    fn cycle(&self) -> f32 {
        self.duration / self.sync.cycles
    }
}

/// What state `node` plays: its clip, or its samples' with its input at
/// `at` (a blend space's).
fn plays(node: &FlowNode, at: Vec2, libraries: &ClipLibraries) -> Vec<Played> {
    let played = |kind: &str, weight: f32| {
        let clip = libraries.0.get(kind)?;
        Some(Played {
            kind: kind.to_owned(),
            name: clip.name.clone(),
            weight,
            looping: clip.looping,
            duration: clip.duration,
            sync: clip.sync,
        })
    };
    if node.kind == SELECT_2D {
        return selected(node, at)
            .and_then(|kind| played(kind, 1.0))
            .into_iter()
            .collect();
    }
    if !is_blend_space(&node.kind) {
        return played(&node.kind, 1.0).into_iter().collect();
    }
    let mut plays: Vec<Played> = Vec::new();
    for (sample, weight) in node.samples.iter().zip(sample_weights(node, at)) {
        // A clip played by two samples plays once, with both weights.
        match plays.iter_mut().find(|p| p.kind == sample.kind) {
            Some(same) => same.weight += weight,
            None => plays.extend(played(&sample.kind, weight)),
        }
    }
    plays
}

/// The kind of the sample select `node` picks with its inputs at `at`.
fn selected(node: &FlowNode, at: Vec2) -> Option<&str> {
    let weights = sample_weights(node, at);
    let index = weights.iter().position(|weight| *weight > 0.0)?;
    Some(node.samples[index].kind.as_str())
}

/// The state the entry's `start` leads to.
fn entry_state(graph: &FlowGraph) -> Option<String> {
    let entry = graph.nodes.iter().find(|node| node.kind == ENTRY)?;
    graph
        .links
        .iter()
        .find(|link| link.from == entry.id && link.output == "start")
        .map(|link| link.to.clone())
}

/// The state `current`'s first transition whose condition holds leads to,
/// and its blend.
fn fired_transition(
    graph: &FlowGraph,
    current: &str,
    inputs: &dyn Fn(&str) -> Vec<FlowValue>,
) -> Option<(String, Blend)> {
    graph
        .links
        .iter()
        .filter(|link| link.from == current && link.output == "exit" && link.input == "from")
        .filter(|link| graph.node(&link.to).is_some_and(|n| n.kind == TRANSITION))
        .find_map(|link| {
            let inputs = inputs(&link.to);
            if !inputs.get(1)?.as_bool() {
                return None;
            }
            let to = graph
                .links
                .iter()
                .find(|out| out.from == link.to && out.output == "to")?;
            let blend = Blend {
                duration: inputs.get(2).map_or(0.0, |v| v.as_f32()),
                easing: inputs.get(3).map_or(Easing::LINEAR, |v| v.as_easing()),
            };
            Some((to.to.clone(), blend))
        })
}

#[cfg(test)]
mod tests {
    use bevy::platform::collections::HashMap;
    use bevy_noodle_jackdaw::FlowLink;

    use super::*;

    fn node(id: &str, kind: &str) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: kind.into(),
            ..default()
        }
    }

    fn machine(id: &str, graph: FlowGraph) -> FlowNode {
        FlowNode {
            graph: Some(graph),
            ..node(id, STATE_MACHINE)
        }
    }

    /// `from` → transition `id` → `to`.
    fn transition(graph: &mut FlowGraph, id: &str, from: &str, to: &str) {
        graph.nodes.push(node(id, TRANSITION));
        graph.links.push(FlowLink::new(from, "exit", id, "from"));
        graph.links.push(FlowLink::new(id, "to", to, "enter"));
    }

    fn entry(graph: &mut FlowGraph, to: &str) {
        graph.nodes.push(node("entry", ENTRY));
        graph
            .links
            .push(FlowLink::new("entry", "start", to, "enter"));
    }

    /// Grounded (Idle ⇄ Walk) ⇄ Airborne (Fall).
    fn graph() -> FlowGraph {
        let mut grounded = FlowGraph {
            nodes: vec![node("idle", "clip/idle"), node("walk", "clip/walk")],
            ..default()
        };
        entry(&mut grounded, "idle");
        transition(&mut grounded, "start_walking", "idle", "walk");
        let mut airborne = FlowGraph {
            nodes: vec![node("fall", "clip/fall")],
            ..default()
        };
        entry(&mut airborne, "fall");
        let mut top = FlowGraph {
            nodes: vec![machine("grounded", grounded), machine("airborne", airborne)],
            ..default()
        };
        entry(&mut top, "grounded");
        transition(&mut top, "takeoff", "grounded", "airborne");
        top
    }

    /// The path after a step, and the blend's duration.
    fn step(current: &[&str], firing: &[(&str, f32)]) -> (Vec<String>, f32) {
        let firing: HashMap<String, f32> =
            firing.iter().map(|(id, d)| (id.to_string(), *d)).collect();
        let inputs = |id: &str| match firing.get(id) {
            Some(duration) => vec![
                FlowValue::State(true),
                FlowValue::Bool(true),
                FlowValue::Float(*duration),
            ],
            None => vec![],
        };
        let is_state =
            |node: &FlowNode| node.kind == STATE_MACHINE || node.kind.starts_with("clip/");
        let current: Vec<String> = current.iter().map(|id| id.to_string()).collect();
        let (path, blend) = next_path(&graph(), &current, &is_state, &inputs);
        (path, blend.duration)
    }

    #[test]
    fn the_entry_leads_down_into_machines() {
        assert_eq!(
            step(&[], &[]),
            (vec!["grounded".into(), "idle".into()], 0.0)
        );
    }

    #[test]
    fn an_inner_transition_changes_the_inner_state() {
        let (path, blend) = step(&["grounded", "idle"], &[("grounded/start_walking", 0.3)]);
        assert_eq!(path, ["grounded", "walk"]);
        assert_eq!(blend, 0.3);
    }

    #[test]
    fn an_outer_transition_wins_and_enters_the_next_machine() {
        let (path, blend) = step(
            &["grounded", "idle"],
            &[("grounded/start_walking", 0.3), ("takeoff", 0.2)],
        );
        assert_eq!(path, ["airborne", "fall"]);
        assert_eq!(blend, 0.2);
    }

    #[test]
    fn a_state_gone_from_the_graph_restarts_its_machine() {
        assert_eq!(step(&["grounded", "gone"], &[]).0, ["grounded", "idle"]);
        assert_eq!(step(&["gone", "idle"], &[]).0, ["grounded", "idle"]);
    }

    #[test]
    fn a_blend_fades_in_along_its_easing() {
        let linear = Blend {
            duration: 0.4,
            ..default()
        };
        assert!((linear.fade(0.1) - 0.25).abs() < 1e-3);
        let eased = Blend {
            easing: Easing::ease_in_out(0.0),
            ..linear
        };
        assert!(eased.fade(0.1) < 0.25);
        assert!((eased.fade(0.2) - 0.5).abs() < 1e-3);
        assert_eq!(eased.fade(0.4), 1.0);
        assert!(eased.done(0.4) && !eased.done(0.39));
        assert_eq!(Blend::default().fade(0.0), 1.0);
    }

    #[test]
    fn nothing_firing_keeps_the_states() {
        assert_eq!(step(&["grounded", "walk"], &[]).0, ["grounded", "walk"]);
    }

    fn clip(kind: &str, root: Option<RootMotion>) -> Clip {
        Clip {
            kind: kind.into(),
            title: kind.into(),
            name: kind.into(),
            looping: false,
            duration: 1.0,
            sync: default(),
            root,
        }
    }

    #[test]
    fn turns_tell_how_far_they_turn() {
        let mut app = App::new();
        app.register_clip_library(ClipLibrary {
            path: "clips.glb".into(),
            clips: vec![
                clip("clip/idle", None),
                clip(
                    "clip/turn",
                    Some(RootMotion {
                        turn: vec![0.0, 1.0, 2.0],
                        travel: vec![Vec2::ZERO; 3],
                    }),
                ),
            ],
        });
        let types = app.world().resource::<bevy_noodle_jackdaw::FlowNodeTypes>();
        let outputs = |kind: &str| -> Vec<String> {
            let outputs = &types.get(kind).unwrap().outputs;
            outputs.iter().map(|port| port.name.clone()).collect()
        };
        assert_eq!(outputs("clip/idle"), ["exit", "time", "finished"]);
        assert_eq!(
            outputs("clip/turn"),
            ["exit", "time", "finished", "turn", "turned"]
        );

        let bevy_noodle_jackdaw::NodeRun::Compute(compute) =
            types.get("clip/turn").unwrap().run.clone()
        else {
            panic!("a clip computes");
        };
        let playing = app
            .world_mut()
            .spawn(AnimationState {
                kind: "clip/turn".into(),
                time: 0.25,
                ..default()
            })
            .id();
        let values = compute(app.world(), playing, &[]);
        assert_eq!(values[3].as_f32(), 2.0_f32.to_degrees());
        assert_eq!(values[4].as_f32(), 0.5_f32.to_degrees());
    }
}
