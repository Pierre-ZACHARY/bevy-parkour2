//! Levels: a node holding a graph (a state machine's) draws it inside, live
//! and read-only. A double click on the node, or Enter with it selected,
//! opens it: the canvas zooms into the preview, then shows the graph inside
//! to edit in its place. Escape or Backspace goes back up, zooming out onto
//! the node; so does a click on a level in the breadcrumb over the canvas.
//!
//! Edits inside go back into the node holding the graph:
//! [`FlowGraphView::root_graph`] is the file's graph with them.
//!
//! A double click on any other node frames it, and one on an easing's
//! curve zooms onto the curve, to work on it; Escape zooms back out.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_noodle::prelude::*;

use crate::fields::typing;
use crate::ties::TieSelection;
use crate::view::{
    FlowGraphView, GraphPreview, OuterLevel, ViewNode, canvases_under_pointer, draw_level,
};

/// The size of the preview inside a node holding a graph.
pub(crate) const PREVIEW_SIZE: Vec2 = Vec2::new(260.0, 150.0);
/// How long zooming into a node, or out of one, takes, in seconds.
const ZOOM_TIME: f32 = 0.25;
/// Room around a graph fitted into its canvas, as a share of the canvas.
const MARGIN: f32 = 0.08;
/// How far a double click zooms onto a node or a curve, at most.
const FOCUS_ZOOM: f32 = 4.0;

pub(crate) struct LevelsPlugin;

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(open_on_double_click)
            .add_observer(go_to_crumb)
            .add_systems(
                Update,
                (
                    navigate_with_keys,
                    fit_once_laid_out,
                    zoom,
                    show_breadcrumbs,
                )
                    .chain(),
            );
    }
}

/// On a canvas zooming from one view to another: into a node, to open it
/// once there, or back out of one.
#[derive(Component, Clone, Debug)]
struct Zoom {
    from: CanvasView,
    to: CanvasView,
    elapsed: f32,
    /// The node to open once zoomed in.
    open: Option<String>,
}

/// On a canvas to fit around its nodes once they are laid out: a preview,
/// or a view showing a new level.
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct FitOnceLaidOut {
    /// Whether to zoom there rather than jump.
    pub zoom: bool,
}

/// On a canvas zoomed onto a node: where it looked before, to go back.
#[derive(Component, Clone, Debug)]
struct Focused {
    before: CanvasView,
}

/// Over a view's canvas: the levels it went through, to go back to one.
#[derive(Component)]
struct Breadcrumb {
    canvas: Entity,
    /// The level names it shows.
    shown: Vec<String>,
}

/// A level in a breadcrumb: a click goes back up to it.
#[derive(Component)]
struct Crumb {
    canvas: Entity,
    /// How many levels down it is.
    depth: usize,
}

/// Open node `id` of the level `canvas` shows, if it holds a graph: zoom
/// into its preview, if it is laid out, then show its graph.
pub fn open_level(commands: &mut Commands, canvas: Entity, id: impl Into<String>) {
    let id = id.into();
    commands.queue(move |world: &mut World| start_opening(world, canvas, id));
}

/// Show level `depth` of `canvas`'s view again (0: the file's graph),
/// zooming out of the node it was left through.
pub fn close_level(commands: &mut Commands, canvas: Entity, depth: usize) {
    commands.queue(move |world: &mut World| leave(world, canvas, depth));
}

/// Zoom `canvas` onto its node `id`, once laid out; Escape zooms back.
pub fn focus_node(commands: &mut Commands, canvas: Entity, id: impl Into<String>) {
    let id = id.into();
    commands.queue(move |world: &mut World| {
        let node = world
            .run_system_cached_with(find_node, (canvas, id))
            .ok()
            .flatten();
        if let Some(node) = node {
            start_focusing(world, canvas, node);
        }
    });
}

/// Zoom `canvas` onto `part`, something drawn on it (a node, a field).
pub(crate) fn focus_on(commands: &mut Commands, canvas: Entity, part: Entity) {
    commands.queue(move |world: &mut World| start_focusing(world, canvas, part));
}

