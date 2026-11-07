//! The Ninja on a character, animated by the game's animation graph
//! (`assets/flow/animation.flowgraph.bsn`) with Yummy clips retargeted onto
//! it: idle standing still, walking, jogging or sprinting by its velocity
//! as it faces (a blend space: walks and jogs every way, the sprint
//! ahead), stopping (a select), falling in the air and landing.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::gltf::{GltfAssetLabel, GltfLoaderSettings};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::world_serialization::WorldAssetRoot;
use bevy_noodle_jackdaw::blend::{BLEND_SPACE_2D, SELECT_2D};
use bevy_noodle_jackdaw::leafwing::action_kind;
use bevy_noodle_jackdaw::nodes::{BONE, SKELETON, SMOOTH_POLAR};
use bevy_noodle_jackdaw::{
    BlendSample, FlowGraph, FlowGraphPlayback, FlowGraphPlugin, FlowLink, FlowNode, FlowNodeTypes,
    FlowValue, FlowVariable, PortKind,
};
use bevy_tnua_avian3d::TnuaAvian3dPlugin;
use character_animation::{
    AIM_TWIST, ANIMATION_GRAPH, AimTwist, AnimationController, AnimationState,
    CharacterAnimationPlugin, ClipMotions, ENTRY, FACING, Facing, RETARGETED, ROOT_MOTION,
    Retarget, STATE_MACHINE, TRANSITION,
};
use character_controller::{
    CharacterController, CharacterControllerPlugin, FLOAT_HEIGHT, JOG_SPEED, MOVEMENT_GRAPH,
    PlayerAction, SPRINT_SPEED, STOP_DISTANCE, StopDistance, WALK_SPEED,
};
use leafwing_input_manager::prelude::*;
use ninja_character::YummyClipsPlugin;
use ninja_character::yummy::{
    FALL, IDLE, JOG_FORWARD, JOG_STOP_LF, JOG_STOP_RF, JOGS, LAND, LIBRARY, SPRINT, SPRINT_STOP,
    TURNS, WALK_FORWARD, WALK_STOP, WALKS,
};

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");
/// Below this speed (m/s), the Ninja idles.
const WALKING: f32 = 0.1;
/// Below this length, the Move stick is let go (the Gait node's dead zone).
const LET_GO: f32 = 0.05;
/// How long (s) Locomotion's velocity takes to follow Tnua's.
const SMOOTHING: f32 = 0.3;
/// How far (degrees) the model may be left behind its body, standing,
/// before it turns to it (a turn clip).
const TURN_BEHIND: f32 = 70.0;

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

/// Where the `index`th of [`WALKS`] (or [`JOGS`]) goes, `speed` m/s: `x`
/// to the character's right, `y` ahead.
fn heading(index: usize, speed: f32) -> Vec2 {
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let ways = [
        (0.0, 1.0),
        (s, s),
        (1.0, 0.0),
        (s, -s),
        (0.0, -1.0),
        (-s, -s),
        (-1.0, 0.0),
        (-s, s),
    ];
    Vec2::from(ways[index]) * speed
}

