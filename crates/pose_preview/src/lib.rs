//! Blend space previews: a selected blend space node shows its blend on a
//! character, under its axis (see [`BlendPreviewSlot`]); a selected clip
//! node, its clip, under its ports; a selected Skeleton or Bone node, the
//! character at rest, its skeleton drawn over it and the bones picked lit
//! (see [`SkeletonPreview`]). For the editor,
//! which plays nothing of the game's: the character and the clips are the
//! game's [`BlendPreviewAssets`], read from its node catalog. The
//! character's bone names go to [`PreviewBones`], for Bone nodes to pick
//! from.
//!
//! Each slot showing a blend gets a stage of its own, far from the scene and
//! on render layers of its own: a camera drawing into an image the slot
//! shows, a light, the character (bones named like UE5 Manny's), and the
//! clips' skeleton retargeted onto it, as the game does. The samples play
//! in phase, mixed by their weights at the slot's point, as fast as the
//! game plays them there. The image stays as many pixels as the slot covers
//! on screen, at any zoom, to stay sharp.

use bevy::app::{HierarchyPropagatePlugin, Propagate};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::gltf::{Gltf, GltfNode, GltfSkin};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::world_serialization::WorldAssetRoot;
use bevy_noodle_jackdaw::blend::{
    BlendPreviewAssets, PreviewClip, playrate, sample_weights, synced_duration,
};
use bevy_noodle_jackdaw::nodes::skeleton_of;
use bevy_noodle_jackdaw::view::{BlendPreview, BlendPreviewSlot, PreviewBones, SkeletonPreview};
use skeleton_retarget::{Retarget, SkeletonRetargetPlugin};

/// The render layers stages take, one each from here up: above those an
/// editor keeps for itself.
const FIRST_LAYER: usize = 16;
/// How far below the scene stages stand, and apart.
const BELOW: f32 = -10_000.0;
const APART: f32 = 100.0;
/// The camera's view of the character, from the stage's floor.
const EYE: Vec3 = Vec3::new(1.0, 1.25, 2.4);
const LOOK_AT: Vec3 = Vec3::new(0.0, 0.88, 0.0);
const BACKGROUND: Color = Color::srgb_u8(30, 31, 36);
const FLOOR: Color = Color::srgb_u8(58, 60, 68);
/// A skeleton preview's bones, and those lit, over them: colour and width
/// (pixels).
const BONE: (Color, f32) = (Color::srgb_u8(150, 155, 168), 1.5);
const LIT: (Color, f32) = (Color::srgb_u8(255, 166, 38), 3.0);
/// The ring round a lit bone's joint, in metres.
const JOINT: f32 = 0.03;
/// The image's largest side, in pixels.
const MAX_SIDE: u32 = 2048;
/// Sync cycles played wrap after this many: whole cycles of any clip of up
/// to 6, so the wrap does not show.
const WRAP: f32 = 3600.0;

/// Draws [`BlendPreview`]s on the game's character. Add it with the flow
/// graph view, and [`BlendPreviewAssets`] read from the game's catalog.
pub struct PosePreviewPlugin;

impl Plugin for PosePreviewPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SkeletonRetargetPlugin>() {
            app.add_plugins(SkeletonRetargetPlugin);
        }
        if !app.is_plugin_added::<HierarchyPropagatePlugin<RenderLayers>>() {
            app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate));
        }
        app.init_resource::<BlendPreviewAssets>()
            .init_resource::<PreviewBones>()
            .add_systems(
                Update,
                (
                    read_bones,
                    (
                        stage_slots,
                        bind_stages,
                        pose_stages,
                        draw_skeletons,
                        fit_stages,
                    )
                        .chain(),
                ),
            );
    }
}

/// A slot's stage: the root of everything it draws with. An editor may mark
/// it as its own on `Add` (it is never part of a scene).
#[derive(Component, Debug)]
pub struct PoseStage {
    slot: Entity,
    layer: usize,
    camera: Entity,
    /// The character, above its skeleton.
    model: Entity,
    /// Under the character: the clips' skeleton.
    holder: Entity,
    /// The clips' glb; none for a skeleton preview.
    library: Option<String>,
    gltf: Option<Handle<Gltf>>,
    image: Handle<Image>,
    /// A skeleton preview's lines: its bones', and its lit bones'.
    bones: [Handle<GizmoAsset>; 2],
    /// Once the clips' skeleton drives the character's.
    bound: Option<Bound>,
    /// Sync cycles played so far.
    cycles: f32,
}

