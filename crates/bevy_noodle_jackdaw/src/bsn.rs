//! `.flowgraph.bsn` files: a Jackdaw BSN document whose root is a
//! [`FlowGraph`], read and written the way Jackdaw's own `.animgraph.bsn`
//! files are. Node catalogs (`.flowcatalog.bsn`) are the same with a
//! [`FlowNodeCatalog`] root.

use std::any::TypeId;
use std::path::Path;

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use bevy::reflect::{TypeRegistry, TypeRegistryArc};
use jackdaw_bsn::{
    BsnPatch, BsnPatches, BsnValue, SceneBsnAst, bsn_value_to_reflect, component_to_bsn_patch,
    emit_scene, parse_bsn_text,
};

use crate::model::{FlowGraph, FlowNodeCatalog};

/// The file extension graphs load from.
pub const EXTENSION: &str = "flowgraph.bsn";
/// The file extension of node catalogs.
pub const CATALOG_EXTENSION: &str = "flowcatalog.bsn";

#[derive(Debug)]
pub enum FlowGraphLoadError {
    Io(String),
    Parse(String),
    /// The document has no root of the type read.
    NoGraph,
    /// A field does not read as its type.
    Fields,
    /// The type read is not registered for reflection.
    Unregistered,
}

impl std::fmt::Display for FlowGraphLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "could not read the file: {err}"),
            Self::Parse(err) => write!(f, "could not parse the file: {err}"),
            Self::NoGraph => write!(f, "the file holds no root of the expected type"),
            Self::Fields => write!(f, "a field does not read as its type"),
            Self::Unregistered => write!(f, "the root type is not registered for reflection"),
        }
    }
}

impl std::error::Error for FlowGraphLoadError {}

/// Read the `T` a BSN document carries as its root. Fields the file leaves
/// out keep their `Default`.
fn parse_root<T: FromReflect + TypePath + Default>(
    text: &str,
    registry: &TypeRegistry,
) -> Result<T, FlowGraphLoadError> {
    let ast = parse_bsn_text(text).map_err(|err| FlowGraphLoadError::Parse(err.to_string()))?;
    if registry.get(TypeId::of::<T>()).is_none() {
        return Err(FlowGraphLoadError::Unregistered);
    }
    let wanted = T::type_path();
    for &root in &ast.roots {
        let Some(patches) = ast.get_patches(root) else {
            continue;
        };
        for &patch in &patches.0 {
            match ast.get_patch(patch) {
                Some(BsnPatch::Type(path)) if path == wanted => return Ok(T::default()),
                Some(BsnPatch::Struct(data)) if data.type_path == wanted => {
                    let value = bsn_value_to_reflect(
                        &BsnValue::Struct(data.clone()),
                        TypeId::of::<T>(),
                        registry,
                        None,
                    )
                    .ok_or(FlowGraphLoadError::Fields)?;
                    return T::from_reflect(&*value).ok_or(FlowGraphLoadError::Fields);
                }
                _ => {}
            }
        }
    }
    Err(FlowGraphLoadError::NoGraph)
}

/// A BSN document with `value` as its root, in the shape [`parse_root`]
/// reads back.
fn root_to_bsn(value: &dyn PartialReflect, registry: &TypeRegistry) -> String {
    let mut ast = SceneBsnAst::default();
    let patch = component_to_bsn_patch(value, registry);
    let patch = ast.world.spawn(patch).id();
    let root = ast.world.spawn(BsnPatches(vec![patch])).id();
    ast.add_to_roots(root);
    emit_scene(&ast)
}

fn read_file_text(path: &Path) -> Result<String, FlowGraphLoadError> {
    let bytes = std::fs::read(path).map_err(|err| FlowGraphLoadError::Io(err.to_string()))?;
    jackdaw_bsn::document_text_from_bytes(&bytes, path)
        .map_err(|err| FlowGraphLoadError::Parse(err.to_string()))
}

/// Read the [`FlowGraph`] a BSN document carries as its root.
pub fn parse_flow_graph(
    text: &str,
    registry: &TypeRegistry,
) -> Result<FlowGraph, FlowGraphLoadError> {
    parse_root(text, registry)
}

/// The text of a graph file, in the shape [`parse_flow_graph`] reads back.
pub fn flow_graph_to_bsn(graph: &FlowGraph, registry: &TypeRegistry) -> String {
    root_to_bsn(graph.as_partial_reflect(), registry)
}

/// Read the graph file at `path`, outside the asset server (for tools).
pub fn read_flow_graph_file(
    path: &Path,
    registry: &TypeRegistry,
) -> Result<FlowGraph, FlowGraphLoadError> {
    parse_flow_graph(&read_file_text(path)?, registry)
}

/// The text of a catalog file.
pub fn catalog_to_bsn(catalog: &FlowNodeCatalog, registry: &TypeRegistry) -> String {
    root_to_bsn(catalog.as_partial_reflect(), registry)
}

/// Read the catalog file at `path`.
pub fn read_catalog_file(
    path: &Path,
    registry: &TypeRegistry,
) -> Result<FlowNodeCatalog, FlowGraphLoadError> {
    parse_root(&read_file_text(path)?, registry)
}

/// Write `catalog` to `path` unless it already holds it. Returns whether it
/// wrote.
pub fn write_catalog_file(
    path: &Path,
    catalog: &FlowNodeCatalog,
    registry: &TypeRegistry,
) -> std::io::Result<bool> {
    let text = catalog_to_bsn(catalog, registry);
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(true)
}

/// Loads `.flowgraph.bsn` files as [`FlowGraph`] assets.
#[derive(TypePath)]
pub struct FlowGraphLoader {
    /// Shared with `AppTypeRegistry`, so types registered after the loader
    /// was built are still readable.
    registry: TypeRegistryArc,
}

impl FromWorld for FlowGraphLoader {
    fn from_world(world: &mut World) -> Self {
        Self {
            registry: world.resource::<AppTypeRegistry>().0.clone(),
        }
    }
}

impl AssetLoader for FlowGraphLoader {
    type Asset = FlowGraph;
    type Settings = ();
    type Error = FlowGraphLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .await
            .map_err(|err| FlowGraphLoadError::Io(err.to_string()))?;
        let text = jackdaw_bsn::document_text_from_bytes(&bytes, load_context.path().path())
            .map_err(|err| FlowGraphLoadError::Parse(err.to_string()))?;
        parse_flow_graph(&text, &self.registry.read())
    }

    fn extensions(&self) -> &[&str] {
        &[EXTENSION]
    }
}
