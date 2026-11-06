//! The Fly node: a camera free of the character, moving where it looks
//! (`move`: +Y forward, +X right), straight `up` and `down`, at a speed the
//! wheel changes (`speed_step`, notches: each one `speed_factor` times
//! faster or slower, from `speed` m/s to begin with, within `min_speed` and
//! `max_speed`). It turns with `look` and `look_stick` as the Third Person
//! node does, and does nothing while inactive.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, FlowValue, PortKind};

use crate::controller::{is_active, look_turn, mouse_looks, set_world_transform, world_transform};

pub const FLY: &str = "camera/fly";

/// Pitch is held short of straight up and down, where yaw would flip.
const MAX_PITCH: f32 = 89.0;

/// How fast a fly camera goes, m/s, as the wheel left it.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct FlySpeed(pub f32);

/// The node's inputs, in order.
const MOVE: usize = 0;
const UP: usize = 1;
const DOWN: usize = 2;
const SPEED_STEP: usize = 3;
const LOOK: usize = 4;
const LOOK_STICK: usize = 5;
const SENSITIVITY: usize = 6;
const STICK_SPEED: usize = 7;
const SPEED: usize = 8;
const SPEED_FACTOR: usize = 9;
const MIN_SPEED: usize = 10;
const MAX_SPEED: usize = 11;

pub(crate) fn register(app: &mut App) {
    app.register_type::<FlySpeed>().register_flow_node(
        FlowNodeType::apply(
            FLY,
            "Fly",
            &[
                ("move", PortKind::Vec2),
                ("up", PortKind::Bool),
                ("down", PortKind::Bool),
                ("speed_step", PortKind::Float),
                ("look", PortKind::Vec2),
                ("look_stick", PortKind::Vec2),
                ("sensitivity", PortKind::Float),
                ("stick_speed", PortKind::Float),
                ("speed", PortKind::Float),
                ("speed_factor", PortKind::Float),
                ("min_speed", PortKind::Float),
                ("max_speed", PortKind::Float),
            ],
            fly,
        )
        .with_default("sensitivity", FlowValue::Float(0.003))
        .with_default("stick_speed", FlowValue::Float(3.0))
        .with_default("speed", FlowValue::Float(6.0))
        .with_default("speed_factor", FlowValue::Float(1.25))
        .with_default("min_speed", FlowValue::Float(0.5))
        .with_default("max_speed", FlowValue::Float(200.0)),
    );
}

fn fly(world: &mut World, camera: Entity, inputs: &[FlowValue]) {
    if !is_active(world, camera) {
        return;
    }
    let Some(transform) = world_transform(world, camera) else {
        return;
    };
    let dt = world.resource::<Time>().delta_secs();
    let mouse = if mouse_looks(world) {
        inputs[LOOK].as_vec2()
    } else {
        Vec2::ZERO
    };
    let turn = look_turn(
        mouse,
        inputs[LOOK_STICK].as_vec2(),
        inputs[SENSITIVITY].as_f32(),
        inputs[STICK_SPEED].as_f32(),
        dt,
    );
    let speed = world
        .get::<FlySpeed>(camera)
        .map_or(inputs[SPEED].as_f32(), |speed| speed.0);
    let speed = step_speed(
        speed,
        inputs[SPEED_STEP].as_f32(),
        inputs[SPEED_FACTOR].as_f32(),
        (inputs[MIN_SPEED].as_f32(), inputs[MAX_SPEED].as_f32()),
    );
    let rise =
        f32::from(u8::from(inputs[UP].as_bool())) - f32::from(u8::from(inputs[DOWN].as_bool()));
    let pose = fly_pose(transform, turn, inputs[MOVE].as_vec2(), rise, speed * dt);
    set_world_transform(world, camera, pose);
    world.entity_mut(camera).insert(FlySpeed(speed));
}

/// `speed` after `notches` of the wheel, each `factor` times faster (or
/// slower, turned back), within `range`.
pub fn step_speed(speed: f32, notches: f32, factor: f32, range: (f32, f32)) -> f32 {
    let (min, max) = (range.0.max(1e-3), range.1.max(range.0.max(1e-3)));
    (speed * factor.max(1.0).powf(notches)).clamp(min, max)
}

/// The camera at `transform`, turned by `turn` (yaw, pitch), then moved
/// `step` metres along where it looks (`motion`: +Y forward, +X right) and
/// up (`rise`, -1 to 1). Diagonals go no faster than straight lines.
pub fn fly_pose(transform: Transform, turn: Vec2, motion: Vec2, rise: f32, step: f32) -> Transform {
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    let limit = MAX_PITCH.to_radians();
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        yaw + turn.x,
        (pitch + turn.y).clamp(-limit, limit),
        0.0,
    );
    let direction = rotation * Vec3::new(motion.x, 0.0, -motion.y) + Vec3::Y * rise;
    Transform {
        translation: transform.translation + direction.clamp_length_max(1.0) * step,
        rotation,
        ..transform
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn it_flies_where_it_looks_and_straight_up() {
        // Looking down -X, a little downwards.
        let start = Transform::default().looking_to(Vec3::new(-1.0, -1.0, 0.0), Vec3::Y);
        let moved = fly_pose(start, Vec2::ZERO, Vec2::Y, 0.0, 2.0);
        let expected = Vec3::new(-1.0, -1.0, 0.0).normalize() * 2.0;
        assert!(
            moved.translation.abs_diff_eq(expected, 1e-4),
            "{}",
            moved.translation
        );
        let risen = fly_pose(start, Vec2::ZERO, Vec2::ZERO, 1.0, 2.0);
        assert!(risen.translation.abs_diff_eq(Vec3::Y * 2.0, 1e-5));
        // Forward and up at once, looking level: no faster than one.
        let both = fly_pose(Transform::default(), Vec2::ZERO, Vec2::Y, 1.0, 2.0);
        assert!((both.translation.length() - 2.0).abs() < 1e-4);
    }

    #[test]
    fn it_turns_and_stops_short_of_straight_up() {
        let turned = fly_pose(
            Transform::default(),
            Vec2::new(FRAC_PI_2, 3.0),
            Vec2::ZERO,
            0.0,
            0.0,
        );
        let (yaw, pitch, _) = turned.rotation.to_euler(EulerRot::YXZ);
        assert!((yaw - FRAC_PI_2).abs() < 1e-4);
        assert!((pitch - MAX_PITCH.to_radians()).abs() < 1e-4);
    }

    #[test]
    fn the_wheel_steps_the_speed_within_its_range() {
        assert!((step_speed(4.0, 1.0, 1.25, (0.5, 200.0)) - 5.0).abs() < 1e-5);
        assert!((step_speed(4.0, -2.0, 2.0, (0.5, 200.0)) - 1.0).abs() < 1e-5);
        assert_eq!(step_speed(4.0, -10.0, 2.0, (0.5, 200.0)), 0.5);
        assert_eq!(step_speed(150.0, 3.0, 2.0, (0.5, 200.0)), 200.0);
    }
}
