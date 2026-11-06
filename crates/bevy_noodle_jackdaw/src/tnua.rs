//! Tnua integration: a sink feeding the walk basis (motion, and where to
//! face) and a source reading the controller's state, for a scheme whose
//! basis is `TnuaBuiltinWalk`.
//!
//! Tnua State reads the controller of the running entity or, failing that,
//! of its nearest ancestor: a graph run for a character's model (its
//! animation) sees the character's state.

use std::marker::PhantomData;

use bevy::prelude::*;
use bevy_tnua::builtins::TnuaBuiltinWalk;
use bevy_tnua::prelude::*;

use crate::model::{FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType, nearest_component};
use crate::run::FlowGraphSystems;

pub const WALK: &str = "tnua/walk";
pub const STATE: &str = "tnua/state";

/// Registers `tnua/walk` and `tnua/state` for controllers of scheme `S`, and
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
        ));
    }
}
