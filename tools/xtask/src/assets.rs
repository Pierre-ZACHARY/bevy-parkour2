use crate::{cache, process};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Only runtime assets tracked in Git belong to a distribution. Never traverse
/// asset_sources, the editor SDK, or local authoring downloads.
pub fn runtime_files(root: &Path) -> Result<Vec<PathBuf>> {
    let listing = process::output(
        Command::new("git")
            .args(["ls-files", "-z", "--", "assets"])
            .current_dir(root),
    )?;
    let mut files: Vec<_> = listing
        .split('\0')
        .filter(|s| !s.is_empty() && !s.ends_with(".flowcatalog.bsn"))
        .map(PathBuf::from)
        .collect();
    let private = root.join("private-assets");
    if private.join(".git").exists() {
        let listing = process::output(
            Command::new("git")
                .args([
                    "ls-files",
                    "-z",
                    "--",
                    "assets/character",
                    "assets/animations",
                ])
                .current_dir(&private),
        )?;
        files.extend(
            listing
                .split('\0')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    files.sort();
    files.dedup();
    anyhow::ensure!(!files.is_empty(), "no tracked runtime assets");
    Ok(files)
}
pub fn validate(root: &Path) -> Result<Vec<PathBuf>> {
    let files = runtime_files(root)?;
    for path in &files {
        cache::safe_relative(path)?;
        let full = root.join(path);
        let metadata = fs::symlink_metadata(&full)
            .with_context(|| format!("missing asset {}", path.display()))?;
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "asset must be a regular file: {}",
            path.display()
        );
        anyhow::ensure!(metadata.len() > 0, "empty asset {}", path.display());
        let private = root.join("private-assets").join(path);
        if root.join("private-assets/.git").exists() && private.is_file() {
            anyhow::ensure!(
                cache::hash_file(&full)? == cache::hash_file(&private)?,
                "private runtime copy differs: {}; run tools/setup_assets.ps1",
                path.display()
            );
        }
        let mut prefix = [0_u8; 128];
        use std::io::Read;
        let n = fs::File::open(&full)?.read(&mut prefix)?;
        anyhow::ensure!(
            !prefix[..n].starts_with(b"version https://git-lfs.github.com/spec/v1"),
            "unhydrated LFS pointer: {}; hydrate runtime assets before testing/building",
            path.display()
        );
        if path.extension().is_some_and(|x| x == "gltf") {
            let gltf: serde_json::Value = serde_json::from_slice(&fs::read(&full)?)?;
            validate_gltf(root, path, &gltf)?;
        }
        if path.extension().is_some_and(|x| x == "glb") {
            let bytes = fs::read(&full)?;
            anyhow::ensure!(
                bytes.len() >= 20 && &bytes[..4] == b"glTF",
                "invalid GLB {}",
                path.display()
            );
            let length = u32::from_le_bytes(bytes[12..16].try_into()?) as usize;
            anyhow::ensure!(
                bytes.len() >= 20 + length && &bytes[16..20] == b"JSON",
                "invalid GLB JSON chunk"
            );
            let gltf: serde_json::Value = serde_json::from_slice(&bytes[20..20 + length])?;
            validate_gltf(root, path, &gltf)?;
        }
    }
    Ok(files)
}
fn validate_gltf(root: &Path, path: &Path, gltf: &serde_json::Value) -> Result<()> {
    for field in ["buffers", "images"] {
        for entry in gltf[field].as_array().into_iter().flatten() {
            if let Some(uri) = entry["uri"].as_str() {
                if uri.starts_with("data:") {
                    continue;
                }
                cache::safe_relative(Path::new(uri))?;
                let referenced = path.parent().context("asset parent")?.join(uri);
                anyhow::ensure!(
                    root.join(&referenced).is_file(),
                    "{} references missing {}",
                    path.display(),
                    referenced.display()
                );
                anyhow::ensure!(
                    runtime_files(root)?.contains(&referenced),
                    "untracked glTF dependency {}",
                    referenced.display()
                );
            }
        }
    }
    Ok(())
}
pub fn copy(root: &Path, destination: &Path) -> Result<()> {
    for relative in validate(root)? {
        let out = destination.join(relative.strip_prefix("assets")?);
        fs::create_dir_all(out.parent().unwrap())?;
        fs::copy(root.join(relative), out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let result = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[test]
    fn private_assets_package_only_hydrated_runtime_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("assets/character")).unwrap();
        fs::write(root.join("assets/scene.bsn"), "scene").unwrap();
        fs::write(root.join("assets/character/model.bin"), "model").unwrap();
        git(root, &["init", "--quiet"]);
        git(root, &["add", "assets/scene.bsn"]);
        let private = root.join("private-assets");
        fs::create_dir_all(private.join("assets/character")).unwrap();
        fs::create_dir_all(private.join("asset_sources")).unwrap();
        fs::create_dir_all(private.join("licenses")).unwrap();
        fs::write(private.join("assets/character/model.bin"), "model").unwrap();
        fs::write(
            private.join("asset_sources/original.zip"),
            "restricted authoring data",
        )
        .unwrap();
        fs::write(private.join("licenses/receipt.txt"), "private receipt").unwrap();
        git(&private, &["init", "--quiet"]);
        git(&private, &["add", "."]);
        let destination = root.join("package");
        copy(root, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("character/model.bin")).unwrap(),
            b"model"
        );
        assert!(destination.join("scene.bsn").is_file());
        assert!(!destination.join("asset_sources").exists());
        assert!(!destination.join("licenses").exists());
        fs::write(root.join("assets/character/model.bin"), "modified copy").unwrap();
        assert!(
            validate(root)
                .unwrap_err()
                .to_string()
                .contains("private runtime copy differs")
        );
        let pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:abcd\nsize 5\n";
        fs::write(root.join("assets/character/model.bin"), pointer).unwrap();
        fs::write(private.join("assets/character/model.bin"), pointer).unwrap();
        assert!(
            validate(root)
                .unwrap_err()
                .to_string()
                .contains("unhydrated LFS pointer")
        );
    }
}
