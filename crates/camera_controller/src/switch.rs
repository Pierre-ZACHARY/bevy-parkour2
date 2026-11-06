//! The Switch Camera node: pressed while its camera is active, the next
//! [`CameraController`] camera (in entity order, round) renders instead. A
//! camera that follows nothing (no Third Person node has run for it: the
//! fly camera) starts where the one left was.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, PortKind};

use crate::controller::{CameraController, is_active, set_world_transform, world_transform};
use crate::third_person::ThirdPersonView;

pub const SWITCH: &str = "camera/switch";

/// Whether the node's button was down last frame, so a press switches once.
/// A camera switched to starts with it down: the press that switched to it
/// does not switch back.
#[derive(Component, Clone, Copy, Debug, Default)]
struct SwitchHeld(bool);

pub(crate) fn register(app: &mut App) {
    app.register_flow_node(FlowNodeType::apply(
        SWITCH,
        "Switch Camera",
        &[("pressed", PortKind::Bool)],
        |world, camera, inputs| {
            let pressed = inputs[0].as_bool();
            let held = world.get::<SwitchHeld>(camera).is_some_and(|held| held.0);
            world.entity_mut(camera).insert(SwitchHeld(pressed));
            if pressed && !held && is_active(world, camera) {
                switch(world, camera);
            }
        },
    ));
}

/// Render through the camera after `from` instead.
pub fn switch(world: &mut World, from: Entity) {
    let mut cameras: Vec<Entity> = world
        .query_filtered::<Entity, (With<CameraController>, With<Camera>)>()
        .iter(world)
        .collect();
    cameras.sort();
    let Some(index) = cameras.iter().position(|&camera| camera == from) else {
        return;
    };
    let to = cameras[(index + 1) % cameras.len()];
    if to == from {
        return;
    }
    if !world.entity(to).contains::<ThirdPersonView>()
        && let Some(pose) = world_transform(world, from)
    {
        set_world_transform(world, to, pose);
    }
    for (camera, active) in [(from, false), (to, true)] {
        if let Some(mut own) = world.get_mut::<Camera>(camera) {
            own.is_active = active;
        }
    }
    world.entity_mut(to).insert(SwitchHeld(true));
}
