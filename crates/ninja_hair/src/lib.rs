//! Stable minification for the Ninja's hair cards, ported from bevy-parkour
//! (`src/animation/ninja_materials.rs`).
//!
//! The hair PNGs load without a mip chain, so the fine strands shimmer at a
//! distance. Once the glTF is loaded, build the chain for the shared hair
//! color and normal maps (color filtered in linear light and weighted by
//! alpha, normals renormalized) and switch the hair to alpha-to-coverage,
//! which uses the camera's MSAA. Files under `assets/` are not modified.
//!
//! Jackdaw loads scene models with GPU-only textures, whose pixels are
//! dropped once uploaded; those hair textures are read again from their file
//! in the background.
//!
//! Bevy only: `ninja_character` re-exports it, and the blend space previews
//! of the editor and the `flow_graph` example add it for their Ninja.

use bevy::{
    asset::{AssetPath, RenderAssetUsages},
    gltf::Gltf,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::render_resource::TextureFormat,
    tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future},
    world_serialization::WorldAssetRoot,
};

pub struct NinjaHairPlugin;

impl Plugin for NinjaHairPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Rereads>().add_systems(
            Update,
            (track_sources, prepare_hair, finish_rereads).chain(),
        );
    }
}

/// glTF whose `MAT_HAIR` material gets prepared; also keeps the glTF loaded
/// (a scene spawned from `file.gltf#Scene0` alone lets it go).
#[derive(Component)]
struct HairSource(Handle<Gltf>);

/// Every glTF scene placed in the world, whether by the Jackdaw scene or by
/// code, is a hair candidate.
fn track_sources(
    mut commands: Commands,
    roots: Query<(Entity, &WorldAssetRoot), Without<HairSource>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, root) in &roots {
        let Some(path) = asset_server.get_path(root.0.id()) else {
            continue;
        };
        if path.label().is_none() || !matches!(path.get_full_extension(), Some("gltf" | "glb")) {
            continue;
        }
        let gltf = asset_server.load::<Gltf>(path.without_label().into_owned());
        // A scene reload can despawn the entity before the command applies.
        commands.entity(entity).try_insert(HairSource(gltf));
    }
}

/// Hair textures read again from their file, with their mip chain being
/// built.
#[derive(Resource, Default)]
struct Rereads(Vec<Reread>);

struct Reread {
    image: AssetId<Image>,
    /// Touched once the image is replaced, so it binds the new texture.
    material: Handle<StandardMaterial>,
    task: Task<Option<Image>>,
}

fn prepare_hair(
    mut rereads: ResMut<Rereads>,
    mut events: MessageReader<AssetEvent<Gltf>>,
    sources: Query<&HairSource>,
    gltfs: Res<Assets<Gltf>>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };
        if !sources.iter().any(|source| source.0.id() == *id) {
            continue;
        }
        let Some(hair) = gltfs
            .get(*id)
            .and_then(|gltf| hair_material(gltf, &asset_server))
        else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&hair) else {
            continue;
        };
        material.alpha_mode = AlphaMode::AlphaToCoverage;
        let textures = [
            (material.base_color_texture.clone(), false),
            (material.normal_map_texture.clone(), true),
        ];
        for (texture, normal) in textures {
            let Some(handle) = texture else {
                continue;
            };
            let Some(mut image) = images.get_mut(&handle) else {
                continue;
            };
            let path = asset_server.get_path(handle.id());
            if image.data.is_some() {
                if add_mips(&mut image, normal) {
                    log_mips(&image, path);
                }
            } else if image.texture_descriptor.mip_level_count == 1
                && let Some(path) = path
            {
                let server = asset_server.clone();
                let path = path.into_owned();
                let is_srgb = image.texture_descriptor.format == TextureFormat::Rgba8UnormSrgb;
                let (sampler, usage) = (image.sampler.clone(), image.asset_usage);
                let task = AsyncComputeTaskPool::get().spawn(async move {
                    reread(&server, &path, is_srgb, sampler, usage, normal)
                        .await
                        .inspect_err(|err| warn!("Hair mips: cannot reread {path}: {err}"))
                        .ok()
                });
                rereads.0.push(Reread {
                    image: handle.id(),
                    material: hair.clone(),
                    task,
                });
            }
        }
    }
}

/// Decodes the texture at `path` again and builds its mip chain.
async fn reread(
    server: &AssetServer,
    path: &AssetPath<'static>,
    is_srgb: bool,
    sampler: ImageSampler,
    usage: RenderAssetUsages,
    normal: bool,
) -> Result<Image, String> {
    let source = server
        .get_source(path.source())
        .map_err(|e| e.to_string())?;
    let mut reader = source
        .reader()
        .read(path.path())
        .await
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| e.to_string())?;
    let extension = path.get_full_extension().unwrap_or_default();
    let mut image = Image::from_buffer(
        &bytes,
        ImageType::Extension(extension),
        CompressedImageFormats::NONE,
        is_srgb,
        sampler,
        usage,
    )
    .map_err(|e| e.to_string())?;
    if !add_mips(&mut image, normal) {
        return Err("not a single-level RGBA8 image".into());
    }
    Ok(image)
}

