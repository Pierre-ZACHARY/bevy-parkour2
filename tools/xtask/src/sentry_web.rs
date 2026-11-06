//! Build the official browser + WASM SDK from an integrity-locked dependency tree.
use crate::{cache, model::Common, process, sentry};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};
const PACKAGE: &str = include_str!("../../sentry-web/package.json");
const LOCK: &str = include_str!("../../sentry-web/package-lock.json");
const ENTRY: &str = include_str!("../../sentry-web/index.js");
fn directory(opts: &Common) -> PathBuf {
    opts.cache_dir.join("sentry-web").join(format!(
        "{:x}",
        Sha256::digest(format!("{PACKAGE}{LOCK}{ENTRY}"))
    ))
}
pub fn bundle(opts: &Common) -> Result<PathBuf> {
    let dir = directory(opts);
    fs::create_dir_all(&dir)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("build.lock"))?;
    lock.try_lock()
        .context("another Sentry browser SDK build is running")?;
    let file = dir.join("sentry-browser.js");
    let receipt = dir.join("sha256");
    if file.is_file() && receipt.is_file() {
        ensure!(
            cache::hash_file(&file)? == fs::read_to_string(receipt)?,
            "cached browser SDK was modified"
        );
        return Ok(file);
    }
    fs::write(dir.join("package.json"), PACKAGE)?;
    fs::write(dir.join("package-lock.json"), LOCK)?;
    fs::write(dir.join("index.js"), ENTRY)?;
    let mut npm = Command::new(if cfg!(windows) { "npm.cmd" } else { "npm" });
    sentry::configure(&mut npm, None);
    npm.current_dir(&dir)
        .args(["ci", "--ignore-scripts", "--no-audit", "--no-fund"]);
    if opts.offline {
        npm.arg("--offline");
    }
    process::output(&mut npm)
        .context("prepare pinned Sentry browser dependencies (Node 22+ and npm required)")?;
    let mut node = Command::new("node");
    sentry::configure(&mut node, None);
    node.current_dir(&dir).args([
        "node_modules/esbuild/bin/esbuild",
        "index.js",
        "--bundle",
        "--format=iife",
        "--global-name=Sentry",
        "--target=es2022",
        "--outfile=sentry-browser.js",
    ]);
    process::output(&mut node).context("bundle Sentry browser + WebAssembly integrations")?;
    fs::write(receipt, cache::hash_file(&file)?)?;
    Ok(file)
}
#[cfg(test)]
pub fn fixture(opts: &Common) {
    let dir = directory(opts);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("sentry-browser.js"), "window.Sentry = {};").unwrap();
    fs::write(
        dir.join("sha256"),
        cache::hash_file(&dir.join("sentry-browser.js")).unwrap(),
    )
    .unwrap();
}
