//! bevy-parkour2: Bevy 0.20 project with Avian, Tnua and Jackdaw; a Tnua
//! character walks the Ninja around a Jackdaw scene.
//!
//! The character lives in `crates/character_controller` (headless) and
//! `crates/ninja_character` (its looks). Dioxus CLI (`dx serve --hot-patch`)
//! only hotpatches this top-level crate, not theirs.

#[cfg(feature = "sentry")]
mod bug_report;
mod game;
#[cfg(feature = "perf_ui")]
mod perf_ui;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
mod web;
#[cfg(target_arch = "wasm32")]
mod web_capture;

use bevy::prelude::*;

fn main() -> AppExit {
    #[cfg(feature = "sentry")]
    let _sentry_guard = game_telemetry::init();
    let default_plugins = DefaultPlugins;
    #[cfg(feature = "sentry")]
    let default_plugins = default_plugins.set(bevy::log::LogPlugin {
        custom_layer: |_| Some(game_telemetry::layer()),
        ..default()
    });
    #[cfg(feature = "distribution")]
    let default_plugins = default_plugins.set(AssetPlugin {
        file_path: distribution_assets(),
        ..default()
    });
    #[cfg(target_arch = "wasm32")]
    let default_plugins = default_plugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Bevy Parkour".into(),
            canvas: Some("#game".into()),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    });
    #[cfg(feature = "editor")]
    let default_plugins = jackdaw_runtime::maybe_windowless(default_plugins);

    let mut app = App::new();
    #[cfg(feature = "distribution")]
    app.add_plugins(jackdaw_runtime::JackdawAssetSourcePlugin {
        file_path: distribution_assets(),
        ..default()
    });
    app.add_plugins((default_plugins, game::GamePlugin));
    #[cfg(feature = "sentry")]
    app.add_plugins(bug_report::BugReportPlugin);
    #[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
    app.add_plugins((
        bevy_basisu_loader::BasisuLoaderPlugin,
        jackdaw_runtime::JackdawWebCatalogPlugin,
    ));
    #[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
    app.add_plugins(web::WebDiagnosticsPlugin);
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(web_capture::WebCapturePlugin);
    #[cfg(all(feature = "sentry", target_arch = "wasm32"))]
    game_telemetry::install_panic_hook();
    app.run()
}

/// Distribution assets are relative to the executable/bundle, independent of cwd.
#[cfg(feature = "distribution")]
fn distribution_assets() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        "assets".into()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let exe = std::env::current_exe().expect("executable path");
        let directory = exe.parent().expect("executable directory");
        #[cfg(target_os = "macos")]
        let assets = directory.join("../Resources/assets");
        #[cfg(not(target_os = "macos"))]
        let assets = directory.join("assets");
        assets.to_string_lossy().into_owned()
    }
}
