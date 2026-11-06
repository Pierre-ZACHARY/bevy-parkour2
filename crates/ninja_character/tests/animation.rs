//! The Ninja on a character, animated by the game's animation graph
//! (`assets/flow/animation.flowgraph.bsn`) with Yummy clips retargeted onto
//! it: idle standing still, walking, jogging or sprinting by its speed
//! (a blend space), stopping (a select), falling in the air and landing.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::gltf::{GltfAssetLabel, GltfLoaderSettings};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::world_serialization::WorldAssetRoot;
use bevy_noodle_jackdaw::blend::{BLEND_SPACE_1D, SELECT_2D};
use bevy_noodle_jackdaw::leafwing::action_kind;
use bevy_noodle_jackdaw::{
    BlendSample, FlowGraph, FlowGraphPlayback, FlowGraphPlugin, FlowLink, FlowNode, FlowNodeTypes,
    FlowValue, FlowVariable, PortKind,
};
use bevy_tnua_avian3d::TnuaAvian3dPlugin;
use character_animation::{
    ANIMATION_GRAPH, AnimationController, AnimationState, CharacterAnimationPlugin, ClipMotions,
    ENTRY, RETARGETED, Retarget, STATE_MACHINE, TRANSITION,
};
use character_controller::{
    CharacterController, CharacterControllerPlugin, FLOAT_HEIGHT, JOG_SPEED, MOVEMENT_GRAPH,
    PlayerAction, SPRINT_SPEED, STOP_DISTANCE, StopDistance, WALK_SPEED,
};
use leafwing_input_manager::prelude::*;
use ninja_character::YummyClipsPlugin;
use ninja_character::yummy::{
    FALL, IDLE, JOG_FORWARD, JOG_STOP_LF, JOG_STOP_RF, LAND, LIBRARY, SPRINT, SPRINT_STOP,
    WALK_FORWARD, WALK_STOP,
};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
/// Below this speed (m/s), the Ninja idles.
const WALKING: f32 = 0.1;
/// Below this length, the Move stick is let go (the Gait node's dead zone).
const LET_GO: f32 = 0.05;

/// Builds graphs from the registered node types.
struct Nodes<'a>(&'a FlowNodeTypes);

impl Nodes<'_> {
    fn node(&self, id: &str, kind: &str, x: f32, y: f32) -> FlowNode {
        self.0
            .get(kind)
            .unwrap_or_else(|| panic!("{kind} is registered"))
            .instantiate(id, Vec2::new(x, y))
    }

    /// A node with input `input` set to `value`.
    fn with(&self, mut node: FlowNode, input: &str, value: FlowValue) -> FlowNode {
        let port = node.inputs.iter_mut().find(|p| p.name == input).unwrap();
        port.value = Some(value);
        node
    }

    fn transition(&self, id: &str, duration: f32, x: f32, y: f32) -> FlowNode {
        let node = self.node(id, TRANSITION, x, y);
        self.with(node, "duration", FlowValue::Float(duration))
    }

    fn machine(&self, id: &str, title: &str, graph: FlowGraph, x: f32, y: f32) -> FlowNode {
        FlowNode {
            title: title.into(),
            graph: Some(graph),
            ..self.node(id, STATE_MACHINE, x, y)
        }
    }

    /// A float comparison node (`float/less`, `float/greater`) against `b`.
    fn compare(&self, id: &str, kind: &str, b: f32, x: f32, y: f32) -> FlowNode {
        let node = self.node(id, kind, x, y);
        self.with(node, "b", FlowValue::Float(b))
    }

    /// A blend space sample playing clip `kind` at speed `at`.
    fn sample(&self, kind: &str, at: f32) -> BlendSample {
        self.sample_2d(kind, Vec2::new(at, 0.0))
    }

    /// A sample playing clip `kind` at `at`.
    fn sample_2d(&self, kind: &str, at: Vec2) -> BlendSample {
        BlendSample {
            kind: kind.into(),
            title: self.0.get(kind).expect("a clip").title.clone(),
            at,
        }
    }
}