#[derive(Debug)]
struct Bound {
    /// The clips' skeleton's player; none for a skeleton preview.
    player: Option<Entity>,
    /// The character's skeleton (skinned mesh).
    skeleton: Option<Entity>,
    /// Its graph's node for each clip kind.
    clips: HashMap<String, AnimationNodeIndex>,
}

/// On a slot: its stage.
#[derive(Component)]
struct SlotStage(Entity);

/// The library of the first of `preview`'s samples that has a clip.
fn library_of<'a>(preview: &BlendPreview, assets: &'a BlendPreviewAssets) -> Option<&'a str> {
    preview
        .space
        .samples
        .iter()
        .find_map(|sample| Some(assets.clip(&sample.kind)?.library.as_str()))
}

fn target_image(size: UVec2) -> Image {
    Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    )
}

/// A preview slot: what it is asked for, its stage, its layout.
type Slot<'a> = (
    Entity,
    Option<&'a BlendPreview>,
    Option<&'a SkeletonPreview>,
    Option<&'a SlotStage>,
    &'a mut Node,
);

/// Give each slot asked for a preview (a blend, or a skeleton) a stage,
/// once the game said what to draw it with; drop stages whose slot went, or
/// whose clips come from another library now. Show a slot while it is asked for a preview and
/// its stage is ready.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn stage_slots(
    mut slots: Query<Slot, With<BlendPreviewSlot>>,
    mut stages: Query<(Entity, &PoseStage, &mut Visibility)>,
    mut cameras: Query<&mut Camera>,
    assets: Res<BlendPreviewAssets>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmos: ResMut<Assets<GizmoAsset>>,
    mut commands: Commands,
) {
    for (stage, PoseStage { slot, .. }, _) in &stages {
        if !slots.contains(*slot) {
            commands.entity(stage).despawn();
        }
    }
    let mut layers: Vec<usize> = stages.iter().map(|(_, stage, _)| stage.layer).collect();
    for (slot, preview, skeleton, staged, mut node) in &mut slots {
        let library = preview.and_then(|preview| library_of(preview, &assets));
        let wanted = library.is_some() || skeleton.is_some();
        let mut staged = staged.map(|staged| staged.0);
        if let Some(stage) = staged
            && let Ok((_, PoseStage { library: had, .. }, _)) = stages.get(stage)
            && library.is_some_and(|library| had.as_deref() != Some(library))
        {
            commands.entity(stage).despawn();
            commands.entity(slot).remove::<SlotStage>();
            staged = None;
        }
        let ready = match staged.and_then(|stage| stages.get_mut(stage).ok()) {
            Some((_, stage, mut visibility)) => {
                let ready = wanted && stage.bound.is_some();
                visibility.set_if_neq(if ready {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
                if let Ok(mut camera) = cameras.get_mut(stage.camera)
                    && camera.is_active != ready
                {
                    camera.is_active = ready;
                }
                ready
            }
            None => {
                if wanted && !assets.model.is_empty() {
                    let layer = (FIRST_LAYER..)
                        .find(|l| !layers.contains(l))
                        .unwrap_or(FIRST_LAYER);
                    layers.push(layer);
                    let stage = spawn_stage(
                        &mut commands,
                        &asset_server,
                        (&mut images, &mut meshes, &mut materials, &mut gizmos),
                        slot,
                        layer,
                        &assets.model,
                        library,
                    );
                    commands.entity(slot).insert(SlotStage(stage));
                }
                false
            }
        };
        let display = if ready { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
}

/// Spawn `slot`'s stage on render layer `layer`: the character `model`
/// (a glTF), the clips' skeleton from `library` under it (if any), a camera
/// drawing into an image the slot shows, a light, a floor, and lines for
/// the character's skeleton, drawn over it.
fn spawn_stage(
    commands: &mut Commands,
    asset_server: &AssetServer,
    (images, meshes, materials, gizmos): (
        &mut Assets<Image>,
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &mut Assets<GizmoAsset>,
    ),
    slot: Entity,
    layer: usize,
    model: &str,
    library: Option<&str>,
) -> Entity {
    let image = images.add(target_image(UVec2::splat(64)));
    let root = commands
        .spawn((
            Name::new("Blend space preview"),
            Transform::from_xyz(APART * (layer - FIRST_LAYER) as f32, BELOW, 0.0),
            // Hidden until posed: scene entities take the layer a frame on.
            Visibility::Hidden,
            Propagate(RenderLayers::layer(layer)),
        ))
        .id();
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                clear_color: ClearColorConfig::Custom(BACKGROUND),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            AmbientLight {
                brightness: 600.0,
                ..default()
            },
            Transform::from_translation(EYE).looking_at(LOOK_AT, Vec3::Y),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            illuminance: 6000.0,
            ..default()
        },
        Transform::from_xyz(2.0, 4.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        ChildOf(root),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(0.6))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: FLOOR,
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ChildOf(root),
    ));
    let bones = [BONE, LIT].map(|(_, width)| {
        let handle = gizmos.add(GizmoAsset::default());
        commands.spawn((
            Gizmo {
                handle: handle.clone(),
                line_config: GizmoLineConfig { width, ..default() },
                // Over the character.
                depth_bias: -1.0,
            },
            ChildOf(root),
        ));
        handle
    });
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(model.to_owned()));
    let character = commands
        .spawn((
            Name::new("Character"),
            WorldAssetRoot(scene),
            Transform::IDENTITY,
            ChildOf(root),
        ))
        .id();
    let holder = commands
        .spawn((
            Name::new("Source skeleton"),
            Transform::IDENTITY,
            ChildOf(character),
        ))
        .id();
    commands.entity(slot).insert(ImageNode::new(image.clone()));
    commands.entity(root).insert(PoseStage {
        slot,
        layer,
        camera,
        model: character,
        holder,
        library: library.map(str::to_owned),
        gltf: library.map(|library| asset_server.load(library.to_owned())),
        image,
        bones,
        bound: None,
        cycles: 0.0,
    });
    root
}

