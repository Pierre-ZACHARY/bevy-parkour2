//! Explicit browser integration fixture; never included in a game package.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    use tracing_subscriber::prelude::*;
    let _guard = game_telemetry::init();
    tracing_subscriber::registry()
        .with(game_telemetry::layer())
        .init();
    game_telemetry::install_panic_hook();
    game_telemetry::update_ecs("{\"schema\":1,\"probe\":true}".into());
    tracing::warn!("browser validation breadcrumb");
    tracing::error!(
        probe = "pulumi-xtask-browser",
        "Bevy Parkour browser validation"
    );
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn panic_probe() {
    panic!("Bevy Parkour browser panic validation");
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn reporting_enabled(enabled: bool) {
    game_telemetry::set_enabled(enabled);
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn manual_probe() {
    game_telemetry::report_bug(Some(&[137, 80, 78, 71]));
}
