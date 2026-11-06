#[cfg(target_arch = "wasm32")]
#[path = "../../../src/web_capture.rs"]
mod web_capture;
use bevy::{
    asset::{LoadState, RenderAssetUsages},
    image::ImageLoaderSettings,
    platform::time::Instant,
    prelude::*,
};
use bevy_basisu_loader::{BasisuLoaderPlugin, BasisuLoaderSettings};
use serde_json::{Value, json};

#[derive(Component, Reflect, Clone, Default)]
#[reflect(Component, Default)]
struct Painted {
    material: Handle<StandardMaterial>,
}
#[derive(Resource)]
struct Probe {
    handles: Vec<(String, Handle<Image>)>,
    start: Instant,
    done: bool,
    mode: String,
    negative: bool,
    #[cfg(feature = "jackdaw")]
    scene: Handle<jackdaw_runtime::JackdawScene>,
}
fn main() -> AppExit {
    let assets = asset_path();
    let mut app = App::new();
    #[cfg(feature = "jackdaw")]
    app.add_plugins(jackdaw_runtime::JackdawAssetSourcePlugin {
        file_path: assets.clone(),
        ..default()
    });
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: assets,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Basis WebGPU prototype".into(),
                    resolution: (1000, 800).into(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::log::LogPlugin {
                filter: "info,bevy_basisu_loader=debug".into(),
                ..default()
            }),
    )
    .add_plugins(BasisuLoaderPlugin)
    .insert_resource(ClearColor(Color::srgb(0.16, 0.18, 0.22)))
    .register_type::<Painted>();
    #[cfg(feature = "jackdaw")]
    app.add_plugins(jackdaw_runtime::JackdawPlugin);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, capture_native);
    #[cfg(target_arch = "wasm32")]
    app.add_plugins(web_capture::WebCapturePlugin);
    app.add_systems(Startup, setup)
        .add_systems(Update, (attach_scene_mesh, check))
        .run()
}
fn asset_path() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        "assets".into()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("BASISU_ASSETS").unwrap_or_else(|_| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("assets")
                .to_string_lossy()
                .into_owned()
        })
    }
}
fn mode() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .unwrap()
            .location()
            .search()
            .unwrap_or_default()
            .trim_start_matches('?')
            .to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("BASISU_CASE").unwrap_or_default()
    }
}
fn setup(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0., 0., 7.).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let mode = mode();
    let negative = mode == "missing" || mode == "corrupt";
    let paths = if negative {
        vec![format!("{mode}.basisu.ktx2")]
    } else {
        vec![
            "color.png".into(),
            "color.basisu.ktx2".into(),
            "linear.png".into(),
            "linear.basisu.ktx2".into(),
        ]
    };
    let mut handles = vec![];
    for (i, path) in paths.iter().enumerate() {
        let handle: Handle<Image> = if path.ends_with(".png") {
            let linear = path.starts_with("linear");
            server
                .load_builder()
                .with_settings(move |s: &mut ImageLoaderSettings| {
                    s.is_srgb = !linear;
                    s.asset_usage = RenderAssetUsages::all();
                })
                .load(path.clone())
        } else {
            server
                .load_builder()
                .with_settings(|s: &mut BasisuLoaderSettings| {
                    s.asset_usage = RenderAssetUsages::all();
                })
                .load(path.clone())
        };
        commands.spawn((
            Mesh3d(meshes.add(Rectangle::new(1.8, 1.8))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(handle.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
            Transform::from_xyz(
                if i.is_multiple_of(2) { -1.05 } else { 1.05 },
                if i < 2 { 1.05 } else { -1.05 },
                0.,
            ),
        ));
        handles.push((path.clone(), handle));
    }
    #[cfg(feature = "jackdaw")]
    let scene = server.load("scene.bsn");
    #[cfg(feature = "jackdaw")]
    commands.spawn(jackdaw_runtime::JackdawSceneRoot(scene.clone()));
    commands.insert_resource(Probe {
        handles,
        start: Instant::now(),
        done: false,
        mode,
        negative,
        #[cfg(feature = "jackdaw")]
        scene,
    });
}
fn attach_scene_mesh(
    mut commands: Commands,
    query: Query<(Entity, &Painted), Without<Mesh3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    for (entity, painted) in &query {
        commands.entity(entity).insert((
            Mesh3d(meshes.add(Rectangle::new(0.75, 0.75))),
            MeshMaterial3d(painted.material.clone()),
            Transform::from_xyz(2.7, 0., 0.),
        ));
    }
}
fn check(
    mut commands: Commands,
    mut p: ResMut<Probe>,
    server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    materials: Res<Assets<StandardMaterial>>,
    painted: Query<&Painted>,
    mut exit: MessageWriter<AppExit>,
) {
    if p.done {
        return;
    }
    let mut rows = vec![];
    let mut pending = false;
    let mut failed = false;
    for (path, h) in &p.handles {
        match server.get_load_state(h.id()) {
            Some(LoadState::Failed(e)) => {
                rows.push(json!({"path":path,"status":"FAIL","error":e.to_string()}));
                failed = true;
            }
            Some(LoadState::Loaded) => {
                if let Some(i) = images.get(h) {
                    let d = &i.texture_descriptor;
                    let compressed = d.format.is_compressed();
                    rows.push(json!({"path":path,"status":"PASS","format":format!("{:?}",d.format),"gpu_compressed":compressed,"rgba_fallback":path.ends_with("ktx2")&&!compressed,"srgb":d.format.is_srgb(),"mips":d.mip_level_count,"width":d.size.width,"height":d.size.height,"gpu_payload_bytes":i.data.as_ref().map(Vec::len)}));
                    if d.size.width != 256
                        || d.size.height != 256
                        || d.format.is_srgb() == path.starts_with("linear")
                        || (path.ends_with("ktx2") && d.mip_level_count != 9)
                    {
                        failed = true;
                    }
                } else {
                    pending = true;
                }
            }
            _ => {
                pending = true;
            }
        }
    }
    let mut jackdaw = json!({"status":"NOT_ENABLED"});
    if cfg!(feature = "jackdaw") && !p.negative {
        if let Some(panel) = painted.iter().next() {
            if let Some(m) = materials.get(&panel.material) {
                if let Some(i) = m.base_color_texture.as_ref().and_then(|h| images.get(h)) {
                    jackdaw = json!({"status":"PASS","material_texture_format":format!("{:?}",i.texture_descriptor.format),"source":"scene.bsb; inline material; external catalog not validated"});
                } else {
                    pending = true;
                }
            } else {
                pending = true;
            }
        } else {
            pending = true;
        }
        #[cfg(feature = "jackdaw")]
        if let Some(LoadState::Failed(e)) = server.get_load_state(p.scene.id()) {
            failed = true;
            pending = false;
            jackdaw = json!({"status":"FAIL","error":e.to_string()});
        }
    }
    let elapsed = p.start.elapsed().as_secs_f64();
    if pending && elapsed < 60. && !failed {
        return;
    }
    let status = if pending {
        "TIMEOUT"
    } else if failed {
        "FAIL"
    } else {
        "PASS"
    };
    let result = json!({"status":status,"case":p.mode,"expected_failure":p.negative,"load_and_transcode_ms":elapsed*1000.,"textures":rows,"jackdaw":jackdaw,"visual_validation":"manual; asset loading does not prove rendered output"});
    publish(&result);
    p.done = true;
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("BASISU_EXIT").is_some() {
        if status == "PASS" {
            commands.insert_resource(NativeCapture {
                start: Instant::now(),
                requested: false,
            });
        } else {
            exit.write(AppExit::error());
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (&mut exit, &mut commands);
}
fn publish(value: &Value) {
    let text = serde_json::to_string_pretty(value).unwrap();
    info!("BASISU_RESULT {text}");
    #[cfg(target_arch = "wasm32")]
    if let Some(e) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("results"))
    {
        e.set_text_content(Some(&text));
        e.set_attribute("data-status", value["status"].as_str().unwrap())
            .unwrap();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = std::env::var_os("BASISU_REPORT") {
        std::fs::write(path, text).expect("write runtime result");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct NativeCapture {
    start: Instant,
    requested: bool,
}
#[cfg(not(target_arch = "wasm32"))]
fn capture_native(mut commands: Commands, capture: Option<ResMut<NativeCapture>>) {
    let Some(mut capture) = capture else {
        return;
    };
    if capture.requested || capture.start.elapsed().as_secs_f64() < 3. {
        return;
    }
    capture.requested = true;
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(
            |event: On<bevy::render::view::screenshot::ScreenshotCaptured>,
             mut exit: MessageWriter<AppExit>| {
                let report = std::path::PathBuf::from(
                    std::env::var_os("BASISU_REPORT").expect("capture report"),
                );
                let path = report.with_extension("png");
                let result = event
                    .image
                    .clone()
                    .try_into_dynamic()
                    .map_err(|e| format!("{e:?}"))
                    .and_then(|image| image.save(&path).map_err(|e| e.to_string()));
                if let Err(err) = result {
                    error!("capture failed: {err}");
                    let mut report_value: Value =
                        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
                    report_value["status"] = json!("FAIL");
                    report_value["capture_error"] = json!(err);
                    publish(&report_value);
                    exit.write(AppExit::error());
                } else {
                    exit.write(AppExit::Success);
                }
            },
        );
}
