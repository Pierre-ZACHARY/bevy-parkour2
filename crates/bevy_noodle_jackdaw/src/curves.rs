//! Easing fields: an unwired easing input shows its curve on the node, time
//! across and progress up, with its two handles to drag and its duration
//! to type. A double click on the curve zooms onto it, to work on it;
//! Escape zooms back out.
//!
//! While the game plays, the curve easing a value is marked where it is:
//! a node with easing inputs says which one is under way on its `easing`
//! output (the input's index, -1 for none) and how far on its `progress`
//! output (0 to 1).

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_noodle::style::{WireMaterial, stroke_material};

use crate::fields::{FIELD, FIELD_BORDER, Targets, edit_value};
use crate::levels::focus_on;
use crate::model::{Easing, FlowValue};
use crate::run::FlowGraphPlayback;
use crate::view::{ACTIVE, FlowGraphView, same_graph_path};

/// The curve's size on the node.
pub(crate) const PLOT_SIZE: Vec2 = Vec2::new(84.0, 48.0);
const HANDLE: f32 = 8.0;
/// Stroke widths of the curve and of the handles' stems.
const CURVE_WIDTH: f32 = 2.0;
const STEM_WIDTH: f32 = 1.0;
const MARK: f32 = 7.0;
const CURVE: Color = Color::srgb(0.7, 0.45, 0.95);
const STEM: Color = Color::srgb_u8(90, 80, 115);
const HANDLE_COLOR: Color = Color::srgb_u8(214, 218, 224);

/// The curve of an easing input. Focusable: drags on it are its own, not
/// the node's.
#[derive(Component)]
pub(crate) struct EasingPlot;

/// A stroke of the plot, drawn by the wires' shader so it stays smooth at
/// any zoom: the curve, or a handle's stem (which handle).
#[derive(Component, Clone, Copy, PartialEq)]
enum PlotStroke {
    Curve,
    Stem(usize),
}

/// A handle to drag: 0 leaves (0, 0), 1 reaches (1, 1).
#[derive(Component)]
struct EasingHandle(usize);

/// Where the curve is while it eases a value.
#[derive(Component)]
struct ProgressMark;

pub(crate) struct EasingFieldsPlugin;

impl Plugin for EasingFieldsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(drag_handle)
            .add_observer(zoom_onto_plot)
            .add_systems(Update, sync_easing_plots);
    }
}

/// Draw an easing input's curve in its row's `fields`.
pub(crate) fn spawn_easing_plot(
    commands: &mut Commands,
    materials: &mut Assets<WireMaterial>,
    fields: Entity,
) {
    let plot = commands
        .spawn((
            EasingPlot,
            TabIndex(0),
            Node {
                width: px(PLOT_SIZE.x),
                height: px(PLOT_SIZE.y),
                margin: UiRect::vertical(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(FIELD_BORDER),
            ChildOf(fields),
        ))
        .id();
    let dot = |size: f32, color: Color, round: bool| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: px(size),
                height: px(size),
                border_radius: BorderRadius::all(px(if round { size / 2.0 } else { 0.0 })),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
            ChildOf(plot),
        )
    };
    for stroke in [PlotStroke::Stem(0), PlotStroke::Stem(1), PlotStroke::Curve] {
        commands.spawn((
            stroke,
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            MaterialNode(materials.add(stroke_material([Vec2::ZERO; 4], CURVE, 1.0).1)),
            Pickable::IGNORE,
            ChildOf(plot),
        ));
    }
    commands.spawn((ProgressMark, dot(MARK, ACTIVE, true)));
    for handle in 0..2 {
        let (node, background, _, child_of) = dot(HANDLE, HANDLE_COLOR, true);
        commands.spawn((
            EasingHandle(handle),
            node,
            background,
            EntityCursor::System(SystemCursorIcon::Grab),
            child_of,
        ));
    }
}

/// Where a point of the curve's square (0 to 1 up and across) is on the
/// plot, in its pixels.
fn on_plot(point: Vec2) -> Vec2 {
    Vec2::new(point.x, 1.0 - point.y) * PLOT_SIZE
}

/// Where a point of the curve's square is on the plot, for a dot of `size`
/// centred there.
fn place(node: &mut Node, point: Vec2, size: f32) {
    let corner = on_plot(point) - size / 2.0;
    let (left, top) = (px(corner.x), px(corner.y));
    if node.left != left {
        node.left = left;
    }
    if node.top != top {
        node.top = top;
    }
}