/// On the ground: idle until asked to move, then walking, jogging or
/// sprinting by the velocity as the character faces (the Locomotion blend
/// space: walks and jogs every way, the sprint ahead), and once the Move
/// stick is let go, stopping by how fast it went (from a jog, on the foot
/// ahead: the Stop select, its clip matched to how far the character has
/// left to go) before idling again; idle, its model left behind its body by
/// more than [`TURN_BEHIND`], turning to it (the Turn select, the turn
/// clip nearest how far). In the air: falling, then landing. The top graph
/// sets the `airborne`, `speed` (m/s) and `facing` (the Facing offset,
/// degrees) variables; the machines read them. Its Facing holds the model's
/// yaw while the character stands, turning back to the body as a turn clip
/// turns (Root Motion), and follows the body while it moves or is in the
/// air; its Aim Twist turns the upper body, spine_01 to spine_05, toward the
/// camera's aim. Locomotion reads Tnua's local velocity, smoothed
/// (Smooth Polar, over [`SMOOTHING`] seconds): turning from one way to the
/// next, its clips mix over a moment rather than in a few frames.
fn default_animation_graph(types: &FlowNodeTypes) -> FlowGraph {
    let nodes = Nodes(types);
    let airborne = FlowVariable::new("airborne", PortKind::Bool);
    let speed = FlowVariable::new("speed", PortKind::Float);
    let facing = FlowVariable::new("facing", PortKind::Float);
    let threshold = |node: FlowNode| nodes.with(node, "b", FlowValue::Float(WALKING));
    let bone = |id: &str, name: &str, x: f32, y: f32| {
        let node = nodes.node(id, BONE, x, y);
        nodes.with(node, "name", FlowValue::Text(name.into()))
    };
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
            nodes.node(
                "velocity",
                bevy_noodle_jackdaw::tnua::LOCAL_VELOCITY,
                300.0,
                480.0,
            ),
            nodes.with(
                nodes.node("smooth_velocity", SMOOTH_POLAR, 580.0, 400.0),
                "time",
                FlowValue::Float(SMOOTHING),
            ),
            nodes.node("stop_distance", STOP_DISTANCE, 1140.0, 760.0),
            nodes.node("idle", IDLE, 300.0, 260.0),
            nodes.transition("start", 0.2, 580.0, 40.0),
            FlowNode {
                title: "Locomotion".into(),
                // Each at its gait's speed, its way.
                samples: [(WALKS, WALK_SPEED), (JOGS, JOG_SPEED)]
                    .iter()
                    .flat_map(|(clips, speed)| {
                        clips
                            .iter()
                            .enumerate()
                            .map(|(index, kind)| nodes.sample_2d(kind, heading(index, *speed)))
                    })
                    .chain([nodes.sample_2d(SPRINT, Vec2::new(0.0, SPRINT_SPEED))])
                    .collect(),
                ..nodes.node("locomotion", BLEND_SPACE_2D, 860.0, 260.0)
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
            facing.get_node("facing", Vec2::new(40.0, 1000.0)),
            nodes.node("to_turn", "float/negate", 300.0, 1000.0),
            nodes.node("behind_by", "float/abs", 300.0, 1140.0),
            nodes.compare("behind", "float/greater", TURN_BEHIND, 560.0, 1140.0),
            nodes.transition("turning", 0.2, 580.0, 860.0),
            FlowNode {
                title: "Turn".into(),
                // Each at how far it turns.
                samples: TURNS
                    .iter()
                    .map(|(kind, turn)| nodes.sample_2d(kind, Vec2::new(*turn, 0.0)))
                    .collect(),
                ..nodes.node("turn", SELECT_2D, 860.0, 1000.0)
            },
            nodes.transition("turn_done", 0.3, 300.0, 860.0),
            nodes.transition("turn_moving", 0.2, 860.0, 860.0),
        ],
        links: [
            vec![
                FlowLink::new("entry", "start", "idle", "enter"),
                FlowLink::new("move", "vector", "stick", "vector"),
                FlowLink::new("stick", "length", "moving", "a"),
                FlowLink::new("moving", "result", "let_go", "value"),
                FlowLink::new("velocity", "vector", "smooth_velocity", "vector"),
                FlowLink::new("smooth_velocity", "x", "locomotion", "x"),
                FlowLink::new("smooth_velocity", "y", "locomotion", "y"),
                FlowLink::new("speed", "value", "stop", "x"),
                FlowLink::new("locomotion", "phase", "stop", "y"),
                FlowLink::new("stop_distance", "distance", "stop", "distance"),
                FlowLink::new("facing", "value", "to_turn", "value"),
                FlowLink::new("facing", "value", "behind_by", "value"),
                FlowLink::new("behind_by", "result", "behind", "a"),
                FlowLink::new("to_turn", "result", "turn", "x"),
            ],
            transition_links("idle", "start", "locomotion", ("moving", "result")).to_vec(),
            transition_links("locomotion", "stopping", "stop", ("let_go", "result")).to_vec(),
            // Back to Locomotion if asked to move again, else to Idle once
            // it is done.
            transition_links("stop", "stop_moving", "locomotion", ("moving", "result")).to_vec(),
            transition_links("stop", "stop_done", "idle", ("stop", "finished")).to_vec(),
            // Idle, left behind its body: turning to it, back to Idle once
            // done, or to Locomotion if asked to move.
            transition_links("idle", "turning", "turn", ("behind", "result")).to_vec(),
            transition_links("turn", "turn_done", "idle", ("turn", "finished")).to_vec(),
            transition_links("turn", "turn_moving", "locomotion", ("moving", "result")).to_vec(),
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
            nodes.node("follows", "logic/or", 1440.0, 480.0),
            nodes.node("facing", FACING, 1700.0, 480.0),
            nodes.node("root_motion", ROOT_MOTION, 1440.0, 300.0),
            facing.set_node("set_facing", Vec2::new(1960.0, 480.0)),
            nodes.node("skeleton", SKELETON, 1180.0, 760.0),
            bone("split", "spine_01", 1440.0, 700.0),
            bone("chest", "spine_05", 1440.0, 840.0),
            nodes.node("aim", camera_controller::AIM, 1440.0, 980.0),
            nodes.node("twist", AIM_TWIST, 1700.0, 760.0),
        ],
        links: [
            vec![
                FlowLink::new("entry", "start", "grounded", "enter"),
                FlowLink::new("airborne", "value", "landed", "value"),
                FlowLink::new("speed", "value", "moving", "a"),
                FlowLink::new("state", "airborne", "set_airborne", "value"),
                FlowLink::new("state", "velocity", "length", "vector"),
                FlowLink::new("length", "length", "set_speed", "value"),
                FlowLink::new("moving", "result", "follows", "a"),
                FlowLink::new("airborne", "value", "follows", "b"),
                FlowLink::new("follows", "result", "facing", "follow"),
                FlowLink::new("root_motion", "turn", "facing", "turn"),
                FlowLink::new("root_motion", "turned", "facing", "turned"),
                FlowLink::new("facing", "offset", "set_facing", "value"),
                FlowLink::new("skeleton", "skeleton", "split", "skeleton"),
                FlowLink::new("skeleton", "skeleton", "chest", "skeleton"),
                FlowLink::new("split", "bone", "twist", "bone"),
                FlowLink::new("chest", "bone", "twist", "to"),
                FlowLink::new("aim", "forward", "twist", "direction"),
            ],
            transition_links("grounded", "takeoff", "in_the_air", ("airborne", "value")).to_vec(),
            transition_links("in_the_air", "touchdown", "land", ("landed", "result")).to_vec(),
            transition_links("land", "land_done", "grounded", ("land", "finished")).to_vec(),
            transition_links("land", "land_walking", "grounded", ("moving", "result")).to_vec(),
        ]
        .concat(),
        variables: vec![airborne, speed, facing],
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
            camera_controller::CameraFlowNodesPlugin,
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
    spawn_ninja_with(app, RenderAssetUsages::empty())
}

