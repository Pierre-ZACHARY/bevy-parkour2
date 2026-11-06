//! Retargeting, after Unreal 5's runtime retargeter: clips play on a source
//! skeleton spawned under the character, and every frame its pose is copied
//! onto the character's own skeleton.
//!
//! Bones are matched by their UE5 Manny name (the canonical skeleton: Yummy
//! Games' clips use it, and so does the Ninja). A character whose bones are
//! named otherwise gets a [`Rig`] naming its bone for each canonical one.
//!
//! For each retargeted bone, the target turns, in model space, as the source
//! turned away from its rest pose. Limb bones are first aimed the way the
//! source's point at rest, so a target in T-pose follows an A-pose source.
//! The pelvis also travels as the source's does, scaled by the ratio of
//! their heights, which keeps longer or shorter legs on the ground. Other
//! target bones (fingers, twist bones, hair) keep their rest pose under
//! their parent.
//!
//! It knows nothing of what plays the source: bind a [`Retarget`] onto the
//! character's model and add [`SkeletonRetargetPlugin`].

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// The canonical bones the retargeter moves, each with the child a limb
/// aims at, from the root down.
pub const RETARGETED: &[(&str, Option<&str>)] = &[
    ("pelvis", None),
    ("spine_01", None),
    ("spine_02", None),
    ("spine_03", None),
    ("spine_04", None),
    ("spine_05", None),
    ("neck_01", None),
    ("neck_02", None),
    ("head", None),
    ("clavicle_l", None),
    ("upperarm_l", Some("lowerarm_l")),
    ("lowerarm_l", Some("hand_l")),
    ("hand_l", Some("middle_01_l")),
    ("clavicle_r", None),
    ("upperarm_r", Some("lowerarm_r")),
    ("lowerarm_r", Some("hand_r")),
    ("hand_r", Some("middle_01_r")),
    ("thigh_l", Some("calf_l")),
    ("calf_l", Some("foot_l")),
    ("foot_l", Some("ball_l")),
    ("ball_l", None),
    ("thigh_r", Some("calf_r")),
    ("calf_r", Some("foot_r")),
    ("foot_r", Some("ball_r")),
    ("ball_r", None),
];

/// The bone that carries the pelvis travel.
const PELVIS: &str = "pelvis";

/// A character's bone names, for skeletons not named like UE5 Manny: put it
/// on the character's model.
#[derive(Component, Reflect, Clone, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct Rig {
    /// Canonical (Manny) bone name, then the character's; canonical bones
    /// left out keep their name.
    pub bones: Vec<(String, String)>,
}

impl Rig {
    /// The character's name for canonical bone `bone`.
    pub fn own<'a>(&'a self, bone: &'a str) -> &'a str {
        self.bones
            .iter()
            .find(|(canonical, _)| canonical == bone)
            .map_or(bone, |(_, own)| own)
    }
}

/// Poses every [`Retarget`]ed character from its source skeleton, after
/// animation and before transforms propagate.
pub struct SkeletonRetargetPlugin;

