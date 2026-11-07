//! The default camera graphs, headless: the third-person camera follows
//! and aims, looks around freely with the middle button, and F switches to
//! the fly camera, which starts where it was and flies.

use std::f32::consts::PI;
use std::path::PathBuf;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_noodle_jackdaw::{FlowGraph, FlowGraphRunner};
use camera_controller::{
    CameraAim, CameraController, CameraControllerPlugin, CameraTarget, CullingCamera, FLY_GRAPH,
    THIRD_PERSON_GRAPH,
};
use leafwing_input_manager::prelude::*;

/// A fresh assets folder for one test: the plugin writes its graphs there.
fn temp_assets(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("camera-controller-{name}-{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Scene {
    app: App,
    target: Entity,
    third_person: Entity,
    fly: Entity,
}

fn scene(assets: &std::path::Path) -> Scene {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin {
            file_path: assets.to_string_lossy().into_owned(),
            ..default()
        },
        bevy::input::InputPlugin,
        CameraControllerPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.finish();
    app.cleanup();
    let world = app.world_mut();
    // Facing +Z, as the scene's player does.
    let target = world
        .spawn((
            CameraTarget,
            Transform::from_xyz(1.0, 0.95, 2.0).with_rotation(Quat::from_rotation_y(PI)),
        ))
        .id();
    let third_person = world
        .spawn((
            Camera3d::default(),
            CameraController::default(),
            CullingCamera,
        ))
        .id();
    let fly = world
        .spawn((
            Camera3d::default(),
            Camera {
                is_active: false,
                ..default()
            },
            CameraController {
                graph: FLY_GRAPH.into(),
            },
        ))
        .id();
    for _ in 0..500 {
        app.update();
        let world = app.world();
        let graphs = world.resource::<Assets<FlowGraph>>();
        if [third_person, fly]
            .iter()
            .all(|&camera| graphs.contains(&world.get::<FlowGraphRunner>(camera).unwrap().graph))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    app.update();
    Scene {
        app,
        target,
        third_person,
        fly,
    }
}

impl Scene {
    fn transform(&self, entity: Entity) -> Transform {
        *self.app.world().get::<Transform>(entity).unwrap()
    }

    fn aim(&self) -> CameraAim {
        *self.app.world().get::<CameraAim>(self.target).unwrap()
    }

    fn active(&self, camera: Entity) -> bool {
        self.app.world().get::<Camera>(camera).unwrap().is_active
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
        }
    }

    /// One frame of the mouse moving `delta` pixels.
    fn mouse(&mut self, delta: Vec2) {
        MouseMove::default().set_axis_pair(self.app.world_mut(), delta);
        self.app.update();
    }
}

#[test]
fn the_third_person_camera_follows_aims_and_looks_around() {
    let assets = temp_assets("follow");
    let mut scene = scene(&assets);
    assert!(assets.join(THIRD_PERSON_GRAPH).exists());

    // Behind the target, looking where it faces, which it aims at.
    let camera = scene.transform(scene.third_person);
    assert!(camera.translation.z < 2.0 - 1.0, "{camera:?}");
    assert!(camera.forward().z > 0.99, "{camera:?}");
    assert!(
        (scene.aim().yaw.abs() - PI).abs() < 1e-4,
        "{:?}",
        scene.aim()
    );

    // The mouse to the right turns the camera and the aim right.
    scene.mouse(Vec2::new(100.0, 0.0));
    let yaw = scene.aim().yaw;
    assert!(
        (scene.aim().forward().x + 0.3_f32.sin()).abs() < 1e-3,
        "{:?}",
        scene.aim()
    );
    let turned = scene.transform(scene.third_person);
    assert!(turned.forward().x < -0.25, "{turned:?}");

    // Middle button held: the camera turns, the aim stays.
    MouseButton::Middle.press(scene.app.world_mut());
    scene.mouse(Vec2::new(-400.0, 0.0));
    assert_eq!(scene.aim().yaw, yaw);
    let free = scene.transform(scene.third_person);
    assert!(free.forward().x > 0.5, "{free:?}");

    // Let go: the camera stays, the aim comes to where it looks.
    MouseButton::Middle.release(scene.app.world_mut());
    scene.frames(30);
    let stayed = scene.transform(scene.third_person);
    assert!(
        stayed.rotation.angle_between(free.rotation) < 0.02,
        "{stayed:?}"
    );
    let forward = scene.aim().forward();
    let looks = stayed.forward().as_vec3().with_y(0.0).normalize();
    assert!(forward.dot(looks) > 0.999, "{forward} vs {looks}");
    assert!(turned.rotation.angle_between(free.rotation) > 0.5 && yaw != scene.aim().yaw);

    // The target moves; the camera follows.
    scene
        .app
        .world_mut()
        .get_mut::<Transform>(scene.target)
        .unwrap()
        .translation
        .x += 3.0;
    scene.frames(1);
    let moved = scene.transform(scene.third_person);
    assert!((moved.translation.x - stayed.translation.x - 3.0).abs() < 0.01);
    _ = std::fs::remove_dir_all(assets);
}

#[test]
fn f_switches_to_the_fly_camera_where_the_view_was() {
    let assets = temp_assets("switch");
    let mut scene = scene(&assets);
    let view = scene.transform(scene.third_person);

    KeyCode::KeyF.press(scene.app.world_mut());
    scene.frames(1);
    assert!(scene.active(scene.fly) && !scene.active(scene.third_person));
    let start = scene.transform(scene.fly);
    assert!(
        start.translation.abs_diff_eq(view.translation, 1e-4),
        "{start:?}"
    );
    // Held, it does not switch back.
    scene.frames(5);
    assert!(scene.active(scene.fly));
    KeyCode::KeyF.release(scene.app.world_mut());
    scene.frames(1);

    // Forward flies where it looks, at 6 m/s; the other camera stays
    // with the target.
    KeyCode::KeyW.press(scene.app.world_mut());
    scene.frames(30);
    KeyCode::KeyW.release(scene.app.world_mut());
    let flown = scene.transform(scene.fly).translation - start.translation;
    assert!(
        flown
            .normalize()
            .abs_diff_eq(start.forward().as_vec3(), 1e-3),
        "{flown}"
    );
    assert!((flown.length() - 6.0 * 30.0 / 60.0).abs() < 0.3, "{flown}");
    let stayed = scene.transform(scene.third_person);
    assert!(stayed.translation.abs_diff_eq(view.translation, 1e-4));

    // Space goes straight up.
    let before = scene.transform(scene.fly).translation;
    KeyCode::Space.press(scene.app.world_mut());
    scene.frames(10);
    KeyCode::Space.release(scene.app.world_mut());
    let risen = scene.transform(scene.fly).translation - before;
    assert!(
        risen.x.abs() < 1e-4 && risen.z.abs() < 1e-4 && risen.y > 0.5,
        "{risen}"
    );

    // F again: back to the player's view.
    KeyCode::KeyF.press(scene.app.world_mut());
    scene.frames(1);
    assert!(scene.active(scene.third_person) && !scene.active(scene.fly));
    _ = std::fs::remove_dir_all(assets);
}

#[cfg(feature = "reticle")]
#[test]
fn the_reticle_shows_on_the_third_person_camera_while_it_aims() {
    use bevy::ui::UiTargetCamera;
    use camera_controller::Reticle;

    let assets = temp_assets("reticle");
    let mut scene = scene(&assets);
    let world = scene.app.world_mut();
    let (reticle, visibility, parent) = world
        .query::<(&Reticle, &Visibility, &ChildOf)>()
        .single(world)
        .unwrap();
    let (reticle, parent) = (*reticle, parent.parent());
    assert_eq!(reticle.camera, scene.third_person);
    assert_eq!(*visibility, Visibility::Inherited);
    let drawn_on = world.get::<UiTargetCamera>(parent).unwrap().entity();
    assert_eq!(drawn_on, scene.third_person);

    let shown = |scene: &mut Scene| {
        let world = scene.app.world_mut();
        *world
            .query_filtered::<&Visibility, With<Reticle>>()
            .single(world)
            .unwrap()
            == Visibility::Inherited
    };
    // Looking around freely: off the aim, hidden; back on it once let go.
    MouseButton::Middle.press(scene.app.world_mut());
    scene.mouse(Vec2::new(-400.0, 0.0));
    scene.frames(1);
    assert!(!shown(&mut scene));
    MouseButton::Middle.release(scene.app.world_mut());
    scene.frames(60);
    assert!(shown(&mut scene));
    _ = std::fs::remove_dir_all(assets);
}
