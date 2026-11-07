//! Node types every graph can use, needing nothing but Bevy.
//!
//! Among them, what finds an entity for a node to act on: Entity (by name),
//! Skeleton (the skinned mesh under an entity, by default the one the graph
//! runs for) and Bone (a joint of a skeleton, by name), their names picked
//! in views from the world's (a Bone node's, without a skeleton there, from
//! the preview character's: see [`PreviewBones`](crate::view::PreviewBones)).
//!
//! Smooth Polar follows a vector a little behind, turning its direction and
//! easing its length apart (see [`PolarSpring`]): a velocity feeding a
//! blend space, so the clips mix over a moment, not in a frame.

use std::collections::VecDeque;

use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;

use crate::model::{FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType};

pub const MAKE_VECTOR2: &str = "core/make_vector2";
pub const CAMERA_RELATIVE: &str = "core/camera_relative";
pub const DOUBLE_TAP: &str = "core/double_tap";
pub const ENTITY: &str = "core/entity";
pub const SKELETON: &str = "core/skeleton";
pub const BONE: &str = "core/bone";
pub const SMOOTH_POLAR: &str = "core/smooth_polar";

pub(crate) fn register(app: &mut App) {
    let entity = FlowNodeType::compute(
        ENTITY,
        "Entity",
        &[("name", PortKind::Text)],
        &[("entity", PortKind::Entity)],
        |world, _, inputs| vec![FlowValue::Entity(named(world, inputs[0].as_text()))],
    );
    let bone = FlowNodeType::compute(
        BONE,
        "Bone",
        &[("skeleton", PortKind::Entity), ("name", PortKind::Text)],
        &[("bone", PortKind::Entity)],
        |world, _, inputs| {
            let bone = inputs[0]
                .as_entity()
                .and_then(|skeleton| bone_of(world, skeleton, inputs[1].as_text()));
            vec![FlowValue::Entity(bone)]
        },
    );
    // Only views pick names.
    #[cfg(feature = "view")]
    let (entity, bone) = (
        entity.with_choices("name", |world, _| entity_names(world)),
        bone.with_choices("name", |world, inputs| match inputs[0].as_entity() {
            Some(skeleton) => bone_names(world, skeleton),
            None => world
                .get_resource::<crate::view::PreviewBones>()
                .map(|bones| bones.0.clone())
                .unwrap_or_default(),
        }),
    );
    app.register_flow_node(entity)
        .register_flow_node(FlowNodeType::compute(
            SKELETON,
            "Skeleton",
            &[("entity", PortKind::Entity)],
            &[("skeleton", PortKind::Entity)],
            |world, entity, inputs| {
                let skeleton = skeleton_of(world, inputs[0].as_entity().unwrap_or(entity));
                vec![FlowValue::Entity(skeleton)]
            },
        ))
        .register_flow_node(bone);
    app.register_flow_node(FlowNodeType::compute(
        MAKE_VECTOR2,
        "Make Vector",
        &[("x", PortKind::Float), ("y", PortKind::Float)],
        &[("vector", PortKind::Vec2)],
        |_, _, inputs| {
            vec![FlowValue::Vec2(Vec2::new(
                inputs[0].as_f32(),
                inputs[1].as_f32(),
            ))]
        },
    ))
    .register_flow_node(FlowNodeType::compute(
        CAMERA_RELATIVE,
        "Camera Relative",
        &[("input", PortKind::Vec2)],
        &[("direction", PortKind::Vec3)],
        |world, _, inputs| {
            let yaw = first_camera_yaw(world).unwrap_or(Quat::IDENTITY);
            vec![FlowValue::Vec3(camera_relative(inputs[0].as_vec2(), yaw))]
        },
    ))
    .register_flow_node(
        FlowNodeType::step(
            DOUBLE_TAP,
            "Double Tap",
            &[("button", PortKind::Bool), ("window", PortKind::Float)],
            &[("held", PortKind::Bool), ("double", PortKind::Bool)],
            |world, _, inputs, memory| {
                let dt = world.get_resource::<Time>().map_or(0.0, Time::delta_secs);
                let (held, double) =
                    double_tap(memory, inputs[0].as_bool(), inputs[1].as_f32(), dt);
                vec![FlowValue::Bool(held), FlowValue::Bool(double)]
            },
        )
        .with_default("window", FlowValue::Float(0.3)),
    )
    .register_flow_node(
        FlowNodeType::step(
            SMOOTH_POLAR,
            "Smooth Polar",
            &[("vector", PortKind::Vec2), ("time", PortKind::Float)],
            &[
                ("x", PortKind::Float),
                ("y", PortKind::Float),
                ("vector", PortKind::Vec2),
            ],
            |world, _, inputs, memory| {
                let dt = world.get_resource::<Time>().map_or(0.0, Time::delta_secs);
                let target = inputs[0].as_vec2();
                let mut spring = match memory.as_slice() {
                    [
                        FlowValue::Vec2(at),
                        FlowValue::Float(turning),
                        FlowValue::Float(growing),
                    ] => PolarSpring {
                        at: *at,
                        turning: *turning,
                        growing: *growing,
                    },
                    _ => PolarSpring::at(target),
                };
                let smoothed = spring.step(target, inputs[1].as_f32(), dt);
                *memory = vec![
                    FlowValue::Vec2(spring.at),
                    FlowValue::Float(spring.turning),
                    FlowValue::Float(spring.growing),
                ];
                vec![
                    FlowValue::Float(smoothed.x),
                    FlowValue::Float(smoothed.y),
                    FlowValue::Vec2(smoothed),
                ]
            },
        )
        .with_default("time", FlowValue::Float(0.15)),
    );
}

