//! Draw a flow graph file in a window, to look at it outside the editor:
//! open its state machines and go back up, move nodes, rewire. Nothing is
//! saved and nothing runs. As the game's node catalog says (a development
//! build of the game writes it), transitions are drawn as arrows and blend
//! spaces preview their blend on the game's character.
//!
//! ```sh
//! cargo run --example flow_graph -- assets/flow/animation.flowgraph.bsn
//! ```

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_noodle_jackdaw::view::{FlowArrows, FlowGraphViewPlugin, spawn_flow_graph_view};
use bevy_noodle_jackdaw::{FlowGraphTypesPlugin, read_catalog_file, read_flow_graph_file};
use ninja_hair::NinjaHairPlugin;
use pose_preview::PosePreviewPlugin;

/// The game's node catalog, with what blend spaces preview with.
const CATALOG: &str = "assets/flow/nodes.flowcatalog.bsn";

fn main() -> AppExit {
    let path = std::env::args()
        .nth(1)
        .unwrap_or("assets/flow/animation.flowgraph.bsn".into());
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("flow graph: {path}"),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            FlowGraphTypesPlugin,
            FlowGraphViewPlugin,
            PosePreviewPlugin,
            NinjaHairPlugin,
        ))
        .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)))
        .insert_resource(GraphFile(path.into()))
        .add_systems(Startup, open)
        .run()
}

#[derive(Resource)]
struct GraphFile(PathBuf);

fn open(world: &mut World) {
    let path = world.resource::<GraphFile>().0.clone();
    let catalog = read_catalog_file(
        CATALOG.as_ref(),
        &world.resource::<AppTypeRegistry>().read(),
    );
    match catalog {
        Ok(catalog) => {
            world.insert_resource(FlowArrows::from_catalog(&catalog));
            world.insert_resource(catalog.preview);
        }
        Err(err) => warn!("{CATALOG}: {err}"),
    }
    let graph = read_flow_graph_file(&path, &world.resource::<AppTypeRegistry>().read());
    let graph = match graph {
        Ok(graph) => graph,
        Err(err) => {
            error!("{}: {err}", path.display());
            return;
        }
    };
    world.spawn(Camera2d);
    let root = world
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            ..default()
        })
        .id();
    let mut commands = world.commands();
    spawn_flow_graph_view(&mut commands, root, path.display().to_string(), graph);
    world.flush();
}
