//! A headless character controller: a Tnua character on an Avian capsule,
//! walking where its movement graph says. No model and no animation: the
//! app puts its visuals under the controller's entity.
//!
//! ```rust,ignore
//! app.add_plugins((
//!     DefaultPlugins,
//!     PhysicsPlugins::default(),
//!     TnuaAvian3dPlugin::new(FixedUpdate),
//!     CharacterControllerPlugin,
//! ));
//! commands.spawn((CharacterController::default(), Transform::from_xyz(0.0, FLOAT_HEIGHT, 0.0)));
//! ```
//!
//! Add the plugin after the asset plugin: it gives the app a copy of the
//! default movement graph, [`DEFAULT_MOVEMENT_GRAPH`], at [`MOVEMENT_GRAPH`]
//! in its assets unless it has one there already. That copy is the one to
//! edit; the crate's never changes under it.
//!
//! The `view` feature adds `view::MovementGraphViewPlugin`, the graph
//! over the game on Tab.

mod controller;
mod gait;
mod graph;
mod input;
mod state;
#[cfg(feature = "view")]
pub mod view;

use bevy::prelude::*;
use bevy_noodle_jackdaw::FlowGraphPlugin;
use bevy_noodle_jackdaw::leafwing::LeafwingFlowNodesPlugin;
use bevy_noodle_jackdaw::tnua::TnuaFlowNodesPlugin;
use bevy_tnua::prelude::*;
use leafwing_input_manager::prelude::*;

pub use controller::{CharacterController, ControlScheme, FLOAT_HEIGHT, HEIGHT, RADIUS, SPEED};
pub use gait::{GAIT, Gait, GaitStep, JOG_SPEED, SPRINT_SPEED, WALK_SPEED};
pub use graph::{DEFAULT_MOVEMENT_GRAPH, MOVEMENT_GRAPH};
pub use input::{PlayerAction, default_input_map};
pub use state::{SET_STOP_DISTANCE, STOP_DISTANCE, StopDistance};

/// The controller, its actions and its movement graph's node types.
pub struct CharacterControllerPlugin;

impl Plugin for CharacterControllerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<CharacterController>()
            .add_plugins((
                InputManagerPlugin::<PlayerAction>::default(),
                TnuaControllerPlugin::<ControlScheme>::new(FixedUpdate),
                FlowGraphPlugin,
                LeafwingFlowNodesPlugin::<PlayerAction>::default(),
                TnuaFlowNodesPlugin::<ControlScheme>::default(),
                CharacterFlowNodesPlugin,
            ))
            .add_observer(controller::build_controller)
            .add_systems(
                FixedUpdate,
                controller::apply_controls.in_set(TnuaUserControlsSystems),
            );
        #[cfg(not(target_arch = "wasm32"))]
        graph::install_default_graph(app);
    }
}

/// The crate's own node types: Gait, Set Stop Distance and Stop Distance.
/// [`CharacterControllerPlugin`] adds it; a tool writing graphs may add it
/// alone.
pub struct CharacterFlowNodesPlugin;

impl Plugin for CharacterFlowNodesPlugin {
    fn build(&self, app: &mut App) {
        gait::register(app);
        state::register(app);
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use avian3d::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy_noodle_jackdaw::{FlowGraph, FlowGraphRunner};
    use bevy_tnua_avian3d::TnuaAvian3dPlugin;

    use super::*;

    /// A fresh assets folder for one test: the plugin writes its graph
    /// there.
    pub(crate) fn temp_assets(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "character-controller-{name}-{}",
            std::process::id()
        ));
        _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A headless app with physics and a static floor, its assets in
    /// `assets`.
    pub(crate) fn floor_app(assets: &Path) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..default()
            },
            bevy::input::InputPlugin,
            PhysicsPlugins::default(),
            TnuaAvian3dPlugin::new(FixedUpdate),
            CharacterControllerPlugin,
        ))
        .init_asset::<Mesh>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )));
        app.world_mut().spawn((
            RigidBody::Static,
            Collider::cuboid(20.0, 0.1, 20.0),
            Transform::from_xyz(0.0, -0.05, 0.0),
        ));
        app.finish();
        app.cleanup();
        app
    }

    /// Load the character's graph if it has one, settle on the floor, then
    /// hold the physical W key for one second: how far it went, and where.
    pub(crate) fn walk_forward(app: &mut App, character: Entity) -> (Vec3, Vec3) {
        run_forward(app, character, 0)
    }

    /// [`walk_forward`], with left Shift pressed `presses` times (0.05 s
    /// apart) and held before W: 1 to jog, 2 to sprint.
    pub(crate) fn run_forward(app: &mut App, character: Entity, presses: usize) -> (Vec3, Vec3) {
        if let Some(runner) = app.world().get::<FlowGraphRunner>(character) {
            let graph = runner.graph.clone();
            for _ in 0..500 {
                app.update();
                if app.world().resource::<Assets<FlowGraph>>().contains(&graph) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        for _ in 0..30 {
            app.update();
        }
        for press in 0..presses {
            if press > 0 {
                KeyCode::ShiftLeft.release(app.world_mut());
                for _ in 0..3 {
                    app.update();
                }
            }
            KeyCode::ShiftLeft.press(app.world_mut());
            for _ in 0..3 {
                app.update();
            }
        }
        let start = app.world().get::<Transform>(character).unwrap().translation;
        KeyCode::KeyW.press(app.world_mut());
        for _ in 0..60 {
            app.update();
        }
        let end = app.world().get::<Transform>(character).unwrap().translation;
        (end - start, end)
    }
}