/// What Smooth Polar remembers: where its vector is, and how fast it
/// turns (radians a second) and grows (units a second).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PolarSpring {
    pub at: Vec2,
    pub turning: f32,
    pub growing: f32,
}

impl PolarSpring {
    /// Still, at `at`.
    pub fn at(at: Vec2) -> Self {
        Self { at, ..default() }
    }

    /// One step of `dt` seconds on toward `target`, most of the way there
    /// (90%) in `time` seconds, easing in and out: its direction turns
    /// toward the target's the short way round, its length goes toward the
    /// target's, each on a critically damped spring. A vector turning half
    /// a turn goes round by a side, rather than through zero. From zero, it
    /// heads the target's way at once; toward zero, it keeps its way. No
    /// `time`: the target.
    pub fn step(&mut self, target: Vec2, time: f32, dt: f32) -> Vec2 {
        if time <= 0.0 {
            *self = Self::at(target);
            return target;
        }
        let rate = 4.0 / time;
        let (length, growing) =
            smooth_damp(self.at.length(), target.length(), self.growing, rate, dt);
        let (way, turning) = match (self.at.try_normalize(), target.try_normalize()) {
            (Some(from), to) => {
                let angle = to.map_or(0.0, |to| from.angle_to(to));
                let (turned, turning) = smooth_damp(0.0, angle, self.turning, rate, dt);
                (Vec2::from_angle(turned).rotate(from), turning)
            }
            (None, Some(to)) => (to, 0.0),
            (None, None) => (Vec2::ZERO, 0.0),
        };
        *self = Self {
            at: way * length.max(0.0),
            turning,
            growing,
        };
        self.at
    }
}

/// `current`, going at `speed`, one step of `dt` on toward `target` on a
/// critically damped spring of `rate` (radians a second): where it is then,
/// and its speed. Unity's `SmoothDamp`, its smooth time `2 / rate`.
fn smooth_damp(current: f32, target: f32, speed: f32, rate: f32, dt: f32) -> (f32, f32) {
    let x = rate * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let pull = (speed + rate * change) * dt;
    (
        target + (change + pull) * decay,
        (speed - rate * pull) * decay,
    )
}

/// One step of a double tap: whether the button is held, and whether it
/// was pressed a second time within `window` seconds of the first press
/// and is held since. Remembers the time since the last press, whether
/// the button was down and whether the hold is a double one.
pub fn double_tap(memory: &mut Vec<FlowValue>, down: bool, window: f32, dt: f32) -> (bool, bool) {
    if memory.len() != 3 {
        *memory = vec![
            FlowValue::Float(f32::INFINITY),
            FlowValue::Bool(false),
            FlowValue::Bool(false),
        ];
    }
    let since = memory[0].as_f32() + dt;
    let (was_down, double) = (memory[1].as_bool(), memory[2].as_bool());
    let (since, double) = match (down, was_down) {
        // Pressed: a second press soon after the first is a double tap.
        (true, false) => (0.0, since <= window),
        (true, true) => (since, double),
        (false, _) => (since, false),
    };
    *memory = vec![
        FlowValue::Float(since),
        FlowValue::Bool(down),
        FlowValue::Bool(double),
    ];
    (down, double)
}