impl Plugin for SkeletonRetargetPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Rig>().add_systems(
            PostUpdate,
            retarget
                .after(bevy::app::AnimationSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

/// One bone of a skeleton walked from its top, parents first.
#[derive(Clone, Debug)]
struct Bone {
    entity: Entity,
    /// Index of its parent in the walk; `None` under the skeleton's frame.
    parent: Option<usize>,
}

/// A target bone the source drives.
#[derive(Clone, Debug)]
struct Driven {
    /// Index in the target walk.
    target: usize,
    /// Index in the source walk.
    source: usize,
    /// Source model rotation to target model rotation:
    /// `source_rest⁻¹ · aim · target_rest`.
    correction: Quat,
}

/// The pelvis travel: the target's rest position, the source's, and the
/// ratio of their heights.
#[derive(Clone, Debug)]
struct Travel {
    target: usize,
    source: usize,
    target_rest: Vec3,
    source_rest: Vec3,
    ratio: f32,
}

/// How one source skeleton drives one character's skeleton, both in the
/// frame of the character's model (the entity holding both).
#[derive(Component, Clone, Debug)]
pub struct Retarget {
    source: Vec<Bone>,
    target: Vec<Bone>,
    driven: Vec<Driven>,
    travel: Option<Travel>,
}

impl Retarget {
    /// Bind the skeleton under `source` (Manny names) to the one under
    /// `target`'s other children, from the poses they are in now: their
    /// rest poses, before anything plays. `None` if they share no bone.
    pub fn bind(world: &World, source: Entity, target: Entity, rig: Option<&Rig>) -> Option<Self> {
        let source_bones = walk(world, source, None);
        let target_bones = walk(world, target, Some(source));
        let named = |bones: &[Bone]| -> HashMap<String, usize> {
            bones
                .iter()
                .enumerate()
                .filter_map(|(index, bone)| {
                    Some((world.get::<Name>(bone.entity)?.as_str().to_owned(), index))
                })
                .collect()
        };
        let (source_names, target_names) = (named(&source_bones), named(&target_bones));
        let source_rest = model_poses(world, &source_bones);
        let target_rest = model_poses(world, &target_bones);
        let default_rig = Rig::default();
        let rig = rig.unwrap_or(&default_rig);
        let pair = |bone: &str| Some((*source_names.get(bone)?, *target_names.get(rig.own(bone))?));
        let mut driven = Vec::new();
        for (bone, aim) in RETARGETED {
            let Some((source, target)) = pair(bone) else {
                continue;
            };
            let aim = aim
                .and_then(pair)
                .map(|(source_child, target_child)| {
                    let to_source =
                        source_rest[source_child].translation - source_rest[source].translation;
                    let to_target =
                        target_rest[target_child].translation - target_rest[target].translation;
                    match (to_target.try_normalize(), to_source.try_normalize()) {
                        (Some(from), Some(to)) => Quat::from_rotation_arc(from, to),
                        _ => Quat::IDENTITY,
                    }
                })
                .unwrap_or(Quat::IDENTITY);
            driven.push(Driven {
                target,
                source,
                correction: source_rest[source].rotation.inverse()
                    * aim
                    * target_rest[target].rotation,
            });
        }
        if driven.is_empty() {
            return None;
        }
        driven.sort_by_key(|driven| driven.target);
        let travel = pair(PELVIS).and_then(|(source, target)| {
            let (source_rest, target_rest) = (source_rest[source], target_rest[target]);
            (source_rest.translation.y > 1e-3).then(|| Travel {
                target,
                source,
                target_rest: target_rest.translation,
                source_rest: source_rest.translation,
                ratio: target_rest.translation.y / source_rest.translation.y,
            })
        });
        Some(Self {
            source: source_bones,
            target: target_bones,
            driven,
            travel,
        })
    }

    /// How many target bones the source drives.
    pub fn driven_bones(&self) -> usize {
        self.driven.len()
    }

    /// Pose the target from the source's current pose.
    fn apply(&self, transforms: &mut Query<&mut Transform>) {
        let local = |transforms: &Query<&mut Transform>, entity| {
            transforms.get(entity).copied().unwrap_or_default()
        };
        let mut source = Vec::with_capacity(self.source.len());
        for bone in &self.source {
            let parent = bone.parent.map_or(Transform::IDENTITY, |p| source[p]);
            source.push(parent.mul_transform(local(transforms, bone.entity)));
        }
        let mut driven = self.driven.iter().peekable();
        let mut target: Vec<Transform> = Vec::with_capacity(self.target.len());
        for (index, bone) in self.target.iter().enumerate() {
            let parent = bone.parent.map_or(Transform::IDENTITY, |p| target[p]);
            let mut own = local(transforms, bone.entity);
            if let Some(drive) = driven.next_if(|drive| drive.target == index) {
                let rotation = source[drive.source].rotation * drive.correction;
                own.rotation = (parent.rotation.inverse() * rotation).normalize();
                if let Some(travel) = self.travel.as_ref().filter(|t| t.target == index) {
                    let position = travel.target_rest
                        + (source[travel.source].translation - travel.source_rest) * travel.ratio;
                    own.translation = parent.compute_affine().inverse().transform_point3(position);
                }
                if let Ok(mut transform) = transforms.get_mut(bone.entity) {
                    transform.rotation = own.rotation;
                    transform.translation = own.translation;
                }
            }
            target.push(parent.mul_transform(own));
        }
    }
}

/// The descendants of `top`, parents first, leaving out `skip`'s subtree.
fn walk(world: &World, top: Entity, skip: Option<Entity>) -> Vec<Bone> {
    let mut bones = Vec::new();
    let mut stack: Vec<(Entity, Option<usize>)> = children(world, top)
        .rev()
        .map(|child| (child, None))
        .collect();
    while let Some((entity, parent)) = stack.pop() {
        if Some(entity) == skip || world.get::<Transform>(entity).is_none() {
            continue;
        }
        let index = bones.len();
        bones.push(Bone { entity, parent });
        stack.extend(children(world, entity).rev().map(|c| (c, Some(index))));
    }
    bones
}

fn children(world: &World, entity: Entity) -> impl DoubleEndedIterator<Item = Entity> + '_ {
    world
        .get::<Children>(entity)
        .map(|children| children.to_vec())
        .unwrap_or_default()
        .into_iter()
}

/// Each bone's pose in the skeleton's frame, from the local poses now.
fn model_poses(world: &World, bones: &[Bone]) -> Vec<Transform> {
    let mut poses: Vec<Transform> = Vec::with_capacity(bones.len());
    for bone in bones {
        let parent = bone.parent.map_or(Transform::IDENTITY, |p| poses[p]);
        let local = world
            .get::<Transform>(bone.entity)
            .copied()
            .unwrap_or_default();
        poses.push(parent.mul_transform(local));
    }
    poses
}

/// Pose every bound character from its source skeleton: after animation,
/// before transforms propagate.
pub fn retarget(retargets: Query<&Retarget>, mut transforms: Query<&mut Transform>) {
    for retarget in &retargets {
        retarget.apply(&mut transforms);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    /// A pelvis, a thigh and a calf hanging down from `height`, under
    /// `parent`; the thigh `length` long. Returns the entities top down.
    fn leg(world: &mut World, parent: Entity, height: f32, length: f32, rest: Quat) -> [Entity; 4] {
        let pelvis = world
            .spawn((
                Name::new("pelvis"),
                Transform::from_xyz(0.0, height, 0.0).with_rotation(rest),
                ChildOf(parent),
            ))
            .id();
        // The thigh's own frame is turned by `rest`; its child hangs
        // straight down in model space either way.
        let down = rest.inverse() * Vec3::NEG_Y;
        let thigh = world
            .spawn((Name::new("thigh_l"), Transform::IDENTITY, ChildOf(pelvis)))
            .id();
        let calf = world
            .spawn((
                Name::new("calf_l"),
                Transform::from_translation(down * length),
                ChildOf(thigh),
            ))
            .id();
        let foot = world
            .spawn((
                Name::new("foot_l"),
                Transform::from_translation(down * length),
                ChildOf(calf),
            ))
            .id();
        [pelvis, thigh, calf, foot]
    }

    fn model_position(world: &World, entity: Entity, top: Entity) -> Vec3 {
        let mut pose = Transform::IDENTITY;
        let mut current = entity;
        while current != top {
            pose = world.get::<Transform>(current).unwrap().mul_transform(pose);
            current = world.get::<ChildOf>(current).unwrap().parent();
        }
        pose.translation
    }

    /// A leg twice as long, with another rest frame, swings as the source's
    /// does, and its pelvis travels twice as far.
    #[test]
    fn a_longer_leg_follows_the_source_swing_and_travel() {
        let mut world = World::new();
        let model = world.spawn(Transform::IDENTITY).id();
        let skeleton = world.spawn((Transform::IDENTITY, ChildOf(model))).id();
        let source = leg(&mut world, skeleton, 1.0, 0.5, Quat::IDENTITY);
        let target = leg(&mut world, model, 2.0, 1.0, Quat::from_rotation_z(1.0));
        let binding = Retarget::bind(&world, skeleton, model, None).expect("bound");
        assert_eq!(binding.driven_bones(), 4);
        world.entity_mut(model).insert(binding);

        // The source thigh kicks forward 90°, its pelvis drops 10 cm.
        world.get_mut::<Transform>(source[1]).unwrap().rotation =
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        world.get_mut::<Transform>(source[0]).unwrap().translation.y = 0.9;
        world.run_system_once(retarget).unwrap();

        let pelvis = model_position(&world, target[0], model);
        assert!((pelvis.y - 1.8).abs() < 1e-4, "pelvis {pelvis}");
        let thigh = model_position(&world, target[1], model);
        let knee = model_position(&world, target[2], model);
        let swing = (knee - thigh).normalize();
        assert!(swing.abs_diff_eq(Vec3::Z, 1e-4), "thigh points {swing}");
        assert!(((knee - thigh).length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_rig_names_the_character_bones() {
        let rig = Rig {
            bones: vec![("pelvis".into(), "Hips".into())],
        };
        assert_eq!(rig.own("pelvis"), "Hips");
        assert_eq!(rig.own("head"), "head");
    }
}
