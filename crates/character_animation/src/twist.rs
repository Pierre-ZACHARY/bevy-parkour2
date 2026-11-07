//! Aim Twist: the upper body turning toward a direction (the camera's aim),
//! the legs keeping the clip's pose.
//!
//! The graph's Aim Twist node (`anim/aim_twist`) sets it:
//!
//! - `bone` (entity, a Bone node's): where the upper body starts, all of it
//!   above turning;
//! - `to` (entity, a bone above `bone`): the twist spreads evenly over the
//!   bones from `bone` up to it, rather than all at `bone`;
//! - `direction` (world): where to face, level (the Camera Aim node's
//!   `forward`);
//! - `max` (degrees): how far the upper body may turn from the model's
//!   facing at most;
//! - `weight` (0 to 1): how much of the turn it takes;
//! - `time` (s): how long it takes to turn most of the way (90%) to it;
//!
//! and tells how far it turns (`twist`, degrees, left positive).
//! [`twist`] applies it every frame, over the pose the clips (retargeted)
//! and [`Facing`](crate::Facing) gave.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{
    FlowGraph, FlowGraphPlayback, FlowGraphRunner, FlowNodeType, FlowValue, PortKind,
};

use crate::facing::{wrap, yaw};

pub const AIM_TWIST: &str = "anim/aim_twist";

/// How far the upper body turns, kept from frame to frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct AimTwist {
    /// Radians around +Y, left positive.
    pub angle: f32,
}

/// The Aim Twist node: settings in, the twist out.
pub(crate) fn node() -> FlowNodeType {
    FlowNodeType::compute(
        AIM_TWIST,
        "Aim Twist",
        &[
            ("bone", PortKind::Entity),
            ("to", PortKind::Entity),
            ("direction", PortKind::Vec3),
            ("max", PortKind::Float),
            ("weight", PortKind::Float),
            ("time", PortKind::Float),
        ],
        &[("twist", PortKind::Float)],
        // Applied by `twist`, every frame.
        |world, entity, _| {
            let angle = world.get::<AimTwist>(entity).map_or(0.0, |t| t.angle);
            vec![FlowValue::Float(angle.to_degrees())]
        },
    )
    .with_default("max", FlowValue::Float(60.0))
    .with_default("weight", FlowValue::Float(1.0))
    .with_default("time", FlowValue::Float(0.15))
}

/// The inputs of a graph's Aim Twist node, as it last ran.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    bone: Entity,
    to: Option<Entity>,
    direction: Vec3,
    max: f32,
    weight: f32,
    time: f32,
}

impl Settings {
    fn of(graph: &FlowGraph, playback: &FlowGraphPlayback) -> Option<Self> {
        let id = crate::facing::node_path(graph, AIM_TWIST)?;
        let inputs = &playback.node(&id)?.inputs;
        let input = |index: usize| inputs.get(index).cloned().unwrap_or(FlowValue::Float(0.0));
        Some(Self {
            bone: input(0).as_entity()?,
            to: input(1).as_entity(),
            direction: input(2).as_vec3(),
            max: input(3).as_f32().to_radians().max(0.0),
            weight: input(4).as_f32().clamp(0.0, 1.0),
            time: input(5).as_f32(),
        })
    }

    /// How far to turn from `facing` (the model's yaw): toward the
    /// direction, within the max, by the weight; without one, not at all.
    fn target(&self, facing: f32) -> f32 {
        let level = Vec2::new(self.direction.x, self.direction.z);
        if level.length_squared() < 1e-6 {
            return 0.0;
        }
        let toward = wrap(level.x.atan2(level.y) - facing);
        toward.clamp(-self.max, self.max) * self.weight
    }

    /// One frame of `dt` on from `angle` toward `target`.
    fn step(&self, angle: f32, target: f32, dt: f32) -> f32 {
        if self.time <= 0.0 {
            return target;
        }
        target + (angle - target) * 0.1_f32.powf(dt / self.time)
    }
}