/// The first entity named `name`; none for no name.
pub fn named(world: &World, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    let mut named = world.try_query::<(Entity, &Name)>()?;
    named
        .iter(world)
        .find(|(_, own)| own.as_str() == name)
        .map(|(entity, _)| entity)
}

/// The skeleton of `entity`'s model: the skinned mesh nearest under it
/// (or itself), the first visible one if any is. A character may hold
/// another, hidden (a skeleton its animations are played on, then copied
/// from).
pub fn skeleton_of(world: &World, entity: Entity) -> Option<Entity> {
    let mut first = None;
    let mut queue = VecDeque::from([entity]);
    while let Some(entity) = queue.pop_front() {
        if world.get::<SkinnedMesh>(entity).is_some() {
            let visible = world
                .get::<InheritedVisibility>(entity)
                .is_none_or(|visibility| visibility.get());
            if visible {
                return Some(entity);
            }
            first = first.or(Some(entity));
        }
        if let Some(children) = world.get::<Children>(entity) {
            queue.extend(children);
        }
    }
    first
}

/// The joint of `skeleton` (a skinned mesh) named `name`.
pub fn bone_of(world: &World, skeleton: Entity, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    let joints = &world.get::<SkinnedMesh>(skeleton)?.joints;
    joints.iter().copied().find(|&joint| {
        world
            .get::<Name>(joint)
            .is_some_and(|own| own.as_str() == name)
    })
}