/// Draw `stroke` of `easing`'s plot with `node` and `material`.
fn draw_stroke(
    stroke: PlotStroke,
    easing: &Easing,
    node: &mut Node,
    material: &Handle<WireMaterial>,
    materials: &mut Assets<WireMaterial>,
) {
    let [start, end] = handles(easing);
    let (points, color, width) = match stroke {
        PlotStroke::Curve => ([Vec2::ZERO, start, end, Vec2::ONE], CURVE, CURVE_WIDTH),
        PlotStroke::Stem(h) => {
            let (from, to) = [(Vec2::ZERO, start), (Vec2::ONE, end)][h];
            ([from, from, to, to], STEM, STEM_WIDTH)
        }
    };
    let (rect, drawn) = stroke_material(points.map(on_plot), color, width);
    let (left, top) = (px(rect.min.x), px(rect.min.y));
    let (width, height) = (px(rect.width()), px(rect.height()));
    if (node.left, node.top, node.width, node.height) != (left, top, width, height) {
        (node.left, node.top, node.width, node.height) = (left, top, width, height);
    }
    if materials.get(material) != Some(&drawn)
        && let Some(mut current) = materials.get_mut(material)
    {
        *current = drawn;
    }
}

fn set_display(node: &mut Node, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
}

/// Where `easing`'s handles are.
fn handles(easing: &Easing) -> [Vec2; 2] {
    [easing.start, easing.end]
}

/// What a plot draws with: a stroke, a handle or the mark.
type PlotParts<'a> = (
    &'a mut Node,
    Option<(&'a PlotStroke, &'a MaterialNode<WireMaterial>)>,
    Option<&'a EasingHandle>,
    Has<ProgressMark>,
);

/// Plots show their input's curve, and where it is while it eases.
fn sync_easing_plots(
    plots: Query<(Entity, &Children), With<EasingPlot>>,
    mut parts: Query<PlotParts>,
    mut borders: Query<&mut BorderColor, With<EasingPlot>>,
    targets: Targets,
    views: Query<&FlowGraphView>,
    playbacks: Query<&FlowGraphPlayback>,
    mut materials: ResMut<Assets<WireMaterial>>,
) {
    for (plot, children) in &plots {
        let Some(target) = targets.of(plot) else {
            continue;
        };
        let Ok(view) = views.get(target.canvas) else {
            continue;
        };
        let Some(node) = view.graph.node(&target.node) else {
            continue;
        };
        let Some(port) = node.inputs.get(target.input) else {
            continue;
        };
        let easing = port.unwired_value().as_easing();
        // The progress, if this curve is the one under way.
        let output = |name: &str| {
            let index = node.outputs.iter().position(|port| port.name == name)?;
            let playback = playbacks
                .iter()
                .find(|playback| same_graph_path(&playback.graph, &view.path))?;
            let state = playback.node(&format!("{}{}", view.prefix(), node.id))?;
            Some(state.outputs.get(index)?.as_f32())
        };
        let progress = output("easing")
            .filter(|&easing| easing == target.input as f32)
            .and(output("progress"));
        if let Ok(mut border) = borders.get_mut(plot) {
            let color = if progress.is_some() {
                ACTIVE
            } else {
                FIELD_BORDER
            };
            border.set_if_neq(BorderColor::all(color));
        }
        for &child in children {
            let Ok((mut part, stroke, handle, mark)) = parts.get_mut(child) else {
                continue;
            };
            if let Some((stroke, material)) = stroke {
                draw_stroke(*stroke, &easing, &mut part, &material.0, &mut materials);
            }
            if let Some(EasingHandle(h)) = handle {
                place(&mut part, handles(&easing)[*h], HANDLE);
            }
            if mark {
                set_display(&mut part, progress.is_some());
                if let Some(t) = progress {
                    place(&mut part, Vec2::new(t, easing.at(t)), MARK);
                }
            }
        }
    }
}

/// A double click on a curve zooms onto it.
fn zoom_onto_plot(
    mut click: On<PointerClick>,
    plots: Query<(), With<EasingPlot>>,
    targets: Targets,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count != 2 || !plots.contains(click.entity) {
        return;
    }
    if let Some(target) = targets.of(click.entity) {
        click.propagate(false);
        focus_on(&mut commands, target.canvas, click.entity);
    }
}