fn start_focusing(world: &mut World, canvas: Entity, part: Entity) {
    if world.get::<Zoom>(canvas).is_some() {
        return;
    }
    let rect = rect_on_canvas(world, canvas, part);
    let (Some(rect), Some(size)) = (rect, canvas_size(world, canvas)) else {
        return;
    };
    let from = world.get::<CanvasView>(canvas).copied().unwrap_or_default();
    let to = fit(&from, rect, size, MARGIN, FOCUS_ZOOM);
    let before = world
        .get::<Focused>(canvas)
        .map_or(from, |focused| focused.before);
    world.entity_mut(canvas).insert((
        Focused { before },
        Zoom {
            from,
            to,
            elapsed: 0.0,
            open: None,
        },
    ));
}

/// Zoom `canvas` back to where it looked before zooming onto a node.
fn unfocus(world: &mut World, canvas: Entity) {
    let Some(Focused { before }) = world.entity_mut(canvas).take::<Focused>() else {
        return;
    };
    let from = world.get::<CanvasView>(canvas).copied().unwrap_or_default();
    world.entity_mut(canvas).insert(Zoom {
        from,
        to: before,
        elapsed: 0.0,
        open: None,
    });
}

fn start_opening(world: &mut World, canvas: Entity, id: String) {
    let Some(view) = world.get::<FlowGraphView>(canvas) else {
        return;
    };
    if view
        .graph
        .node(&id)
        .and_then(|node| node.graph.as_ref())
        .is_none()
        || world.get::<Zoom>(canvas).is_some()
    {
        return;
    }
    let target = preview_of(world, canvas, &id).zip(canvas_size(world, canvas));
    let Some(((rect, _), size)) = target else {
        return enter(world, canvas, &id, None);
    };
    let from = world.get::<CanvasView>(canvas).copied().unwrap_or_default();
    let to = fit(&from, rect, size, 0.0, f32::INFINITY);
    world.entity_mut(canvas).insert(Zoom {
        from,
        to,
        elapsed: 0.0,
        open: Some(id),
    });
}

/// Show the graph inside node `id` in its place. `before` is where the
/// canvas looked before zooming in, to zoom back out to.
fn enter(world: &mut World, canvas: Entity, id: &str, before: Option<CanvasView>) {
    let current = world.get::<CanvasView>(canvas).copied().unwrap_or_default();
    // The inner graph shows where its preview showed it, then fits.
    let inner_view = preview_of(world, canvas, id).map(|(rect, preview)| CanvasView {
        pan: current.graph_to_canvas(rect.min) + preview.pan * current.zoom,
        zoom: preview.zoom * current.zoom,
        ..current
    });
    let Some(mut view) = world.get_mut::<FlowGraphView>(canvas) else {
        return;
    };
    let Some(inner) = view.graph.node(id).and_then(|node| node.graph.clone()) else {
        return;
    };
    let outer = std::mem::replace(&mut view.graph, inner);
    view.outer.push(OuterLevel {
        node: id.to_owned(),
        graph: outer,
        left_at: before.map(|before| (before, current)),
    });
    redraw(world, canvas);
    world.entity_mut(canvas).remove::<Focused>();
    let fitting = FitOnceLaidOut {
        zoom: inner_view.is_some(),
    };
    let start = inner_view.unwrap_or(CanvasView {
        pan: Vec2::ZERO,
        zoom: 1.0,
        ..current
    });
    world.entity_mut(canvas).insert((start, fitting));
}

fn leave(world: &mut World, canvas: Entity, depth: usize) {
    let Some(mut view) = world.get_mut::<FlowGraphView>(canvas) else {
        return;
    };
    if depth >= view.outer.len() {
        return;
    }
    let mut graph = view.graph.clone();
    let mut left_at = None;
    while view.outer.len() > depth {
        let Some(level) = view.outer.pop() else {
            break;
        };
        let mut outer = level.graph;
        if let Some(node) = outer.nodes.iter_mut().find(|node| node.id == level.node) {
            node.graph = Some(graph);
        }
        graph = outer;
        left_at = level.left_at;
    }
    view.graph = graph;
    redraw(world, canvas);
    let mut canvas = world.entity_mut(canvas);
    canvas.remove::<(Zoom, Focused)>();
    match left_at {
        Some((before, entered)) => {
            canvas.insert((
                entered,
                Zoom {
                    from: entered,
                    to: before,
                    elapsed: 0.0,
                    open: None,
                },
            ));
        }
        None => {
            canvas.insert(FitOnceLaidOut::default());
        }
    }
}

