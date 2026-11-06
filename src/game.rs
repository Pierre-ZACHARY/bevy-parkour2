//! Game plugin: spawns the Jackdaw scene and the systems that drive it.
//!
//! The scene's player carries a `CharacterController`, which runs
//! `assets/flow/movement.flowgraph.bsn`; Tab shows that graph over the game.
//! Its camera follows it over the shoulder
//! (`assets/flow/third_person_camera.flowgraph.bsn`); F switches to a fly
//! camera (`assets/flow/fly_camera.flowgraph.bsn`), the player standing
//! still meanwhile.
//! Its Ninja carries an `AnimationController`, which runs
//! `assets/flow/animation.flowgraph.bsn` on Yummy clips retargeted onto it.
//! Development builds write every node type to
//! `assets/flow/nodes.flowcatalog.bsn` at startup: the editor's node library
//! lists them from there.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_tnua_avian3d::TnuaAvian3dPlugin;
use jackdaw_avian_integration::AvianColliderBridgePlugin;
use jackdaw_runtime::JackdawSceneRoot;

use camera_controller::{CameraController, CameraControllerPlugin, CameraCursor, CullingCamera};
use character_animation::CharacterAnimationPlugin;
use character_controller::view::{GraphPanel, MovementGraphViewPlugin};
use character_controller::{CharacterControllerPlugin, PlayerAction};
use leafwing_input_manager::prelude::ActionState;
use ninja_character::NinjaPlugin;

/// Where development builds list the node types, for the editor.
#[cfg(all(debug_assertions, feature = "editor"))]
const NODE_CATALOG: &str = "flow/nodes.flowcatalog.bsn";

/// Physics, the Jackdaw scene and the player character.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(jackdaw_runtime::JackdawPlugin);
        app.add_plugins((
            PhysicsPlugins::default(),
            // Builds the colliders the scene authors as `AvianCollider`.
            AvianColliderBridgePlugin,
            TnuaAvian3dPlugin::new(FixedUpdate),
            CharacterControllerPlugin,
            CameraControllerPlugin,
            CharacterAnimationPlugin,
            MovementGraphViewPlugin,
            NinjaPlugin,
        ))
        .add_systems(Startup, spawn_scene)
        .add_systems(Update, (hold_players_off_view, free_cursor_over_graph));
        #[cfg(feature = "perf_ui")]
        app.add_plugins(crate::perf_ui::GamePerfUiPlugin);
        #[cfg(all(debug_assertions, feature = "editor"))]
        app.add_plugins(bevy_noodle_jackdaw::FlowNodeCatalogPlugin {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(NODE_CATALOG),
        });
    }
}

/// The scene (ground, player and Ninja, light, camera) is authored in Jackdaw in
/// `assets/scene.bsn`. `JackdawPlugin` spawns its entities under this root
/// and, with `asset_watch`, respawns them when the file is saved.
fn spawn_scene(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Name::new("Scene"),
        JackdawSceneRoot(asset_server.load("scene.bsn")),
    ));
}

/// The player stands still while the view is not theirs (the fly camera,
/// which their keys fly): their actions are off.
fn hold_players_off_view(
    cameras: Query<(&Camera, Has<CullingCamera>), With<CameraController>>,
    mut players: Query<&mut ActionState<PlayerAction>>,
) {
    let detached = cameras
        .iter()
        .any(|(camera, culling)| camera.is_active && !culling);
    for mut actions in &mut players {
        if detached && !actions.disabled() {
            actions.disable();
        } else if !detached && actions.disabled() {
            actions.enable();
        }
    }
}

