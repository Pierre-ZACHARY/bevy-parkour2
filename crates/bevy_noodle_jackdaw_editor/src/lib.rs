//! Jackdaw editor extension: a "Flow Graph" window that opens the project's
//! `.flowgraph.bsn` files as bevy_noodle canvases, saves edits back, and,
//! during play-in-editor, lights up what flows in the running game.
//!
//! The lighting needs nothing from the game but its runner: play-in-editor
//! streams the game's reflected components, among them the
//! `FlowGraphPlayback` each runner records, and the view reads it like any
//! other component once its type is registered here.
//!
//! A selected blend space node previews its blend on the model the game's
//! catalog names ([`pose_preview`]). Model-specific fixes, such as the
//! Ninja's hair, come from the project's own extension.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_noodle_jackdaw::blend::BlendPreviewAssets;
use bevy_noodle_jackdaw::view::{FlowGraphView, FlowGraphViewPlugin, spawn_flow_graph_editor};
use bevy_noodle_jackdaw::{
    CATALOG_EXTENSION, EXTENSION, FlowGraphTypesPlugin, FlowNode, FlowNodeCatalog, builtin_catalog,
    flow_graph_to_bsn, read_catalog_file, read_flow_graph_file,
};
use jackdaw_extension::EditorEntity;
use jackdaw_extension::document::SceneFilePath;
use jackdaw_extension::prelude::*;
use pose_preview::{PosePreviewPlugin, PoseStage};

const WINDOW: &str = "bevy_noodle_jackdaw.flow_graph";

#[derive(Default)]
pub struct FlowGraphExtension;

impl JackdawExtension for FlowGraphExtension {
    fn id(&self) -> String {
        "bevy_noodle_jackdaw".to_string()
    }

    fn label(&self) -> String {
        "Flow Graph".to_string()
    }

    fn description(&self) -> String {
        "Edit .flowgraph.bsn node graphs and watch them run during play.".to_string()
    }

    fn register(&self, registrar: &mut ExtensionRegistrar<'_>) {
        registrar.register_window(
            WindowDescriptor::new(WINDOW)
                .with_name("Flow Graph")
                .with_build(|spawner| {
                    spawner.spawn((
                        FlowGraphWindow,
                        Node {
                            width: percent(100),
                            height: percent(100),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                    ));
                }),
        );
    }

    fn app_plugin(&self) -> Option<Box<dyn Plugin>> {
        Some(Box::new(|app: &mut App| {
            app.add_plugins((FlowGraphTypesPlugin, FlowGraphViewPlugin, PosePreviewPlugin))
                .add_systems(Update, (fill_windows, show_file_names, hide_stages));
        }))
    }
}

/// The window's root, spawned by the dock.
#[derive(Component)]
struct FlowGraphWindow;

/// Where a window draws its canvas, and the label naming the open file.
#[derive(Component)]
struct CanvasArea {
    label: Entity,
}

/// The project's `assets/` folder: above the open scene's file, else under
/// `JACKDAW_OPEN_PROJECT`, else under the working directory.
fn assets_dir(scene: Option<&SceneFilePath>) -> PathBuf {
    let from_scene = scene
        .and_then(|scene| scene.path.as_deref())
        .and_then(|path| {
            Path::new(path)
                .ancestors()
                .find(|dir| dir.file_name().is_some_and(|name| name == "assets"))
                .map(Path::to_path_buf)
        });
    from_scene
        .or_else(|| {
            std::env::var_os("JACKDAW_OPEN_PROJECT").map(|root| PathBuf::from(root).join("assets"))
        })
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// Files under `dir` ending in `.{extension}`, as asset paths (relative,
/// `/`-separated).
fn files_with(dir: &Path, extension: &str) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, suffix: &str, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, suffix, found);
            } else if path.to_string_lossy().ends_with(suffix)
                && let Ok(relative) = path.strip_prefix(root)
            {
                found.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &format!(".{extension}"), &mut found);
    found.sort();
    found
}

/// Every catalog under `assets`, merged, then the built-in types they do
/// not name: the node types the library lists. The previews play the first
/// catalog's model and clips that has some. The game writes its catalog
/// when it starts in a development build.
fn read_catalogs(world: &World, assets: &Path) -> FlowNodeCatalog {
    let registry = world.resource::<AppTypeRegistry>().read();
    let mut catalog = FlowNodeCatalog::default();
    let mut add = |nodes: Vec<FlowNode>| {
        for node in nodes {
            if !catalog.nodes.iter().any(|known| known.kind == node.kind) {
                catalog.nodes.push(node);
            }
        }
    };
    for file in files_with(assets, CATALOG_EXTENSION) {
        match read_catalog_file(&assets.join(&file), &registry) {
            Ok(read) => {
                if catalog.preview.model.is_empty() {
                    catalog.preview = read.preview;
                }
                for kind in read.arrows {
                    if !catalog.arrows.contains(&kind) {
                        catalog.arrows.push(kind);
                    }
                }
                add(read.nodes);
            }
            Err(err) => warn!("node catalog {file}: {err}"),
        }
    }
    add(builtin_catalog().nodes);
    catalog
}