/// Draw the level `canvas`'s view shows in place of what it drew.
fn redraw(world: &mut World, canvas: Entity) {
    let drawn = world
        .run_system_cached_with(drawn_on, canvas)
        .unwrap_or_default();
    for entity in drawn {
        // Edges first: they belong to no node.
        _ = world.try_despawn(entity);
    }
    if let Some(mut selection) = world.get_mut::<TieSelection>(canvas) {
        *selection = TieSelection::default();
    }
    let Some(view) = world.get::<FlowGraphView>(canvas) else {
        return;
    };
    let (path, prefix, graph) = (view.path.clone(), view.prefix(), view.graph.clone());
    draw_level(&mut world.commands(), canvas, &path, &prefix, &graph);
    world.flush();
}

/// What `canvas` draws: the edges of its own and its previews', then its
/// nodes (with their previews).
fn drawn_on(
    In(canvas): In<Entity>,
    graph: GraphQuery,
    previews: Query<&ChildOf, With<GraphPreview>>,
    preview_canvases: Query<Entity, With<GraphPreview>>,
) -> Vec<Entity> {
    let previews: Vec<Entity> = preview_canvases
        .iter()
        .filter(|&preview| {
            previews
                .get(preview)
                .is_ok_and(|child_of| graph.canvas_of(child_of.parent()) == Some(canvas))
        })
        .collect();
    let edges = previews
        .iter()
        .flat_map(|&preview| graph.edges_in(preview))
        .chain(graph.edges_in(canvas));
    edges.chain(graph.nodes_in(canvas)).collect()
}

/// The size of `canvas`, in logical pixels, once laid out.
fn canvas_size(world: &World, canvas: Entity) -> Option<Vec2> {
    let computed = world.get::<ComputedNode>(canvas)?;
    let size = computed.size() * computed.inverse_scale_factor();
    (size.min_element() > 0.0).then_some(size)
}

/// The preview inside node `id` of `canvas`, once laid out: where it is in
/// the canvas's graph space, and its own view.
fn preview_of(world: &mut World, canvas: Entity, id: &str) -> Option<(Rect, CanvasView)> {
    let preview = world
        .run_system_cached_with(find_preview, (canvas, id.to_owned()))
        .ok()??;
    let rect = rect_on_canvas(world, canvas, preview)?;
    Some((rect, *world.get::<CanvasView>(preview)?))
}

/// Where `part`, drawn on `canvas` at the graph's scale (a node, or
/// something in one), is in the canvas's graph space, once laid out.
fn rect_on_canvas(world: &World, canvas: Entity, part: Entity) -> Option<Rect> {
    let view = *world.get::<CanvasView>(canvas)?;
    let [(canvas_node, canvas_at), (part_node, part_at)] = [canvas, part].map(|e| {
        (
            world.get::<ComputedNode>(e).copied(),
            world.get::<UiGlobalTransform>(e).copied(),
        )
    });
    let (canvas_node, canvas_at) = (canvas_node?, canvas_at?);
    let (part_node, part_at) = (part_node?, part_at?);
    let scale = canvas_node.inverse_scale_factor();
    let size = part_node.size() * part_node.inverse_scale_factor();
    if size.min_element() <= 0.0 || canvas_node.size().min_element() <= 0.0 {
        return None;
    }
    // Centers, in physical pixels; the canvas itself is not transformed.
    let offset = (part_at.translation - canvas_at.translation) * scale;
    let center = view.canvas_to_graph(canvas_node.size() * scale / 2.0 + offset);
    Some(Rect::from_center_size(center, size))
}

/// The canvas node drawing graph node `id` on `canvas`.
fn find_node(
    In((canvas, id)): In<(Entity, String)>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
) -> Option<Entity> {
    graph
        .nodes_in(canvas)
        .find(|&node| view_nodes.get(node).is_ok_and(|view| view.0 == id))
}