fn finish_rereads(
    mut rereads: ResMut<Rereads>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    rereads.0.retain_mut(|reread| {
        let Some(result) = block_on(future::poll_once(&mut reread.task)) else {
            return true;
        };
        if let Some(image) = result {
            log_mips(&image, asset_server.get_path(reread.image));
            // Fails only if the texture was dropped meanwhile.
            images.insert(reread.image, image).ok();
            // Any write marks the material modified, so it rebinds.
            if let Some(mut material) = materials.get_mut(&reread.material) {
                material.alpha_mode = AlphaMode::AlphaToCoverage;
            }
        }
        false
    });
}

fn log_mips(image: &Image, path: Option<AssetPath>) {
    info!(
        "Hair mips: {} levels for {:?}",
        image.texture_descriptor.mip_level_count, path
    );
}

/// The rendered `StandardMaterial` of the glTF's `MAT_HAIR`.
fn hair_material(gltf: &Gltf, asset_server: &AssetServer) -> Option<Handle<StandardMaterial>> {
    let hair = gltf.named_materials.get("MAT_HAIR")?;
    // Bevy's PBR glTF extension stores the rendered material under /std.
    let path = hair.path()?;
    let rendered = path.clone().with_label(format!("{}/std", path.label()?));
    asset_server.get_handle(rendered)
}

/// Appends a full mip chain to a single-level RGBA8 image. Returns whether
/// the image changed.
fn add_mips(image: &mut Image, normal: bool) -> bool {
    let size = image.texture_descriptor.size;
    if image.texture_descriptor.mip_level_count != 1
        || size.depth_or_array_layers != 1
        || !matches!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
        )
    {
        return false;
    }
    let Some(data) = image.data.as_mut() else {
        return false;
    };
    if data.len() != size.width as usize * size.height as usize * 4 {
        return false;
    }
    let (mut width, mut height, mut offset) = (size.width as usize, size.height as usize, 0);
    let mut levels = 1;
    while width > 1 || height > 1 {
        let next = downsample(&data[offset..], width, height, normal);
        offset = data.len();
        data.extend(next);
        width = (width / 2).max(1);
        height = (height / 2).max(1);
        levels += 1;
    }
    image.texture_descriptor.mip_level_count = levels;
    true
}

fn downsample(source: &[u8], width: usize, height: usize, normal: bool) -> Vec<u8> {
    let linear: [f32; 256] = std::array::from_fn(|i| {
        let v = i as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut output = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let mut rgb = Vec3::ZERO;
            let mut alpha = 0.0;
            let mut count = 0.0;
            for dy in 0..2.min(height) {
                for dx in 0..2.min(width) {
                    let i =
                        ((y * 2 + dy).min(height - 1) * width + (x * 2 + dx).min(width - 1)) * 4;
                    let a = source[i + 3] as f32 / 255.0;
                    rgb += if normal {
                        Vec3::new(source[i] as f32, source[i + 1] as f32, source[i + 2] as f32)
                            / 127.5
                            - Vec3::ONE
                    } else {
                        Vec3::new(
                            linear[source[i] as usize],
                            linear[source[i + 1] as usize],
                            linear[source[i + 2] as usize],
                        ) * a
                    };
                    alpha += a;
                    count += 1.0;
                }
            }
            let rgb = if normal {
                (rgb.try_normalize().unwrap_or(Vec3::Z) + Vec3::ONE) * 0.5
            } else {
                (rgb / alpha.max(1e-8)).map(|v| {
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                })
            };
            output.extend(
                rgb.to_array()
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
            );
            output.push((alpha / count * 255.0).round() as u8);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension},
    };

    #[test]
    fn hair_mips_preserve_color_at_transparent_edges_and_average_normal_directions() {
        let color = downsample(
            &[255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            2,
            2,
            false,
        );
        assert_eq!(color, [255, 255, 255, 64]);
        let normal = downsample(
            &[
                255, 128, 128, 255, 128, 128, 255, 255, 255, 128, 128, 255, 128, 128, 255, 255,
            ],
            2,
            2,
            true,
        );
        let direction =
            Vec3::new(normal[0] as f32, normal[1] as f32, normal[2] as f32) / 127.5 - Vec3::ONE;
        assert!(
            (direction.length() - 1.0).abs() < 0.01 && (direction.x - direction.z).abs() < 0.01
        );
        assert_eq!(
            downsample(&[10, 20, 30, 255, 10, 20, 30, 255], 1, 2, false),
            [10, 20, 30, 255]
        );
    }

    #[test]
    fn add_mips_builds_full_chain_once() {
        let mut image = Image::new_fill(
            Extent3d {
                width: 8,
                height: 2,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[200, 100, 50, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        assert!(add_mips(&mut image, false));
        // 8x2, 4x1, 2x1, 1x1.
        assert_eq!(image.texture_descriptor.mip_level_count, 4);
        assert_eq!(image.data.as_ref().unwrap().len(), (16 + 4 + 2 + 1) * 4);
        assert!(!add_mips(&mut image, false));
    }
}
