//! What the movement graph tells the rest of the game about the character:
//! for now how far it goes before it is at rest, [`StopDistance`], which
//! its animation matches its stop clip with.
//!
//! Set Stop Distance (a sink, in the movement graph) keeps the value on the
//! character; Stop Distance (a source) reads it from the running entity or
//! its nearest ancestor, so a graph run for the character's model sees it.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, FlowValue, PortKind, nearest_component};

pub const SET_STOP_DISTANCE: &str = "character/set_stop_distance";
pub const STOP_DISTANCE: &str = "character/stop_distance";

/// How far (m) the character goes before it is at rest, as its movement
/// graph last said: none when it stands still.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct StopDistance(pub f32);

pub(crate) fn register(app: &mut App) {
    app.register_type::<StopDistance>()
        .register_flow_node(FlowNodeType::apply(
            SET_STOP_DISTANCE,
            "Set Stop Distance",
            &[("distance", PortKind::Float)],
            |world, entity, inputs| {
                if let Ok(mut character) = world.get_entity_mut(entity) {
                    character.insert(StopDistance(inputs[0].as_f32().max(0.0)));
                }
            },
        ))
        .register_flow_node(FlowNodeType::compute(
            STOP_DISTANCE,
            "Stop Distance",
            &[],
            &[("distance", PortKind::Float)],
            |world, entity, _| {
                let distance =
                    nearest_component::<StopDistance>(world, entity).map_or(0.0, |d| d.0);
                vec![FlowValue::Float(distance)]
            },
        ));
}
