//! The Camera Aim node, for a character's movement graph: a stick or keys
//! (+Y forward, +X right) made relative to where the character's camera
//! aims (`direction`), and that aim, level (`forward`, for Tnua Walk's
//! `facing`). It reads the [`CameraAim`] of the running entity or its
//! nearest ancestor. Without one, forward is world -Z and `forward` is
//! zero (the character faces where it walks).

use bevy::prelude::*;
use bevy_noodle_jackdaw::nodes::camera_relative;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, FlowValue, PortKind, nearest_component};

use crate::controller::CameraAim;

pub const AIM: &str = "camera/aim";

pub(crate) fn register(app: &mut App) {
    app.register_type::<CameraAim>()
        .register_flow_node(FlowNodeType::compute(
            AIM,
            "Camera Aim",
            &[("input", PortKind::Vec2)],
            &[("direction", PortKind::Vec3), ("forward", PortKind::Vec3)],
            |world, entity, inputs| {
                let aim = nearest_component::<CameraAim>(world, entity);
                let yaw = Quat::from_rotation_y(aim.map_or(0.0, |aim| aim.yaw));
                vec![
                    FlowValue::Vec3(camera_relative(inputs[0].as_vec2(), yaw)),
                    FlowValue::Vec3(aim.map_or(Vec3::ZERO, CameraAim::forward)),
                ]
            },
        ));
}