fn find_preview(
    In((canvas, id)): In<(Entity, String)>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    children: Query<&Children>,
    previews: Query<(), With<GraphPreview>>,
) -> Option<Entity> {
    let node = graph
        .nodes_in(canvas)
        .find(|&node| view_nodes.get(node).is_ok_and(|view| view.0 == id))?;
    children
        .get(node)
        .ok()?
        .iter()
        .find(|&child| previews.contains(child))
}

/// The view of `current`'s canvas, of size `size`, fitting `rect` (graph
/// space) with `margin` around it, zoomed in no further than `max_zoom`.
fn fit(current: &CanvasView, rect: Rect, size: Vec2, margin: f32, max_zoom: f32) -> CanvasView {
    let room = size * (1.0 - 2.0 * margin);
    let zoom = (room / rect.size().max(Vec2::ONE))
        .min_element()
        .min(max_zoom);
    CanvasView {
        pan: size / 2.0 - rect.center() * zoom,
        zoom,
        ..*current
    }
}

/// Fit the canvases waiting for it around their nodes, once laid out.
fn fit_once_laid_out(
    mut canvases: Query<(Entity, &FitOnceLaidOut, &mut CanvasView, &ComputedNode)>,
    graph: GraphQuery,
    nodes: Query<(&NodePosition, &ComputedNode)>,
    mut commands: Commands,
) {
    for (canvas, fitting, mut view, computed) in &mut canvases {
        let size = computed.size() * computed.inverse_scale_factor();
        if size.min_element() <= 0.0 {
            continue;
        }
        let mut bounds: Option<Rect> = None;
        let mut laid_out = true;
        for node in graph.nodes_in(canvas) {
            let Ok((position, computed)) = nodes.get(node) else {
                continue;
            };
            let node_size = computed.size() * computed.inverse_scale_factor();
            laid_out &= node_size.min_element() > 0.0;
            let rect = Rect::from_corners(position.0, position.0 + node_size);
            bounds = Some(bounds.map_or(rect, |bounds| bounds.union(rect)));
        }
        if !laid_out {
            continue;
        }
        let mut entity = commands.entity(canvas);
        entity.remove::<FitOnceLaidOut>();
        let Some(bounds) = bounds else {
            continue;
        };
        let fitted = fit(&view, bounds, size, MARGIN, 1.0);
        if fitting.zoom {
            entity.insert(Zoom {
                from: *view,
                to: fitted,
                elapsed: 0.0,
                open: None,
            });
        } else {
            *view = fitted;
        }
    }
}

/// Move zooming canvases along, opening the node zoomed into once there.
fn zoom(
    time: Res<Time>,
    mut canvases: Query<(Entity, &mut Zoom, &mut CanvasView, &ComputedNode)>,
    mut commands: Commands,
) {
    for (canvas, mut zoom, mut view, computed) in &mut canvases {
        zoom.elapsed += time.delta_secs();
        let t = (zoom.elapsed / ZOOM_TIME).min(1.0);
        let size = computed.size() * computed.inverse_scale_factor();
        *view = between(&zoom.from, &zoom.to, t * t * (3.0 - 2.0 * t), size);
        if t < 1.0 {
            continue;
        }
        commands.entity(canvas).remove::<Zoom>();
        if let Some(id) = zoom.open.take() {
            let before = zoom.from;
            commands.queue(move |world: &mut World| enter(world, canvas, &id, Some(before)));
        }
    }
}

/// The view `t` of the way from `from` to `to`, on a canvas of `size`: the
/// zoom changes at a steady rate, and the graph point at the canvas's
/// center moves with it.
fn between(from: &CanvasView, to: &CanvasView, t: f32, size: Vec2) -> CanvasView {
    let zoom = (from.zoom.ln() + (to.zoom.ln() - from.zoom.ln()) * t).exp();
    let center = from
        .canvas_to_graph(size / 2.0)
        .lerp(to.canvas_to_graph(size / 2.0), t);
    CanvasView {
        pan: size / 2.0 - center * zoom,
        zoom,
        ..*to
    }
}