/// [`spawn_ninja`], its meshes kept as `meshes` says: in the main world, its
/// skinned meshes are there for a Skeleton node to find.
fn spawn_ninja_with(app: &mut App, meshes: RenderAssetUsages) -> (Entity, Entity) {
    let scene = app
        .world()
        .resource::<AssetServer>()
        .load_builder()
        .with_settings(move |settings: &mut GltfLoaderSettings| {
            settings.load_meshes = meshes;
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

/// Aimed ahead (-Z), the Ninja faces there whichever way it walks: to its
/// left, back, right or ahead and right, it plays the walk that way, then
/// stops and idles again.
#[test]
fn aimed_ahead_the_ninja_walks_every_way_facing_it() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    app.world_mut()
        .entity_mut(character)
        .insert(camera_controller::CameraAim::default());
    settle(&mut app, model);

    let ways: [(&[KeyCode], &str); 4] = [
        (&[KeyCode::KeyA], "walk l"),
        (&[KeyCode::KeyS], "walk b"),
        (&[KeyCode::KeyD], "walk r"),
        (&[KeyCode::KeyW, KeyCode::KeyD], "walk fr"),
    ];
    for (keys, sample) in ways {
        for key in keys {
            key.press(app.world_mut());
        }
        for _ in 0..90 {
            app.update();
        }
        assert_eq!(state(&app, model).path, ["grounded", "locomotion"]);
        assert!(
            weight(&app, model, sample) > 0.95,
            "{sample}: {:?}",
            state(&app, model).sample_weights
        );
        let forward = app
            .world()
            .get::<GlobalTransform>(character)
            .unwrap()
            .forward();
        assert!(forward.dot(Vec3::NEG_Z) > 0.99, "faces {forward}");
        for key in keys {
            key.release(app.world_mut());
        }
        let states = states_until_idle(&mut app, model);
        assert_eq!(
            states.last().map(|(path, _)| &path[..]),
            Some(&IDLE_PATH.map(String::from)[..])
        );
    }
}

/// The yaw (radians, left positive) `entity` faces in the world, its
/// forward being `forward`.
fn yaw(app: &App, entity: Entity, forward: Vec3) -> f32 {
    let way = app
        .world()
        .get::<GlobalTransform>(entity)
        .unwrap()
        .rotation()
        * forward;
    way.x.atan2(way.z)
}

/// How far `a` is turned from `b`, within half a turn either way.
fn turned(a: f32, b: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a - b + PI).rem_euclid(TAU) - PI
}

/// Standing, the Ninja holds its yaw as the aim turns the body under it,
/// its feet planted; walking, it turns back to face where the body does.
#[test]
fn standing_the_ninja_holds_its_feet_as_the_body_turns() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    // Aimed the way the body faces as it spawns.
    let aim = camera_controller::CameraAim {
        yaw: std::f32::consts::PI,
        ..default()
    };
    app.world_mut().entity_mut(character).insert(aim);
    settle(&mut app, model);
    let feet = [
        bone(&mut app, model, "foot_l"),
        bone(&mut app, model, "foot_r"),
    ];
    let planted = feet.map(|foot| position(&app, foot));
    let facing = yaw(&app, model, Vec3::Z);
    let facing_body = yaw(&app, character, Vec3::NEG_Z);

    let turn = 60_f32.to_radians();
    app.world_mut()
        .get_mut::<camera_controller::CameraAim>(character)
        .unwrap()
        .yaw = aim.yaw + turn;
    for _ in 0..120 {
        app.update();
    }
    let body = yaw(&app, character, Vec3::NEG_Z);
    assert!(
        turned(body, facing_body + turn).abs() < 0.03,
        "the body faces {body}"
    );
    assert_eq!(state(&app, model).path, IDLE_PATH);
    let held = yaw(&app, model, Vec3::Z);
    assert!(
        turned(held, facing).abs() < 0.01,
        "the Ninja turned to {held}"
    );
    for (foot, planted) in feet.iter().zip(planted) {
        let slid = position(&app, *foot).xz().distance(planted.xz());
        assert!(slid < 0.03, "a foot slid {slid} m");
    }
    let offset = app.world().get::<Facing>(model).unwrap().offset;
    assert!((offset + turn).abs() < 0.03, "offset {offset}");

    KeyCode::KeyW.press(app.world_mut());
    for _ in 0..60 {
        app.update();
    }
    let offset = app.world().get::<Facing>(model).unwrap().offset;
    assert!(offset.abs() < 0.02, "walking, offset {offset}");
    let body = yaw(&app, character, Vec3::NEG_Z);
    let model_yaw = yaw(&app, model, Vec3::Z);
    assert!(
        turned(model_yaw - body, facing - facing_body).abs() < 0.03,
        "{model_yaw} for {body}"
    );
    KeyCode::KeyW.release(app.world_mut());
}

