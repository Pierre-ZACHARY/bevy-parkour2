//! Load authored material files as actual Bevy assets. The editor's catalog
//! reads their definitions too, but scene handles resolve via AssetServer.
use std::{any::TypeId, io};

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;

#[derive(TypePath)]
pub struct MaterialLoader {
    registry: AppTypeRegistry,
    server: AssetServer,
}

impl FromWorld for MaterialLoader {
    fn from_world(world: &mut World) -> Self {
        Self {
            registry: world.resource::<AppTypeRegistry>().clone(),
            server: world.resource::<AssetServer>().clone(),
        }
    }
}

impl AssetLoader for MaterialLoader {
    type Asset = StandardMaterial;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        context: &mut LoadContext<'_>,
    ) -> Result<StandardMaterial, io::Error> {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "expected a StandardMaterial BSN asset",
            )
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let text = jackdaw_bsn::document_text_from_bytes(&bytes, context.path().path())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let ast = jackdaw_bsn::parse_bsn_text(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let registry = self.registry.read();
        let (_, value) = ast
            .roots
            .iter()
            .filter_map(|&root| jackdaw_bsn::catalog::asset_value_from_root(&ast, root))
            .find(|(path, _)| path == StandardMaterial::type_path())
            .ok_or_else(invalid)?;
        let assets = jackdaw_bsn::BsnApplyAssets {
            server: &self.server,
            local: None,
        };
        let reflected = jackdaw_bsn::bsn_value_to_reflect(
            &value,
            TypeId::of::<StandardMaterial>(),
            &registry,
            Some(&assets),
        )
        .ok_or_else(invalid)?;
        StandardMaterial::from_reflect(&*reflected).ok_or_else(invalid)
    }

    fn extensions(&self) -> &[&str] {
        &["material.bsn"]
    }
}