/// A double click on a node holding a graph opens it, through its preview
/// too; on another node, zooms onto it.
fn open_on_double_click(
    mut click: On<PointerClick>,
    views: Query<&FlowGraphView>,
    view_nodes: Query<&ViewNode>,
    graph: GraphQuery,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count != 2 {
        return;
    }
    let Ok(id) = view_nodes.get(click.entity) else {
        return;
    };
    // A preview's nodes are on its canvas: the click goes on to the node
    // holding it.
    let Some(canvas) = graph.canvas_of(click.entity) else {
        return;
    };
    let Ok(view) = views.get(canvas) else {
        return;
    };
    let Some(node) = view.graph.node(&id.0) else {
        return;
    };
    click.propagate(false);
    if node.graph.is_some() {
        open_level(&mut commands, canvas, id.0.clone());
    } else {
        focus_node(&mut commands, canvas, id.0.clone());
    }
}

/// Over a view: Enter opens the one selected node holding a graph; Escape
/// or Backspace zooms back out of a node zoomed onto, else goes up a
/// level.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn navigate_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    hovers: Option<Res<HoverMap>>,
    views: Query<&FlowGraphView>,
    view_nodes: Query<&ViewNode>,
    graph: GraphQuery,
    focus: Option<Res<InputFocus>>,
    (texts, focused): (Query<(), With<EditableText>>, Query<(), With<Focused>>),
    mut commands: Commands,
) {
    let open = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    let up = keys.any_just_pressed([KeyCode::Escape, KeyCode::Backspace]);
    if !(open || up) || focus.is_some_and(|focus| typing(&focus, &texts)) {
        return;
    }
    let Some(hovers) = hovers else {
        return;
    };
    for canvas in canvases_under_pointer(&hovers, &graph, |canvas| views.contains(canvas)) {
        let Ok(view) = views.get(canvas) else {
            continue;
        };
        if up && focused.contains(canvas) {
            commands.queue(move |world: &mut World| unfocus(world, canvas));
        } else if up && !view.outer.is_empty() {
            close_level(&mut commands, canvas, view.outer.len() - 1);
        }
        if open {
            let holders: Vec<&str> = graph
                .selected_in(canvas)
                .filter_map(|node| view_nodes.get(node).ok())
                .filter(|id| view.graph.node(&id.0).is_some_and(|n| n.graph.is_some()))
                .map(|id| id.0.as_str())
                .collect();
            if let [id] = holders[..] {
                open_level(&mut commands, canvas, id);
            }
        }
    }
}

