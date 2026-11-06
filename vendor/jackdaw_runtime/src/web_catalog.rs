//! An exported HTTP asset index replaces the native directory walk.
//! Documents remain BSB files fetched by the configured Bevy asset reader.
use super::*;

#[derive(Asset, TypePath)]
struct WebDocuments(Vec<(PathBuf, String)>);

#[derive(Default, TypePath)]
struct WebDocumentsLoader;

impl AssetLoader for WebDocumentsLoader {
    type Asset = WebDocuments;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<WebDocuments, Self::Error> {
        use std::io::{Error, ErrorKind};
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let paths: Vec<String> = serde_json::from_slice(&bytes)?;
        let mut documents = Vec::new();
        for path in paths {
            let file = PathBuf::from(&path);
            if !file
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
                || path.contains(['\\', ':'])
                || !jackdaw_bsn::is_document_path(&file)
            {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "unsafe catalog document path",
                ));
            }
            let bytes = context
                .read_asset_bytes(path.clone())
                .await
                .map_err(|e| Error::other(format!("{path}: {e}")))?;
            let document = jackdaw_bsn::document_from_bytes(&bytes, &file)
                .map_err(|e| Error::other(format!("{path}: {e}")))?;
            documents.push((
                file,
                jackdaw_bsn::document_as_text(&document.ast, document.preamble.as_deref()),
            ));
        }
        Ok(WebDocuments(documents))
    }

    fn extensions(&self) -> &[&str] {
        &["jdcatalog"]
    }
}

/// Readiness and any fatal catalog error; scenes wait until the catalog is applied.
#[derive(Resource, Default, Debug)]
pub struct JackdawWebCatalogState {
    pub ready: bool,
    pub error: Option<String>,
    handle: Handle<WebDocuments>,
}

/// Add after `JackdawPlugin` to read the exporter's `jackdaw.jdcatalog` via HTTP.
/// It also works with native readers, so the same path can be regression tested.
pub struct JackdawWebCatalogPlugin;
impl Plugin for JackdawWebCatalogPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<WebDocuments>()
            .init_asset_loader::<WebDocumentsLoader>()
            .init_resource::<JackdawWebCatalogState>()
            .add_systems(
                Startup,
                |server: Res<AssetServer>, mut state: ResMut<JackdawWebCatalogState>| {
                    state.handle = server.load("jackdaw.jdcatalog");
                },
            )
            .add_systems(Update, apply_documents.before(spawn_loaded_scenes));
    }
}

fn apply_documents(world: &mut World) {
    let state = world.resource::<JackdawWebCatalogState>();
    if state.ready || state.error.is_some() {
        return;
    }
    let handle = state.handle.clone();
    if let Some(bevy::asset::LoadState::Failed(error)) =
        world.resource::<AssetServer>().get_load_state(handle.id())
    {
        let error = error.to_string();
        error!("HTTP Jackdaw catalog failed: {error}");
        world.resource_mut::<JackdawWebCatalogState>().error = Some(error);
        return;
    }
    let Some(documents) = world
        .resource_mut::<Assets<WebDocuments>>()
        .remove(handle.id())
    else {
        return;
    };
    let mut skipped = SkippedTypes::default();
    let mut stems = jackdaw_bsn::StemIndex::default();
    for (path, _) in &documents.0 {
        stems.insert(jackdaw_bsn::text_twin(path));
    }
    for (path, text) in &documents.0 {
        if jackdaw_bsn::text_twin(path) == Path::new("catalog.bsn") {
            continue;
        }
        if let Some(handle) = load_asset_text(world, path, text, &mut skipped) {
            let mut catalog = world.resource_mut::<JackdawCatalog>();
            catalog
                .files
                .insert(path.to_string_lossy().into_owned(), handle.clone());
            catalog.files.insert(
                jackdaw_bsn::text_twin(path).to_string_lossy().into_owned(),
                handle.clone(),
            );
            let stem = jackdaw_bsn::path_stem(path);
            if stems.unique(&stem).is_some() {
                catalog.handles.insert(format!("@{stem}"), handle);
            }
        }
    }
    // Match native precedence: inline catalog entries come after files.
    for (path, text) in &documents.0 {
        if jackdaw_bsn::text_twin(path) != Path::new("catalog.bsn") {
            continue;
        }
        match load_bsn_assets(world, text) {
            Ok(entries) => {
                for entry in entries {
                    world
                        .resource_mut::<JackdawCatalog>()
                        .handles
                        .entry(format!("@{}", entry.name))
                        .or_insert(entry.handle);
                }
            }
            Err(error) => {
                let error = format!("{}: {error}", path.display());
                error!("HTTP Jackdaw catalog failed: {error}");
                world.resource_mut::<JackdawWebCatalogState>().error = Some(error);
                return;
            }
        }
    }
    skipped.report();
    world.resource_mut::<JackdawWebCatalogState>().ready = true;
    info!(
        "Applied HTTP Jackdaw catalog ({} documents)",
        documents.0.len()
    );
}
