//! Cameras driven by node graphs, run every frame: a third-person camera
//! over a character's shoulder, the character facing where it aims, and a
//! fly camera free of it.
//!
//! ```rust,ignore
//! app.add_plugins((DefaultPlugins, CameraControllerPlugin));
//! commands.spawn((CharacterController::default(), CameraTarget));
//! commands.spawn((Camera3d::default(), CameraController::default(), CullingCamera));
//! commands.spawn((
//!     Camera3d::default(),
//!     Camera { is_active: false, ..default() },
//!     CameraController { graph: FLY_GRAPH.into() },
//! ));
//! ```
//!
//! - [`CameraController`] on a camera runs its graph. Its sinks place the
//!   camera: Third Person follows the first [`CameraTarget`] and leaves it
//!   a [`CameraAim`], which the Camera Aim node reads in the character's
//!   movement graph; Fly flies; Switch Camera renders through the next
//!   camera.
//! - [`CullingCamera`] tags the cameras the player sees through, for
//!   culling to keep what they see.
//! - The mouse turns the cameras once the window has the cursor: a left
//!   click takes it, Escape lets it go (see [`CameraCursor`]). Under Remote
//!   Desktop, how far the cursor moved stands for the mouse's motion (see
//!   [`MouseLook`]).
//! - With the `reticle` feature, a dot marks the aim at the middle of the
//!   screen while a third-person camera renders (see [`Reticle`]).
//!
//! Add the plugin after the asset plugin: like the character's, it gives
//! the app copies of the default graphs ([`THIRD_PERSON_GRAPH`],
//! [`FLY_GRAPH`]) unless it has them already.

mod aim;
mod controller;
mod fly;
mod graph;
mod input;
#[cfg(feature = "reticle")]
mod reticle;
mod switch;
mod third_person;

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy_noodle_jackdaw::FlowGraphPlugin;
use bevy_noodle_jackdaw::leafwing::LeafwingFlowNodesPlugin;
use leafwing_input_manager::plugin::InputManagerSystem;
use leafwing_input_manager::prelude::*;

pub use aim::AIM;
pub use controller::{
    CameraAim, CameraController, CameraCursor, CameraTarget, CullingCamera, MouseLook, look_turn,
};
pub use fly::{FLY, FlySpeed, fly_pose, step_speed};
pub use graph::{DEFAULT_FLY_GRAPH, DEFAULT_THIRD_PERSON_GRAPH, FLY_GRAPH, THIRD_PERSON_GRAPH};
pub use input::{CameraAction, default_camera_input_map};
#[cfg(feature = "reticle")]
pub use reticle::Reticle;
pub use switch::{SWITCH, switch};
pub use third_person::{THIRD_PERSON, ThirdPersonView};

/// The cameras, their actions and their graphs' node types.
pub struct CameraControllerPlugin;

impl Plugin for CameraControllerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CameraController>()
            .register_type::<CullingCamera>()
            .register_type::<CameraTarget>()
            .init_resource::<CameraCursor>()
            .init_resource::<MouseLook>()
            // Read by the cursor look; there without a window plugin too.
            .add_message::<bevy::window::CursorMoved>()
            .add_message::<bevy::input::mouse::MouseMotion>()
            .add_plugins((
                InputManagerPlugin::<CameraAction>::default(),
                CameraFlowNodesPlugin,
            ))
            .add_observer(controller::build_camera)
            .add_systems(
                PreUpdate,
                // Over the mouse's motion once Bevy has summed it, before
                // the actions read it.
                controller::cursor_look
                    .after(InputSystems)
                    .before(InputManagerSystem::Unify),
            )
            .add_systems(Update, controller::grab_cursor);
        #[cfg(feature = "reticle")]
        app.add_plugins(reticle::ReticlePlugin);
        if !app.is_plugin_added::<FlowGraphPlugin>() {
            app.add_plugins(FlowGraphPlugin);
        }
        #[cfg(not(target_arch = "wasm32"))]
        graph::install_default_graphs(app);
    }
}

/// The crate's node types: the camera actions', Third Person, Fly, Switch
/// Camera and Camera Aim. [`CameraControllerPlugin`] adds it; a character's
/// movement graph needs it for Camera Aim.
pub struct CameraFlowNodesPlugin;

impl Plugin for CameraFlowNodesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LeafwingFlowNodesPlugin::<CameraAction>::default());
        aim::register(app);
        third_person::register(app);
        fly::register(app);
        switch::register(app);
    }
}
