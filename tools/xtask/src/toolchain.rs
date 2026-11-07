//! Inspect the active compiler, including immutable Nix toolchains without rustup.
use crate::process;
use anyhow::{Context, Result};
use std::{fs, path::Path, process::Command};

fn contains_std(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("libstd-") && name.ends_with(".rlib")
        })
    })
}

pub fn has_target(root: &Path, triple: &str) -> Result<bool> {
    let libdir = process::output(
        Command::new("rustc")
            .args(["--print", "target-libdir", "--target", triple])
            .current_dir(root),
    )?;
    Ok(contains_std(Path::new(libdir.trim())))
}

pub fn require_target(root: &Path, triple: &str, offline: bool) -> Result<()> {
    if has_target(root, triple)? {
        return Ok(());
    }
    anyhow::ensure!(
        std::env::var_os("PARKOUR_NIX_RUSTC").is_none(),
        "missing target {triple}; add it to the Nix toolchain targets and re-enter nix develop"
    );
    anyhow::ensure!(!offline, "missing target {triple} in offline mode");
    let status = Command::new("rustup")
        .args(["target", "add", triple])
        .current_dir(root)
        .status()
        .context("install Rust target")?;
    anyhow::ensure!(status.success(), "rustup target installation failed");
    anyhow::ensure!(
        has_target(root, triple)?,
        "target {triple} still unavailable"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_target_path_without_installed_std_is_not_an_installed_target() -> Result<()> {
        let temp = tempfile::tempdir()?;
        assert!(!contains_std(temp.path()));
        fs::write(temp.path().join("libstd-test.rmeta"), "")?;
        assert!(!contains_std(temp.path()));
        fs::write(temp.path().join("libstd-test.rlib"), "")?;
        assert!(contains_std(temp.path()));
        assert!(!contains_std(&temp.path().join("missing")));
        Ok(())
    }
}
