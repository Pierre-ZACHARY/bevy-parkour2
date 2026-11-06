//! The character's movement as a node graph, run by `bevy_noodle_jackdaw`:
//!
//! ```text
//! Move ─ x ─┐
//!      ─ y ─┴─ Make Vector ── Camera Relative ── motion ─┐
//! Run ── Double Tap ── held ─────────────────── jog ────┤
//!                   ── double ───────────────── sprint ─┴─ Gait ── Tnua Walk
//!                                                            └─ Set Stop Distance
//! ```
//!
//! The Move, Run and Tnua nodes come from the crate's leafwing and Tnua
//! plugins, which derive them from [`PlayerAction`] and [`ControlScheme`];
//! Gait and Set Stop Distance are this crate's (see `gait.rs`, `state.rs`).
//!
//! The crate ships that graph, read-only ([`DEFAULT_MOVEMENT_GRAPH`]). At
//! startup the app gets a copy at [`MOVEMENT_GRAPH`] in its assets, unless
//! it has one already: the app's copy is the one to edit, the crate's never
//! changes under it.

#[cfg(any(not(target_arch = "wasm32"), test))]
use bevy::prelude::*;

#[cfg(doc)]
use crate::{ControlScheme, PlayerAction};

/// Where an app keeps the movement graph, under its assets folder.
pub const MOVEMENT_GRAPH: &str = "flow/movement.flowgraph.bsn";

/// The movement graph the crate ships.
pub const DEFAULT_MOVEMENT_GRAPH: &str = include_str!("../assets/movement.flowgraph.bsn");

