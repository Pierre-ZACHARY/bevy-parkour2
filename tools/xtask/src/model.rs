use anyhow::{Result, bail};
use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    WindowsX64,
    LinuxX64,
    LinuxArm64,
    MacosArm64,
    Webgpu,
    AndroidArm64,
    IosArm64,
}
impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x64",
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::MacosArm64 => "macos-arm64",
            Self::Webgpu => "webgpu",
            Self::AndroidArm64 => "android-arm64",
            Self::IosArm64 => "ios-arm64",
        }
    }
    pub fn triple(self) -> &'static str {
        match self {
            Self::WindowsX64 => "x86_64-pc-windows-msvc",
            Self::LinuxX64 => "x86_64-unknown-linux-gnu",
            Self::LinuxArm64 => "aarch64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::Webgpu => "wasm32-unknown-unknown",
            Self::AndroidArm64 => "aarch64-linux-android",
            Self::IosArm64 => "aarch64-apple-ios",
        }
    }
    pub fn backends(self) -> &'static [&'static str] {
        match self {
            Self::WindowsX64 => &["dx12", "vulkan"],
            Self::MacosArm64 => &["metal", "vulkan"],
            Self::Webgpu => &["webgpu"],
            Self::IosArm64 => &["metal"],
            _ => &["vulkan"],
        }
    }
    pub fn features(self) -> &'static str {
        match self {
            Self::Webgpu => "distribution,webgpu",
            Self::MacosArm64 => "distribution,macos-vulkan",
            _ => "distribution",
        }
    }
    pub fn validate(self, host: &str) -> Result<()> {
        if matches!(self, Self::AndroidArm64 | Self::IosArm64) {
            bail!(
                "{} is planned for the mobile milestone, not implemented",
                self.name()
            );
        }
        if self != Self::Webgpu && self.triple() != host {
            bail!(
                "{} requires a native {} runner; detected {host}",
                self.name(),
                self.triple()
            );
        }
        Ok(())
    }
}
pub fn host() -> Result<String> {
    let out = Command::new("rustc").arg("-vV").output()?;
    anyhow::ensure!(out.status.success(), "rustc -vV failed");
    String::from_utf8(out.stdout)?
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
        .ok_or_else(|| anyhow::anyhow!("rustc did not report its host"))
}
pub fn native_target(host: &str) -> Result<Target> {
    [
        Target::WindowsX64,
        Target::LinuxX64,
        Target::LinuxArm64,
        Target::MacosArm64,
    ]
    .into_iter()
    .find(|t| t.triple() == host)
    .ok_or_else(|| anyhow::anyhow!("unsupported native runner {host}"))
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    #[default]
    Local,
    Official,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Dev,
    /// Reuse the instrumentable CI cache for local browser iteration.
    Ci,
    #[default]
    Release,
}
impl Profile {
    pub fn cargo(self, target: Target) -> &'static str {
        match (self, target) {
            (Self::Dev, _) => "dev",
            (Self::Ci, _) => "ci",
            (_, Target::Webgpu) => "wasm-release",
            _ => "release",
        }
    }
    pub fn directory(self, target: Target) -> &'static str {
        if self == Self::Dev {
            "debug"
        } else {
            self.cargo(target)
        }
    }
}
#[derive(Clone, Debug, Args)]
pub struct Common {
    /// Parallel compiler jobs and test processes (defaults to two).
    #[arg(long, global = true, default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..))]
    pub jobs: u16,
    /// Download cache. Never installs tools into the global PATH.
    #[arg(long, global = true, default_value = ".tools/xtask")]
    pub cache_dir: PathBuf,
    /// Use temporary coverage/doctest build directories, preserving only reports.
    #[arg(long, global = true)]
    pub discard_build_artifacts: bool,
    /// Parent of unique run directories; existing results are never overwritten.
    #[arg(long, global = true, default_value = "artifacts")]
    pub out_dir: PathBuf,
    #[arg(long, global = true)]
    pub offline: bool,
}
impl Common {
    pub fn resolve(&mut self, root: &Path) {
        if self.cache_dir.is_relative() {
            self.cache_dir = root.join(&self.cache_dir);
        }
        if self.out_dir.is_relative() {
            self.out_dir = root.join(&self.out_dir);
        }
    }
}
#[derive(Clone, Debug, Args, Serialize)]
pub struct BuildArgs {
    #[arg(long, value_delimiter = ',')]
    pub targets: Vec<Target>,
    #[arg(long, value_enum, default_value = "release")]
    pub profile: Profile,
    #[arg(long, value_enum, default_value = "local")]
    pub channel: Channel,
    #[arg(long)]
    pub tag: Option<String>,
    /// Legacy option retained for scripts; client configuration now comes from Infisical.
    #[arg(long)]
    pub sentry_stack: Option<String>,
    /// Versioned per-target asset preset, independent of the Cargo profile.
    #[arg(long, default_value = "original")]
    pub asset_quality: String,
    /// Override tools/asset-export/presets.json with a custom preset catalog.
    #[arg(long)]
    pub asset_presets: Option<PathBuf>,
}
impl BuildArgs {
    pub fn resolve(&mut self, host: &str) -> Result<()> {
        if self.targets.is_empty() {
            self.targets.push(native_target(host)?);
        }
        for target in &self.targets {
            target.validate(host)?;
        }
        let mut unique = Vec::new();
        self.targets.retain(|target| {
            if unique.contains(target) {
                false
            } else {
                unique.push(*target);
                true
            }
        });
        if self.channel == Channel::Official && self.tag.is_none() {
            self.tag = std::env::var("XTASK_TAG").ok().filter(|s| !s.is_empty());
        }
        if self.channel == Channel::Official && self.tag.is_none() {
            bail!("official builds require --tag");
        }
        if self.channel == Channel::Local && self.tag.is_some() {
            bail!("--tag requires --channel official");
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Suite {
    pub name: &'static str,
    pub manifest: &'static str,
    pub features: &'static str,
    pub workspace: bool,
}
pub const SUITES: &[Suite] = &[
    Suite {
        name: "game",
        manifest: "Cargo.toml",
        features: "",
        workspace: true,
    },
    Suite {
        name: "xtask",
        manifest: "tools/xtask/Cargo.toml",
        features: "",
        workspace: false,
    },
    Suite {
        name: "graph",
        manifest: "crates/bevy_noodle_jackdaw/Cargo.toml",
        features: "view,leafwing,tnua",
        workspace: false,
    },
    Suite {
        name: "asset-export",
        manifest: "tools/asset-export/Cargo.toml",
        features: "",
        workspace: false,
    },
    Suite {
        name: "noodle",
        manifest: "crates/bevy_noodle/Cargo.toml",
        features: "default_style,scene",
        workspace: false,
    },
];
pub fn suites(selection: &[String]) -> Result<Vec<Suite>> {
    for s in selection {
        anyhow::ensure!(SUITES.iter().any(|x| x.name == s), "unknown suite {s}");
    }
    Ok(SUITES
        .iter()
        .copied()
        .filter(|s| selection.is_empty() || selection.iter().any(|n| n == s.name))
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runner_contract() {
        assert!(
            Target::LinuxArm64
                .validate("x86_64-pc-windows-msvc")
                .is_err()
        );
        assert!(
            Target::LinuxArm64
                .validate("aarch64-unknown-linux-gnu")
                .is_ok()
        );
        assert!(Target::Webgpu.validate("x86_64-pc-windows-msvc").is_ok());
        assert!(Target::IosArm64.validate("aarch64-apple-darwin").is_err());
    }
    #[test]
    fn suites_include_independent_workspaces() {
        let all = suites(&[]).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[2].features, "view,leafwing,tnua");
        assert!(suites(&["typo".into()]).is_err());
    }
}
