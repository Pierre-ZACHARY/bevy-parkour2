//! Bounded diagnostics shared by tracing, the main thread and the crash reporter.
use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

static ENABLED: AtomicBool = AtomicBool::new(true);
#[cfg(test)]
pub(super) fn set_test_enabled(value: bool) {
    ENABLED.store(value, Ordering::Release);
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
const LOG_LINES: usize = 128;
const MAX_TEXT: usize = 256 * 1024;
pub const MAX_SCREENSHOT: usize = 2 * 1024 * 1024;
#[derive(Default)]
struct State {
    ecs: String,
    logs: VecDeque<String>,
}
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(Mutex::default)
}

#[cfg(not(target_arch = "wasm32"))]
fn preference_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("PARKOUR_SETTINGS_DIRECTORY") {
        return std::path::PathBuf::from(path).join("telemetry-enabled");
    }
    // No user name, home directory or environment is included in reports.
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|p| p.join(".config"))
                .unwrap_or_else(std::env::temp_dir)
        });
    base.join("bevy-parkour2").join("telemetry-enabled")
}
#[cfg(not(target_arch = "wasm32"))]
pub fn persisted_enabled() -> bool {
    std::fs::read_to_string(preference_path())
        .map(|s| s.trim() != "off")
        .unwrap_or(true)
}
#[cfg(target_arch = "wasm32")]
pub fn persisted_enabled() -> bool {
    super::browser::preference()
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}
pub fn initialize() {
    ENABLED.store(persisted_enabled(), Ordering::Release);
}
/// TODO(settings): expose this switch in the settings page. F9 already calls it.
/// Returns false if persistence failed; the current process still applies the switch.
pub fn set_enabled(value: bool) -> bool {
    ENABLED.store(value, Ordering::Release);
    if !value && let Ok(mut state) = state().lock() {
        *state = State::default();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = preference_path();
        let saved = path
            .parent()
            .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
            && std::fs::write(path, if value { "on" } else { "off" }).is_ok();
        sentry::with_integration(
            |integration: &sentry::integrations::minidump::MinidumpIntegration, _| {
                integration.set_extra("telemetry.enabled".into(), Some(value.into()));
            },
        );
        saved
    }
    #[cfg(target_arch = "wasm32")]
    {
        super::browser::set_preference(value)
    }
}
pub fn update_ecs(json: String) {
    if !enabled() || json.len() > MAX_TEXT {
        return;
    }
    if let Ok(mut state) = state().lock() {
        state.ecs = json;
    }
}
pub(super) fn record(line: String) {
    if !enabled() {
        return;
    }
    let line: String = line.chars().take(1024).collect();
    if let Ok(mut state) = state().lock() {
        if state.logs.len() == LOG_LINES {
            state.logs.pop_front();
        }
        state.logs.push_back(line);
    }
}
pub(super) fn snapshot() -> (String, String) {
    state()
        .try_lock()
        .map(|s| {
            (
                s.ecs.clone(),
                s.logs.iter().cloned().collect::<Vec<_>>().join("\n"),
            )
        })
        .unwrap_or_else(|_| {
            (
                "{\"unavailable\":\"diagnostics busy\"}".into(),
                String::new(),
            )
        })
}
/// Keeps bounded context in the separate native crash reporter. No GPU/world access on crash.
pub fn sync_crash_context() {
    #[cfg(not(target_arch = "wasm32"))]
    if enabled() {
        let (ecs, logs) = snapshot();
        sentry::with_integration(
            |integration: &sentry::integrations::minidump::MinidumpIntegration, _| {
                integration.set_extra("ecs.snapshot".into(), Some(ecs.into()));
                integration.set_extra("recent.logs".into(), Some(logs.into()));
            },
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_bounded() {
        for _ in 0..256 {
            record("x".repeat(3000));
        }
        update_ecs("x".repeat(MAX_TEXT + 1));
        let (ecs, logs) = snapshot();
        assert!(ecs.len() <= MAX_TEXT);
        assert!(logs.lines().count() <= LOG_LINES);
        assert!(logs.len() <= LOG_LINES * 1025);
    }
}