/// Copy [`DEFAULT_MOVEMENT_GRAPH`] to [`MOVEMENT_GRAPH`] in the app's
/// assets folder, unless a file is already there.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn install_default_graph(app: &App) {
    use bevy::asset::io::file::FileAssetReader;

    let Some(assets) = app
        .get_added_plugins::<AssetPlugin>()
        .first()
        .map(|plugin| plugin.file_path.clone())
    else {
        return;
    };
    let path = FileAssetReader::get_base_path()
        .join(assets)
        .join(MOVEMENT_GRAPH);
    if path.exists() || path.with_extension("bsb").exists() {
        return;
    }
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, DEFAULT_MOVEMENT_GRAPH));
    match written {
        Ok(()) => info!("wrote the default movement graph to {}", path.display()),
        Err(err) => warn!(
            "could not write the movement graph {}: {err}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use bevy_noodle_jackdaw::leafwing::action_kind;
    use bevy_noodle_jackdaw::nodes::{CAMERA_RELATIVE, DOUBLE_TAP, MAKE_VECTOR2};
    use bevy_noodle_jackdaw::tnua::WALK;
    use bevy_noodle_jackdaw::{
        FlowGraph, FlowGraphPlayback, FlowGraphPlugin, FlowLink, FlowNodeTypes, flow_graph_to_bsn,
    };

    use super::*;
    use crate::tests::{floor_app, run_forward, temp_assets, walk_forward};
    use crate::{
        CharacterController, CharacterControllerPlugin, FLOAT_HEIGHT, GAIT, JOG_SPEED,
        PlayerAction, SET_STOP_DISTANCE, SPRINT_SPEED, WALK_SPEED,
    };

    /// The movement graph as first written, from the registered node types.
    fn default_movement_graph(types: &FlowNodeTypes) -> FlowGraph {
        let node = |id: &str, kind: &str, x: f32, y: f32| {
            types
                .get(kind)
                .unwrap_or_else(|| panic!("{kind} is registered"))
                .instantiate(id, Vec2::new(x, y))
        };
        FlowGraph {
            nodes: vec![
                node("move", &action_kind(&PlayerAction::Move), 40.0, 40.0),
                node("run", &action_kind(&PlayerAction::Run), 40.0, 260.0),
                node("make_vector", MAKE_VECTOR2, 280.0, 40.0),
                node("double_tap", DOUBLE_TAP, 280.0, 240.0),
                node("camera_relative", CAMERA_RELATIVE, 520.0, 55.0),
                node("gait", GAIT, 780.0, 40.0),
                node("walk", WALK, 1060.0, 55.0),
                node("set_stop_distance", SET_STOP_DISTANCE, 1060.0, 220.0),
            ],
            links: vec![
                FlowLink::new("move", "x", "make_vector", "x"),
                FlowLink::new("move", "y", "make_vector", "y"),
                FlowLink::new("make_vector", "vector", "camera_relative", "input"),
                FlowLink::new("camera_relative", "direction", "gait", "motion"),
                FlowLink::new("run", "pressed", "double_tap", "button"),
                FlowLink::new("double_tap", "held", "gait", "jog"),
                FlowLink::new("double_tap", "double", "gait", "sprint"),
                FlowLink::new("gait", "velocity", "walk", "motion"),
                FlowLink::new("gait", "stop_distance", "set_stop_distance", "distance"),
            ],
            ..default()
        }
    }

    /// Rewrites the crate's `assets/movement.flowgraph.bsn` as first
    /// authored: `cargo test -p character_controller write_movement_graph -- --ignored`.
    #[test]
    #[ignore = "writes an asset"]
    fn write_movement_graph() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), FlowGraphPlugin));
        app.add_plugins((
            bevy_noodle_jackdaw::leafwing::LeafwingFlowNodesPlugin::<PlayerAction>::default(),
            bevy_noodle_jackdaw::tnua::TnuaFlowNodesPlugin::<crate::ControlScheme>::default(),
        ));
        app.add_plugins(crate::CharacterFlowNodesPlugin);
        let graph = default_movement_graph(app.world().resource::<FlowNodeTypes>());
        let text = flow_graph_to_bsn(&graph, &app.world().resource::<AppTypeRegistry>().read());
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/movement.flowgraph.bsn");
        std::fs::write(path, text).unwrap();
    }

    /// An app with no movement graph gets the crate's, and holding forward
    /// walks the character through it alone.
    #[test]
    fn holding_forward_walks_through_the_default_graph() {
        let assets = temp_assets("walk");
        let mut app = floor_app(&assets);
        let installed = std::fs::read_to_string(assets.join(MOVEMENT_GRAPH)).unwrap();
        assert_eq!(installed, DEFAULT_MOVEMENT_GRAPH);

        let character = app
            .world_mut()
            .spawn((
                CharacterController::default(),
                Transform::from_xyz(0.0, FLOAT_HEIGHT, 0.0),
            ))
            .id();
        let (moved, _) = walk_forward(&mut app, character);
        assert!(
            moved.z < -0.8 * WALK_SPEED && moved.z > -WALK_SPEED,
            "moved {moved}"
        );
        assert!(moved.x.abs() < 0.05, "moved {moved}");

        // What flowed is recorded for views, every node lit but the
        // unpressed Run's.
        let playback = app.world().get::<FlowGraphPlayback>(character).unwrap();
        assert_eq!(playback.graph, MOVEMENT_GRAPH);
        let dark: Vec<&str> = playback
            .nodes
            .iter()
            .filter(|node| !node.is_active())
            .map(|node| node.id.as_str())
            .collect();
        assert_eq!(dark, ["run", "double_tap"], "{playback:?}");
        _ = std::fs::remove_dir_all(assets);
    }

    /// Shift held jogs; tapped twice and held, it sprints.
    #[test]
    fn shift_jogs_and_a_double_shift_sprints() {
        for (presses, speed) in [(1, JOG_SPEED), (2, SPRINT_SPEED)] {
            let assets = temp_assets(&format!("run-{presses}"));
            let mut app = floor_app(&assets);
            let character = app
                .world_mut()
                .spawn((
                    CharacterController::default(),
                    Transform::from_xyz(0.0, FLOAT_HEIGHT, 0.0),
                ))
                .id();
            let (moved, _) = run_forward(&mut app, character, presses);
            assert!(
                moved.z < -0.8 * speed && moved.z > -speed,
                "{presses} presses: moved {moved}"
            );
            _ = std::fs::remove_dir_all(assets);
        }
    }

    /// The app's own graph is kept.
    #[test]
    fn an_edited_graph_is_not_overwritten() {
        let assets = temp_assets("keep");
        let path = assets.join(MOVEMENT_GRAPH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "edited").unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: assets.to_string_lossy().into_owned(),
                ..default()
            },
            CharacterControllerPlugin,
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited");
        _ = std::fs::remove_dir_all(assets);
    }
}
