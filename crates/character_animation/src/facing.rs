//! Facing: which way the character's model faces, apart from its body.
//!
//! The controller turns the body (to face the aim, where it walks); the
//! model's yaw is its own, held while the character stands so its feet stay
//! planted as the body turns under it, and following the body otherwise.
//! The graph's Facing node (`anim/facing`) sets how:
//!
//! - `follow` (bool): whether the model turns back to the body, rather than
//!   holding its yaw;
//! - `time` (s): how long it takes, following, to turn most of the way
//!   (90%) back;
//! - `max` (degrees): how far apart it may be from the body at most;
//! - `turn`, `turned` (degrees): how far a turn clip playing turns in all
//!   and has turned (the Root Motion node's): as it turns, the model turns
//!   back to the body in step, by the share of what it had left to turn it
//!   turned, so it faces the body as the clip ends, however far apart
//!   they were;
//!
//! and tells how far apart they are (`offset`, degrees, left positive).
//! [`face`] applies it every frame, after the body's interpolation, on top
//! of the model's own rotation under the body (as authored).

use bevy::prelude::*;
use bevy_noodle_jackdaw::{
    FlowGraph, FlowGraphPlayback, FlowGraphRunner, FlowNodeType, FlowValue, PortKind,
};

pub const FACING: &str = "anim/facing";

/// How far the model's yaw is from its body's, kept from frame to frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Facing {
    /// Radians around +Y, left positive.
    pub offset: f32,
    /// The model's rotation under the body as authored, once seen.
    rest: Option<Quat>,
    /// The model's yaw in the world last frame, once turned.
    last: Option<f32>,
    /// How far the turn clip playing had turned last frame (radians).
    turned: f32,
}

/// The Facing node: settings in, the offset out.
pub(crate) fn node() -> FlowNodeType {
    FlowNodeType::compute(
        FACING,
        "Facing",
        &[
            ("follow", PortKind::Bool),
            ("time", PortKind::Float),
            ("max", PortKind::Float),
            ("turn", PortKind::Float),
            ("turned", PortKind::Float),
        ],
        &[("offset", PortKind::Float)],
        // Applied by `face`, every frame.
        |world, entity, _| {
            let offset = world.get::<Facing>(entity).map_or(0.0, |f| f.offset);
            vec![FlowValue::Float(offset.to_degrees())]
        },
    )
    .with_default("time", FlowValue::Float(0.3))
    .with_default("max", FlowValue::Float(120.0))
}

/// The settings of a graph's Facing node, as it last ran.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    follow: bool,
    time: f32,
    max: f32,
    /// Radians.
    turn: f32,
    turned: f32,
}

impl Settings {
    fn of(graph: &FlowGraph, playback: &FlowGraphPlayback) -> Option<Self> {
        let id = node_path(graph, FACING)?;
        let inputs = &playback.node(&id)?.inputs;
        let input = |index: usize| inputs.get(index).cloned().unwrap_or(FlowValue::Float(0.0));
        Some(Self {
            follow: input(0).as_bool(),
            time: input(1).as_f32(),
            max: input(2).as_f32().to_radians(),
            turn: input(3).as_f32().to_radians(),
            turned: input(4).as_f32().to_radians(),
        })
    }

    /// `offset` closed as the turn clip turns: since `last` (how far it
    /// had turned), by the share of what it had left to turn it turned.
    fn close(self, offset: f32, last: f32) -> f32 {
        let left = self.turn - last;
        if left.abs() < 1e-4 {
            return offset;
        }
        let share = ((self.turned - last) / left).clamp(0.0, 1.0);
        offset * (1.0 - share)
    }

    /// One frame of `dt` on from `offset`.
    fn step(self, offset: f32, dt: f32) -> f32 {
        let offset = if !self.follow {
            offset
        } else if self.time > 0.0 {
            offset * 0.1_f32.powf(dt / self.time)
        } else {
            0.0
        };
        offset.clamp(-self.max.max(0.0), self.max.max(0.0))
    }
}

/// The playback id of the graph's first node of `kind`, inner graphs'
/// included.
pub(crate) fn node_path(graph: &FlowGraph, kind: &str) -> Option<String> {
    graph.nodes.iter().find_map(|node| {
        if node.kind == kind {
            Some(node.id.clone())
        } else {
            let inner = node_path(node.graph.as_ref()?, kind)?;
            Some(format!("{}/{inner}", node.id))
        }
    })
}

