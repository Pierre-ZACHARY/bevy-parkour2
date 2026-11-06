//! Data-flow node graphs saved as Jackdaw BSN (`.flowgraph.bsn`), run by the
//! game and drawn with bevy_noodle, lighting up what flows while it plays.
//!
//! - [`FlowGraphPlugin`]: the asset, its loader and the runner. Put a
//!   [`FlowGraphRunner`] on an entity to run a graph for it every fixed step,
//!   and [`RunEveryFrame`] with it to run it every frame instead.
//! - Node types come from a registry ([`FlowGraphAppExt::register_flow_node`]).
//!   The crate depends on no input or controller library: the `leafwing` and
//!   `tnua` features each add a plugin registering nodes for them.
//! - The `view` feature draws a graph as a bevy_noodle canvas, writes edits
//!   back to the graph, and lights up the nodes and wires a running graph's
//!   [`FlowGraphPlayback`] reports active.

#[cfg(feature = "view")]
mod arrows;
#[cfg(feature = "view")]
mod axis;
pub mod blend;
mod bsn;
#[cfg(feature = "view")]
mod current;
#[cfg(feature = "view")]
mod curves;
#[cfg(feature = "view")]
mod dots;
#[cfg(feature = "view")]
mod fields;
#[cfg(feature = "view")]
pub mod library;
mod math;
pub mod model;
pub mod nodes;
mod registry;
mod run;

#[cfg(feature = "leafwing")]
pub mod leafwing;
#[cfg(feature = "view")]
pub mod levels;
#[cfg(feature = "view")]
mod routing;
#[cfg(feature = "view")]
pub mod ties;
#[cfg(feature = "tnua")]
pub mod tnua;
#[cfg(feature = "view")]
pub mod view;
#[cfg(feature = "view")]
pub mod wires;

#[cfg(test)]
mod tests;

/// The README's examples, compiled by `cargo test`.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use bevy::prelude::*;

pub use bsn::{
    CATALOG_EXTENSION, EXTENSION, FlowGraphLoadError, FlowGraphLoader, catalog_to_bsn,
    flow_graph_to_bsn, parse_flow_graph, read_catalog_file, read_flow_graph_file,
    write_catalog_file,
};
pub use model::{
    BlendSample, Easing, FlowGraph, FlowLink, FlowNode, FlowNodeCatalog, FlowTie, FlowValue,
    FlowVariable, PortDef, PortKind, VAR_GET, VAR_SET,
};
pub use registry::{
    ApplyFn, ChoicesFn, ComputeFn, FlowGraphAppExt, FlowNodeType, FlowNodeTypes, NodeRun, StepFn,
    nearest_component,
};
pub use run::{
    Evaluation, FlowGraphPlayback, FlowGraphRunner, FlowGraphSystems, FlowNodeMemory,
    FlowNodeState, FlowVariables, RunEveryFrame, evaluate, run_flow_graphs, run_frame_flow_graphs,
};

/// Graph types for reflection: what BSN files and play-in-editor need,
/// without the asset or the runner (the editor side wants only these).
pub struct FlowGraphTypesPlugin;

impl Plugin for FlowGraphTypesPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<FlowGraph>()
            .register_type::<FlowNode>()
            .register_type::<BlendSample>()
            .register_type::<FlowNodeCatalog>()
            .register_type::<FlowLink>()
            .register_type::<FlowTie>()
            .register_type::<FlowVariable>()
            .register_type::<FlowVariables>()
            .register_type::<PortDef>()
            .register_type::<PortKind>()
            .register_type::<FlowValue>()
            .register_type::<Easing>()
            .register_type::<FlowGraphPlayback>()
            .register_type::<FlowNodeState>();
    }
}

/// Loads `.flowgraph.bsn` graphs and runs them, in `FixedUpdate` (in
/// `PostUpdate` with [`RunEveryFrame`]), for every entity with a
/// [`FlowGraphRunner`].
pub struct FlowGraphPlugin;

impl Plugin for FlowGraphPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FlowGraphTypesPlugin>() {
            app.add_plugins(FlowGraphTypesPlugin);
        }
        app.init_asset::<FlowGraph>()
            .init_asset_loader::<FlowGraphLoader>()
            .init_resource::<FlowNodeTypes>()
            .add_systems(FixedUpdate, run_flow_graphs.in_set(FlowGraphSystems::Run))
            .add_systems(
                PostUpdate,
                run_frame_flow_graphs
                    .in_set(FlowGraphSystems::RunFrame)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
        register_builtins(app);
    }
}

/// The node types every [`FlowGraphPlugin`] app has.
fn register_builtins(app: &mut App) {
    nodes::register(app);
    math::register(app);
    blend::register(app);
}

/// The built-in node types: what every app running graphs has, and what a
/// view, which may not run them (the editor's), knows.
pub fn builtin_types() -> FlowNodeTypes {
    let mut app = App::new();
    register_builtins(&mut app);
    app.world().resource::<FlowNodeTypes>().clone()
}

/// The built-in node types as a catalog: what the editor lists without a
/// catalog file from the game.
pub fn builtin_catalog() -> FlowNodeCatalog {
    builtin_types().catalog()
}

/// `types`' catalog without the built-in types (but with those the app
/// registered differently).
pub fn project_catalog(types: &FlowNodeTypes) -> FlowNodeCatalog {
    let builtins = builtin_catalog();
    let mut catalog = types.catalog();
    catalog.nodes.retain(|node| !builtins.nodes.contains(node));
    catalog
}

/// Writes the game's own node types (see [`project_catalog`]) to a catalog
/// file at startup, when they changed: the editor extension lists them in
/// its node library with the built-ins, since it cannot see the game's
/// registry. With them, what blend spaces preview their samples with (see
/// [`blend::BlendPreviewAssets`]). For development builds; the path is usually
/// `assets/flow/nodes.flowcatalog.bsn` in the game's sources.
pub struct FlowNodeCatalogPlugin {
    pub path: std::path::PathBuf,
}

impl Plugin for FlowNodeCatalogPlugin {
    fn build(&self, app: &mut App) {
        let path = self.path.clone();
        app.add_systems(
            Startup,
            move |types: Res<FlowNodeTypes>,
                  preview: Option<Res<blend::BlendPreviewAssets>>,
                  registry: Res<AppTypeRegistry>| {
                let catalog = FlowNodeCatalog {
                    preview: preview.map(|p| p.clone()).unwrap_or_default(),
                    ..project_catalog(&types)
                };
                match write_catalog_file(&path, &catalog, &registry.read()) {
                    Ok(true) => info!("wrote the node catalog {}", path.display()),
                    Ok(false) => {}
                    Err(err) => warn!("node catalog {}: {err}", path.display()),
                }
            },
        );
    }
}
