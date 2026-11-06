//! Jackdaw editor extension for this project: what the editor needs to show
//! its models as the game does. The Ninja's hair gets its mip chain, in the
//! scene's viewport and in the Flow Graph's blend space previews alike.

use bevy::prelude::*;
use jackdaw_extension::prelude::*;
use ninja_hair::NinjaHairPlugin;

#[derive(Default)]
pub struct ProjectExtension;

impl JackdawExtension for ProjectExtension {
    fn id(&self) -> String {
        "bevy_parkour2".to_string()
    }

    fn label(&self) -> String {
        "bevy-parkour2".to_string()
    }

    fn description(&self) -> String {
        "Shows this project's models as the game does.".to_string()
    }

    fn register(&self, _registrar: &mut ExtensionRegistrar<'_>) {}

    fn app_plugin(&self) -> Option<Box<dyn Plugin>> {
        Some(Box::new(|app: &mut App| {
            app.add_plugins(NinjaHairPlugin);
        }))
    }
}