/// Turns each model with a Facing node in its graph by its offset from the
/// body: holding its yaw in the world as the body turns, unless it follows.
pub(crate) fn face(
    graphs: Res<Assets<FlowGraph>>,
    time: Res<Time>,
    mut models: Query<(
        &FlowGraphRunner,
        &FlowGraphPlayback,
        &mut Facing,
        &mut Transform,
        &ChildOf,
    )>,
    bodies: Query<(&Transform, Option<&ChildOf>), Without<Facing>>,
) {
    for (runner, playback, mut facing, mut transform, child_of) in &mut models {
        let Some(settings) = graphs
            .get(&runner.graph)
            .and_then(|graph| Settings::of(graph, playback))
        else {
            continue;
        };
        let Some(body) = body_yaw(&bodies, child_of.parent()) else {
            continue;
        };
        let rest = *facing.rest.get_or_insert(transform.rotation);
        // Where the model faced last frame, from where the body faces now.
        let held = facing.last.map_or(0.0, |last| wrap(last - body));
        // A new turn starts from none turned.
        let last = if settings.turned.abs() < facing.turned.abs() {
            0.0
        } else {
            facing.turned
        };
        let closed = settings.close(held, last);
        facing.turned = settings.turned;
        facing.offset = settings.step(closed, time.delta_secs());
        transform.rotation = Quat::from_rotation_y(facing.offset) * rest;
        facing.last = Some(body + facing.offset);
    }
}

/// The yaw (radians, left positive) of `entity` in the world, by its own
/// and its ancestors' transforms: this frame's, before they propagate.
fn body_yaw(
    bodies: &Query<(&Transform, Option<&ChildOf>), Without<Facing>>,
    entity: Entity,
) -> Option<f32> {
    let mut rotation = Quat::IDENTITY;
    let mut current = Some(entity);
    while let Some(entity) = current {
        let (transform, child_of) = bodies.get(entity).ok()?;
        rotation = transform.rotation * rotation;
        current = child_of.map(ChildOf::parent);
    }
    Some(yaw(rotation))
}

/// `rotation`'s turn around +Y.
pub(crate) fn yaw(rotation: Quat) -> f32 {
    let forward = rotation * Vec3::Z;
    forward.x.atan2(forward.z)
}

/// `angle` within half a turn of zero.
pub(crate) fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLD: Settings = Settings {
        follow: false,
        time: 0.3,
        max: 2.0,
        turn: 0.0,
        turned: 0.0,
    };

    #[test]
    fn standing_it_holds_its_offset_up_to_the_max() {
        assert_eq!(HOLD.step(1.0, 0.1), 1.0);
        assert_eq!(HOLD.step(-3.0, 0.1), -2.0);
    }

    #[test]
    fn following_it_turns_most_of_the_way_back_in_its_time() {
        let follow = Settings {
            follow: true,
            ..HOLD
        };
        let mut offset = 1.0;
        for _ in 0..30 {
            offset = follow.step(offset, 0.01);
        }
        assert!((offset - 0.1).abs() < 1e-4, "{offset}");
        let at_once = Settings {
            time: 0.0,
            ..follow
        };
        assert_eq!(at_once.step(1.0, 0.01), 0.0);
    }

    #[test]
    fn a_turn_closes_the_offset_as_it_turns() {
        // A quarter turn left, for a model 1 rad right of the body.
        let turning = |turned: f32| Settings {
            turn: std::f32::consts::FRAC_PI_2,
            turned,
            ..HOLD
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        let half_way = turning(quarter / 2.0).close(-1.0, 0.0);
        assert!((half_way + 0.5).abs() < 1e-5, "{half_way}");
        // The body turned on meanwhile: the rest closes all the same.
        assert!(turning(quarter).close(-0.8, quarter / 2.0).abs() < 1e-5);
        // Done turning, or no turn: held.
        assert_eq!(turning(quarter).close(-0.3, quarter), -0.3);
        assert_eq!(HOLD.close(-0.3, 0.0), -0.3);
    }

    #[test]
    fn yaws_wrap_within_half_a_turn() {
        use std::f32::consts::PI;
        assert!((wrap(1.5 * PI) + 0.5 * PI).abs() < 1e-5);
        assert!((yaw(Quat::from_rotation_y(0.5)) - 0.5).abs() < 1e-5);
    }
}