/// Give each new window its toolbar and canvas area, and open the first
/// graph file.
fn fill_windows(
    windows: Query<Entity, Added<FlowGraphWindow>>,
    scene: Option<Res<SceneFilePath>>,
    mut commands: Commands,
) {
    for window in &windows {
        let assets = assets_dir(scene.as_deref());
        let files = files_with(&assets, EXTENSION);
        let toolbar = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    padding: UiRect::all(px(4)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(window),
            ))
            .id();
        let label = commands
            .spawn((
                Text::new(if files.is_empty() {
                    format!("no .{EXTENSION} file under {}", assets.display())
                } else {
                    String::new()
                }),
                TextFont::from_font_size(13.0),
                ChildOf(toolbar),
            ))
            .id();
        let area = commands
            .spawn((
                CanvasArea { label },
                Node {
                    flex_grow: 1.0,
                    min_height: px(0),
                    position_type: PositionType::Relative,
                    ..default()
                },
                ChildOf(window),
            ))
            .id();
        for file in &files {
            let file = file.clone();
            let assets = assets.clone();
            toolbar_button(&mut commands, toolbar, &file.clone()).observe(
                move |_: On<PointerClick>, mut commands: Commands| {
                    let (file, assets) = (file.clone(), assets.clone());
                    commands.queue(move |world: &mut World| open(world, area, &assets, &file));
                },
            );
        }
        toolbar_button(&mut commands, toolbar, "Save").observe(
            move |_: On<PointerClick>, mut commands: Commands| {
                commands.queue(move |world: &mut World| save(world, area));
            },
        );
        if let Some(first) = files.first().cloned() {
            commands.queue(move |world: &mut World| open(world, area, &assets, &first));
        }
    }
}

fn toolbar_button<'a>(
    commands: &'a mut Commands,
    toolbar: Entity,
    label: &str,
) -> EntityCommands<'a> {
    let mut button = commands.spawn((
        Node {
            padding: UiRect::axes(px(8), px(3)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(60, 63, 71)),
        ChildOf(toolbar),
    ));
    button.with_child((Text::new(label), TextFont::from_font_size(13.0)));
    button
}

/// On a canvas area: the file its canvas was opened from, and the canvas.
#[derive(Component)]
struct OpenFile {
    path: PathBuf,
    canvas: Entity,
}

/// Draw `file` (an asset path under `assets`) in `area`, with the node
/// library, replacing what it showed.
fn open(world: &mut World, area: Entity, assets: &Path, file: &str) {
    let path = assets.join(file);
    let graph = read_flow_graph_file(&path, &world.resource::<AppTypeRegistry>().read());
    let graph = match graph {
        Ok(graph) => graph,
        Err(err) => {
            error!("flow graph {}: {err}", path.display());
            return;
        }
    };
    let catalog = read_catalogs(world, assets);
    world.insert_resource::<BlendPreviewAssets>(catalog.preview.clone());
    world.entity_mut(area).despawn_related::<Children>();
    let mut commands = world.commands();
    let canvas = spawn_flow_graph_editor(&mut commands, area, file, graph, &catalog);
    world.flush();
    world.entity_mut(area).insert(OpenFile { path, canvas });
}

/// Write the graph `area` shows back to its file.
fn save(world: &mut World, area: Entity) {
    let Some((path, canvas)) = world
        .get::<OpenFile>(area)
        .map(|open| (open.path.clone(), open.canvas))
    else {
        return;
    };
    let Some(text) = world.get::<FlowGraphView>(canvas).map(|view| {
        let registry = world.resource::<AppTypeRegistry>().read();
        flow_graph_to_bsn(&view.root_graph(), &registry)
    }) else {
        return;
    };
    if let Err(err) = std::fs::write(&path, text) {
        error!("flow graph {}: {err}", path.display());
        return;
    }
    if let Some(mut view) = world.get_mut::<FlowGraphView>(canvas) {
        view.dirty = false;
    }
    info!("saved {}", path.display());
}

/// Name the open file in each window's label, `*` marking unsaved edits.
fn show_file_names(
    areas: Query<(&CanvasArea, &OpenFile)>,
    views: Query<&FlowGraphView>,
    mut labels: Query<&mut Text>,
) {
    for (area, open) in &areas {
        let Ok(view) = views.get(open.canvas) else {
            continue;
        };
        let name = format!("{}{}", view.path, if view.dirty { " *" } else { "" });
        if let Ok(mut text) = labels.get_mut(area.label)
            && text.0 != name
        {
            text.0 = name;
        }
    }
}

/// Mark the preview stages, down to their model's bones, as the editor's
/// own: the hierarchy, picking and the camera preview skip them.
fn hide_stages(
    stages: Query<Entity, With<PoseStage>>,
    children: Query<&Children>,
    unmarked: Query<(), Without<EditorEntity>>,
    mut commands: Commands,
) {
    for stage in &stages {
        for entity in std::iter::once(stage).chain(children.iter_descendants(stage)) {
            if unmarked.contains(entity) {
                commands.entity(entity).try_insert(EditorEntity);
            }
        }
    }
}