/// Standing, the body turned further than [`TURN_BEHIND`] from the model:
/// the model turns to it, playing the turn clip nearest how far (Turn L 90
/// for 100° left), back to Idle once done, facing the body.
#[test]
fn standing_the_ninja_turns_to_the_body_left_far_behind() {
    let mut app = app();
    let (character, model) = spawn_ninja(&mut app);
    let aim = camera_controller::CameraAim {
        yaw: std::f32::consts::PI,
        ..default()
    };
    app.world_mut().entity_mut(character).insert(aim);
    settle(&mut app, model);
    let facing_body = yaw(&app, character, Vec3::NEG_Z);
    let facing = yaw(&app, model, Vec3::Z);

    let turn = 100_f32.to_radians();
    app.world_mut()
        .get_mut::<camera_controller::CameraAim>(character)
        .unwrap()
        .yaw = aim.yaw + turn;
    let mut turned_with = None;
    for _ in 0..120 {
        app.update();
        let state = state(&app, model);
        if state.path == ["grounded", "turn"] {
            turned_with = Some(state.kind);
            break;
        }
    }
    assert_eq!(
        turned_with.as_deref(),
        Some("standing_turn/rm.standing.turn.l.90")
    );

    let mut states = Vec::new();
    for _ in 0..360 {
        app.update();
        let path = state(&app, model).path;
        if states.last() != Some(&path) {
            states.push(path.clone());
        }
        if path == IDLE_PATH {
            break;
        }
    }
    assert_eq!(
        states,
        [vec!["grounded", "turn"], IDLE_PATH.to_vec()]
            .map(|path| path.into_iter().map(String::from).collect::<Vec<_>>())
    );
    let body = yaw(&app, character, Vec3::NEG_Z);
    assert!(
        turned(body, facing_body + turn).abs() < 0.03,
        "the body faces {body}"
    );
    let model_yaw = yaw(&app, model, Vec3::Z);
    assert!(
        turned(model_yaw - body, facing - facing_body).abs() < 0.05,
        "the Ninja faces {model_yaw} for {body}"
    );
    let offset = app.world().get::<Facing>(model).unwrap().offset;
    assert!(offset.abs() < 0.05, "offset {offset}");
}

