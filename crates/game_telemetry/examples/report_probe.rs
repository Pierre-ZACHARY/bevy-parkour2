//! Explicit integration probe, never run by the game or packaging pipeline.
#[cfg(feature = "official")]
fn main() {
    use tracing_subscriber::prelude::*;
    let _guard = game_telemetry::init();
    tracing_subscriber::registry()
        .with(game_telemetry::layer())
        .init();
    if std::env::args().any(|arg| arg == "--disabled") {
        game_telemetry::set_enabled(false);
    }
    game_telemetry::update_ecs("{\"schema\":1,\"probe\":true,\"entities\":[]}".into());
    game_telemetry::sync_crash_context();
    if std::env::args().any(|arg| arg == "--crash") {
        // Windows abort uses fast-fail and bypasses OS exception handlers. An
        // illegal instruction exercises the actual unhandled-exception path.
        #[cfg(target_arch = "x86_64")]
        unsafe {
            std::arch::asm!("ud2", options(noreturn));
        }
        #[cfg(not(target_arch = "x86_64"))]
        std::process::abort();
    }
    tracing::error!(
        probe = "pulumi-xtask-integration",
        "Bevy Parkour Sentry integration validation"
    );
}
#[cfg(not(feature = "official"))]
fn main() {
    eprintln!(
        "report_probe requires --features official and explicit build-time Sentry configuration"
    );
}
