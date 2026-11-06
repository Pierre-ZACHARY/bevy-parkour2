use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    schema: u32,
    presets: BTreeMap<String, BTreeMap<String, String>>,
    recipes: BTreeMap<String, Vec<Rule>>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    path: String,
    semantic: Semantic,
    channels: Channels,
    #[serde(default)]
    max_size: Option<u32>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Semantic {
    Color,
    Normal,
    Data,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Channels {
    Preserve,
    Rgb,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn export(directory: &Path, target: &str, quality: &str, presets: Option<&Path>) -> Result<()> {
    let bytes = match presets {
        Some(path) => fs::read(path)?,
        None => include_bytes!("../presets.json").to_vec(),
    };
    export_with(directory, target, quality, &bytes)
}

fn export_with(directory: &Path, target: &str, quality: &str, bytes: &[u8]) -> Result<()> {
    let directory = directory
        .canonicalize()
        .context("missing staged directory")?;
    ensure!(directory.is_dir(), "staged assets must be a directory");
    let sources = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .canonicalize()?;
    ensure!(
        !directory.starts_with(&sources) && !sources.starts_with(&directory),
        "export requires a staged copy, not source assets"
    );
    let catalog: Catalog = serde_json::from_slice(bytes).context("invalid texture presets")?;
    ensure!(catalog.schema == 1, "unsupported texture preset schema");
    let recipe = catalog
        .presets
        .get(quality)
        .with_context(|| format!("unknown asset quality {quality}"))?
        .get(target)
        .with_context(|| format!("no {quality} preset for {target}"))?;
    let rules = catalog.recipes.get(recipe).context("unknown recipe")?;
    let fingerprint = hash(bytes);
    let mut files = BTreeMap::new();
    for entry in walkdir::WalkDir::new(&directory).follow_links(false) {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "symlink in staged assets: {}",
            entry.path().display()
        );
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            let relative = entry
                .path()
                .strip_prefix(&directory)?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, entry.into_path());
        }
    }
    let mut seen = BTreeSet::new();
    for rule in rules {
        ensure!(
            !rule.path.contains('\\')
                && Path::new(&rule.path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "invalid preset path {}",
            rule.path
        );
        ensure!(
            seen.insert(&rule.path),
            "duplicate texture rule {}",
            rule.path
        );
        ensure!(
            files.contains_key(&rule.path),
            "preset texture missing: {}",
            rule.path
        );
        ensure!(rule.max_size != Some(0), "max_size must be positive");
    }
    let report_path = directory.join("asset-export.json");
    if report_path.exists() {
        let old: serde_json::Value = serde_json::from_slice(&fs::read(&report_path)?)?;
        ensure!(
            old["presets_sha256"] == fingerprint
                && old["quality"] == quality
                && old["target"] == target,
            "already exported with another preset; rebuild from fresh sources"
        );
        let previous = old["textures"]
            .as_array()
            .context("invalid previous export report")?;
        ensure!(
            previous.len() == files.len(),
            "staged texture inventory changed; rebuild from sources"
        );
        for record in previous {
            let path = record["path"].as_str().context("invalid report path")?;
            let file = files.get(path).context("staged texture missing")?;
            ensure!(
                record["distributed_sha256"] == hash(&fs::read(file)?),
                "staged texture changed: {path}; rebuild from sources"
            );
        }
        return Ok(());
    }
    let mut reports = Vec::new();
    let mut outputs = Vec::new();
    // Validate and prepare all transformations before writing any texture.
    for (relative, path) in &files {
        let source = fs::read(path)?;
        let decoded = image::load_from_memory_with_format(&source, ImageFormat::Png)?;
        let source_dimensions = [decoded.width(), decoded.height()];
        let source_color = format!("{:?}", decoded.color());
        let rule = rules.iter().find(|r| r.path == *relative);
        let (converted, changed) = transform(decoded, rule, target == "webgpu")
            .with_context(|| format!("texture {relative}"))?;
        let output = if changed {
            let mut buffer = Vec::new();
            image::codecs::png::PngEncoder::new_with_quality(
                &mut buffer,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            )
            .write_image(
                converted.as_bytes(),
                converted.width(),
                converted.height(),
                converted.color().into(),
            )?;
            buffer
        } else {
            source.clone()
        };
        reports.push(serde_json::json!({
            "path": relative, "rule": rule, "changed": changed,
            "source_sha256": hash(&source), "distributed_sha256": hash(&output),
            "source_bytes": source.len(), "distributed_bytes": output.len(),
            "source_dimensions": source_dimensions,
            "distributed_dimensions": [converted.width(), converted.height()],
            "source_color": source_color, "distributed_color": format!("{:?}", converted.color()),
        }));
        if changed {
            outputs.push((path, output));
        }
    }
    for (path, output) in outputs {
        fs::write(path, output)?;
    }
    fs::write(
        report_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1, "target": target, "quality": quality, "recipe": recipe,
            "presets_sha256": fingerprint, "textures": reports,
        }))?,
    )?;
    println!(
        "Texture preset {quality}/{target}: {recipe}, {} textures inventoried",
        files.len()
    );
    Ok(())
}