/// `from` → `transition` → `to`, on `condition` (a node and its output).
fn transition_links(
    from: &str,
    transition: &str,
    to: &str,
    condition: (&str, &str),
) -> [FlowLink; 3] {
    [
        FlowLink::new(from, "exit", transition, "from"),
        FlowLink::new(transition, "to", to, "enter"),
        FlowLink::new(condition.0, condition.1, transition, "condition"),
    ]
}

/// On the ground: idle until asked to move, then walking, jogging or
/// sprinting by the speed (the Locomotion blend space), and once the Move
/// stick is let go, stopping by how fast it went (from a jog, on the foot
/// ahead: the Stop select, its clip matched to how far the character has
/// left to go) before idling again. In the air: falling, then landing. The top
/// graph sets the `airborne` and `speed` (m/s) variables from Tnua's state;
/// the machines read them.
fn default_animation_graph(types: &FlowNodeTypes) -> FlowGraph {
    let nodes = Nodes(types);
    let airborne = FlowVariable::new("airborne", PortKind::Bool);
    let speed = FlowVariable::new("speed", PortKind::Float);
    let threshold = |node: FlowNode| nodes.with(node, "b", FlowValue::Float(WALKING));
    // Each stop at its gait's speed and Locomotion's phase with its foot
    // ahead (the left in the first half of the cycle); a walk's and a
    // sprint's on either foot.
    let stop = |kind: &str, speed: f32, phase: f32| nodes.sample_2d(kind, Vec2::new(speed, phase));
    let (left, right) = (0.25, 0.75);

    let grounded = FlowGraph {
        nodes: vec![
            nodes.node("entry", ENTRY, 40.0, 260.0),
            nodes.node("move", &action_kind(&PlayerAction::Move), 40.0, -200.0),
            nodes.node("stick", "vec2/length", 300.0, -200.0),
            nodes.compare("moving", "float/greater", LET_GO, 560.0, -260.0),
            nodes.node("let_go", "logic/not", 820.0, -200.0),
            speed.get_node("speed", Vec2::new(40.0, 660.0)),
            nodes.node("stop_distance", STOP_DISTANCE, 1140.0, 760.0),
            nodes.node("idle", IDLE, 300.0, 260.0),
            nodes.transition("start", 0.2, 580.0, 40.0),
            FlowNode {
                title: "Locomotion".into(),
                // Each at its gait's speed.
                samples: vec![
                    nodes.sample(WALK_FORWARD, WALK_SPEED),
                    nodes.sample(JOG_FORWARD, JOG_SPEED),
                    nodes.sample(SPRINT, SPRINT_SPEED),
                ],
                ..nodes.node("locomotion", BLEND_SPACE_1D, 860.0, 260.0)
            },
            nodes.transition("stopping", 0.2, 1140.0, 40.0),
            FlowNode {
                title: "Stop".into(),
                samples: vec![
                    stop(WALK_STOP, WALK_SPEED, left),
                    stop(WALK_STOP, WALK_SPEED, right),
                    stop(JOG_STOP_LF, JOG_SPEED, left),
                    stop(JOG_STOP_RF, JOG_SPEED, right),
                    stop(SPRINT_STOP, SPRINT_SPEED, left),
                    stop(SPRINT_STOP, SPRINT_SPEED, right),
                ],
                ..nodes.node("stop", SELECT_2D, 1420.0, 260.0)
            },
            nodes.transition("stop_moving", 0.2, 1140.0, 560.0),
            nodes.transition("stop_done", 0.3, 860.0, 700.0),
        ],
        links: [
            vec![
                FlowLink::new("entry", "start", "idle", "enter"),
                FlowLink::new("move", "vector", "stick", "vector"),
                FlowLink::new("stick", "length", "moving", "a"),
                FlowLink::new("moving", "result", "let_go", "value"),
                FlowLink::new("speed", "value", "locomotion", "x"),
                FlowLink::new("speed", "value", "stop", "x"),
                FlowLink::new("locomotion", "phase", "stop", "y"),
                FlowLink::new("stop_distance", "distance", "stop", "distance"),
            ],
            transition_links("idle", "start", "locomotion", ("moving", "result")).to_vec(),
            transition_links("locomotion", "stopping", "stop", ("let_go", "result")).to_vec(),
            // Back to Locomotion if asked to move again, else to Idle once
            // it is done.
            transition_links("stop", "stop_moving", "locomotion", ("moving", "result")).to_vec(),
            transition_links("stop", "stop_done", "idle", ("stop", "finished")).to_vec(),
        ]
        .concat(),
        ..default()
    };
    let in_the_air = FlowGraph {
        nodes: vec![
            nodes.node("entry", ENTRY, 40.0, 40.0),
            nodes.node("fall", FALL, 300.0, 40.0),
        ],
        links: vec![FlowLink::new("entry", "start", "fall", "enter")],
        ..default()
    };
    FlowGraph {
        nodes: vec![
            nodes.node("entry", ENTRY, 40.0, 40.0),
            nodes.machine("grounded", "Grounded", grounded, 300.0, 40.0),
            airborne.get_node("airborne", Vec2::new(300.0, -160.0)),
            nodes.transition("takeoff", 0.25, 600.0, -60.0),
            nodes.machine("in_the_air", "In the Air", in_the_air, 900.0, 40.0),
            nodes.node("landed", "logic/not", 900.0, -160.0),
            nodes.transition("touchdown", 0.1, 1180.0, 160.0),
            nodes.node("land", LAND, 900.0, 300.0),
            nodes.transition("land_done", 0.3, 600.0, 300.0),
            speed.get_node("speed", Vec2::new(900.0, 520.0)),
            threshold(nodes.node("moving", "float/greater", 1180.0, 480.0)),
            nodes.transition("land_walking", 0.2, 600.0, 480.0),
            nodes.node("state", bevy_noodle_jackdaw::tnua::STATE, 40.0, 700.0),
            nodes.node("length", "vec3/length", 300.0, 760.0),
            airborne.set_node("set_airborne", Vec2::new(560.0, 680.0)),
            speed.set_node("set_speed", Vec2::new(560.0, 800.0)),
        ],
        links: [
            vec![
                FlowLink::new("entry", "start", "grounded", "enter"),
                FlowLink::new("airborne", "value", "landed", "value"),
                FlowLink::new("speed", "value", "moving", "a"),
                FlowLink::new("state", "airborne", "set_airborne", "value"),
                FlowLink::new("state", "velocity", "length", "vector"),
                FlowLink::new("length", "length", "set_speed", "value"),
            ],
            transition_links("grounded", "takeoff", "in_the_air", ("airborne", "value")).to_vec(),
            transition_links("in_the_air", "touchdown", "land", ("landed", "result")).to_vec(),
            transition_links("land", "land_done", "grounded", ("land", "finished")).to_vec(),
            transition_links("land", "land_walking", "grounded", ("moving", "result")).to_vec(),
        ]
        .concat(),
        variables: vec![airborne, speed],
        ..default()
    }
}

