//! The cameras' graphs, run by `bevy_noodle_jackdaw` every frame:
//!
//! ```text
//! Look ──────── vector ─┐
//! Look Stick ── vector ─┼─ Third Person          Switch ── pressed ── Switch Camera
//! Free Look ── pressed ─┘
//!
//! Fly Move ── vector ─┐
//! Fly Up, Fly Down ───┤
//! Fly Speed ── value ─┼─ Fly                     Switch ── pressed ── Switch Camera
//! Look, Look Stick ───┘
//! ```
//!
//! The action nodes come from the leafwing plugin, which derives them from
//! [`CameraAction`]. The crate ships both graphs, read-only; at startup
//! the app gets a copy of each in its assets unless it has one already, as
//! for the movement graph: the app's copies are the ones to edit.

#[cfg(any(not(target_arch = "wasm32"), test))]
use bevy::prelude::*;

#[cfg(doc)]
use crate::CameraAction;

/// Where an app keeps the third-person camera's graph, under its assets
/// folder.
pub const THIRD_PERSON_GRAPH: &str = "flow/third_person_camera.flowgraph.bsn";
/// Where an app keeps the fly camera's graph.
pub const FLY_GRAPH: &str = "flow/fly_camera.flowgraph.bsn";

/// The third-person camera's graph the crate ships.
pub const DEFAULT_THIRD_PERSON_GRAPH: &str =
    include_str!("../assets/third_person_camera.flowgraph.bsn");
/// The fly camera's graph the crate ships.
pub const DEFAULT_FLY_GRAPH: &str = include_str!("../assets/fly_camera.flowgraph.bsn");

/// Copy the crate's graphs to the app's assets folder, each unless a file
/// is already there.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn install_default_graphs(app: &App) {
    use bevy::asset::io::file::FileAssetReader;

    let Some(assets) = app
        .get_added_plugins::<AssetPlugin>()
        .first()
        .map(|plugin| plugin.file_path.clone())
    else {
        return;
    };
    for (graph, contents) in [
        (THIRD_PERSON_GRAPH, DEFAULT_THIRD_PERSON_GRAPH),
        (FLY_GRAPH, DEFAULT_FLY_GRAPH),
    ] {
        let path = FileAssetReader::get_base_path().join(&assets).join(graph);
        if path.exists() || path.with_extension("bsb").exists() {
            continue;
        }
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, contents));
        match written {
            Ok(()) => info!("wrote the default camera graph to {}", path.display()),
            Err(err) => warn!("could not write the camera graph {}: {err}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy_noodle_jackdaw::leafwing::action_kind;
    use bevy_noodle_jackdaw::{
        FlowGraph, FlowGraphPlugin, FlowLink, FlowNodeTypes, flow_graph_to_bsn,
    };

    use super::*;
    use crate::fly::FLY;
    use crate::switch::SWITCH;
    use crate::third_person::THIRD_PERSON;
    use crate::{CameraAction, CameraFlowNodesPlugin};

    fn node_maker(
        types: &FlowNodeTypes,
    ) -> impl Fn(&str, &str, f32, f32) -> bevy_noodle_jackdaw::FlowNode {
        move |id, kind, x, y| {
            types
                .get(kind)
                .unwrap_or_else(|| panic!("{kind} is registered"))
                .instantiate(id, Vec2::new(x, y))
        }
    }

    /// The third-person camera's graph as first written.
    fn default_third_person_graph(types: &FlowNodeTypes) -> FlowGraph {
        let node = node_maker(types);
        FlowGraph {
            nodes: vec![
                node("look", &action_kind(&CameraAction::Look), 40.0, 40.0),
                node(
                    "look_stick",
                    &action_kind(&CameraAction::LookStick),
                    40.0,
                    200.0,
                ),
                node(
                    "free_look",
                    &action_kind(&CameraAction::FreeLook),
                    40.0,
                    360.0,
                ),
                node("third_person", THIRD_PERSON, 320.0, 40.0),
                node(
                    "switch_key",
                    &action_kind(&CameraAction::Switch),
                    40.0,
                    480.0,
                ),
                node("switch", SWITCH, 320.0, 480.0),
            ],
            links: vec![
                FlowLink::new("look", "vector", "third_person", "look"),
                FlowLink::new("look_stick", "vector", "third_person", "look_stick"),
                FlowLink::new("free_look", "pressed", "third_person", "free_look"),
                FlowLink::new("switch_key", "pressed", "switch", "pressed"),
            ],
            ..default()
        }
    }

    /// The fly camera's graph as first written.
    fn default_fly_graph(types: &FlowNodeTypes) -> FlowGraph {
        let node = node_maker(types);
        FlowGraph {
            nodes: vec![
                node("fly_move", &action_kind(&CameraAction::FlyMove), 40.0, 40.0),
                node("up", &action_kind(&CameraAction::FlyUp), 40.0, 200.0),
                node("down", &action_kind(&CameraAction::FlyDown), 40.0, 300.0),
                node("speed", &action_kind(&CameraAction::FlySpeed), 40.0, 400.0),
                node("look", &action_kind(&CameraAction::Look), 40.0, 500.0),
                node(
                    "look_stick",
                    &action_kind(&CameraAction::LookStick),
                    40.0,
                    660.0,
                ),
                node("fly", FLY, 320.0, 40.0),
                node(
                    "switch_key",
                    &action_kind(&CameraAction::Switch),
                    40.0,
                    820.0,
                ),
                node("switch", SWITCH, 320.0, 820.0),
            ],
            links: vec![
                FlowLink::new("fly_move", "vector", "fly", "move"),
                FlowLink::new("up", "pressed", "fly", "up"),
                FlowLink::new("down", "pressed", "fly", "down"),
                FlowLink::new("speed", "value", "fly", "speed_step"),
                FlowLink::new("look", "vector", "fly", "look"),
                FlowLink::new("look_stick", "vector", "fly", "look_stick"),
                FlowLink::new("switch_key", "pressed", "switch", "pressed"),
            ],
            ..default()
        }
    }

    /// Rewrites the crate's graphs as first authored:
    /// `cargo test -p camera_controller write_camera_graphs -- --ignored`.
    #[test]
    #[ignore = "writes assets"]
    fn write_camera_graphs() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), FlowGraphPlugin));
        app.add_plugins(CameraFlowNodesPlugin);
        let types = app.world().resource::<FlowNodeTypes>();
        let registry = app.world().resource::<AppTypeRegistry>().read();
        for (file, graph) in [
            ("third_person_camera", default_third_person_graph(types)),
            ("fly_camera", default_fly_graph(types)),
        ] {
            let path = format!("{}/assets/{file}.flowgraph.bsn", env!("CARGO_MANIFEST_DIR"));
            std::fs::write(path, flow_graph_to_bsn(&graph, &registry)).unwrap();
        }
    }
}