/// Dragging a handle moves it, held within the curve's square.
fn drag_handle(
    mut drag: On<PointerDrag>,
    handles: Query<(&EasingHandle, &ChildOf)>,
    plots: Query<(&ComputedNode, &UiGlobalTransform), With<EasingPlot>>,
    ui_scale: Res<UiScale>,
    targets: Targets,
    mut views: Query<&mut FlowGraphView>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((handle, child_of)) = handles.get(drag.entity) else {
        return;
    };
    drag.propagate(false);
    let plot = child_of.parent();
    let Ok((computed, transform)) = plots.get(plot) else {
        return;
    };
    // Window pointer positions are logical; the plot's are physical.
    let scale = computed.inverse_scale_factor();
    let physical = drag.pointer.position / (scale * ui_scale.0);
    let Some(normalized) = computed.normalize_point(*transform, physical) else {
        return;
    };
    let point = Vec2::new(normalized.x + 0.5, 0.5 - normalized.y).clamp(Vec2::ZERO, Vec2::ONE);
    let Some(target) = targets.of(plot) else {
        return;
    };
    let which = handle.0;
    edit_value(&mut views, &target, |value| {
        let mut easing = value.as_easing();
        match which {
            0 => easing.start = point,
            _ => easing.end = point,
        }
        FlowValue::Easing(easing)
    });
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input_focus::InputFocus;
    use bevy::scene::ScenePlugin;
    use bevy_noodle::prelude::*;

    use super::*;
    use crate::fields::ValueFieldsPlugin;
    use crate::model::{FlowGraph, FlowNode, PortDef, PortKind};
    use crate::run::FlowNodeState;
    use crate::view::spawn_flow_graph_view;

    const PATH: &str = "flow/test.flowgraph.bsn";

    /// A node easing with its one input, saying so on its outputs.
    fn graph(easing: Easing) -> FlowGraph {
        FlowGraph {
            nodes: vec![FlowNode {
                id: "gait".into(),
                kind: "test/gait".into(),
                title: "Gait".into(),
                inputs: vec![PortDef {
                    value: Some(FlowValue::Easing(easing)),
                    ..PortDef::new("stop", PortKind::Easing)
                }],
                outputs: vec![
                    PortDef::new("easing", PortKind::Float),
                    PortDef::new("progress", PortKind::Float),
                ],
                ..default()
            }],
            ..default()
        }
    }

    fn app(easing: Easing) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
        ))
        .init_resource::<InputFocus>()
        // The default style's, drawing the strokes.
        .init_asset::<WireMaterial>()
        .add_plugins((ValueFieldsPlugin, EasingFieldsPlugin));
        let root = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(&mut commands, root, PATH, graph(easing))
            })
            .unwrap();
        app.update();
        app.update();
        app
    }

    /// Where the parts marked `M` are, by their top left corner.
    fn placed<M: Component>(app: &mut App) -> Vec<(Val, Val, Display)> {
        app.world_mut()
            .query_filtered::<&Node, With<M>>()
            .iter(app.world())
            .map(|node| (node.left, node.top, node.display))
            .collect()
    }

    #[test]
    fn an_easing_input_draws_its_curve_and_handles() {
        let easing = Easing::new(0.3, Vec2::new(0.25, 0.0), Vec2::new(0.5, 1.0));
        let mut app = app(easing);
        assert_eq!(placed::<EasingPlot>(&mut app).len(), 1);
        let mut handles: Vec<(usize, Val, Val)> = app
            .world_mut()
            .query::<(&EasingHandle, &Node)>()
            .iter(app.world())
            .map(|(handle, node)| (handle.0, node.left, node.top))
            .collect();
        handles.sort_by_key(|(which, ..)| *which);
        let half = HANDLE / 2.0;
        assert_eq!(
            handles,
            [
                (0, px(0.25 * PLOT_SIZE.x - half), px(PLOT_SIZE.y - half)),
                (1, px(0.5 * PLOT_SIZE.x - half), px(-half)),
            ]
        );
        assert_eq!(placed::<PlotStroke>(&mut app).len(), 3);
        // Nothing plays: no mark.
        assert!(
            placed::<ProgressMark>(&mut app)
                .iter()
                .all(|(.., display)| *display == Display::None)
        );
    }

    #[test]
    fn the_curve_shows_while_its_node_is_selected() {
        use crate::fields::ValueFields;
        use bevy::ui::Selected;
        let mut app = app(Easing::LINEAR);
        let shown = |app: &mut App| placed::<ValueFields>(app)[0].2 != Display::None;
        assert!(!shown(&mut app), "ports alone");
        let node = app
            .world_mut()
            .query_filtered::<Entity, With<crate::view::ViewNode>>()
            .single(app.world())
            .unwrap();
        app.world_mut().entity_mut(node).insert(Selected);
        app.update();
        assert!(shown(&mut app));
        app.world_mut().entity_mut(node).remove::<Selected>();
        app.update();
        assert!(!shown(&mut app));
    }

    #[test]
    fn the_curve_under_way_is_marked_where_it_is() {
        let mut app = app(Easing::LINEAR);
        app.world_mut().spawn(FlowGraphPlayback {
            graph: PATH.into(),
            nodes: vec![FlowNodeState {
                id: "gait".into(),
                inputs: vec![],
                outputs: vec![FlowValue::Float(0.0), FlowValue::Float(0.5)],
            }],
        });
        app.update();
        let half = MARK / 2.0;
        let [(left, Val::Px(top), Display::Flex)] = placed::<ProgressMark>(&mut app)[..] else {
            panic!("one mark, shown");
        };
        assert_eq!(left, px(0.5 * PLOT_SIZE.x - half));
        assert!((top - (0.5 * PLOT_SIZE.y - half)).abs() < 0.01, "{top}");
    }
}