/// Rewrites the game's `assets/flow/animation.flowgraph.bsn` as first
/// authored: `cargo test -p ninja_character --test animation write_animation_graph -- --ignored`.
#[test]
#[ignore = "writes an asset"]
fn write_animation_graph() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), FlowGraphPlugin))
        .add_plugins((
            bevy_noodle_jackdaw::tnua::TnuaFlowNodesPlugin::<character_controller::ControlScheme>::default(),
            bevy_noodle_jackdaw::leafwing::LeafwingFlowNodesPlugin::<PlayerAction>::default(),
            character_controller::CharacterFlowNodesPlugin,
            CharacterAnimationPlugin,
            YummyClipsPlugin,
        ));
    let graph = default_animation_graph(app.world().resource::<FlowNodeTypes>());
    let text = bevy_noodle_jackdaw::flow_graph_to_bsn(
        &graph,
        &app.world().resource::<AppTypeRegistry>().read(),
    );
    std::fs::write(format!("{ASSETS}/{ANIMATION_GRAPH}"), text).unwrap();
}

/// A headless app with physics, a floor, and the game's assets.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin {
            file_path: ASSETS.into(),
            ..default()
        },
        bevy::world_serialization::WorldSerializationPlugin,
        bevy::input::InputPlugin,
        bevy::animation::AnimationPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
    .add_plugins((
        bevy::gltf::GltfPlugin::default(),
        PhysicsPlugins::default(),
        TnuaAvian3dPlugin::new(FixedUpdate),
        CharacterControllerPlugin,
        // The game's movement graph aims with the camera's Camera Aim node.
        camera_controller::CameraFlowNodesPlugin,
        CharacterAnimationPlugin,
        YummyClipsPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(200.0, 0.1, 200.0),
        Transform::from_xyz(0.0, -0.05, 0.0),
    ));
    app.finish();
    app.cleanup();
    app
}

