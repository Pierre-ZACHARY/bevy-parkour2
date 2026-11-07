//! A turn's root motion: how its root turns and travels, taken out of its
//! poses (which play in place) and kept as curves, for the character's
//! controller and facing to use rather than the clip moving the body.
//!
//! The graph's Root Motion node (`anim/root_motion`) tells the current
//! clip's (in a select, the one it picked): how far it `turn`s in all and
//! has `turned` (degrees, left positive); none for a clip without.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowNodeType, FlowValue, PortKind};

use crate::{AnimationState, ClipLibraries};

pub const ROOT_MOTION: &str = "anim/root_motion";

/// The Root Motion node: the current clip's turn.
pub(crate) fn node() -> FlowNodeType {
    FlowNodeType::compute(
        ROOT_MOTION,
        "Root Motion",
        &[],
        &[("turn", PortKind::Float), ("turned", PortKind::Float)],
        |world, entity, _| {
            let (turn, turned) = world
                .get::<AnimationState>(entity)
                .and_then(|state| {
                    let root = world
                        .get_resource::<ClipLibraries>()?
                        .0
                        .get(&state.kind)?
                        .root
                        .as_ref()?;
                    Some((root.total_turn(), root.turn_at(state.time)))
                })
                .unwrap_or_default();
            vec![
                FlowValue::Float(turn.to_degrees()),
                FlowValue::Float(turned.to_degrees()),
            ]
        },
    )
}

/// How far a clip's root has turned and travelled since its start, sampled
/// evenly over the clip, its first sample at the start and its last at the
/// end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RootMotion {
    /// Radians around +Y, left positive.
    pub turn: Vec<f32>,
    /// Metres in the start's frame (+Z its forward, +X its left).
    pub travel: Vec<Vec2>,
}

impl RootMotion {
    /// How far the whole clip turns.
    pub fn total_turn(&self) -> f32 {
        self.turn.last().copied().unwrap_or_default()
    }

    /// How far it has turned `fraction` (0 to 1) through the clip.
    pub fn turn_at(&self, fraction: f32) -> f32 {
        sample(&self.turn, fraction, f32::lerp)
    }

    /// How far it has travelled `fraction` (0 to 1) through the clip.
    pub fn travel_at(&self, fraction: f32) -> Vec2 {
        sample(&self.travel, fraction, Vec2::lerp)
    }
}

/// `samples` at `fraction` through them, between the nearest two.
fn sample<T: Copy + Default>(samples: &[T], fraction: f32, lerp: impl Fn(T, T, f32) -> T) -> T {
    let Some(last) = samples.len().checked_sub(1) else {
        return T::default();
    };
    let at = (fraction * last as f32).clamp(0.0, last as f32);
    let k = (at.floor() as usize).min(last.saturating_sub(1));
    let next = (k + 1).min(last);
    lerp(samples[k], samples[next], at - k as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarter_turn() -> RootMotion {
        RootMotion {
            turn: vec![0.0, 0.5, 1.5],
            travel: vec![Vec2::ZERO, Vec2::new(0.0, 1.0), Vec2::new(2.0, 1.0)],
        }
    }

    #[test]
    fn samples_between_the_nearest_two() {
        let root = quarter_turn();
        assert_eq!(root.turn_at(0.0), 0.0);
        assert_eq!(root.turn_at(0.25), 0.25);
        assert_eq!(root.turn_at(0.75), 1.0);
        assert_eq!(root.turn_at(1.0), 1.5);
        assert_eq!(root.travel_at(0.75), Vec2::new(1.0, 1.0));
        assert_eq!(root.total_turn(), 1.5);
    }

    #[test]
    fn holds_its_ends_beyond_the_clip() {
        let root = quarter_turn();
        assert_eq!(root.turn_at(-1.0), 0.0);
        assert_eq!(root.turn_at(2.0), 1.5);
        assert_eq!(RootMotion::default().turn_at(0.5), 0.0);
        assert_eq!(
            RootMotion {
                turn: vec![0.3],
                ..default()
            }
            .turn_at(0.5),
            0.3
        );
    }
}
