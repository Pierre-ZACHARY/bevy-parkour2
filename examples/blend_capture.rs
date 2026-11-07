//! Screenshots of the animation graph's Locomotion blend space: the
//! Grounded machine with it, its Idle node selected (its clip previewed),
//! the blend space selected with its axes set (a
//! grid, a higher end), then its blend previewed on the character at
//! several points. Into `target/captures/`, then quits. Given another
//! graph's path, a screenshot of it alone, named after it; given a node's
//! id too, with that node selected (its preview shown).
//!
//! ```sh
//! cargo run --example blend_capture
//! cargo run --example blend_capture -- assets/flow/third_person_camera.flowgraph.bsn
//! cargo run --example blend_capture -- assets/flow/animation.flowgraph.bsn skeleton
//! ```

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::ui::Selected;
use bevy::window::WindowPosition;
use bevy_noodle_jackdaw::blend::sample_weights;
use bevy_noodle_jackdaw::levels::{focus_node, open_level};
use bevy_noodle_jackdaw::view::{
    BlendPreview, BlendPreviewSlot, FlowArrows, FlowGraphViewPlugin, ViewNode,
    spawn_flow_graph_view,
};
use bevy_noodle_jackdaw::{
    AxisSettings, BlendAxes, FlowGraph, FlowGraphTypesPlugin, FlowNode, read_catalog_file,
    read_flow_graph_file,
};
use ninja_hair::NinjaHairPlugin;
use pose_preview::PosePreviewPlugin;

const CATALOG: &str = "assets/flow/nodes.flowcatalog.bsn";
const GRAPH: &str = "assets/flow/animation.flowgraph.bsn";
const OUT: &str = "target/captures";

/// Where to preview the blend: (x right, y ahead) in m/s.
const POINTS: [(f32, f32); 12] = [
    (0.0, 0.7),
    (0.0, 1.4),
    (0.99, 0.99),
    (1.4, 0.0),
    (0.0, -1.4),
    (-1.4, 0.0),
    (-1.2, 0.6),
    (0.0, 2.45),
    (-3.5, 0.0),
    (2.47, -2.47),
    (0.0, 4.75),
    (0.0, 6.0),
];

fn main() -> AppExit {
    let other = std::env::args().nth(1);
    let selected = std::env::args().nth(2);
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "blend capture".into(),
            resolution: (1600, 1000).into(),
            // Off the screens, not to take the focus.
            position: WindowPosition::At(IVec2::new(-6000, 100)),
            focused: false,
            ..default()
        }),
        ..default()
    }))
    .add_plugins((
        FlowGraphTypesPlugin,
        FlowGraphViewPlugin,
        PosePreviewPlugin,
        NinjaHairPlugin,
    ))
    .insert_resource(ClearColor(Color::srgb_u8(24, 25, 29)));
    match other {
        Some(path) => app
            .insert_resource(Other(path, selected))
            .add_systems(Startup, open_other)
            .add_systems(Update, shoot_other),
        None => app.add_systems(Startup, open).add_systems(Update, script),
    };
    app.run()
}

/// The graph to shoot alone, and the node to select in it.
#[derive(Resource)]
struct Other(String, Option<String>);

fn open_other(world: &mut World) {
    let path = world.resource::<Other>().0.clone();
    let registry = world.resource::<AppTypeRegistry>().clone();
    let catalog = read_catalog_file(CATALOG.as_ref(), &registry.read()).expect("catalog");
    world.insert_resource(FlowArrows::from_catalog(&catalog));
    world.insert_resource(catalog.preview);
    let mut graph = read_flow_graph_file(path.as_ref(), &registry.read()).expect("graph");
    // All of it in the window: its top left corner at the view's.
    let corner = graph
        .nodes
        .iter()
        .map(|node| node.position)
        .fold(Vec2::INFINITY, Vec2::min);
    for node in &mut graph.nodes {
        node.position += Vec2::splat(40.0) - corner;
    }
    world.spawn(Camera2d);
    let root = world
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            ..default()
        })
        .id();
    let mut commands = world.commands();
    spawn_flow_graph_view(&mut commands, root, &path, graph);
    world.flush();
}

fn shoot_other(
    time: Res<Time>,
    other: Res<Other>,
    nodes: Query<(Entity, &ViewNode)>,
    mut step: Local<usize>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let steps = [1.0, 5.0, 7.0];
    if *step >= steps.len() || time.elapsed_secs() < steps[*step] {
        return;
    }
    if *step == 0 {
        for (node, ViewNode(id)) in &nodes {
            if other.1.as_ref() == Some(id) {
                commands.entity(node).insert(Selected);
            }
        }
    } else if *step == 1 {
        let name = std::path::Path::new(&other.0)
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.split('.').next())
            .unwrap_or("graph");
        shoot(&mut commands, name);
    } else {
        exit.write(AppExit::Success);
    }
    *step += 1;
}

