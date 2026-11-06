# Local runtime patch

Source: jackdaw_runtime 0.20.1-rc.1, crates.io (upstream 5a4492d61900f92eb4f867f3f90abf1a1f718831).
The original licenses and tests are retained. Registry files are never modified.

`texture-import` separates the legacy Basis encoder/reader from rendering.
It remains a default feature for backwards compatibility. Browser consumers select
`render` explicitly without defaults. Native application builds retain the importer.
The public scene APIs are retained. Native catalog discovery is unchanged; Web
exports can now opt into JackdawWebCatalogPlugin as described below.

`basisu` is opt-in and selects BasisuLoaderSettings for `.basisu.ktx2`
material slots. It preserves sRGB/linear intent and render-world asset usage.
The application installs BasisuLoaderPlugin separately. Both features can coexist
on native; browser consumers must leave texture-import disabled.

The native catalog walk no longer runs on WASM. Both native-walk timing and live
LOD timing use bevy::platform::time::Instant, avoiding std clock panics in browsers.

JackdawWebCatalogPlugin consumes the exporter's `jackdaw.jdcatalog` index through
Bevy's asset reader, decodes the indexed BSB documents and applies them on the main
world before scenes may spawn. The existing reflection loader, path aliases,
unambiguous stem aliases and file-over-inline catalog precedence are retained.
An explicit ready/error resource keeps missing or corrupt exports from silently
spawning an incomplete scene. The game uses it only on WASM; native keeps its walk.

This enables the current game's external ground material and movement graph.
General filesystem prefab resolution and terrain/navmesh sidecar loading are not
ported by this patch; the current game does not author those resources. The Basis
prototype's inline-material validation remains a separate test from the full game.