/// Over the top left of `canvas`, in `parent`: the levels its view went
/// through.
pub(crate) fn spawn_breadcrumb(commands: &mut Commands, parent: Entity, canvas: Entity) {
    commands.spawn((
        Breadcrumb {
            canvas,
            shown: Vec::new(),
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            column_gap: px(4),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
        ZIndex(5),
        ChildOf(parent),
    ));
}

/// The names of the levels `view` went through: its file's, then the
/// titles of the nodes opened.
pub fn level_names(view: &FlowGraphView) -> Vec<String> {
    let file = view.path.rsplit(['/', '\\']).next().unwrap_or_default();
    let file = file.split('.').next().unwrap_or(file).to_owned();
    let opened = view.outer.iter().map(|level| {
        level
            .graph
            .node(&level.node)
            .map_or(level.node.clone(), |node| node.title.clone())
    });
    std::iter::once(file).chain(opened).collect()
}

fn show_breadcrumbs(
    views: Query<&FlowGraphView>,
    mut breadcrumbs: Query<(Entity, &mut Breadcrumb)>,
    mut commands: Commands,
) {
    for (bar, mut breadcrumb) in &mut breadcrumbs {
        let Ok(view) = views.get(breadcrumb.canvas) else {
            continue;
        };
        let names = level_names(view);
        if names == breadcrumb.shown {
            continue;
        }
        commands.entity(bar).despawn_related::<Children>();
        let last = names.len() - 1;
        for (depth, name) in names.iter().enumerate() {
            if depth > 0 {
                commands.spawn((
                    Text::new(">"),
                    TextFont::from_font_size(13.0),
                    TextColor(Color::srgb_u8(120, 124, 134)),
                    Pickable::IGNORE,
                    ChildOf(bar),
                ));
            }
            let here = depth == last;
            let mut crumb = commands.spawn((
                Text::new(name.clone()),
                TextFont::from_font_size(13.0),
                TextColor(if here {
                    Color::srgb_u8(236, 238, 242)
                } else {
                    Color::srgb_u8(160, 166, 178)
                }),
                Node {
                    padding: UiRect::axes(px(8), px(3)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgba_u8(44, 46, 52, 230)),
                ChildOf(bar),
            ));
            if !here {
                crumb.insert((
                    Crumb {
                        canvas: breadcrumb.canvas,
                        depth,
                    },
                    bevy::picking::cursor::EntityCursor::System(
                        bevy::window::SystemCursorIcon::Pointer,
                    ),
                ));
            }
        }
        breadcrumb.shown = names;
    }
}

fn go_to_crumb(mut click: On<PointerClick>, crumbs: Query<&Crumb>, mut commands: Commands) {
    if click.button != PointerButton::Primary {
        return;
    }
    if let Ok(crumb) = crumbs.get(click.entity) {
        click.propagate(false);
        close_level(&mut commands, crumb.canvas, crumb.depth);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::scene::ScenePlugin;

    use super::*;
    use crate::model::{FlowGraph, FlowLink, FlowNode, FlowValue, PortDef, PortKind};
    use crate::run::{FlowGraphPlayback, FlowNodeState};
    use crate::view::{ACTIVE, light_up, spawn_flow_graph_view, write_back_edits};

    fn node(id: &str, inputs: Vec<PortDef>, outputs: Vec<PortDef>) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: format!("test/{id}"),
            title: id.to_uppercase(),
            inputs,
            outputs,
            ..default()
        }
    }

    /// a → b, inside the machine.
    fn inner() -> FlowGraph {
        FlowGraph {
            nodes: vec![
                node("a", vec![], vec![PortDef::new("out", PortKind::Float)]),
                node("b", vec![PortDef::new("in", PortKind::Float)], vec![]),
            ],
            links: vec![FlowLink::new("a", "out", "b", "in")],
            ..default()
        }
    }

    fn outer() -> FlowGraph {
        FlowGraph {
            nodes: vec![
                node("x", vec![], vec![PortDef::new("out", PortKind::Float)]),
                FlowNode {
                    graph: Some(inner()),
                    ..node("machine", vec![], vec![])
                },
            ],
            ..default()
        }
    }

    /// Headless: no layout, so levels open and close without zooming.
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
        ))
        .add_systems(
            Update,
            (write_back_edits, light_up, show_breadcrumbs).chain(),
        );
        let root = app.world_mut().spawn(Node::default()).id();
        let canvas = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(&mut commands, root, "flow/test.flowgraph.bsn", outer())
            })
            .unwrap();
        app.update();
        (app, canvas)
    }

    fn view(app: &App, canvas: Entity) -> FlowGraphView {
        app.world().get::<FlowGraphView>(canvas).unwrap().clone()
    }

    fn open(app: &mut App, canvas: Entity, id: &'static str) {
        app.world_mut()
            .run_system_once(move |mut commands: Commands| open_level(&mut commands, canvas, id))
            .unwrap();
        app.update();
    }

    fn close(app: &mut App, canvas: Entity, depth: usize) {
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                close_level(&mut commands, canvas, depth)
            })
            .unwrap();
        app.update();
    }

    /// The ids of the nodes drawn on `canvas`, and its edge count.
    fn drawn(app: &mut App, canvas: Entity) -> (Vec<String>, usize) {
        app.world_mut()
            .run_system_once(move |graph: GraphQuery, nodes: Query<&ViewNode>| {
                let ids = graph
                    .nodes_in(canvas)
                    .map(|node| nodes.get(node).unwrap().0.clone())
                    .collect();
                (ids, graph.edges_in(canvas).count())
            })
            .unwrap()
    }

    fn previews(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<GraphPreview>>()
            .iter(app.world())
            .collect()
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn a_node_holding_a_graph_previews_it() {
        let (mut app, canvas) = app();
        assert_eq!(drawn(&mut app, canvas), (ids(&["x", "machine"]), 0));
        let [preview] = previews(&mut app)[..] else {
            panic!("one preview");
        };
        assert_eq!(drawn(&mut app, preview), (ids(&["a", "b"]), 1));
        let shown = app.world().get::<GraphPreview>(preview).unwrap();
        assert_eq!(shown.prefix, "machine/");
        assert_eq!(shown.graph, inner());
    }

    #[test]
    fn opening_a_node_shows_its_graph_and_closing_keeps_the_edits() {
        let (mut app, canvas) = app();
        // Nothing inside x.
        open(&mut app, canvas, "x");
        assert!(view(&app, canvas).outer.is_empty());

        open(&mut app, canvas, "machine");
        let inside = view(&app, canvas);
        assert_eq!(inside.graph, inner());
        assert_eq!(inside.prefix(), "machine/");
        assert_eq!(inside.root_graph(), outer());
        assert_eq!(level_names(&inside), ["test", "MACHINE"]);
        assert_eq!(drawn(&mut app, canvas), (ids(&["a", "b"]), 1));
        // The outer level's preview went with it.
        assert!(previews(&mut app).is_empty());

        let edge = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery| graph.edges_in(canvas).next())
            .unwrap()
            .unwrap();
        app.world_mut()
            .graph_edit(canvas, GraphEdit::Disconnect { edge })
            .unwrap();
        app.update();
        let edited = view(&app, canvas);
        assert!(edited.dirty);
        assert!(edited.graph.links.is_empty());
        let machine = edited.root_graph().node("machine").unwrap().clone();
        assert!(machine.graph.unwrap().links.is_empty());

        close(&mut app, canvas, 0);
        let back = view(&app, canvas);
        assert!(back.outer.is_empty());
        assert_eq!(back.graph, edited.root_graph());
        assert_eq!(drawn(&mut app, canvas), (ids(&["x", "machine"]), 0));
        let [preview] = previews(&mut app)[..] else {
            panic!("one preview");
        };
        assert_eq!(drawn(&mut app, preview), (ids(&["a", "b"]), 0));
    }

    #[test]
    fn previews_and_inner_levels_light_from_the_prefixed_playback() {
        let (mut app, canvas) = app();
        let state = |id: &str, outputs: Vec<FlowValue>| FlowNodeState {
            id: id.into(),
            inputs: vec![FlowValue::Float(1.0)],
            outputs,
        };
        app.world_mut().spawn(FlowGraphPlayback {
            graph: "flow/test.flowgraph.bsn".into(),
            nodes: vec![
                state("machine/a", vec![FlowValue::Float(1.0)]),
                state("machine/b", vec![]),
            ],
        });
        app.update();
        let lit = |app: &mut App, canvas: Entity| {
            app.world_mut()
                .run_system_once(
                    move |graph: GraphQuery, borders: Query<&SelectedBorderColor>| {
                        graph
                            .nodes_in(canvas)
                            .map(|node| borders.get(node).unwrap().normal == ACTIVE)
                            .collect::<Vec<_>>()
                    },
                )
                .unwrap()
        };
        // Neither x nor the machine: the states are the inner graph's.
        assert_eq!(lit(&mut app, canvas), [false, false]);
        let preview = previews(&mut app)[0];
        assert_eq!(lit(&mut app, preview), [true, true]);

        open(&mut app, canvas, "machine");
        app.update();
        assert_eq!(lit(&mut app, canvas), [true, true]);
    }

    #[test]
    fn the_breadcrumb_names_the_levels_above_to_go_back_to() {
        let (mut app, canvas) = app();
        let crumbs = |app: &mut App| {
            app.world_mut()
                .query::<(&Crumb, &Text)>()
                .iter(app.world())
                .map(|(crumb, text)| (crumb.depth, text.0.clone()))
                .collect::<Vec<_>>()
        };
        // The level shown is not one to go to.
        assert!(crumbs(&mut app).is_empty());
        open(&mut app, canvas, "machine");
        app.update();
        assert_eq!(crumbs(&mut app), [(0, "test".to_owned())]);
    }

    #[test]
    fn fitting_frames_a_rect_in_the_middle() {
        let rect = Rect::new(100.0, 100.0, 300.0, 200.0);
        let size = Vec2::new(800.0, 400.0);
        let view = fit(&CanvasView::default(), rect, size, 0.0, 10.0);
        assert_eq!(view.zoom, 4.0);
        assert_eq!(view.graph_to_canvas(rect.center()), size / 2.0);
        // Halfway, the zoom has doubled once of twice.
        let halfway = between(&CanvasView::default(), &view, 0.5, size);
        assert!((halfway.zoom - 2.0).abs() < 1e-5);
    }
}