/// Turns each model's upper body with an Aim Twist node in its graph toward
/// the node's direction, over the bones from its `bone` up to its `to`.
pub(crate) fn twist(
    graphs: Res<Assets<FlowGraph>>,
    time: Res<Time>,
    mut models: Query<(Entity, &FlowGraphRunner, &FlowGraphPlayback, &mut AimTwist)>,
    mut transforms: Query<(&mut Transform, Option<&ChildOf>)>,
) {
    for (model, runner, playback, mut twist) in &mut models {
        let Some(settings) = graphs
            .get(&runner.graph)
            .and_then(|graph| Settings::of(graph, playback))
        else {
            continue;
        };
        let Some(facing) = world_rotation(&transforms, model).map(yaw) else {
            continue;
        };
        let target = settings.target(facing);
        twist.angle = settings.step(twist.angle, target, time.delta_secs());
        let chain = chain(&transforms, settings.bone, settings.to);
        let share = twist.angle / chain.len() as f32;
        for bone in chain {
            let Some(parent) = transforms
                .get(bone)
                .ok()
                .and_then(|(_, child_of)| child_of)
                .map(ChildOf::parent)
            else {
                continue;
            };
            let Some(above) = world_rotation(&transforms, parent) else {
                continue;
            };
            // Around the world's up, seen from the bone's parent.
            let turn = Quat::from_axis_angle(above.inverse() * Vec3::Y, share);
            if let Ok((mut transform, _)) = transforms.get_mut(bone) {
                transform.rotation = turn * transform.rotation;
            }
        }
    }
}

/// The bones from `bone` up to `to`, `bone` first; `bone` alone if `to` is
/// none or not above it.
fn chain(
    transforms: &Query<(&mut Transform, Option<&ChildOf>)>,
    bone: Entity,
    to: Option<Entity>,
) -> Vec<Entity> {
    let mut chain = vec![];
    let mut current = to;
    while let Some(entity) = current {
        chain.push(entity);
        if entity == bone {
            chain.reverse();
            return chain;
        }
        current = transforms
            .get(entity)
            .ok()
            .and_then(|(_, child_of)| child_of)
            .map(ChildOf::parent);
    }
    vec![bone]
}

/// `entity`'s rotation in the world, by its own and its ancestors'
/// transforms: this frame's, before they propagate.
fn world_rotation(
    transforms: &Query<(&mut Transform, Option<&ChildOf>)>,
    entity: Entity,
) -> Option<Quat> {
    let mut rotation = Quat::IDENTITY;
    let mut current = Some(entity);
    while let Some(entity) = current {
        let (transform, child_of) = transforms.get(entity).ok()?;
        rotation = transform.rotation * rotation;
        current = child_of.map(ChildOf::parent);
    }
    Some(rotation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(direction: Vec3) -> Settings {
        Settings {
            bone: Entity::PLACEHOLDER,
            to: None,
            direction,
            max: 1.0,
            weight: 1.0,
            time: 0.2,
        }
    }

    #[test]
    fn it_turns_toward_the_direction_within_the_max() {
        // Facing +Z; +X is to the left.
        let left = settings(Vec3::new(1.0, 0.0, 1.0));
        assert!((left.target(0.0) - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
        assert_eq!(settings(Vec3::X).target(0.0), 1.0);
        assert_eq!(settings(Vec3::NEG_X).target(0.0), -1.0);
        assert_eq!(settings(Vec3::Y).target(0.0), 0.0);
        let half = Settings {
            weight: 0.5,
            ..settings(Vec3::X)
        };
        assert_eq!(half.target(0.0), 0.5);
    }

    #[test]
    fn it_turns_most_of_the_way_in_its_time() {
        let settings = settings(Vec3::X);
        let mut angle = 0.0;
        for _ in 0..20 {
            angle = settings.step(angle, 1.0, 0.01);
        }
        assert!((angle - 0.9).abs() < 1e-4, "{angle}");
    }
}
