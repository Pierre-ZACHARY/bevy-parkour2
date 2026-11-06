//! The character: a Tnua controller on an Avian capsule, walking where its
//! movement graph says, or where `PlayerAction::Move` points relative to the
//! camera when it has no graph.
//!
//! Put [`CharacterController`] on an entity whose Transform is at the
//! capsule's centre, with its model as a child placed `FLOAT_HEIGHT` lower.
//! Adding it fills in the physics body, the Tnua controller and the default
//! input map.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_noodle_jackdaw::FlowGraphRunner;
use bevy_tnua::builtins::{TnuaBuiltinWalk, TnuaBuiltinWalkConfig};
use bevy_tnua::prelude::*;
use bevy_tnua_avian3d::prelude::*;
use leafwing_input_manager::prelude::*;

use crate::graph::MOVEMENT_GRAPH;
use crate::input::{PlayerAction, default_input_map};

/// Capsule radius, sized to a human about 1.8 m tall.
pub const RADIUS: f32 = 0.3;
/// Capsule height, caps included.
pub const HEIGHT: f32 = 1.8;
/// Height of the body's centre above the ground at rest. Tnua needs it a
/// little above the capsule's half height; the model child sits this much
/// below the body so its feet touch the ground.
pub const FLOAT_HEIGHT: f32 = HEIGHT / 2.0 + 0.05;
/// Walking speed without a movement graph, m/s. With one, the graph's
/// Gait node sets the speed: Tnua's walk speed is 1 m/s, so the motion fed
/// to Tnua Walk is a velocity.
pub const SPEED: f32 = 4.0;

/// A character driven by a movement graph.
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct CharacterController {
    /// Asset path of the movement graph it runs; empty to walk where
    /// `PlayerAction::Move` points without one.
    pub graph: String,
}

impl Default for CharacterController {
    fn default() -> Self {
        Self {
            graph: MOVEMENT_GRAPH.into(),
        }
    }
}

#[derive(TnuaScheme)]
#[scheme(basis = TnuaBuiltinWalk)]
pub enum ControlScheme {}

pub(crate) fn build_controller(
    add: On<Add<CharacterController>>,
    controllers: Query<&CharacterController>,
    mut commands: Commands,
    mut configs: ResMut<Assets<ControlSchemeConfig>>,
    assets: Res<AssetServer>,
) {
    let config = configs.add(ControlSchemeConfig {
        basis: TnuaBuiltinWalkConfig {
            speed: 1.0,
            float_height: FLOAT_HEIGHT,
            ..default()
        },
    });
    let mut entity = commands.entity(add.entity);
    entity.insert((
        RigidBody::Dynamic,
        Collider::capsule(RADIUS, HEIGHT - 2.0 * RADIUS),
        TnuaController::<ControlScheme>::default(),
        TnuaConfig::<ControlScheme>(config),
        // Ground probe; a cylinder slightly narrower than the capsule.
        TnuaAvian3dSensorShape(Collider::cylinder(RADIUS - 0.01, 0.0)),
        // Tnua turns the body around Y itself; keep physics from tipping it.
        LockedAxes::ROTATION_LOCKED.unlock_rotation_y(),
        // Smooth between physics steps, for what follows it every frame
        // (a camera).
        TransformInterpolation,
        default_input_map(),
    ));
    if let Ok(controller) = controllers.get(add.entity)
        && !controller.graph.is_empty()
    {
        entity.insert(FlowGraphRunner {
            graph: assets.load(&controller.graph),
        });
    }
}

/// Characters that walk without a graph.
type Ungraphed = (With<CharacterController>, Without<FlowGraphRunner>);

/// Walking without a graph.
pub(crate) fn apply_controls(
    camera: Option<Single<&GlobalTransform, With<Camera3d>>>,
    mut controllers: Query<
        (
            &ActionState<PlayerAction>,
            &mut TnuaController<ControlScheme>,
        ),
        Ungraphed,
    >,
) {
    let yaw = camera.map_or(Quat::IDENTITY, |camera| yaw_of(camera.rotation()));
    for (actions, mut controller) in &mut controllers {
        controller.initiate_action_feeding();
        let motion = camera_relative(actions.axis_pair(&PlayerAction::Move), yaw);
        controller.basis = TnuaBuiltinWalk {
            desired_motion: motion * SPEED,
            desired_forward: Dir3::new(motion).ok(),
        };
    }
}

/// The rotation's turn around world Y alone.
fn yaw_of(rotation: Quat) -> Quat {
    let forward = rotation * Vec3::NEG_Z;
    Quat::from_rotation_y(f32::atan2(-forward.x, -forward.z))
}

/// Stick or keys (+Y forward, +X right) to a horizontal world direction,
/// forward being where the camera looks. Length is capped at 1, so a
/// diagonal on keys is no faster than a straight line.
fn camera_relative(input: Vec2, yaw: Quat) -> Vec3 {
    let input = input.clamp_length_max(1.0);
    yaw * Vec3::new(input.x, 0.0, -input.y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{floor_app, temp_assets, walk_forward};

    #[test]
    fn input_is_relative_to_camera_yaw() {
        // A camera looking down +X: forward is +X, right is +Z.
        let yaw = yaw_of(
            Transform::default()
                .looking_to(Vec3::new(1.0, -0.5, 0.0), Vec3::Y)
                .rotation,
        );
        assert!(camera_relative(Vec2::Y, yaw).abs_diff_eq(Vec3::X, 1e-5));
        assert!(camera_relative(Vec2::X, yaw).abs_diff_eq(Vec3::Z, 1e-5));
        assert!((camera_relative(Vec2::ONE, yaw).length() - 1.0).abs() < 1e-5);
    }

    /// Holding the physical W key (Z on AZERTY) walks a character with no
    /// graph forward on a static floor, with no camera (world -Z is
    /// forward).
    #[test]
    fn holding_forward_key_walks_forward() {
        let assets = temp_assets("no-graph");
        let mut app = floor_app(&assets);
        let character = app
            .world_mut()
            .spawn((
                CharacterController {
                    graph: String::new(),
                },
                Transform::from_xyz(0.0, FLOAT_HEIGHT, 0.0),
            ))
            .id();
        let (moved, end) = walk_forward(&mut app, character);
        assert!(moved.z < -0.8 * SPEED, "moved {moved}");
        assert!(moved.x.abs() < 0.05, "moved {moved}");
        assert!((end.y - FLOAT_HEIGHT).abs() < 0.1, "height {}", end.y);
        _ = std::fs::remove_dir_all(assets);
    }
}
