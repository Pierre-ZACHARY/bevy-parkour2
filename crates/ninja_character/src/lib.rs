//! The Ninja Vex (Ida Faber) on a [`character_controller`] character: what
//! it looks like. Its files stay in the app's `assets/character/ninja/`
//! (third-party, not redistributed), where scenes reference them.
//!
//! Its clips are Yummy Games' ([`yummy`]), retargeted onto it by
//! [`character_animation`]: the Ninja's bones are named like UE5 Manny's,
//! so it needs no [`Rig`](character_animation::Rig).

pub mod yummy;
mod yummy_clips;

use bevy::prelude::*;
use character_animation::BlendPreviewAssets;

pub use ninja_hair::NinjaHairPlugin;
use yummy::YummyClip;
pub use yummy::YummyClipsPlugin;

/// The Ninja's glTF, under the app's assets.
pub const MODEL: &str = "character/ninja/outfit_01.gltf";

/// Everything the Ninja needs once its scene is spawned, and its clips. The
/// editor previews blend spaces on it.
/// Add [`CharacterAnimationPlugin`](character_animation::CharacterAnimationPlugin)
/// to play them.
pub struct NinjaPlugin;

impl Plugin for NinjaPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((NinjaHairPlugin, YummyClipsPlugin));
        app.world_mut()
            .get_resource_or_init::<BlendPreviewAssets>()
            .model = MODEL.to_owned();
    }
}