/// The Player and Ninja of `assets/scene.bsn`: the model's feet on the
/// floor, turned to face where the body walks. No meshes or materials.
fn spawn_ninja(app: &mut App) -> (Entity, Entity) {
    let scene = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(|settings: &mut GltfLoaderSettings| {
            settings.load_meshes = RenderAssetUsages::empty();
            settings.load_materials = RenderAssetUsages::empty();
        })
        .load(GltfAssetLabel::Scene(0).from_asset("character/ninja/outfit_01.gltf"));
    let turned = Quat::from_rotation_y(std::f32::consts::PI);
    let character = app
        .world_mut()
        .spawn((
            // The game's movement graph: walk, jog and sprint speeds.
            CharacterController {
                graph: MOVEMENT_GRAPH.into(),
            },
            Transform::from_xyz(0.0, FLOAT_HEIGHT, 0.0).with_rotation(turned),
        ))
        .id();
    let model = app
        .world_mut()
        .spawn((
            Name::new("Ninja"),
            Transform::from_xyz(0.0, -FLOAT_HEIGHT, 0.0).with_rotation(turned),
            WorldAssetRoot(scene),
            AnimationController::default(),
            ChildOf(character),
        ))
        .id();
    (character, model)
}

/// The Ninja's own bone called `name`, not the source skeleton's.
fn bone(app: &mut App, model: Entity, name: &str) -> Entity {
    let world = app.world_mut();
    let named: Vec<Entity> = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter(|(_, n)| n.as_str() == name)
        .map(|(entity, _)| entity)
        .collect();
    named
        .into_iter()
        .find(|entity| {
            let mut ancestors = vec![];
            let mut current = *entity;
            while let Some(parent) = world.get::<ChildOf>(current) {
                current = parent.parent();
                ancestors.push(current);
            }
            let source = ancestors.iter().any(|a| {
                world
                    .get::<Name>(*a)
                    .is_some_and(|n| n.as_str() == "Source skeleton")
            });
            ancestors.contains(&model) && !source
        })
        .unwrap_or_else(|| panic!("the Ninja has no {name}"))
}

fn state(app: &App, model: Entity) -> AnimationState {
    app.world().get::<AnimationState>(model).unwrap().clone()
}

fn position(app: &App, entity: Entity) -> Vec3 {
    app.world()
        .get::<GlobalTransform>(entity)
        .unwrap()
        .translation()
}