#[derive(Resource)]
struct Opened {
    root: Entity,
    canvas: Entity,
    locomotion: FlowNode,
}

fn open(world: &mut World) {
    let catalog = read_catalog_file(
        CATALOG.as_ref(),
        &world.resource::<AppTypeRegistry>().read(),
    )
    .expect("catalog");
    world.insert_resource(FlowArrows::from_catalog(&catalog));
    world.insert_resource(catalog.preview);
    let mut graph =
        read_flow_graph_file(GRAPH.as_ref(), &world.resource::<AppTypeRegistry>().read())
            .expect("graph");
    let locomotion = find(&mut graph, "locomotion").expect("a locomotion node");
    // To show: a grid of 1 m/s across x, y up to 7 m/s in 7 cells.
    locomotion.axes = Some(BlendAxes {
        x: AxisSettings {
            divisions: 7,
            ..default()
        },
        y: AxisSettings {
            max: Some(7.0),
            divisions: 7,
            ..default()
        },
    });
    let locomotion = locomotion.clone();
    world.spawn(Camera2d);
    let root = world
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            ..default()
        })
        .id();
    let mut commands = world.commands();
    let canvas = spawn_flow_graph_view(&mut commands, root, GRAPH, graph);
    world.flush();
    world.insert_resource(Opened {
        root,
        canvas,
        locomotion,
    });
}

fn find<'a>(graph: &'a mut FlowGraph, id: &str) -> Option<&'a mut FlowNode> {
    graph.nodes.iter_mut().find_map(|node| {
        if node.id == id {
            Some(node)
        } else {
            find(node.graph.as_mut()?, id)
        }
    })
}

fn shoot(commands: &mut Commands, name: &str) {
    std::fs::create_dir_all(OUT).unwrap();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(format!("{OUT}/{name}.png")));
}

fn script(
    time: Res<Time>,
    opened: Option<Res<Opened>>,
    mut step: Local<usize>,
    nodes: Query<(Entity, &ViewNode)>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(opened) = opened else {
        return;
    };
    let t = time.elapsed_secs();
    let steps: [f32; 11] = [1.0, 4.0, 4.5, 7.5, 8.0, 11.0, 11.5, 13.0, 14.0, 25.0, 28.0];
    let select = |commands: &mut Commands, which: &str| {
        for (node, ViewNode(id)) in &nodes {
            if id == which {
                commands.entity(node).insert(Selected);
            } else {
                commands.entity(node).remove::<Selected>();
            }
        }
    };
    if *step >= steps.len() || t < steps[*step] {
        return;
    }
    match *step {
        0 => open_level(&mut commands, opened.canvas, "grounded"),
        1 => shoot(&mut commands, "grounded"),
        2 => select(&mut commands, "idle"),
        3 => shoot(&mut commands, "idle_preview"),
        4 => {
            select(&mut commands, "");
            focus_node(&mut commands, opened.canvas, "locomotion");
        }
        5 => shoot(&mut commands, "locomotion"),
        6 => select(&mut commands, "locomotion"),
        7 => shoot(&mut commands, "locomotion_axes"),
        8 => {
            commands.entity(opened.root).despawn();
            spawn_previews(&mut commands, &opened.locomotion);
        }
        9 => shoot(&mut commands, "previews"),
        _ => {
            exit.write(AppExit::Success);
        }
    }
    *step += 1;
}

fn spawn_previews(commands: &mut Commands, space: &FlowNode) {
    let grid = commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::flex(4, 1.0),
            grid_template_rows: RepeatedGridTrack::flex(3, 1.0),
            column_gap: px(6),
            row_gap: px(6),
            padding: UiRect::all(px(6)),
            ..default()
        })
        .id();
    for (x, y) in POINTS {
        let at = Vec2::new(x, y);
        let mut weights: Vec<(f32, &str)> = sample_weights(space, at)
            .into_iter()
            .zip(&space.samples)
            .filter(|(weight, _)| *weight > 0.01)
            .map(|(weight, sample)| (weight, sample.title.as_str()))
            .collect();
        weights.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mix = weights
            .iter()
            .map(|(weight, title)| format!("{title} {:.0}%", weight * 100.0))
            .collect::<Vec<_>>()
            .join(" + ");
        let rate = bevy_noodle_jackdaw::blend::playrate(space, at);
        let cell = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    min_height: px(0),
                    ..default()
                },
                BackgroundColor(Color::srgb_u8(36, 37, 43)),
                ChildOf(grid),
            ))
            .id();
        commands.spawn((
            Text::new(format!("x {x:+.2}  y {y:+.2} m/s  (x{rate:.2})\n{mix}")),
            TextFont::from_font_size(13.0),
            Node {
                padding: UiRect::all(px(4)),
                ..default()
            },
            ChildOf(cell),
        ));
        commands.spawn((
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                ..default()
            },
            BlendPreviewSlot,
            BlendPreview {
                space: space.clone(),
                at,
            },
            ChildOf(cell),
        ));
    }
}