/// The names of `skeleton`'s joints, sorted.
#[cfg(feature = "view")]
fn bone_names(world: &World, skeleton: Entity) -> Vec<String> {
    let Some(skin) = world.get::<SkinnedMesh>(skeleton) else {
        return Vec::new();
    };
    let mut names: Vec<String> = skin
        .joints
        .iter()
        .filter_map(|&joint| world.get::<Name>(joint))
        .map(|name| name.as_str().to_owned())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The names of the world's entities an Entity node may pick, sorted: the
/// named ones in space (with a `Transform`, not UI), but not what spawned
/// with a model or a scene instance (its nodes and bones), nor a
/// skeleton's joints.
#[cfg(feature = "view")]
fn entity_names(world: &World) -> Vec<String> {
    use bevy::platform::collections::HashSet;
    use bevy::world_serialization::WorldAssetRoot;

    let Some(mut named) =
        world.try_query_filtered::<(Entity, &Name), (With<Transform>, Without<Node>)>()
    else {
        return Vec::new();
    };
    let joints: HashSet<Entity> = world
        .try_query::<&SkinnedMesh>()
        .map(|mut skins| {
            skins
                .iter(world)
                .flat_map(|skin| skin.joints.iter().copied())
                .collect()
        })
        .unwrap_or_default();
    let in_instance = |mut entity: Entity| {
        while let Some(child_of) = world.get::<ChildOf>(entity) {
            entity = child_of.parent();
            if world.get::<WorldAssetRoot>(entity).is_some() {
                return true;
            }
        }
        false
    };
    let mut names: Vec<String> = named
        .iter(world)
        .filter(|(entity, _)| !joints.contains(entity) && !in_instance(*entity))
        .map(|(_, name)| name.as_str().to_owned())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The yaw of the first 3D camera, if there is one.
fn first_camera_yaw(world: &World) -> Option<Quat> {
    let mut cameras = world.try_query_filtered::<&GlobalTransform, With<Camera3d>>()?;
    let camera = cameras.iter(world).next()?;
    Some(yaw_of(camera.rotation()))
}

/// The rotation's turn around world Y alone.
pub fn yaw_of(rotation: Quat) -> Quat {
    let forward = rotation * Vec3::NEG_Z;
    Quat::from_rotation_y(f32::atan2(-forward.x, -forward.z))
}

/// Stick or keys (+Y forward, +X right) to a horizontal world direction,
/// forward being where `yaw` looks. Length is capped at 1, so a diagonal on
/// keys is no faster than a straight line.
pub fn camera_relative(input: Vec2, yaw: Quat) -> Vec3 {
    let input = input.clamp_length_max(1.0);
    yaw * Vec3::new(input.x, 0.0, -input.y)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{FlowGraph, FlowLink};
    use crate::run::{FlowNodeMemory, FlowVariables, evaluate};

    /// A character named `Hero`: under it, a hidden skinned mesh nearer
    /// than the visible one, both on joints `neck` and `head`. Returns the
    /// character, the visible skin and its `neck`.
    pub(crate) fn hero(world: &mut World) -> (Entity, Entity, Entity) {
        let hero = world.spawn((Name::new("Hero"), Transform::default())).id();
        let neck = world
            .spawn((Name::new("neck"), Transform::default(), ChildOf(hero)))
            .id();
        let head = world
            .spawn((Name::new("head"), Transform::default(), ChildOf(neck)))
            .id();
        let skin = || SkinnedMesh {
            inverse_bindposes: default(),
            joints: vec![neck, head],
        };
        world.spawn((skin(), InheritedVisibility::HIDDEN, ChildOf(hero)));
        let model = world.spawn(ChildOf(hero)).id();
        let visible = world
            .spawn((skin(), InheritedVisibility::VISIBLE, ChildOf(model)))
            .id();
        (hero, visible, neck)
    }

    /// Entity (named `name`) into Skeleton into Bone (named `bone`).
    pub(crate) fn bone_graph(name: &str, bone: &str) -> FlowGraph {
        let types = crate::builtin_types();
        let node = |kind: &str, id: &str, value: Option<&str>| {
            let mut node = types.get(kind).unwrap().instantiate(id, Vec2::ZERO);
            if let Some(port) = node.inputs.iter_mut().find(|port| port.name == "name") {
                port.value = value.map(|value| FlowValue::Text(value.into()));
            }
            node
        };
        FlowGraph {
            nodes: vec![
                node(ENTITY, "entity", Some(name)),
                node(SKELETON, "skeleton", None),
                node(BONE, "bone", Some(bone)),
            ],
            links: vec![
                FlowLink::new("entity", "entity", "skeleton", "entity"),
                FlowLink::new("skeleton", "skeleton", "bone", "skeleton"),
            ],
            ..default()
        }
    }

    /// What the Entity, Skeleton and Bone nodes give.
    fn found(world: &World, graph: &FlowGraph) -> Vec<Option<Entity>> {
        let evaluation = evaluate(
            world,
            Entity::PLACEHOLDER,
            graph,
            &crate::builtin_types(),
            &FlowVariables::default(),
            &FlowNodeMemory::default(),
        );
        evaluation
            .states
            .iter()
            .map(|state| state.outputs[0].as_entity())
            .collect()
    }

    #[test]
    fn entity_skeleton_and_bone_find_a_bone_by_names() {
        let mut world = World::new();
        let (hero, skin, neck) = hero(&mut world);
        assert_eq!(
            found(&world, &bone_graph("Hero", "neck")),
            [Some(hero), Some(skin), Some(neck)]
        );
        assert_eq!(
            found(&world, &bone_graph("Hero", "tail")),
            [Some(hero), Some(skin), None]
        );
        assert_eq!(found(&world, &bone_graph("Nobody", "neck")), [None; 3]);
        assert_eq!(found(&world, &bone_graph("", "")), [None; 3]);
    }

    #[cfg(feature = "view")]
    #[test]
    fn entity_and_bone_names_are_picked_from_the_world() {
        let mut world = World::new();
        let (_, skin, _) = hero(&mut world);
        world.spawn((Name::new("Level"), Transform::default()));
        world.spawn((Name::new("Panel"), Transform::default(), Node::default()));
        let types = crate::builtin_types();
        let choices = |kind: &str, inputs: &[FlowValue]| {
            let choices = types.get(kind).unwrap().choices_for("name").unwrap();
            choices(&world, inputs)
        };
        // Not the joints, nor the UI.
        assert_eq!(choices(ENTITY, &[]), ["Hero", "Level"]);
        let skeleton = FlowValue::Entity(Some(skin));
        assert_eq!(
            choices(BONE, &[skeleton, FlowValue::Text(String::new())]),
            ["head", "neck"]
        );
        assert!(choices(BONE, &[FlowValue::Entity(None)]).is_empty());
        // Without a skeleton (as in views, running nodes for no entity):
        // the preview character's.
        world.insert_resource(crate::view::PreviewBones(vec![
            "pelvis".into(),
            "spine_01".into(),
        ]));
        let choices = types.get(BONE).unwrap().choices_for("name").unwrap();
        assert_eq!(
            choices(&world, &[FlowValue::Entity(None)]),
            ["pelvis", "spine_01"]
        );
    }

    #[test]
    fn a_smoothed_vector_turns_round_and_gets_there_in_its_time() {
        let dt = 1.0 / 60.0;
        let follow = |from: Vec2, to: Vec2, seconds: f32| {
            let mut spring = PolarSpring::at(from);
            let mut path = vec![from];
            for _ in 0..(seconds / dt).round() as usize {
                path.push(spring.step(to, 0.15, dt));
            }
            path
        };
        // Jog ahead to jog ahead-right: the length kept, the way turning,
        // slowly at first, most of the way in 0.15 s, all of it soon after.
        let ahead_right = Vec2::new(1.0, 1.0).normalize() * 3.5;
        let turned = |at: Vec2| Vec2::Y.angle_to(at) / Vec2::Y.angle_to(ahead_right);
        let path = follow(Vec2::Y * 3.5, ahead_right, 0.3);
        assert!(path.iter().all(|at| (at.length() - 3.5).abs() < 1e-3));
        assert!(turned(path[1]) < 0.1, "{}", turned(path[1]));
        assert!(
            (0.85..0.97).contains(&turned(path[9])),
            "{}",
            turned(path[9])
        );
        assert!(path.iter().all(|&at| turned(at) < 1.0 + 1e-3));
        assert!(path.last().unwrap().distance(ahead_right) < 0.02);
        // Half a turn: round by a side, not through zero.
        let path = follow(Vec2::Y * 3.5, Vec2::NEG_Y * 3.5, 0.4);
        assert!(path.iter().all(|at| at.length() > 3.4));
        assert!(path.last().unwrap().distance(Vec2::NEG_Y * 3.5) < 0.05);
        // Stopping keeps the way; starting takes the target's.
        let stop = PolarSpring::at(Vec2::X * 2.0).step(Vec2::ZERO, 0.15, dt);
        assert!(stop.x > 1.5 && stop.x < 2.0 && stop.y == 0.0, "{stop}");
        let start = PolarSpring::at(Vec2::ZERO).step(Vec2::NEG_X, 0.15, dt);
        assert!(start.x < 0.0 && start.y == 0.0, "{start}");
        assert_eq!(PolarSpring::at(Vec2::Y).step(Vec2::X, 0.0, dt), Vec2::X);
    }

    /// Runs a button through steps of 0.05 s: what the node gave each step.
    fn taps(steps: &[bool]) -> Vec<(bool, bool)> {
        let mut memory = Vec::new();
        steps
            .iter()
            .map(|&down| double_tap(&mut memory, down, 0.3, 0.05))
            .collect()
    }

    #[test]
    fn a_second_press_soon_after_the_first_holds_a_double_tap() {
        let (x, o) = (true, false);
        let given = taps(&[x, x, o, o, x, x, x, o, x]);
        let doubles: Vec<bool> = given.iter().map(|(_, double)| *double).collect();
        assert_eq!(doubles, [o, o, o, o, x, x, x, o, x]);
        assert!(
            given
                .iter()
                .zip([x, x, o, o, x, x, x, o, x])
                .all(|((held, _), down)| *held == down)
        );
    }

    #[test]
    fn presses_far_apart_are_single_taps() {
        let (x, o) = (true, false);
        let mut steps = vec![x];
        steps.extend([o; 7]);
        steps.extend([x, x]);
        assert!(taps(&steps).iter().all(|(_, double)| !double));
    }
}