fn transform(
    mut image: DynamicImage,
    rule: Option<&Rule>,
    web: bool,
) -> Result<(DynamicImage, bool)> {
    let mut changed = false;
    let sixteen = matches!(
        image.color(),
        image::ColorType::L16
            | image::ColorType::La16
            | image::ColorType::Rgb16
            | image::ColorType::Rgba16
    );
    let alpha = image.color().has_alpha();
    if let Some(rule) = rule {
        if matches!(rule.channels, Channels::Rgb) && alpha {
            ensure!(
                image.to_rgba16().pixels().all(|p| p.0[3] == u16::MAX),
                "RGB conversion would discard non-opaque alpha"
            );
        }
        if let Some(max) = rule
            .max_size
            .filter(|max| image.width().max(image.height()) > *max)
        {
            image = resize(&image, max, rule.semantic, sixteen && !web);
            changed = true;
        }
        if matches!(rule.channels, Channels::Rgb) && image.color().has_alpha() {
            image = if sixteen && !web {
                DynamicImage::ImageRgb16(image.into_rgb16())
            } else {
                DynamicImage::ImageRgb8(image.into_rgb8())
            };
            changed = true;
        }
    }
    if web && sixteen {
        image = match image.color() {
            image::ColorType::L16 => DynamicImage::ImageLuma8(image.into_luma8()),
            image::ColorType::La16 => DynamicImage::ImageLumaA8(image.into_luma_alpha8()),
            image::ColorType::Rgb16 => DynamicImage::ImageRgb8(image.into_rgb8()),
            image::ColorType::Rgba16 => DynamicImage::ImageRgba8(image.into_rgba8()),
            _ => image,
        };
        changed = true;
    }
    Ok((image, changed))
}

fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

