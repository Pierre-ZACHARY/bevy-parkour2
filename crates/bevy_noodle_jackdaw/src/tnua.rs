//! Tnua integration: a sink feeding the walk basis (motion, and where to
//! face) and sources reading the controller's state, for a scheme whose
//! basis is `TnuaBuiltinWalk`.
//!
//! Tnua State and Tnua Local Velocity read the controller of the running
//! entity or, failing that, of its nearest ancestor: a graph run for a
//! character's model (its animation) sees the character's state.

use std::marker::PhantomData;

use bevy::prelude::*;
use bevy_tnua::builtins::TnuaBuiltinWalk;
use bevy_tnua::prelude::*;

use crate::model::{FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType, nearest_component, nearest_with};
use crate::run::FlowGraphSystems;

pub const WALK: &str = "tnua/walk";
pub const STATE: &str = "tnua/state";
pub const LOCAL_VELOCITY: &str = "tnua/local_velocity";

/// Registers `tnua/walk`, `tnua/state` and `tnua/local_velocity` for
/// controllers of scheme `S`, and
/// runs graphs among Tnua's user controls. Tnua's controller plugin must run
/// in `FixedUpdate`, as graphs do.
pub struct TnuaFlowNodesPlugin<S>(PhantomData<S>);

impl<S> Default for TnuaFlowNodesPlugin<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S: TnuaScheme<Basis = TnuaBuiltinWalk>> Plugin for TnuaFlowNodesPlugin<S> {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            FixedUpdate,
            FlowGraphSystems::Run.in_set(TnuaUserControlsSystems),
        )
        .register_flow_node(FlowNodeType::apply(
            WALK,
            "Tnua Walk",
            // Facing: where the character turns, zero to face where it
            // walks.
            &[("motion", PortKind::Vec3), ("facing", PortKind::Vec3)],
            |world, entity, inputs| {
                let Some(mut controller) = world.get_mut::<TnuaController<S>>(entity) else {
                    return;
                };
                let (motion, facing) = (inputs[0].as_vec3(), inputs[1].as_vec3());
                controller.initiate_action_feeding();
                controller.basis = TnuaBuiltinWalk {
                    desired_motion: motion,
                    desired_forward: Dir3::new(facing).or_else(|_| Dir3::new(motion)).ok(),
                };
            },
        ))
        .register_flow_node(FlowNodeType::compute(
            STATE,
            "Tnua State",
            &[],
            &[
                ("airborne", PortKind::Bool),
                ("motion", PortKind::Vec3),
                ("velocity", PortKind::Vec3),
            ],
            |world, entity, _| {
                let Some(controller) = nearest_component::<TnuaController<S>>(world, entity) else {
                    return Vec::new();
                };
                vec![
                    FlowValue::Bool(controller.is_airborne().unwrap_or(false)),
                    // What the character is asked to walk: speed and
                    // direction, a length of 1 being the configured speed.
                    FlowValue::Vec3(controller.basis.desired_motion),
                    // How fast it walks, in m/s, along the ground.
                    FlowValue::Vec3(controller.basis_memory.running_velocity),
                ]
            },
        ))
        .register_flow_node(FlowNodeType::compute(
            LOCAL_VELOCITY,
            "Tnua Local Velocity",
            &[],
            &[
                ("x", PortKind::Float),
                ("y", PortKind::Float),
                ("vector", PortKind::Vec2),
            ],
            |world, entity, _| {
                let Some(character) = nearest_with::<TnuaController<S>>(world, entity) else {
                    return Vec::new();
                };
                let velocity = world
                    .get::<TnuaController<S>>(character)
                    .map_or(Vec3::ZERO, |c| c.basis_memory.running_velocity);
                let local = local_velocity(world.get::<GlobalTransform>(character), velocity);
                vec![
                    FlowValue::Float(local.x),
                    FlowValue::Float(local.y),
                    FlowValue::Vec2(local),
                ]
            },
        ));
    }
}

/// How fast a character placed at `transform` walks `velocity` (m/s), as
/// it faces: `x` to its right, `y` ahead (its -Z, which Tnua turns to face
/// where it is asked to).
fn local_velocity(transform: Option<&GlobalTransform>, velocity: Vec3) -> Vec2 {
    let rotation = transform.map_or(Quat::IDENTITY, GlobalTransform::rotation);
    let local = rotation.inverse() * velocity;
    Vec2::new(local.x, -local.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_velocity_is_as_the_character_faces() {
        // Turned to face +X (right of -Z): walking +X is ahead, -Z to its
        // left.
        let facing_x = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
            -std::f32::consts::FRAC_PI_2,
        )));
        let ahead = local_velocity(Some(&facing_x), Vec3::X * 2.0);
        assert!(ahead.distance(Vec2::new(0.0, 2.0)) < 1e-5, "{ahead}");
        let left = local_velocity(Some(&facing_x), Vec3::NEG_Z);
        assert!(left.distance(Vec2::new(-1.0, 0.0)) < 1e-5, "{left}");
        // Unplaced: facing -Z.
        assert_eq!(local_velocity(None, Vec3::X), Vec2::X);
    }
}