/// The movement graph's panel needs the cursor: the camera lets it go.
fn free_cursor_over_graph(
    panels: Query<&Node, With<GraphPanel>>,
    mut cursor: ResMut<CameraCursor>,
) {
    let grab = panels.iter().all(|panel| panel.display == Display::None);
    cursor.set_if_neq(CameraCursor { grab });
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use jackdaw_runtime::JackdawPlugin;

    use super::*;

    /// Load `assets/scene.bsn` headless, the way the game does, until the
    /// ground collider is built.
    fn loaded_scene() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
                ..default()
            },
            bevy::world_serialization::WorldSerializationPlugin,
        ))
        .add_plugins((
            bevy::input::InputPlugin,
            bevy::window::WindowPlugin {
                primary_window: None,
                ..default()
            },
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        // `PbrPlugin` does this in the game; the catalog only loads
        // reflected asset types.
        .register_asset_reflect::<StandardMaterial>()
        .add_plugins((
            JackdawPlugin,
            AvianColliderBridgePlugin,
            bevy::gltf::GltfPlugin::default(),
        ))
        .add_systems(Startup, spawn_scene);
        app.finish();
        app.cleanup();
        for _ in 0..500 {
            app.update();
            if named::<Collider>(&mut app, "Ground").is_some() {
                return app;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the ground never got a collider");
    }

    /// Component `T` of the entity called `name`.
    fn named<'a, T: Component>(app: &'a mut App, name: &str) -> Option<&'a T> {
        let world = app.world_mut();
        let entity = world
            .query::<(Entity, &Name)>()
            .iter(world)
            .find(|(_, n)| n.as_str() == name)?
            .0;
        world.get::<T>(entity)
    }

    #[test]
    fn scene_has_ground_ninja_sun_and_cameras() {
        let mut app = loaded_scene();

        let brush = named::<jackdaw_runtime::Brush>(&mut app, "Ground")
            .expect("ground brush")
            .clone();
        assert_eq!(brush.faces.len(), 6);
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        for face in &brush.faces {
            let material = materials.get(&face.material).expect("ground material");
            assert_eq!(material.base_color, Color::srgb(0.25, 0.38, 0.8478261));
        }

        let ninja = named::<jackdaw_runtime::GltfSource>(&mut app, "Ninja").expect("ninja");
        assert_eq!(ninja.path, "character/ninja/outfit_01.gltf");
        let animation = named::<character_animation::AnimationController>(&mut app, "Ninja");
        assert_eq!(animation, Some(&default()));
        let controller =
            named::<character_controller::CharacterController>(&mut app, "Player").expect("player");
        assert_eq!(*controller, default());
        let body = named::<Transform>(&mut app, "Player")
            .expect("player")
            .translation;
        assert_eq!(body.y, character_controller::FLOAT_HEIGHT);
        assert!(named::<DirectionalLight>(&mut app, "Sun").is_some());
        let camera = *named::<Transform>(&mut app, "Camera").expect("camera");
        let expected = Transform::from_xyz(-2.0, 1.6, 4.0).looking_at(Vec3::Y, Vec3::Y);
        assert!(camera.rotation.angle_between(expected.rotation) < 1e-4);

        // The player's camera follows them and is culling's; the fly
        // camera starts off.
        assert!(named::<camera_controller::CameraTarget>(&mut app, "Player").is_some());
        let controller = named::<CameraController>(&mut app, "Camera").expect("camera controller");
        assert_eq!(*controller, default());
        assert!(named::<CullingCamera>(&mut app, "Camera").is_some());
        let fly = named::<CameraController>(&mut app, "FlyCamera").expect("fly camera");
        assert_eq!(fly.graph, camera_controller::FLY_GRAPH);
        assert!(named::<CullingCamera>(&mut app, "FlyCamera").is_none());
        assert!(
            !named::<Camera>(&mut app, "FlyCamera")
                .expect("fly camera")
                .is_active
        );
    }

    #[test]
    fn the_third_person_camera_graph_follows_the_player() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy_noodle_jackdaw::FlowGraphPlugin,
            camera_controller::CameraFlowNodesPlugin,
        ));
        let path = format!(
            "{}/assets/{}",
            env!("CARGO_MANIFEST_DIR"),
            camera_controller::THIRD_PERSON_GRAPH
        );
        let text = std::fs::read_to_string(&path).expect("the camera graph");
        let graph = {
            let registry = app.world().resource::<AppTypeRegistry>().read();
            bevy_noodle_jackdaw::parse_flow_graph(&text, &registry).expect(&path)
        };
        // The scene's `#Player`, as `scene_has_ground_ninja_sun_and_cameras` finds it.
        let world = app.world_mut();
        world.spawn(Name::new("Ninja"));
        let player = world.spawn(Name::new("Player")).id();
        let evaluation = bevy_noodle_jackdaw::evaluate(
            world,
            Entity::PLACEHOLDER,
            &graph,
            world.resource::<bevy_noodle_jackdaw::FlowNodeTypes>(),
            &default(),
            &default(),
        );
        let id = &graph
            .nodes
            .iter()
            .find(|node| node.kind == camera_controller::THIRD_PERSON)
            .expect("a Third Person node")
            .id;
        let state = evaluation.states.iter().find(|state| &state.id == id);
        let target = state.and_then(|state| state.inputs.first());
        assert_eq!(target.and_then(|target| target.as_entity()), Some(player));
    }
}
