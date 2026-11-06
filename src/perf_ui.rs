//! Development overlay (iyes_perf_ui, F3): frame rate and time, entities,
//! the process's CPU and RAM, and the player's speed. The game-specific
//! entry lives in the binary so dx can hotpatch it, like the other game
//! systems.

use avian3d::prelude::LinearVelocity;
use bevy::{
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    ecs::system::SystemParam,
    prelude::*,
};
use iyes_perf_ui::{PerfUiSet, entry::PerfUiEntry, prelude::*};

use character_controller::{CharacterController, JOG_SPEED, SPRINT_SPEED};

pub struct GamePerfUiPlugin;

impl Plugin for GamePerfUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        if !app.is_plugin_added::<EntityCountDiagnosticsPlugin>() {
            app.add_plugins(EntityCountDiagnosticsPlugin::default());
        }
        if !app.is_plugin_added::<SystemInformationDiagnosticsPlugin>() {
            app.add_plugins(SystemInformationDiagnosticsPlugin);
        }
        app.add_plugins(PerfUiPlugin)
            .add_perf_ui_simple_entry::<PerfUiPlayerSpeed>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (toggle_overlay, bind_camera)
                    .before(PerfUiSet::Setup)
                    .before(PerfUiSet::Update),
            );
    }
}

#[derive(Component)]
struct GamePerfUi;

