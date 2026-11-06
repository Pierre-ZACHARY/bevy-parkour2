//! leafwing-input-manager integration: a source node per action of an
//! `Actionlike` enum, reading the `ActionState` of the running entity or,
//! failing that, of its nearest ancestor (a character's model reads the
//! character's).

use std::marker::PhantomData;

use bevy::prelude::*;
use bevy::reflect::TypeInfo;
use bevy::reflect::enums::{DynamicEnum, DynamicVariant, VariantInfo};
use leafwing_input_manager::prelude::*;
use leafwing_input_manager::{Actionlike, InputControlKind};

use crate::model::{FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType, nearest_component};

/// Registers a node for each unit variant of `A`, kind
/// `leafwing/<Enum>::<Variant>`, whose outputs follow the action's control
/// kind: a button gives `pressed`, an axis `value`, a dual axis `x`, `y`
/// and `vector`, a triple axis `x`, `y`, `z` and `vector`.
pub struct LeafwingFlowNodesPlugin<A>(PhantomData<A>);

impl<A> Default for LeafwingFlowNodesPlugin<A> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<A: Actionlike> Plugin for LeafwingFlowNodesPlugin<A> {
    fn build(&self, app: &mut App) {
        for action in unit_actions::<A>() {
            app.register_flow_node(action_node(action));
        }
    }
}

/// The node kind of `action`.
pub fn action_kind<A: Actionlike>(action: &A) -> String {
    let variant = action.reflect_ref().as_enum().map_or_else(
        |_| format!("{action:?}"),
        |value| value.variant_name().to_owned(),
    );
    format!("leafwing/{}::{variant}", A::short_type_path())
}

/// Every unit variant of `A` (variants with fields cannot be built blind).
fn unit_actions<A: Actionlike>() -> Vec<A> {
    let TypeInfo::Enum(info) = A::type_info() else {
        return Vec::new();
    };
    info.iter()
        .filter_map(|variant| match variant {
            VariantInfo::Unit(unit) => {
                A::from_reflect(&DynamicEnum::new(unit.name(), DynamicVariant::Unit))
            }
            _ => None,
        })
        .collect()
}

fn action_node<A: Actionlike>(action: A) -> FlowNodeType {
    let kind = action_kind(&action);
    let title = kind.rsplit("::").next().unwrap_or(&kind).to_owned();
    let outputs: &[(&str, PortKind)] = match action.input_control_kind() {
        InputControlKind::Button => &[("pressed", PortKind::Bool)],
        InputControlKind::Axis => &[("value", PortKind::Float)],
        InputControlKind::DualAxis => &[
            ("x", PortKind::Float),
            ("y", PortKind::Float),
            ("vector", PortKind::Vec2),
        ],
        InputControlKind::TripleAxis => &[
            ("x", PortKind::Float),
            ("y", PortKind::Float),
            ("z", PortKind::Float),
            ("vector", PortKind::Vec3),
        ],
    };
    FlowNodeType::compute(kind, title, &[], outputs, move |world, entity, _| {
        let Some(state) = nearest_component::<ActionState<A>>(world, entity) else {
            return Vec::new();
        };
        match action.input_control_kind() {
            InputControlKind::Button => vec![FlowValue::Bool(state.pressed(&action))],
            InputControlKind::Axis => vec![FlowValue::Float(state.value(&action))],
            InputControlKind::DualAxis => {
                let pair = state.axis_pair(&action);
                vec![
                    FlowValue::Float(pair.x),
                    FlowValue::Float(pair.y),
                    FlowValue::Vec2(pair),
                ]
            }
            InputControlKind::TripleAxis => {
                let triple = state.axis_triple(&action);
                vec![
                    FlowValue::Float(triple.x),
                    FlowValue::Float(triple.y),
                    FlowValue::Float(triple.z),
                    FlowValue::Vec3(triple),
                ]
            }
        }
    })
}
