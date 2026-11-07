//! The Third Person node: a camera over the shoulder of its target, which
//! faces where the camera aims. Held, `free_look` (the middle button) turns
//! the camera alone round the target, the aim staying where it was. Let
//! go, with `face_on_release` the aim comes to where the camera looks: the
//! target turns to face the reticle. Without, the camera comes back to the
//! aim over `recenter` seconds.
//!
//! Its inputs: what it follows (`target`, an entity, from an Entity node;
//! unwired, the first [`CameraTarget`]) and where on it (`bone`, from a Bone
//! node; unwired, the target's origin), the pivot it turns around
//! (`offset` from there, in world axes), where it sits from the pivot
//! (`shoulder`, right and up of the view; `distance` back), `look` (the
//! mouse's motion, pixels, turning the camera by `sensitivity` radians a
//! pixel once the window has the cursor), `look_stick` (the right stick,
//! `stick_speed` radians a second), and how far it looks down and up
//! (`min_pitch`, `max_pitch`, degrees).
//!
//! Inactive, it still follows the target, unturned: a culling camera keeps
//! what the player would see while another camera renders.

use bevy::prelude::*;
use bevy_noodle_jackdaw::{FlowGraphAppExt, FlowNodeType, FlowValue, PortKind};

use crate::controller::{
    CameraAim, CameraTarget, angle_between, is_active, look_turn, mouse_looks, set_world_transform,
    world_transform,
};

pub const THIRD_PERSON: &str = "camera/third_person";

/// Where a third-person camera aims and looks, in radians (yaw, pitch; see
/// [`CameraAim`]): the same, but while it looks around freely.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct ThirdPersonView {
    pub aim: Vec2,
    pub look: Vec2,
}

impl ThirdPersonView {
    /// Behind a target turned by `rotation`, level.
    pub fn behind(rotation: Quat) -> Self {
        let forward = rotation * Vec3::NEG_Z;
        let aim = Vec2::new(f32::atan2(-forward.x, -forward.z), 0.0);
        Self { aim, look: aim }
    }

    /// Turn by `turn` (yaw, pitch): the look alone while `free`, else the
    /// aim and the look. Out of a free look, the aim comes to the look if
    /// `face` (the target turns), else the look eases back to the aim (all
    /// the way in about `recenter` seconds). Pitches stay within `pitch`.
    pub fn turn(
        &mut self,
        turn: Vec2,
        free: bool,
        face: bool,
        pitch: (f32, f32),
        recenter: f32,
        dt: f32,
    ) {
        let clamp = |angles: Vec2| {
            Vec2::new(
                angle_between(0.0, angles.x),
                angles.y.clamp(pitch.0, pitch.1),
            )
        };
        self.look = clamp(self.look + turn);
        if free {
            return;
        }
        if face {
            // Turned with the look already.
            self.aim = self.look;
            return;
        }
        self.aim = clamp(self.aim + turn);
        let left = Vec2::new(
            angle_between(self.aim.x, self.look.x),
            self.look.y - self.aim.y,
        );
        let keep = if recenter > 0.0 {
            (-5.0 * dt / recenter).exp()
        } else {
            0.0
        };
        let left = if left.length() < 1e-4 {
            Vec2::ZERO
        } else {
            left * keep
        };
        self.look = clamp(self.aim + left);
    }

    /// The camera's pose for a target at `target`.
    pub fn pose(&self, target: Vec3, pivot: Vec3, shoulder: Vec3, distance: f32) -> Transform {
        let rotation = Quat::from_euler(EulerRot::YXZ, self.look.x, self.look.y, 0.0);
        let translation = target + pivot + rotation * (shoulder + Vec3::Z * distance);
        Transform::from_translation(translation).with_rotation(rotation)
    }
}

/// The node's inputs, in order.
const TARGET: usize = 0;
const BONE: usize = 1;
const OFFSET: usize = 2;
const SHOULDER: usize = 3;
const DISTANCE: usize = 4;
const LOOK: usize = 5;
const LOOK_STICK: usize = 6;
const FREE_LOOK: usize = 7;
const SENSITIVITY: usize = 8;
const STICK_SPEED: usize = 9;
const MIN_PITCH: usize = 10;
const MAX_PITCH: usize = 11;
const RECENTER: usize = 12;
const FACE_ON_RELEASE: usize = 13;

