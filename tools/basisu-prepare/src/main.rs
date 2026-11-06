use anyhow::{Context, Result, ensure};
use basisu_c_sys::{BasisTextureFormat, common, extra::*};
use image::{Rgba, RgbaImage};
use serde_json::json;
use std::{fs, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let out = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("usage: basisu-prepare OUTPUT")?,
    );
    fs::create_dir_all(&out)?;
    basisu_encoder_init();
    basisu_transcoder_init();
    let mut reports = vec![];
    for (name, srgb) in [("color", true), ("linear", false)] {
        let pixels = fixture(srgb);
        pixels.save(out.join(format!("{name}.png")))?;
        let mut encoder = BasisuEncoder::new();
        encoder.set_image(SourceImage {
            data: pixels.as_raw(),
            format: SourceImageFormat::Rgba8,
            size: types::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 1,
            },
        })?;
        let mut params = if srgb {
            BasisuEncoderParams::new_with_srgb_defaults(BasisTextureFormat::UastcLdr4x4)
        } else {
            BasisuEncoderParams::new_with_linear_defaults(BasisTextureFormat::UastcLdr4x4)
        };
        params.flags_and_quality |= common::BU_COMP_FLAGS_GEN_MIPS_CLAMP;
        let start = Instant::now();
        let bytes = encoder.compress(params)?;
        let encode_ms = start.elapsed().as_secs_f64() * 1000.;
        fs::write(out.join(format!("{name}.basisu.ktx2")), &bytes)?;
        let t = Instant::now();
        let transcoder = BasisuTranscoder::new(
            &bytes,
            SupportedTextureCompression::empty(),
            ChannelType::Auto,
        )?;
        let decoded = transcoder.transcode(None, None)?;
        let info = transcoder.get_info();
        let transcode_ms = t.elapsed().as_secs_f64() * 1000.;
        ensure!(
            info.width == 256 && info.height == 256 && info.levels == 9,
            "bad dimensions/mips: {info:?}"
        );
        ensure!(info.is_srgb == srgb, "incorrect transfer function");
        let errors: Vec<f64> = pixels
            .as_raw()
            .iter()
            .zip(&decoded.data)
            .map(|(a, b)| a.abs_diff(*b) as f64)
            .collect();
        let mae = errors.iter().sum::<f64>() / errors.len() as f64;
        ensure!(
            errors.len() == pixels.as_raw().len() && mae < 8.,
            "roundtrip error {mae}"
        );
        reports.push(json!({"name":name,"srgb":srgb,"size":[256,256],"mips":info.levels,
            "encoding":"UASTC LDR 4x4 + Zstd", "png_bytes":fs::metadata(out.join(format!("{name}.png")))?.len(),
            "basis_bytes":bytes.len(),"encode_ms":encode_ms,"cpu_transcode_rgba_ms":transcode_ms,
            "mean_absolute_channel_error":mae,"max_channel_error":errors.iter().copied().fold(0.,f64::max)}));
    }
    ensure!(
        BasisuTranscoder::new(
            b"invalid KTX2",
            SupportedTextureCompression::empty(),
            ChannelType::Auto
        )
        .is_err(),
        "corrupt fixture unexpectedly accepted"
    );
    fs::write(out.join("corrupt.basisu.ktx2"), b"invalid KTX2")?;
    let scene = include_str!("../scene.bsn");
    let binary = jackdaw_bsn::text_as_binary(scene, std::path::Path::new("scene.bsn"))?;
    fs::write(out.join("scene.bsb"), binary)?;
    fs::write(
        out.join("preparation.json"),
        serde_json::to_vec_pretty(&json!({"textures":reports,"corrupt_input":"rejected"}))?,
    )?;
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
fn fixture(srgb: bool) -> RgbaImage {
    RgbaImage::from_fn(256, 256, |x, y| {
        let checker = ((x / 32 + y / 32) % 2) as u8;
        if srgb {
            Rgba([
                x as u8,
                y as u8,
                checker * 180 + 40,
                if x < 128 { 255 } else { (y / 2 + 32) as u8 },
            ])
        } else {
            Rgba([x as u8, y as u8, checker * 127 + 64, 255])
        }
    })
}