/// Step until the Ninja is bound to its clips and idles on the ground.
fn settle(app: &mut App, model: Entity) {
    for _ in 0..2000 {
        app.update();
        if app.world().get::<Retarget>(model).is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let retarget = app.world().get::<Retarget>(model).expect("bound");
    assert_eq!(retarget.driven_bones(), RETARGETED.len());
    for _ in 0..300 {
        app.update();
        if state(app, model).path == ["grounded", "idle"] {
            break;
        }
    }
    assert_eq!(state(app, model).path, ["grounded", "idle"]);
    assert_eq!(state(app, model).kind, IDLE);
}

#[test]
fn the_ninja_idles_then_walks_forward_on_its_feet() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    settle(&mut app, model);

    let feet = [
        bone(&mut app, model, "foot_l"),
        bone(&mut app, model, "foot_r"),
    ];
    let head = bone(&mut app, model, "head");
    let standing = position(&app, head).y;
    assert!(standing > 1.4, "head at {standing}");
    for foot in feet {
        let height = position(&app, foot).y;
        assert!((-0.05..0.25).contains(&height), "idle foot at {height}");
    }

    KeyCode::KeyW.press(app.world_mut());
    let mut lowest = [f32::MAX; 2];
    let mut nearest = [f32::MAX; 2];
    let mut farthest = [f32::MIN; 2];
    for _ in 0..90 {
        app.update();
        let body = position(&app, character);
        for (index, foot) in feet.iter().enumerate() {
            let foot = position(&app, *foot);
            lowest[index] = lowest[index].min(foot.y);
            // Along the walk (-Z), ahead of the body.
            let ahead = body.z - foot.z;
            nearest[index] = nearest[index].min(ahead);
            farthest[index] = farthest[index].max(ahead);
        }
    }
    assert_eq!(state(&app, model).path, ["grounded", "locomotion"]);
    assert!(weight(&app, model, "walk") > 0.99, "walking at its speed");
    for index in 0..2 {
        assert!(
            (-0.05..0.15).contains(&lowest[index]),
            "foot {index} lowest at {}",
            lowest[index]
        );
        let swing = farthest[index] - nearest[index];
        assert!(swing > 0.3, "foot {index} swings {swing} m");
    }
    assert!(position(&app, head).y > 1.3);

    KeyCode::KeyW.release(app.world_mut());
    stops_with_the_body(&mut app, character, model, WALK_STOP);
    assert_eq!(
        states_until_idle(&mut app, model),
        [(IDLE_PATH.map(String::from).to_vec(), IDLE.to_owned())]
    );
}

/// How far the library's clips go.
fn clip_motions() -> ClipMotions {
    let bytes = std::fs::read(format!("{ASSETS}/{LIBRARY}")).unwrap();
    ClipMotions::from_glb(&bytes).unwrap()
}

/// The clip's animation in the library: its kind past the category.
fn animation(kind: &str) -> &str {
    kind.split_once('/').unwrap().1
}

/// By their feet, the gaits' clips go about as fast as the gaits, and each
/// stop goes its way to rest.
#[test]
fn the_clips_feet_say_how_far_they_go() {
    let motions = clip_motions();
    for (kind, speed) in [
        (WALK_FORWARD, WALK_SPEED),
        (JOG_FORWARD, JOG_SPEED),
        (SPRINT, SPRINT_SPEED),
    ] {
        let motion = motions.get(animation(kind)).unwrap();
        let mean = motion.total() / motion.duration();
        assert!((mean / speed - 1.0).abs() < 0.15, "{kind}: {mean} m/s");
    }
    for (kind, distance) in [(WALK_STOP, 1.37), (JOG_STOP_LF, 1.21), (SPRINT_STOP, 6.29)] {
        let motion = motions.get(animation(kind)).unwrap();
        assert!(
            (motion.total() / distance - 1.0).abs() < 0.1,
            "{kind}: {} m",
            motion.total()
        );
        // Less to go, later on.
        let times: Vec<f32> = [1.0, 0.5, 0.2, 0.0]
            .map(|left| motion.time_left(left))
            .to_vec();
        assert!(times.is_sorted(), "{kind}: {times:?}");
        assert!(times[3] <= motion.duration());
    }
}