pub(crate) fn register(app: &mut App) {
    app.register_type::<ThirdPersonView>().register_flow_node(
        FlowNodeType::apply(
            THIRD_PERSON,
            "Third Person",
            &[
                ("target", PortKind::Entity),
                ("bone", PortKind::Entity),
                ("offset", PortKind::Vec3),
                ("shoulder", PortKind::Vec3),
                ("distance", PortKind::Float),
                ("look", PortKind::Vec2),
                ("look_stick", PortKind::Vec2),
                ("free_look", PortKind::Bool),
                ("sensitivity", PortKind::Float),
                ("stick_speed", PortKind::Float),
                ("min_pitch", PortKind::Float),
                ("max_pitch", PortKind::Float),
                ("recenter", PortKind::Float),
                ("face_on_release", PortKind::Bool),
            ],
            follow,
        )
        .with_default("sensitivity", FlowValue::Float(0.003))
        .with_default("stick_speed", FlowValue::Float(3.0))
        .with_default("offset", FlowValue::Vec3(Vec3::new(0.0, 0.55, 0.0)))
        .with_default("shoulder", FlowValue::Vec3(Vec3::new(0.5, 0.15, 0.0)))
        .with_default("distance", FlowValue::Float(2.2))
        .with_default("min_pitch", FlowValue::Float(-60.0))
        .with_default("max_pitch", FlowValue::Float(70.0))
        .with_default("recenter", FlowValue::Float(0.3))
        .with_default("face_on_release", FlowValue::Bool(true)),
    );
}

fn follow(world: &mut World, camera: Entity, inputs: &[FlowValue]) {
    let Some(target) = target(world, inputs[TARGET].as_entity()) else {
        return;
    };
    let Some(target_transform) = world_transform(world, target) else {
        return;
    };
    let anchor = anchor(world, target, target_transform, inputs[BONE].as_entity());
    let mut view = world
        .get::<ThirdPersonView>(camera)
        .copied()
        .unwrap_or_else(|| ThirdPersonView::behind(target_transform.rotation));
    if is_active(world, camera) {
        let mouse = if mouse_looks(world) {
            inputs[LOOK].as_vec2()
        } else {
            Vec2::ZERO
        };
        let dt = world.resource::<Time>().delta_secs();
        let turn = look_turn(
            mouse,
            inputs[LOOK_STICK].as_vec2(),
            inputs[SENSITIVITY].as_f32(),
            inputs[STICK_SPEED].as_f32(),
            dt,
        );
        let pitch = (
            inputs[MIN_PITCH].as_f32().to_radians(),
            inputs[MAX_PITCH].as_f32().to_radians(),
        );
        view.turn(
            turn,
            inputs[FREE_LOOK].as_bool(),
            inputs[FACE_ON_RELEASE].as_bool(),
            pitch,
            inputs[RECENTER].as_f32(),
            dt,
        );
    }
    let pose = view.pose(
        anchor,
        inputs[OFFSET].as_vec3(),
        inputs[SHOULDER].as_vec3(),
        inputs[DISTANCE].as_f32(),
    );
    set_world_transform(world, camera, pose);
    world.entity_mut(camera).insert(view);
    world.entity_mut(target).insert(CameraAim {
        yaw: view.aim.x,
        pitch: view.aim.y,
    });
}

/// What the camera follows: the entity wired into `target`, else the first
/// [`CameraTarget`].
fn target(world: &mut World, wired: Option<Entity>) -> Option<Entity> {
    wired
        .filter(|&entity| world.get_entity(entity).is_ok())
        .or_else(|| {
            world
                .query_filtered::<Entity, With<CameraTarget>>()
                .iter(world)
                .next()
        })
}

