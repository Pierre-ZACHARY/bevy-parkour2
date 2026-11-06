//! Browser diagnostics are opt-in (`?diagnostics`), never a substitute for pixels.
use bevy::prelude::*;
use serde_json::json;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(
    inline_js = "export function diagnostics_enabled() { return new URLSearchParams(location.search).has('diagnostics'); } export function publish_diagnostics(json) { const d=JSON.parse(json); if(diagnostics_enabled()) window.__parkour=d; window.dispatchEvent(new CustomEvent('parkour-status',{detail:d})); }"
)]
extern "C" {
    fn publish_diagnostics(json: &str);
}

pub struct WebDiagnosticsPlugin;
impl Plugin for WebDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, diagnostics);
    }
}

fn diagnostics(
    time: Res<Time>,
    catalog: Res<jackdaw_runtime::JackdawWebCatalogState>,
    players: Query<&Transform, With<character_controller::CharacterController>>,
    (meshes, cameras): (Query<&Mesh3d>, Query<&Camera3d>),
    runners: Query<&bevy_noodle_jackdaw::FlowGraphRunner>,
    graphs: Res<Assets<bevy_noodle_jackdaw::FlowGraph>>,
    mut frames: Local<u64>,
) {
    *frames += 1;
    // Keep this cheap while retaining a monotonic frame counter and real state.
    if !frames.is_multiple_of(15) {
        return;
    }
    let positions: Vec<_> = players.iter().map(|t| t.translation.to_array()).collect();
    let graph_ready = runners.iter().any(|r| graphs.contains(r.graph.id()));
    publish_diagnostics(
        &json!({
            "frames": *frames, "elapsed_seconds": time.elapsed_secs_f64(),
            "delta_seconds": time.delta_secs_f64(), "catalog_ready": catalog.ready,
            "catalog_error": catalog.error, "players": positions,
            "meshes": meshes.iter().count(), "cameras": cameras.iter().count(),
            "movement_graph_ready": graph_ready,
        })
        .to_string(),
    );
}