/// Shift held jogs, a double Shift sprints; letting go of everything stops
/// by how fast it went: a jog on the foot ahead, a sprint with its stop.
#[test]
fn the_ninja_jogs_sprints_and_stops_by_its_speed() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    settle(&mut app, model);

    KeyCode::KeyW.press(app.world_mut());
    KeyCode::ShiftLeft.press(app.world_mut());
    for _ in 0..120 {
        app.update();
    }
    assert_eq!(state(&app, model).path, ["grounded", "locomotion"]);
    assert!(weight(&app, model, "jog") > 0.99, "jogging at its speed");
    KeyCode::KeyW.release(app.world_mut());
    KeyCode::ShiftLeft.release(app.world_mut());
    // Which foot is ahead depends on the phase, the frame it stops.
    let stops = states_until_idle(&mut app, model);
    let [(stop, picked), (idle, _)] = &stops[..] else {
        panic!("a stop then idle: {stops:?}");
    };
    assert_eq!(stop, &STOP);
    assert!(
        [JOG_STOP_LF, JOG_STOP_RF].contains(&picked.as_str()),
        "{stops:?}"
    );
    assert_eq!(idle, &IDLE_PATH);

    KeyCode::KeyW.press(app.world_mut());
    KeyCode::ShiftLeft.press(app.world_mut());
    app.update();
    KeyCode::ShiftLeft.release(app.world_mut());
    for _ in 0..6 {
        app.update();
    }
    KeyCode::ShiftLeft.press(app.world_mut());
    let mut mixed = false;
    for _ in 0..150 {
        app.update();
        let [jog, sprint] = ["jog", "sprint"].map(|sample| weight(&app, model, sample));
        mixed |= jog > 0.1 && sprint > 0.1;
    }
    assert!(mixed, "eases from a jog to a sprint");
    assert!(
        weight(&app, model, "sprint") > 0.99,
        "sprinting at its speed: {:?}",
        ["walk", "jog", "sprint"].map(|sample| weight(&app, model, sample))
    );
    KeyCode::KeyW.release(app.world_mut());
    KeyCode::ShiftLeft.release(app.world_mut());
    stops_with_the_body(&mut app, character, model, SPRINT_STOP);
    assert_eq!(
        states_until_idle(&mut app, model),
        [(IDLE_PATH.map(String::from).to_vec(), IDLE.to_owned())]
    );
}

/// Shift let go a moment before the stick: the sprint has barely eased
/// down, so it stops as a sprint, its clip and the controller alike.
#[test]
fn letting_go_of_shift_just_before_stopping_still_stops_a_sprint() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    settle(&mut app, model);

    KeyCode::KeyW.press(app.world_mut());
    KeyCode::ShiftLeft.press(app.world_mut());
    app.update();
    KeyCode::ShiftLeft.release(app.world_mut());
    for _ in 0..6 {
        app.update();
    }
    KeyCode::ShiftLeft.press(app.world_mut());
    for _ in 0..150 {
        app.update();
    }
    assert!(weight(&app, model, "sprint") > 0.99, "sprinting");
    KeyCode::ShiftLeft.release(app.world_mut());
    for _ in 0..6 {
        app.update();
    }
    KeyCode::KeyW.release(app.world_mut());
    stops_with_the_body(&mut app, character, model, SPRINT_STOP);
    assert_eq!(
        states_until_idle(&mut app, model),
        [(IDLE_PATH.map(String::from).to_vec(), IDLE.to_owned())]
    );
}

