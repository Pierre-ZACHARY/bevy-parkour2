include!(concat!(env!("OUT_DIR"), "/sentry_config.rs"));
#[path = "diagnostics.rs"]
mod diagnostics;
pub use diagnostics::{enabled, set_enabled, sync_crash_context, update_ecs};
use tracing_subscriber::{Layer, Registry};

#[cfg(not(target_arch = "wasm32"))]
pub fn init() -> sentry::ClientInitGuard {
    diagnostics::initialize();
    let guard = sentry::init(
        options().add_integration(
            sentry::integrations::minidump::MinidumpIntegration::new()
                .crashes_dir(
                    std::env::var_os("PARKOUR_CRASH_DIRECTORY")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| std::env::temp_dir().join("bevy-parkour2-crashes")),
                )
                .on_process(|command| {
                    for name in [
                        "SENTRY_AUTH_TOKEN",
                        "SONAR_TOKEN",
                        "PULUMI_ACCESS_TOKEN",
                        "AWS_ACCESS_KEY_ID",
                        "AWS_SECRET_ACCESS_KEY",
                        "OVH_APPLICATION_KEY",
                        "OVH_APPLICATION_SECRET",
                        "OVH_CONSUMER_KEY",
                    ] {
                        command.env_remove(name);
                    }
                }),
        ),
    );
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sentry::with_scope(|scope| add_attachments(scope, None), || previous(info));
    }));
    guard
}
#[cfg(not(target_arch = "wasm32"))]
fn options() -> sentry::ClientOptions {
    sentry::ClientOptions::new()
        .dsn(DSN)
        .release(RELEASE)
        .environment(ENVIRONMENT)
        .send_default_pii(false)
        .debug(std::env::var_os("PARKOUR_SENTRY_DEBUG").is_some())
        .attach_stacktrace(true)
        .traces_sample_rate(0.0)
        .before_send(|event| {
            (enabled()
                && diagnostics::persisted_enabled()
                && event
                    .extra
                    .get("telemetry.enabled")
                    .and_then(|v| v.as_bool())
                    != Some(false))
            .then_some(event)
        })
}
#[cfg(not(target_arch = "wasm32"))]
fn add_attachments(scope: &mut sentry::Scope, png: Option<&[u8]>) {
    if !enabled() {
        return;
    }
    let (ecs, logs) = diagnostics::snapshot();
    for (filename, content_type, buffer) in [
        ("ecs.json", "application/json", ecs.into_bytes()),
        ("recent.log", "text/plain", logs.into_bytes()),
        (
            "thread.txt",
            "text/plain",
            format!(
                "current thread: {:?}\n{}",
                std::thread::current().name(),
                std::backtrace::Backtrace::force_capture()
            )
            .into_bytes(),
        ),
    ] {
        if !buffer.is_empty() {
            scope.add_attachment(sentry::protocol::Attachment {
                filename: filename.into(),
                content_type: Some(content_type.into()),
                buffer,
                ..Default::default()
            });
        }
    }
    if let Some(png) = png.filter(|p| p.len() <= diagnostics::MAX_SCREENSHOT) {
        scope.add_attachment(sentry::protocol::Attachment {
            filename: "screenshot.png".into(),
            content_type: Some("image/png".into()),
            buffer: png.to_vec(),
            ..Default::default()
        });
    }
}
pub fn report_bug(png: Option<&[u8]>) {
    report("Player bug report (F8)", png);
}
fn report(message: &str, png: Option<&[u8]>) {
    if !enabled() {
        return;
    }
    let png = png.filter(|p| p.len() <= diagnostics::MAX_SCREENSHOT);
    #[cfg(not(target_arch = "wasm32"))]
    sentry::with_scope(
        |scope| add_attachments(scope, png),
        || {
            sentry::capture_message(message, sentry::Level::Error);
        },
    );
    #[cfg(target_arch = "wasm32")]
    {
        let (ecs, logs) = diagnostics::snapshot();
        browser::capture(message, &ecs, &logs, png.unwrap_or_default());
    }
}
#[derive(Default)]
struct Fields(String);
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}
struct DiagnosticLayer;
impl<S: tracing::Subscriber> Layer<S> for DiagnosticLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if !enabled() {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        diagnostics::record(format!(
            "{} {} {}",
            event.metadata().level(),
            event.metadata().target(),
            fields.0
        ));
        if *event.metadata().level() == tracing::Level::ERROR {
            report(&fields.0, None);
        }
    }
}
pub fn layer() -> Box<dyn Layer<Registry> + Send + Sync> {
    Box::new(DiagnosticLayer)
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(
        inline_js = "export function preference() { try { return localStorage.getItem('parkour.telemetry') !== 'off'; } catch { return true; } } export function set_preference(enabled) { globalThis.__parkourTelemetryEnabled=enabled; try {localStorage.setItem('parkour.telemetry',enabled?'on':'off');return true;} catch {return false;} } export function capture(message, ecs, logs, png) { if (!globalThis.Sentry || globalThis.__parkourTelemetryEnabled === false) return; Sentry.withScope(scope => { scope.addAttachment({filename:'ecs.json',contentType:'application/json',data:ecs});scope.addAttachment({filename:'recent.log',contentType:'text/plain',data:logs}); if(png.length && png.length<=2097152) scope.addAttachment({filename:'screenshot.png',contentType:'image/png',data:png.slice()}); Sentry.captureException(Object.assign(new Error(message),{name:'RustError'})); }); }"
    )]
    extern "C" {
        pub fn preference() -> bool;
        pub fn set_preference(enabled: bool) -> bool;
        pub fn capture(message: &str, ecs: &str, logs: &str, png: &[u8]);
    }
}
#[cfg(target_arch = "wasm32")]
pub struct BrowserGuard;
#[cfg(target_arch = "wasm32")]
pub fn init() -> BrowserGuard {
    diagnostics::initialize();
    BrowserGuard
}
#[cfg(target_arch = "wasm32")]
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(&info.to_string(), None);
        previous(info);
    }));
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::prelude::*;
    #[test]
    fn disabled_reports_are_dropped_and_attachments_are_scoped() {
        use sentry::protocol::{Envelope, EnvelopeItem};
        struct Recorder(Arc<Mutex<Vec<Envelope>>>);
        impl sentry::Transport for Recorder {
            fn send_envelope(&self, envelope: Envelope) {
                self.0.lock().unwrap().push(envelope);
            }
        }
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let _guard = sentry::init(options().transport(Arc::new(Recorder(captured))));
        update_ecs("{\"entities\":[]}".into());
        tracing::subscriber::with_default(Registry::default().with(layer()), || {
            tracing::info!("recent context");
            tracing::error!("telemetry error validation");
            report_bug(Some(&[1, 2, 3]));
            report_bug(Some(&vec![0; diagnostics::MAX_SCREENSHOT + 1]));
            diagnostics::set_test_enabled(false);
            tracing::error!("must be dropped");
            report_bug(None);
            sentry::capture_message("SDK direct call also dropped", sentry::Level::Error);
            diagnostics::set_test_enabled(true);
        });
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 3);
        let names = |envelope: &Envelope| {
            envelope
                .items()
                .filter_map(|i| {
                    if let EnvelopeItem::Attachment(a) = i {
                        Some(a.filename.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        assert!(names(&events[0]).contains(&"ecs.json".into()));
        assert!(names(&events[0]).contains(&"recent.log".into()));
        assert!(!names(&events[0]).contains(&"screenshot.png".into()));
        assert!(names(&events[1]).contains(&"screenshot.png".into()));
        assert!(!names(&events[2]).contains(&"screenshot.png".into()));
        for item in events[0].items() {
            if let EnvelopeItem::Event(event) = item {
                assert_eq!(event.release.as_deref(), Some(RELEASE));
                assert!(event.user.is_none());
            }
        }
    }
}