/// Once the library is loaded, spawn its scene in the holder; once both
/// skeletons are spawned, still at rest, give the clips' player the
/// library's clips and bind the character's skeleton to it. As the game
/// binds its characters. Without a library, bound once the character is
/// spawned.
fn bind_stages(world: &mut World) {
    type Unbound = (Entity, Entity, Entity, Option<Handle<Gltf>>, Option<String>);
    let unbound: Vec<Unbound> = world
        .query::<(Entity, &PoseStage)>()
        .iter(world)
        .filter(|(_, stage)| stage.bound.is_none())
        .map(|(entity, stage)| {
            let PoseStage {
                model,
                holder,
                gltf,
                library,
                ..
            } = stage;
            (entity, *model, *holder, gltf.clone(), library.clone())
        })
        .collect();
    for (stage, model, holder, gltf, library) in unbound {
        let skeleton = skeleton_of(world, model);
        let Some((gltf, library)) = gltf.zip(library) else {
            if skeleton.is_some()
                && let Some(mut stage) = world.get_mut::<PoseStage>(stage)
            {
                stage.bound = Some(Bound {
                    player: None,
                    skeleton,
                    clips: default(),
                });
            }
            continue;
        };
        let Some(gltf) = world.resource::<Assets<Gltf>>().get(&gltf) else {
            continue;
        };
        if world.get::<WorldAssetRoot>(holder).is_none() {
            let Some(scene) = gltf.default_scene.clone().or(gltf.scenes.first().cloned()) else {
                continue;
            };
            world.entity_mut(holder).insert(WorldAssetRoot(scene));
            continue;
        }
        let Some(player) = find_player(world, holder) else {
            continue;
        };
        let Some(retarget) = Retarget::bind(world, holder, model, None) else {
            continue;
        };
        let mut graph = AnimationGraph::new();
        let mut clips = HashMap::default();
        for clip in &world.resource::<BlendPreviewAssets>().clips {
            if clip.library != library {
                continue;
            }
            match gltf.named_animations.get(clip.animation.as_str()) {
                Some(handle) => {
                    let index = graph.add_clip(handle.clone(), 1.0, graph.root);
                    clips.insert(clip.kind.clone(), index);
                }
                None => warn!("{library}: no animation {}", clip.animation),
            }
        }
        let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
        world.entity_mut(player).insert(AnimationGraphHandle(graph));
        world.entity_mut(model).insert(retarget);
        if let Some(mut stage) = world.get_mut::<PoseStage>(stage) {
            stage.bound = Some(Bound {
                player: Some(player),
                skeleton,
                clips,
            });
        }
    }
}

