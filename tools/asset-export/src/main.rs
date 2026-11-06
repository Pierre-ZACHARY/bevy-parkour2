//! Convert only a staged asset tree. The repository sources stay as BSN.
use anyhow::{Result, ensure};
use jackdaw_bsn::{
    DocumentForm, binary_twin, document_as_text, document_from_bytes, text_as_binary,
};
use std::{fs, io::Write, path::Path};
mod textures;

#[derive(clap::Parser)]
struct Args {
    /// A disposable staged copy, never the source assets directory.
    directory: std::path::PathBuf,
    #[arg(long, required_unless_present = "web")]
    target: Option<String>,
    #[arg(long, default_value = "original")]
    quality: String,
    #[arg(long)]
    presets: Option<std::path::PathBuf>,
    /// Compatibility alias for --target webgpu.
    #[arg(long)]
    web: bool,
}

fn main() -> Result<()> {
    use clap::Parser;
    let args = Args::parse();
    let target = args.target.as_deref().unwrap_or("webgpu");
    ensure!(
        !args.web || target == "webgpu",
        "--web requires --target webgpu"
    );
    textures::export(
        &args.directory,
        target,
        &args.quality,
        args.presets.as_deref(),
    )?;
    let count = convert_tree(&args.directory)?;
    println!("Exported {count} BSN documents as verified BSB binaries");
    Ok(())
}

fn convert_tree(directory: &Path) -> Result<usize> {
    ensure!(directory.is_dir(), "missing staged assets directory");
    let mut documents = Vec::new();
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
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
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("bsn"))
        {
            documents.push(entry.into_path());
        }
    }
    // Validate every document before making changes to the staged tree.
    let mut encoded = Vec::new();
    for path in &documents {
        let destination = binary_twin(path);
        ensure!(
            !destination.exists(),
            "binary twin already exists: {}",
            destination.display()
        );
        let text = fs::read_to_string(path)?;
        let original = document_from_bytes(text.as_bytes(), path)?;
        let bytes = text_as_binary(&text, &destination)?;
        let decoded = document_from_bytes(&bytes, &destination)?;
        ensure!(
            decoded.form == DocumentForm::Binary,
            "encoder did not produce binary"
        );
        ensure!(
            document_as_text(&original.ast, original.preamble.as_deref())
                == document_as_text(&decoded.ast, decoded.preamble.as_deref()),
            "binary round trip changed {}",
            path.display()
        );
        encoded.push((path, destination, bytes));
    }
    for (source, destination, bytes) in encoded {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::remove_file(source)?;
    }
    // Browsers cannot enumerate HTTP directories. Export a deterministic index
    // of the actual binary documents, consumed through Bevy's AssetServer.
    let mut paths: Vec<String> = documents
        .iter()
        .map(|path| {
            binary_twin(path)
                .strip_prefix(directory)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    // Include existing BSB files on repeat exports as well.
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "bsb") {
            paths.push(
                entry
                    .path()
                    .strip_prefix(directory)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    paths.sort();
    paths.dedup();
    fs::write(
        directory.join("jackdaw.jdcatalog"),
        serde_json::to_vec_pretty(&paths)?,
    )?;
    Ok(documents.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_material_and_graph_keep_headers_and_references_in_binary() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for (name, text) in [
            ("scene.bsn", include_str!("../../../assets/scene.bsn")),
            (
                "ground.material.bsn",
                include_str!("../../../assets/materials/ground.material.bsn"),
            ),
            (
                "movement.flowgraph.bsn",
                include_str!("../../../assets/flow/movement.flowgraph.bsn"),
            ),
        ] {
            fs::write(temp.path().join(name), text)?;
        }
        assert_eq!(convert_tree(temp.path())?, 3);
        for entry in fs::read_dir(temp.path())? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "jdcatalog") {
                continue;
            }
            assert_eq!(path.extension().unwrap(), "bsb");
            assert_eq!(
                jackdaw_bsn::read_document(&path)?.form,
                DocumentForm::Binary
            );
        }
        let index = temp.path().join("jackdaw.jdcatalog");
        let bytes = fs::read(&index)?;
        let paths: Vec<String> = serde_json::from_slice(&bytes)?;
        assert_eq!(
            paths,
            ["ground.material.bsb", "movement.flowgraph.bsb", "scene.bsb"]
        );
        assert_eq!(convert_tree(temp.path())?, 0);
        assert_eq!(fs::read(index)?, bytes);
        Ok(())
    }

    #[test]
    fn web_png_export_preserves_dimensions_alpha_and_sources() -> Result<()> {
        let source = tempfile::tempdir()?;
        let stage = tempfile::tempdir()?;
        let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
            2,
            1,
            vec![0, 32768, 65535, 65535, 65535, 0, 32768, 0],
        )
        .unwrap();
        let source_path = source.path().join("color.png");
        image.save(&source_path)?;
        let original = fs::read(&source_path)?;
        fs::copy(&source_path, stage.path().join("color.png"))?;
        textures::export(stage.path(), "webgpu", "original", None)?;
        let distributed = image::open(stage.path().join("color.png"))?;
        assert_eq!(distributed.color(), image::ColorType::Rgba8);
        let rgba = distributed.into_rgba8();
        assert_eq!(rgba.dimensions(), (2, 1));
        assert_eq!(rgba.get_pixel(0, 0).0, [0, 128, 255, 255]);
        assert_eq!(rgba.get_pixel(1, 0).0, [255, 0, 128, 0]);
        assert_eq!(fs::read(source_path)?, original);
        let report = fs::read(stage.path().join("asset-export.json"))?;
        textures::export(stage.path(), "webgpu", "original", None)?;
        assert_eq!(fs::read(stage.path().join("asset-export.json"))?, report);
        Ok(())
    }

    #[test]
    fn malformed_source_and_existing_twin_are_not_overwritten() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("scene.bsn");
        fs::write(&source, "this is not a valid scene {")?;
        assert!(convert_tree(temp.path()).is_err());
        assert!(source.exists());
        assert!(!binary_twin(&source).exists());
        fs::write(&source, include_str!("../../../assets/scene.bsn"))?;
        fs::write(binary_twin(&source), "existing")?;
        assert!(convert_tree(temp.path()).is_err());
        assert_eq!(fs::read_to_string(binary_twin(&source))?, "existing");
        assert!(source.exists());
        Ok(())
    }
}
