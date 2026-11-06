//! F8: voluntary report; F9: persistent telemetry on/off (Leafwing).
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use leafwing_input_manager::prelude::*;

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
enum ReportAction {
    ReportBug,
    ToggleReporting,
}
/// TODO(settings): bind this resource to the future settings page.
#[derive(Resource)]
pub struct ReportingSettings {
    pub enabled: bool,
}
#[derive(Resource, Default)]
struct Pending {
    screenshot: Option<Entity>,
    started: f64,
    last_report: Option<f64>,
}
pub struct BugReportPlugin;
impl Plugin for BugReportPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ReportingSettings {
            enabled: game_telemetry::enabled(),
        })
        .init_resource::<Pending>()
        .add_plugins(InputManagerPlugin::<ReportAction>::default())
        .add_systems(Update, (input, snapshot).chain());
        app.world_mut().spawn((
            ActionState::<ReportAction>::default(),
            InputMap::default()
                .with(ReportAction::ReportBug, KeyCode::F8)
                .with(ReportAction::ToggleReporting, KeyCode::F9),
        ));
    }
}
fn input(
    actions: Single<&ActionState<ReportAction>>,
    mut settings: ResMut<ReportingSettings>,
    mut pending: ResMut<Pending>,
    time: Res<Time>,
    mut commands: Commands,
) {
    if actions.just_pressed(&ReportAction::ToggleReporting) {
        settings.enabled = !settings.enabled;
    }
    if settings.is_changed() {
        let saved = game_telemetry::set_enabled(settings.enabled);
        info!(
            enabled = settings.enabled,
            saved, "Bug reporting preference changed"
        );
    }
    if !settings.enabled {
        if let Some(entity) = pending.screenshot.take() {
            commands.entity(entity).try_despawn();
        }
        return;
    }
    let now = time.elapsed_secs_f64();
    if pending.screenshot.is_some() && now - pending.started > 5.0 {
        if let Some(entity) = pending.screenshot.take() {
            commands.entity(entity).try_despawn();
        }
        game_telemetry::report_bug(None); // GPU capture unavailable: still send the report.
    }
    if !actions.just_pressed(&ReportAction::ReportBug)
        || pending.screenshot.is_some()
        || pending.last_report.is_some_and(|last| now - last < 15.0)
    {
        return;
    }
    pending.started = now;
    pending.last_report = Some(now);
    pending.screenshot = Some(
        commands
            .spawn(Screenshot::primary_window())
            .observe(
                |event: On<ScreenshotCaptured>,
                 mut pending: ResMut<Pending>,
                 settings: Res<ReportingSettings>| {
                    if pending.screenshot != Some(event.entity) || !settings.enabled {
                        return;
                    }
                    pending.screenshot = None;
                    let png = event
                        .image
                        .clone()
                        .try_into_dynamic()
                        .ok()
                        .and_then(|image| {
                            let image = image.thumbnail(1280, 720);
                            let mut bytes = std::io::Cursor::new(Vec::new());
                            image.write_to(&mut bytes, image::ImageFormat::Png).ok()?;
                            Some(bytes.into_inner())
                        });
                    game_telemetry::report_bug(png.as_deref());
                },
            )
            .id(),
    );
}
fn snapshot(world: &mut World) {
    if !game_telemetry::enabled() {
        return;
    }
    let now = world.resource::<Time>().elapsed_secs_f64();
    // At most once per second; no component values are reflected indiscriminately.
    let previous = world.get_resource::<SnapshotTime>().map_or(-1.0, |t| t.0);
    let pending = world.resource::<Pending>();
    let manual = pending.screenshot.is_some() && previous < pending.started;
    if !manual && now - previous < 1.0 {
        return;
    }
    world.insert_resource(SnapshotTime(now));
    let count = world.entities().len();
    let mut players = world.query_filtered::<(
        Entity,
        &Transform,
        Option<&avian3d::prelude::LinearVelocity>,
    ), With<character_controller::CharacterController>>();
    let players: Vec<_> = players.iter(world).take(16).map(|(entity,transform,velocity)|
        serde_json::json!({"entity":format!("{entity:?}"),"position":transform.translation.to_array(),
            "rotation":transform.rotation.to_array(),"velocity":velocity.map(|v|v.0.to_array())})).collect();
    let mut query = world.query::<(
        Entity,
        Option<&Transform>,
        Option<&avian3d::prelude::LinearVelocity>,
    )>();
    let entities: Vec<_> = query
        .iter(world)
        .take(128)
        .map(|(entity, transform, velocity)| {
            serde_json::json!({"entity": format!("{entity:?}"),
            "position": transform.map(|t| t.translation.to_array()),
            "rotation": transform.map(|t| t.rotation.to_array()),
            "velocity": velocity.map(|v| v.0.to_array())})
        })
        .collect();
    let data = serde_json::json!({"schema":1,"elapsed_seconds":now,"entity_count":count,
        "truncated":count>128,"players":players,"entities":entities,"snapshot":"transforms and velocities; no arbitrary resources or component contents"});
    game_telemetry::update_ecs(data.to_string());
    if !manual {
        game_telemetry::sync_crash_context();
    }
}
#[derive(Resource)]
struct SnapshotTime(f64);