/// The clips' skeleton's player: on its scene's top node.
fn find_player(world: &World, holder: Entity) -> Option<Entity> {
    let mut stack = vec![holder];
    while let Some(entity) = stack.pop() {
        if world.get::<AnimationPlayer>(entity).is_some() {
            return Some(entity);
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

/// The clips `preview` plays from `library`, and how much: its samples',
/// a clip played by two samples once with both weights.
fn plays<'a>(
    preview: &BlendPreview,
    assets: &'a BlendPreviewAssets,
    library: &str,
) -> Vec<(&'a PreviewClip, f32)> {
    let mut plays: Vec<(&PreviewClip, f32)> = Vec::new();
    let weights = sample_weights(&preview.space, preview.at);
    for (sample, weight) in preview.space.samples.iter().zip(weights) {
        let Some(clip) = assets.clip(&sample.kind).filter(|c| c.library == library) else {
            continue;
        };
        match plays
            .iter_mut()
            .find(|(played, _)| played.kind == clip.kind)
        {
            Some((_, same)) => *same += weight,
            None => plays.push((clip, weight)),
        }
    }
    plays
}

/// How far through `clip` (in seconds) it is `cycles` sync cycles on.
fn clip_time(clip: &PreviewClip, cycles: f32) -> f32 {
    (clip.sync.offset + cycles / clip.sync.cycles).rem_euclid(1.0) * clip.duration
}

/// Play each stage's blend: its clips weighted, in phase, their cycles as
/// long as the blend of theirs, as fast as the game plays them at the
/// slot's point.
fn pose_stages(
    mut stages: Query<&mut PoseStage>,
    previews: Query<&BlendPreview>,
    assets: Res<BlendPreviewAssets>,
    time: Res<Time>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for mut stage in &mut stages {
        let Ok(preview) = previews.get(stage.slot) else {
            continue;
        };
        let Some(library) = stage.library.as_deref() else {
            continue;
        };
        let plays = plays(preview, &assets, library);
        let cycle = synced_duration(
            &plays
                .iter()
                .map(|(clip, _)| clip.duration / clip.sync.cycles)
                .collect::<Vec<_>>(),
            &plays.iter().map(|(_, weight)| *weight).collect::<Vec<_>>(),
        );
        let speed = playrate(&preview.space, preview.at);
        stage.cycles = (stage.cycles + time.delta_secs() * speed / cycle.max(1e-3)) % WRAP;
        let Some(bound) = &stage.bound else {
            continue;
        };
        let Some(Ok(mut player)) = bound.player.map(|player| players.get_mut(player)) else {
            continue;
        };
        for (kind, index) in &bound.clips {
            match plays.iter().find(|(clip, _)| &clip.kind == kind) {
                Some((clip, weight)) if *weight > 0.0 => {
                    player
                        .play(*index)
                        .set_weight(*weight)
                        .seek_to(clip_time(clip, stage.cycles))
                        .pause();
                }
                _ if player.is_playing_animation(*index) => {
                    player.stop(*index);
                }
                _ => {}
            }
        }
    }
}

/// Read the character's bone names into [`PreviewBones`] once it is
/// loaded: its skins' joints'.
fn read_bones(
    assets: Res<BlendPreviewAssets>,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    skins: Res<Assets<GltfSkin>>,
    nodes: Res<Assets<GltfNode>>,
    mut bones: ResMut<PreviewBones>,
    // The model read, and its glTF until read.
    mut reading: Local<(String, Option<Handle<Gltf>>)>,
) {
    if reading.0 != assets.model {
        let gltf = (!assets.model.is_empty()).then(|| asset_server.load(assets.model.clone()));
        *reading = (assets.model.clone(), gltf);
        if !bones.0.is_empty() {
            bones.0.clear();
        }
    }
    let Some(gltf) = reading.1.as_ref().and_then(|gltf| gltfs.get(gltf)) else {
        return;
    };
    let mut names: Vec<String> = gltf
        .skins
        .iter()
        .filter_map(|skin| skins.get(skin))
        .flat_map(|skin| &skin.joints)
        .filter_map(|joint| nodes.get(joint))
        .map(|joint| joint.name.clone())
        .collect();
    names.sort();
    names.dedup();
    if bones.0 != names {
        bones.0 = names;
    }
    reading.1 = None;
}

/// A joint of a skeleton drawn: where it is, its parent's index among the
/// joints drawn (if its parent is one), whether it is lit.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Joint {
    at: Vec3,
    parent: Option<usize>,
    lit: bool,
}

/// The lines drawing `joints`' bones: from each joint's parent to it, lit
/// when the parent (the bone's joint) is.
fn bone_lines(joints: &[Joint]) -> Vec<(Vec3, Vec3, bool)> {
    joints
        .iter()
        .filter_map(|joint| {
            let parent = joints[joint.parent?];
            Some((parent.at, joint.at, parent.lit))
        })
        .collect()
}

/// Draw each skeleton preview's character's skeleton over it: its bones,
/// those named lit lit and their joints ringed. Not its IK targets (UE5
/// Manny's `ik_*` bones, away from the body), nor from its top joint (on
/// the floor).
fn draw_skeletons(
    stages: Query<(&PoseStage, &GlobalTransform)>,
    previews: Query<&SkeletonPreview>,
    skins: Query<&SkinnedMesh>,
    transforms: Query<(&GlobalTransform, Option<&ChildOf>, Option<&Name>)>,
    mut gizmos: ResMut<Assets<GizmoAsset>>,
) {
    for (stage, root) in &stages {
        let Ok(preview) = previews.get(stage.slot) else {
            continue;
        };
        let Some(Ok(skin)) = stage
            .bound
            .as_ref()
            .and_then(|bound| bound.skeleton)
            .map(|skeleton| skins.get(skeleton))
        else {
            continue;
        };
        let to_stage = root.affine().inverse();
        let found: Vec<_> = skin
            .joints
            .iter()
            .map(|&joint| transforms.get(joint).ok())
            .collect();
        let parents: Vec<Option<usize>> = found
            .iter()
            .map(|found| {
                let parent = (*found)?.1?.parent();
                skin.joints.iter().position(|j| *j == parent)
            })
            .collect();
        let joints: Vec<Joint> = found
            .iter()
            .zip(&parents)
            .map(|(found, parent)| {
                let Some((transform, _, name)) = *found else {
                    return Joint {
                        at: Vec3::ZERO,
                        parent: None,
                        lit: false,
                    };
                };
                let name = name.map_or("", Name::as_str);
                Joint {
                    at: to_stage.transform_point3(transform.translation()),
                    parent: parent
                        .filter(|&parent| !name.starts_with("ik_") && parents[parent].is_some()),
                    lit: preview.lit.iter().any(|lit| lit == name),
                }
            })
            .collect();
        let lines = bone_lines(&joints);
        for (lit, (color, _)) in [(false, BONE), (true, LIT)] {
            let Some(mut gizmo) = gizmos.get_mut(&stage.bones[lit as usize]) else {
                continue;
            };
            gizmo.clear();
            for &(from, to, _) in lines.iter().filter(|line| line.2 == lit) {
                gizmo.line(from, to, color);
            }
            for joint in joints.iter().filter(|joint| lit && joint.lit) {
                gizmo.sphere(Isometry3d::from_translation(joint.at), JOINT, color);
            }
        }
    }
}

/// Keep each stage's image as many pixels as its slot covers on screen:
/// its laid-out size, scaled by the canvas zoom.
fn fit_stages(
    stages: Query<&PoseStage>,
    slots: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut images: ResMut<Assets<Image>>,
) {
    for stage in &stages {
        let Ok((computed, transform)) = slots.get(stage.slot) else {
            continue;
        };
        let (scale, _, _) = transform.to_scale_angle_translation();
        let size = (computed.size() * scale.abs())
            .round()
            .as_uvec2()
            .min(UVec2::splat(MAX_SIDE));
        if size.min_element() == 0 {
            continue;
        }
        let Some(image) = images.get(&stage.image) else {
            continue;
        };
        if image.size() != size
            && let Some(mut image) = images.get_mut(&stage.image)
        {
            image.resize(Extent3d {
                width: size.x,
                height: size.y,
                ..default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_noodle_jackdaw::blend::{BLEND_SPACE_1D, SyncCycles};
    use bevy_noodle_jackdaw::model::{BlendSample, FlowNode};

    use super::*;

    fn clip(kind: &str, duration: f32, cycles: f32, offset: f32) -> PreviewClip {
        PreviewClip {
            kind: kind.into(),
            library: "clips.glb".into(),
            animation: kind.into(),
            duration,
            sync: SyncCycles::new(cycles, offset),
        }
    }

    fn walk_jog() -> (BlendPreviewAssets, FlowNode) {
        let assets = BlendPreviewAssets {
            model: "character.gltf".into(),
            clips: vec![clip("walk", 1.2, 2.0, 0.5), clip("jog", 0.8, 2.0, 0.25)],
        };
        let sample = |kind: &str, x: f32| BlendSample {
            kind: kind.into(),
            title: kind.into(),
            at: Vec2::new(x, 0.0),
        };
        let space = FlowNode {
            kind: BLEND_SPACE_1D.into(),
            samples: vec![sample("walk", 1.4), sample("jog", 3.5), sample("jog", 3.5)],
            ..default()
        };
        (assets, space)
    }

    #[test]
    fn a_preview_plays_its_samples_clips_once_each() {
        let (assets, space) = walk_jog();
        let at = |x: f32| BlendPreview {
            space: space.clone(),
            at: Vec2::new(x, 0.0),
        };
        let weights = |x: f32| -> Vec<(String, f32)> {
            plays(&at(x), &assets, "clips.glb")
                .into_iter()
                .map(|(clip, weight)| (clip.kind.clone(), weight))
                .collect()
        };
        assert_eq!(weights(1.4), [("walk".into(), 1.0), ("jog".into(), 0.0)]);
        assert_eq!(weights(3.5), [("walk".into(), 0.0), ("jog".into(), 1.0)]);
        // From another library: nothing.
        assert!(plays(&at(2.0), &assets, "other.glb").is_empty());
        assert_eq!(library_of(&at(2.0), &assets), Some("clips.glb"));
    }

    #[test]
    fn a_lit_bone_lights_the_lines_from_its_joint() {
        let joint = |y: f32, parent: Option<usize>, lit: bool| Joint {
            at: Vec3::new(0.0, y, 0.0),
            parent,
            lit,
        };
        // Pelvis, spine (lit), chest; a stray joint under no other drawn.
        let joints = [
            joint(1.0, None, false),
            joint(1.2, Some(0), true),
            joint(1.4, Some(1), false),
            joint(9.0, None, false),
        ];
        assert_eq!(
            bone_lines(&joints),
            [
                (Vec3::Y, Vec3::Y * 1.2, false),
                (Vec3::Y * 1.2, Vec3::Y * 1.4, true),
            ]
        );
    }

    #[test]
    fn clips_meet_at_their_cycles_starts() {
        let walk = clip("walk", 1.2, 2.0, 0.5);
        assert!((clip_time(&walk, 0.0) - 0.6).abs() < 1e-5);
        // One cycle on: the other foot, half the clip further.
        assert!(clip_time(&walk, 1.0).abs() < 1e-5);
        assert!((clip_time(&walk, 2.0) - 0.6).abs() < 1e-5);
        // Whole cycles of every clip at the wrap.
        for cycles in 1..=6 {
            assert_eq!(WRAP % cycles as f32, 0.0);
        }
    }
}