/// Once the stick is let go, the Ninja stops with `kind`, its clip played
/// where it has as far left to go as the body has, by the controller, until
/// the body is at rest: whatever the stop's easing in the movement graph.
fn stops_with_the_body(app: &mut App, character: Entity, model: Entity, kind: &str) {
    let motions = clip_motions();
    let motion = motions.get(animation(kind)).unwrap();
    let stopping = (0..60)
        .map(|_| {
            app.update();
            state(app, model)
        })
        .find(|state| state.path == STOP)
        .expect("stops");
    assert_eq!(stopping.kind, kind);
    let mut matched = 0;
    for _ in 0..240 {
        let left = app
            .world()
            .get::<StopDistance>(character)
            .map_or(0.0, |d| d.0);
        if left <= 0.02 {
            break;
        }
        let now = state(app, model);
        assert_eq!(now.path, STOP);
        if left < motion.total() {
            // A frame behind at most: in time where the clip goes fast, in
            // distance where it hardly does, near rest.
            let (at, expected) = (now.time * motion.duration(), motion.time_left(left));
            let clip_left = motion.total() - motion.gone_at(at);
            assert!(
                (at - expected).abs() < 0.05 || (clip_left - left).abs() < 0.03,
                "{left} m left: the clip is {at} s in ({clip_left} m left), not {expected}"
            );
            matched += 1;
        }
        app.update();
    }
    assert!(matched > 0, "never matched");
}

/// The weight of Locomotion's sample `sample` (walk, jog or sprint, in
/// that order), as it plays.
fn weight(app: &App, model: Entity, sample: &str) -> f32 {
    let index = ["walk", "jog", "sprint"]
        .iter()
        .position(|s| *s == sample)
        .unwrap_or_else(|| panic!("no sample {sample}"));
    state(app, model)
        .sample_weights
        .get(index)
        .copied()
        .unwrap_or(0.0)
}

const STOP: [&str; 2] = ["grounded", "stop"];
const IDLE_PATH: [&str; 2] = ["grounded", "idle"];

/// The states one after another, from the next one until idle again (or
/// 10 s), with the clip each played.
fn states_until_idle(app: &mut App, model: Entity) -> Vec<(Vec<String>, String)> {
    let before = state(app, model).path;
    let mut seen: Vec<(Vec<String>, String)> = vec![];
    for _ in 0..600 {
        app.update();
        let AnimationState { path, kind, .. } = state(app, model);
        if seen.is_empty() && path == before {
            continue;
        }
        if seen.last().map(|(seen, _)| seen) != Some(&path) {
            seen.push((path.clone(), kind));
        }
        if path == IDLE_PATH {
            break;
        }
    }
    seen
}

/// Lifted 4 m: the Ninja falls in the air machine, lands, then idles again
/// on the ground; the playback lights the machine it is in.
#[test]
fn the_ninja_falls_lands_then_idles() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    settle(&mut app, model);

    app.world_mut()
        .get_mut::<Transform>(character)
        .unwrap()
        .translation
        .y += 4.0;
    let mut seen: Vec<Vec<String>> = vec![];
    let mut lit_in_the_air = false;
    for _ in 0..400 {
        app.update();
        let path = state(&app, model).path;
        if seen.last() != Some(&path) {
            seen.push(path.clone());
        }
        if path == ["in_the_air", "fall"] {
            let playback = app.world().get::<FlowGraphPlayback>(model).unwrap();
            let lit = |id: &str| playback.node(id).unwrap().outputs[0] == FlowValue::State(true);
            lit_in_the_air = lit("in_the_air") && lit("in_the_air/fall") && !lit("grounded");
        }
        if seen.len() > 3 && path == ["grounded", "idle"] {
            break;
        }
    }
    let expected: [&[&str]; 4] = [
        &["grounded", "idle"],
        &["in_the_air", "fall"],
        &["land"],
        &["grounded", "idle"],
    ];
    assert_eq!(
        seen,
        expected.map(|p| p.to_vec()),
        "states one after another"
    );
    assert!(
        lit_in_the_air,
        "the playback lights the air machine and its fall"
    );
    assert_eq!(state(&app, model).kind, IDLE);
}