/// Where the camera follows: the target's origin (at `transform`), or its
/// bone. A bone sits on the target where it did when transforms last
/// propagated, on the target as it is now: the camera keeps up with the
/// target's motion, the bone's own lagging a frame.
fn anchor(world: &World, target: Entity, transform: Transform, bone: Option<Entity>) -> Vec3 {
    let Some(bone) = bone.and_then(|bone| world.get::<GlobalTransform>(bone)) else {
        return transform.translation;
    };
    let Some(target_global) = world.get::<GlobalTransform>(target) else {
        return bone.translation();
    };
    let on_target = target_global
        .affine()
        .inverse()
        .transform_point3(bone.translation());
    transform.transform_point(on_target)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use super::*;

    const PITCH: (f32, f32) = (-1.0, 1.2);

    #[test]
    fn it_starts_behind_the_target() {
        // The scene's player faces +Z.
        let view = ThirdPersonView::behind(Quat::from_rotation_y(PI));
        let pose = view.pose(Vec3::ZERO, Vec3::Y, Vec3::ZERO, 2.0);
        assert!(
            pose.translation
                .abs_diff_eq(Vec3::new(0.0, 1.0, -2.0), 1e-5)
        );
        assert!((pose.forward().as_vec3()).abs_diff_eq(Vec3::Z, 1e-5));
    }

    #[test]
    fn turning_turns_the_aim_with_the_camera() {
        let mut view = ThirdPersonView::default();
        view.turn(
            Vec2::new(FRAC_PI_2, 2.0),
            false,
            true,
            PITCH,
            0.3,
            1.0 / 60.0,
        );
        assert!(view.aim.abs_diff_eq(Vec2::new(FRAC_PI_2, PITCH.1), 1e-6));
        assert_eq!(view.look, view.aim);
        // A quarter turn left: the aim looks down -X.
        let aim = CameraAim {
            yaw: view.aim.x,
            pitch: view.aim.y,
        };
        assert!(aim.forward().abs_diff_eq(Vec3::NEG_X, 1e-5));
    }

    #[test]
    fn let_go_of_a_free_look_the_aim_comes_to_the_camera() {
        let mut view = ThirdPersonView::default();
        view.turn(Vec2::new(2.0, 0.5), true, true, PITCH, 0.3, 1.0 / 60.0);
        assert_eq!(view.aim, Vec2::ZERO);
        // Let go: the camera stays, the target turns to where it looks.
        view.turn(Vec2::ZERO, false, true, PITCH, 0.3, 1.0 / 60.0);
        assert!(view.look.abs_diff_eq(Vec2::new(2.0, 0.5), 1e-6));
        assert_eq!(view.aim, view.look);
        // Then they turn together.
        view.turn(Vec2::new(0.1, 0.0), false, true, PITCH, 0.3, 1.0 / 60.0);
        assert!(view.aim.abs_diff_eq(Vec2::new(2.1, 0.5), 1e-6));
        assert_eq!(view.aim, view.look);
    }

    #[test]
    fn free_look_keeps_the_aim_and_comes_back_to_it() {
        let mut view = ThirdPersonView::default();
        view.turn(Vec2::new(2.0, 0.5), true, false, PITCH, 0.3, 1.0 / 60.0);
        assert_eq!(view.aim, Vec2::ZERO);
        assert!(view.look.abs_diff_eq(Vec2::new(2.0, 0.5), 1e-6));
        // Let go: back to the aim within the recenter time, the short way.
        let mut yaws = Vec::new();
        for _ in 0..20 {
            view.turn(Vec2::ZERO, false, false, PITCH, 0.3, 1.0 / 60.0);
            yaws.push(view.look.x);
        }
        assert!(yaws.windows(2).all(|w| w[1] < w[0]), "{yaws:?}");
        assert!(view.look.abs_diff_eq(Vec2::ZERO, 0.02), "{}", view.look);
        assert_eq!(view.aim, Vec2::ZERO);
    }

    /// A tagged target elsewhere, and one at `x` with a `neck` above it.
    fn world_with_targets() -> (World, Entity, Entity) {
        let mut world = World::new();
        world.spawn((CameraTarget, Transform::from_xyz(9.0, 0.0, 0.0)));
        let hero = world
            .spawn((
                Transform::from_xyz(1.0, 0.0, 0.0),
                GlobalTransform::from_xyz(1.0, 0.0, 0.0),
            ))
            .id();
        let neck = world
            .spawn((ChildOf(hero), GlobalTransform::from_xyz(1.0, 1.5, 0.0)))
            .id();
        (world, hero, neck)
    }

    #[test]
    fn it_follows_the_wired_target_else_the_tagged_one() {
        let (mut world, hero, _) = world_with_targets();
        let tagged = world
            .query_filtered::<Entity, With<CameraTarget>>()
            .single(&world)
            .unwrap();
        assert_eq!(target(&mut world, Some(hero)), Some(hero));
        assert_eq!(target(&mut world, None), Some(tagged));
        world.despawn(hero);
        assert_eq!(target(&mut world, Some(hero)), Some(tagged));
    }

    #[test]
    fn a_bone_follows_its_target_as_it_moves() {
        let (world, hero, neck) = world_with_targets();
        let moved =
            Transform::from_xyz(3.0, 0.0, 0.0).with_rotation(Quat::from_rotation_z(FRAC_PI_2));
        // Up 1.5 on the target, now turned onto -X.
        let anchor = anchor(&world, hero, moved, Some(neck));
        assert!(
            anchor.abs_diff_eq(Vec3::new(1.5, 0.0, 0.0), 1e-5),
            "{anchor}"
        );
        assert_eq!(anchor_of_none(&world, hero, moved), moved.translation);
    }

    fn anchor_of_none(world: &World, target: Entity, transform: Transform) -> Vec3 {
        anchor(world, target, transform, None)
    }

    #[test]
    fn the_look_wraps_around() {
        let mut view = ThirdPersonView::default();
        for _ in 0..5 {
            view.turn(Vec2::new(1.0, 0.0), false, true, PITCH, 0.3, 1.0 / 60.0);
        }
        assert!((view.aim.x - (5.0 - 2.0 * PI)).abs() < 1e-4, "{}", view.aim);
    }
}