fn spawn_overlay(mut commands: Commands) {
    // Deliberately not a child of the Jackdaw scene: scene reloads replace its
    // camera and player, while this root and its visibility survive.
    commands.spawn((
        GamePerfUi,
        Visibility::Visible,
        PerfUiRoot {
            position: PerfUiPosition::TopRight,
            margin: 12.0,
            padding: 4.0,
            fontsize_label: 14.0,
            fontsize_value: 14.0,
            background_color: Color::srgba(0.02, 0.02, 0.03, 0.8),
            values_col_width: 100.0,
            ..default()
        },
        PerfUiEntryFPS {
            sort_key: 0,
            ..default()
        },
        PerfUiEntryFrameTime {
            sort_key: 1,
            ..default()
        },
        PerfUiEntryEntityCount {
            sort_key: 2,
            ..default()
        },
        PerfUiEntryCpuUsage {
            label: "CPU (process)".into(),
            sort_key: 3,
            ..default()
        },
        PerfUiEntryMemUsage {
            label: "RAM (process)".into(),
            sort_key: 4,
            ..default()
        },
        PerfUiPlayerSpeed {
            sort_key: 5,
            ..default()
        },
    ));
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlays: Query<&mut Visibility, With<GamePerfUi>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        for mut visibility in &mut overlays {
            *visibility = if *visibility == Visibility::Hidden {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn bind_camera(
    mut commands: Commands,
    cameras: Query<(Entity, &Camera), With<Camera3d>>,
    overlays: Query<(Entity, Option<&UiTargetCamera>), With<GamePerfUi>>,
) {
    let mut active = cameras.iter().filter(|(_, camera)| camera.is_active);
    let camera = active.next().map(|(entity, _)| entity);
    // An ambiguous camera is not a reason to pick an arbitrary render target.
    let camera = camera.filter(|_| active.next().is_none());
    for (entity, target) in &overlays {
        if target.map(|target| target.0) == camera {
            continue;
        }
        if let Some(camera) = camera {
            commands.entity(entity).insert(UiTargetCamera(camera));
        } else {
            commands.entity(entity).remove::<UiTargetCamera>();
        }
    }
}

/// Actual 3D body speed, with the formatting and highlighting hooks of the
/// upstream `examples/custom.rs` (including configurable units and sort
/// order): green standing, yellow jogging, red sprinting, highlighted past
/// the sprint.
#[derive(Component)]
#[require(PerfUiRoot)]
pub struct PerfUiPlayerSpeed {
    pub label: String,
    pub display_units: bool,
    pub threshold_highlight: Option<f32>,
    pub color_gradient: ColorGradient,
    pub digits: u8,
    pub precision: u8,
    pub sort_key: i32,
}

impl Default for PerfUiPlayerSpeed {
    fn default() -> Self {
        Self {
            label: String::new(),
            display_units: true,
            threshold_highlight: Some(SPRINT_SPEED * 1.1),
            color_gradient: ColorGradient::new_preset_gyr(0.0, JOG_SPEED, SPRINT_SPEED).unwrap(),
            digits: 2,
            precision: 2,
            sort_key: iyes_perf_ui::utils::next_sort_key(),
        }
    }
}

impl PerfUiEntry for PerfUiPlayerSpeed {
    type Value = f32;
    type SystemParam = Query<'static, 'static, &'static LinearVelocity, With<CharacterController>>;

    fn label(&self) -> &str {
        if self.label.is_empty() {
            "Speed"
        } else {
            &self.label
        }
    }

    fn sort_key(&self) -> i32 {
        self.sort_key
    }

    fn update_value(
        &self,
        players: &mut <Self::SystemParam as SystemParam>::Item<'_, '_>,
    ) -> Option<Self::Value> {
        let speed = players.single().ok()?.0.length();
        speed.is_finite().then_some(speed)
    }

    fn format_value(&self, value: &Self::Value) -> String {
        let mut text = iyes_perf_ui::utils::format_pretty_float(
            self.digits,
            self.precision,
            f64::from(*value),
        );
        if self.display_units {
            text.push_str(" m/s");
        }
        text
    }

    fn value_color(&self, value: &Self::Value) -> Option<Color> {
        self.color_gradient.get_color_for_value(*value)
    }

    fn value_highlight(&self, value: &Self::Value) -> bool {
        self.threshold_highlight
            .is_some_and(|threshold| *value > threshold)
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemState;

    use super::*;

    fn speed(world: &mut World) -> Option<f32> {
        let mut state = SystemState::<<PerfUiPlayerSpeed as PerfUiEntry>::SystemParam>::new(world);
        PerfUiPlayerSpeed::default().update_value(&mut state.get_mut(world).unwrap())
    }

    #[test]
    fn speed_uses_real_velocity_including_vertical_motion() {
        let mut world = World::new();
        let player = world
            .spawn((
                CharacterController::default(),
                LinearVelocity(Vec3::new(3.0, 4.0, 0.0)),
            ))
            .id();
        assert_eq!(speed(&mut world), Some(5.0));
        world.entity_mut(player).insert(LinearVelocity::ZERO);
        assert_eq!(speed(&mut world), Some(0.0));
    }

    #[test]
    fn unavailable_or_ambiguous_speed_is_not_zero() {
        let mut world = World::new();
        assert_eq!(speed(&mut world), None);
        world.spawn((CharacterController::default(), LinearVelocity::ZERO));
        world.spawn((CharacterController::default(), LinearVelocity::ZERO));
        assert_eq!(speed(&mut world), None);
    }

    #[test]
    fn speed_format_and_highlight_are_configurable() {
        let mut entry = PerfUiPlayerSpeed::default();
        assert_eq!(entry.label(), "Speed");
        assert_eq!(entry.format_value(&4.0).trim(), "4.00 m/s");
        assert!(!entry.value_highlight(&SPRINT_SPEED));
        assert!(entry.value_highlight(&(SPRINT_SPEED * 1.2)));
        assert_ne!(entry.value_color(&0.0), entry.value_color(&6.0));
        entry.label = "Velocity".into();
        entry.display_units = false;
        entry.precision = 1;
        entry.threshold_highlight = None;
        assert_eq!(entry.label(), "Velocity");
        assert_eq!(entry.format_value(&4.0).trim(), "4.0");
        assert!(!entry.value_highlight(&10.0));
    }

    #[test]
    fn toggling_preserves_one_root_and_rebinds_after_camera_reload() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, bind_camera));
        app.update();
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<GamePerfUi>>()
            .single(app.world())
            .unwrap();
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        assert_eq!(app.world().get::<UiTargetCamera>(root).unwrap().0, camera);
        for expected in [Visibility::Hidden, Visibility::Visible, Visibility::Hidden] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F3);
            app.update();
            assert_eq!(*app.world().get::<Visibility>(root).unwrap(), expected);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset(KeyCode::F3);
        }
        app.world_mut().despawn(camera);
        app.update();
        assert!(app.world().get::<UiTargetCamera>(root).is_none());
        let replacement = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        assert_eq!(
            app.world().get::<UiTargetCamera>(root).unwrap().0,
            replacement
        );
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<GamePerfUi>>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
    }

    /// Exercises the real game scene, renderer and input without an OS window.
    /// Run explicitly with `cargo test renders_overlay_offscreen -- --ignored`.
    #[test]
    #[ignore = "requires a GPU; saves rendered evidence under target/perf-ui-smoke"]
    fn renders_overlay_offscreen() {
        use std::{
            path::{Path, PathBuf},
            time::Duration,
        };

        use bevy::{
            camera::RenderTarget,
            render::{
                RenderPlugin,
                render_resource::{PollType, TextureFormat, TextureUsages},
                renderer::RenderDevice,
                view::screenshot::{Screenshot, save_to_disk},
            },
            time::TimeUpdateStrategy,
            window::ExitCondition,
            winit::WinitPlugin,
        };
        use leafwing_input_manager::prelude::*;

        fn tick(app: &mut App) {
            app.update();
            app.world()
                .resource::<RenderDevice>()
                .wgpu_device()
                .poll(PollType::Wait {
                    submission_index: None,
                    timeout: Some(Duration::from_secs(30)),
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }

        fn capture(app: &mut App, target: &Handle<Image>, path: PathBuf) {
            app.world_mut()
                .spawn(Screenshot::image(target.clone()))
                .observe(save_to_disk(path.clone()));
            for _ in 0..120 {
                tick(app);
                if path.is_file() {
                    return;
                }
            }
            panic!("screenshot was not saved to {}", path.display());
        }

        // Retarget asynchronously loaded scene cameras, as windowless PIE does.
        #[derive(Resource)]
        struct Target(Handle<Image>);
        fn retarget(target: Res<Target>, mut cameras: Query<&mut RenderTarget, With<Camera3d>>) {
            for mut camera in &mut cameras {
                if matches!(*camera, RenderTarget::Window(_)) {
                    *camera = RenderTarget::Image(target.0.clone().into());
                }
            }
        }

        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(crate::game::GamePlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .add_systems(PostUpdate, retarget);
        let mut image = Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(Target(target.clone()));
        app.finish();
        app.cleanup();

        for _ in 0..500 {
            tick(&mut app);
            if app
                .world_mut()
                .query_filtered::<Entity, With<CharacterController>>()
                .iter(app.world())
                .count()
                == 1
                // The player's camera; the fly camera starts off.
                && app
                    .world_mut()
                    .query_filtered::<&Camera, With<Camera3d>>()
                    .iter(app.world())
                    .filter(|camera| camera.is_active)
                    .count()
                    == 1
            {
                break;
            }
        }
        for _ in 0..30 {
            tick(&mut app);
        }
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<GamePerfUi>>()
            .single(app.world())
            .unwrap();
        let original_camera = app.world().get::<UiTargetCamera>(root).unwrap().0;
        let run = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/perf-ui-smoke")
            .join(run.to_string());
        std::fs::create_dir_all(&output).unwrap();
        capture(&mut app, &target, output.join("idle.png"));

        KeyCode::KeyW.press(app.world_mut());
        for _ in 0..45 {
            tick(&mut app);
        }
        assert!(speed(app.world_mut()).unwrap() > 1.0);
        capture(&mut app, &target, output.join("walking.png"));
        KeyCode::KeyW.release(app.world_mut());
        KeyCode::F3.press(app.world_mut());
        tick(&mut app);
        KeyCode::F3.release(app.world_mut());
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        capture(&mut app, &target, output.join("hidden.png"));

        app.world().resource::<AssetServer>().reload("scene.bsn");
        let mut replaced = false;
        for _ in 0..500 {
            tick(&mut app);
            if app
                .world()
                .get::<UiTargetCamera>(root)
                .is_some_and(|camera| camera.0 != original_camera)
            {
                replaced = true;
                break;
            }
        }
        assert!(replaced, "scene reload did not replace the camera");
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
        KeyCode::F3.press(app.world_mut());
        tick(&mut app);
        KeyCode::F3.release(app.world_mut());
        for _ in 0..30 {
            tick(&mut app);
        }
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<GamePerfUi>>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Visible
        );
        capture(&mut app, &target, output.join("reloaded.png"));
    }
}