/// How far `entity` has turned around +Y since it was `before`, in the
/// world.
fn turned_since(app: &App, entity: Entity, before: Quat) -> f32 {
    let now = app
        .world()
        .get::<GlobalTransform>(entity)
        .unwrap()
        .rotation();
    let change = now * before.inverse();
    turned(2.0 * change.y.atan2(change.w), 0.0)
}

fn rotation(app: &App, entity: Entity) -> Quat {
    app.world()
        .get::<GlobalTransform>(entity)
        .unwrap()
        .rotation()
}

/// Standing, the Ninja's upper body turns toward the camera's aim (from
/// spine_01 up, spread to spine_05), up to its Aim Twist's max, the feet
/// still planted.
#[test]
fn standing_the_ninja_turns_its_upper_body_to_the_aim() {
    let mut app = app();
    let (character, model) = spawn_ninja_with(&mut app, RenderAssetUsages::MAIN_WORLD);
    let aim = camera_controller::CameraAim {
        yaw: std::f32::consts::PI,
        ..default()
    };
    app.world_mut().entity_mut(character).insert(aim);
    settle(&mut app, model);
    let chest = bone(&mut app, model, "spine_05");
    let pelvis = bone(&mut app, model, "pelvis");
    let foot = bone(&mut app, model, "foot_l");
    let (chest_before, pelvis_before) = (rotation(&app, chest), rotation(&app, pelvis));
    let planted = position(&app, foot);

    let aim_at = |app: &mut App, degrees: f32| {
        app.world_mut()
            .get_mut::<camera_controller::CameraAim>(character)
            .unwrap()
            .yaw = aim.yaw + degrees.to_radians();
        for _ in 0..90 {
            app.update();
        }
    };
    aim_at(&mut app, 40.0);
    let chest_turned = turned_since(&app, chest, chest_before).to_degrees();
    let pelvis_turned = turned_since(&app, pelvis, pelvis_before).to_degrees();
    assert!(
        (chest_turned - 40.0).abs() < 6.0,
        "the chest turned {chest_turned}°"
    );
    assert!(
        pelvis_turned.abs() < 3.0,
        "the pelvis turned {pelvis_turned}°"
    );
    let twist = app
        .world()
        .get::<AimTwist>(model)
        .unwrap()
        .angle
        .to_degrees();
    assert!((twist - 40.0).abs() < 1.0, "twist {twist}°");
    let slid = position(&app, foot).xz().distance(planted.xz());
    assert!(slid < 0.03, "the foot slid {slid} m");

    // Past the max, it turns as far as the max.
    aim_at(&mut app, 100.0);
    let twist = app
        .world()
        .get::<AimTwist>(model)
        .unwrap()
        .angle
        .to_degrees();
    assert!((twist - 60.0).abs() < 1.0, "twist {twist}°");
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

/// Locomotion's samples, in order: the walks then the jogs round from
/// ahead (see [`WALKS`]), then the sprint.
const SAMPLES: [&str; 17] = [
    "walk", "walk fr", "walk r", "walk br", "walk b", "walk bl", "walk l", "walk fl", "jog",
    "jog fr", "jog r", "jog br", "jog b", "jog bl", "jog l", "jog fl", "sprint",
];

/// The weight of Locomotion's sample `sample` (see [`SAMPLES`]; ahead,
/// `walk` and `jog`), as it plays.
fn weight(app: &App, model: Entity, sample: &str) -> f32 {
    let index = SAMPLES
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