/// Color is filtered in linear light with premultiplied alpha; packed data is
/// linear, and tangent-space normal vectors are renormalized after filtering.
fn resize(image: &DynamicImage, max: u32, semantic: Semantic, sixteen: bool) -> DynamicImage {
    let alpha = image.color().has_alpha();
    let mut pixels = image.to_rgba32f();
    for pixel in pixels.pixels_mut() {
        for i in 0..3 {
            pixel.0[i] = match semantic {
                Semantic::Color => linear(pixel.0[i]) * pixel.0[3],
                Semantic::Normal => pixel.0[i] * 2. - 1.,
                Semantic::Data => pixel.0[i],
            };
        }
    }
    let scale = max as f64 / image.width().max(image.height()) as f64;
    let width = ((image.width() as f64 * scale).round() as u32).max(1);
    let height = ((image.height() as f64 * scale).round() as u32).max(1);
    let mut pixels = image::imageops::resize(
        &pixels,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    for pixel in pixels.pixels_mut() {
        pixel.0[3] = pixel.0[3].clamp(0., 1.);
        match semantic {
            Semantic::Color => {
                for i in 0..3 {
                    pixel.0[i] = srgb(if pixel.0[3] > 1e-6 {
                        (pixel.0[i] / pixel.0[3]).clamp(0., 1.)
                    } else {
                        0.
                    });
                }
            }
            Semantic::Normal => {
                let length = (pixel.0[0].powi(2) + pixel.0[1].powi(2) + pixel.0[2].powi(2)).sqrt();
                for i in 0..3 {
                    pixel.0[i] = if length > 1e-6 {
                        pixel.0[i] / length * 0.5 + 0.5
                    } else if i == 2 {
                        1.
                    } else {
                        0.5
                    };
                }
            }
            Semantic::Data => {}
        }
    }
    let output = DynamicImage::ImageRgba32F(pixels);
    match (sixteen, alpha) {
        (true, true) => DynamicImage::ImageRgba16(output.into_rgba16()),
        (true, false) => DynamicImage::ImageRgb16(output.into_rgb16()),
        (false, true) => DynamicImage::ImageRgba8(output.into_rgba8()),
        (false, false) => DynamicImage::ImageRgb8(output.into_rgb8()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog(rules: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"schema":1,
            "presets":{"test":{"webgpu":"test", "windows-x64":"test"}},
            "recipes":{"test":rules}}))
        .unwrap()
    }
    fn rule(path: &str, semantic: &str, channels: &str, max: u32) -> serde_json::Value {
        serde_json::json!({"path":path,"semantic":semantic,"channels":channels,"max_size":max})
    }
    #[test]
    fn opaque_rgb_is_pixel_exact_sources_untouched_and_repeat_is_verified() -> Result<()> {
        let source = tempfile::tempdir()?;
        let stage = tempfile::tempdir()?;
        let image = image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([x as u8 * 90, y as u8 * 70, 35, 255])
        });
        image.save(source.path().join("a.png"))?;
        let original = fs::read(source.path().join("a.png"))?;
        fs::copy(source.path().join("a.png"), stage.path().join("a.png"))?;
        let config = catalog(serde_json::json!([rule("a.png", "color", "rgb", 2048)]));
        export_with(stage.path(), "webgpu", "test", &config)?;
        let distributed = image::open(stage.path().join("a.png"))?;
        assert_eq!(distributed.color(), image::ColorType::Rgb8);
        assert_eq!(distributed.to_rgba8(), image);
        assert_eq!(fs::read(source.path().join("a.png"))?, original);
        let report = fs::read(stage.path().join("asset-export.json"))?;
        export_with(stage.path(), "webgpu", "test", &config)?;
        assert_eq!(fs::read(stage.path().join("asset-export.json"))?, report);
        fs::write(stage.path().join("a.png"), "tampered")?;
        assert!(export_with(stage.path(), "webgpu", "test", &config).is_err());
        Ok(())
    }
    #[test]
    fn invalid_rules_and_alpha_fail_without_modifying_textures() -> Result<()> {
        let temp = tempfile::tempdir()?;
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 128]))
            .save(temp.path().join("a.png"))?;
        let original = fs::read(temp.path().join("a.png"))?;
        for rules in [
            serde_json::json!([rule("a.png", "color", "rgb", 1)]),
            serde_json::json!([rule("absent.png", "data", "preserve", 1)]),
            serde_json::json!([rule("../a.png", "data", "preserve", 1)]),
            serde_json::json!([
                rule("a.png", "data", "preserve", 1),
                rule("a.png", "data", "preserve", 1)
            ]),
            serde_json::json!([rule("a.png", "data", "preserve", 0)]),
        ] {
            assert!(export_with(temp.path(), "webgpu", "test", &catalog(rules)).is_err());
            assert_eq!(fs::read(temp.path().join("a.png"))?, original);
            assert!(!temp.path().join("asset-export.json").exists());
        }
        assert!(export(temp.path(), "webgpu", "typo", None).is_err());
        assert!(export(temp.path(), "android-arm64", "optimized", None).is_err());
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        assert!(export(&source, "webgpu", "original", None).is_err());
        assert!(export(&source.join("character"), "webgpu", "original", None).is_err());
        assert!(export(&temp.path().join("a.png"), "webgpu", "original", None).is_err());
        Ok(())
    }
    #[test]
    fn resizing_respects_color_alpha_data_and_normal_semantics() -> Result<()> {
        let image = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 255])
            }
        }));
        let color = resize(&image, 1, Semantic::Color, false).to_rgba8();
        let data = resize(&image, 1, Semantic::Data, false).to_rgba8();
        assert!((color.get_pixel(0, 0).0[0] as i32 - 188).abs() <= 2);
        assert!((data.get_pixel(0, 0).0[0] as i32 - 128).abs() <= 1);
        let hair = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 255, 0, 0])
            }
        }));
        let p = resize(&hair, 1, Semantic::Color, false)
            .to_rgba8()
            .get_pixel(0, 0)
            .0;
        assert!(p[0] >= 254 && p[1] <= 1 && (p[3] as i32 - 128).abs() <= 1);
        let normals = DynamicImage::ImageRgb8(image::RgbImage::from_fn(4, 2, |x, _| {
            if x < 2 {
                image::Rgb([255, 128, 128])
            } else {
                image::Rgb([128, 255, 128])
            }
        }));
        let output = resize(&normals, 1, Semantic::Normal, false).to_rgb8();
        let v = output.get_pixel(0, 0).0.map(|c| c as f32 / 255. * 2. - 1.);
        assert!(((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.).abs() < 0.015);
        let r = Rule {
            path: "a.png".into(),
            semantic: Semantic::Data,
            channels: Channels::Preserve,
            max_size: Some(2),
        };
        let (output, changed) = transform(DynamicImage::new_rgb8(8, 4), Some(&r), true)?;
        assert!(changed);
        assert_eq!((output.width(), output.height()), (2, 1));
        Ok(())
    }
    #[test]
    fn versioned_target_presets_match_the_actual_asset_inventory() -> Result<()> {
        let parsed: Catalog = serde_json::from_slice(include_bytes!("../presets.json"))?;
        assert_eq!(parsed.presets["optimized"]["webgpu"], "web-2k");
        for target in ["windows-x64", "linux-x64", "linux-arm64", "macos-arm64"] {
            assert_eq!(parsed.presets["optimized"][target], "desktop-lossless");
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for rules in parsed.recipes.values() {
            for r in rules {
                assert!(root.join(&r.path).is_file(), "{}", r.path);
            }
        }
        assert_eq!(
            parsed.recipes["web-2k"]
                .iter()
                .filter(|r| r.max_size == Some(2048))
                .count(),
            11
        );
        Ok(())
    }
}
