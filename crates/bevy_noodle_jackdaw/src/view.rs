//! A graph drawn as a bevy_noodle canvas: edits write back to the graph,
//! and a running graph's [`FlowGraphPlayback`] lights up its active nodes
//! and the wires carrying a value, in the game or (through play-in-editor)
//! in the editor.
//!
//! The view needs no node registry: it draws the titles and ports each
//! [`FlowNode`] carries. [`spawn_flow_graph_editor`] adds a node library on
//! its left, listing a [`FlowNodeCatalog`].

use bevy::input_focus::InputFocus;
use bevy::picking::cursor::CursorIconPlugin;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

pub use crate::arrows::FlowArrows;
use crate::arrows::{ArrowEdge, ArrowNode, ArrowsPlugin, arrow_style, spawn_arrow};
use crate::axis::spawn_blend_axis;
pub use crate::axis::{BlendPreview, BlendPreviewSlot};
use crate::blend::{has_samples, is_2d};
use crate::current::CurrentStatePlugin;
pub use crate::current::{CURRENT, CurrentState};
use crate::dots::{DotGrid, DotGridPlugin};
use crate::fields::{InputRow, ValueFieldsPlugin, typing};
use crate::levels::{FitOnceLaidOut, LevelsPlugin, PREVIEW_SIZE, spawn_breadcrumb};
use crate::library::{NodeLibraryPlugin, spawn_node_library};
use crate::model::{
    FlowGraph, FlowLink, FlowNode, FlowNodeCatalog, FlowTie, FlowValue, PortDef, PortKind,
};
use crate::run::{FlowGraphPlayback, FlowNodeState};
use crate::ties::{TieSelection, ZipTiesPlugin};
use crate::wires::{AvoidNodes, WireLayoutPlugin, WireShape, spawn_wire_buttons};

/// Border of a node something went through, and colour of a wire carrying a
/// value.
pub const ACTIVE: Color = Color::srgb(1.0, 0.72, 0.2);

pub struct FlowGraphViewPlugin;

impl Plugin for FlowGraphViewPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<CursorIconPlugin>() {
            app.add_plugins(CursorIconPlugin);
        }
        if !app.is_plugin_added::<NoodleCorePlugin>() {
            app.add_plugins(NoodlePlugins);
        }
        if !app.is_plugin_added::<NoodleDefaultStylePlugin>() {
            app.add_plugins(NoodleDefaultStylePlugin);
        }
        app.add_plugins((
            ArrowsPlugin,
            CurrentStatePlugin,
            DotGridPlugin,
            LevelsPlugin,
            NodeLibraryPlugin,
            ValueFieldsPlugin,
            crate::curves::EasingFieldsPlugin,
            crate::axis::BlendAxisPlugin,
            WireLayoutPlugin,
            ZipTiesPlugin,
        ))
        .add_systems(
            Update,
            (delete_selection, write_back_edits, light_up).chain(),
        );
    }
}

/// On a canvas drawing a graph: the graph as edited so far.
///
/// It shows one level at a time: the file's graph, or the graph inside one
/// of its nodes, opened from there (see [`crate::levels`]).
#[derive(Component, Clone, Debug)]
pub struct FlowGraphView {
    /// The graph's asset path; playbacks for this path light the view.
    pub path: String,
    /// The level shown, as edited so far.
    pub graph: FlowGraph,
    /// The levels above the one shown, the file's graph first.
    pub outer: Vec<OuterLevel>,
    /// Whether the graph was edited since it was drawn (or last saved).
    pub dirty: bool,
}

/// A level above the one a view shows.
#[derive(Clone, Debug)]
pub struct OuterLevel {
    /// The node opened in it, holding the next level down.
    pub node: String,
    /// Its graph, as it was when left (the node's own graph is the next
    /// level's).
    pub graph: FlowGraph,
    /// Where the canvas looked when the level was left, and where it had
    /// zoomed onto the node by then: the way back runs from one to the other.
    pub(crate) left_at: Option<(CanvasView, CanvasView)>,
}

impl FlowGraphView {
    /// The file's graph, the shown level put back in place.
    pub fn root_graph(&self) -> FlowGraph {
        let mut graph = self.graph.clone();
        for level in self.outer.iter().rev() {
            let mut outer = level.graph.clone();
            if let Some(node) = outer.nodes.iter_mut().find(|n| n.id == level.node) {
                node.graph = Some(graph);
            }
            graph = outer;
        }
        graph
    }

    /// What the shown level's node ids are prefixed with in playbacks:
    /// the ids of the nodes opened, each followed by `/`.
    pub fn prefix(&self) -> String {
        self.outer
            .iter()
            .map(|level| format!("{}/", level.node))
            .collect()
    }
}

/// On a canvas inside a node, drawing the graph the node holds: a live,
/// read-only preview, lit like the view it is in.
#[derive(Component, Clone, Debug)]
pub struct GraphPreview {
    /// The path of the view's graph.
    pub path: String,
    /// What its nodes' ids are prefixed with in playbacks.
    pub prefix: String,
    pub graph: FlowGraph,
}

/// On a canvas node: the id of the graph node it draws.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewNode(pub String);

/// On an edge lit by [`light_up`], which put its own [`EdgeStyle`] there.
#[derive(Component)]
pub(crate) struct LitEdge;

pub fn port_type(kind: PortKind) -> PortType {
    match kind {
        PortKind::Float => PortType::named("float"),
        PortKind::Bool => PortType::named("bool"),
        PortKind::Vec2 => PortType::named("vec2"),
        PortKind::Vec3 => PortType::named("vec3"),
        PortKind::State => PortType::named("state"),
        PortKind::Easing => PortType::named("easing"),
        PortKind::Text => PortType::named("text"),
        PortKind::Entity => PortType::named("entity"),
    }
}

pub fn port_color(kind: PortKind) -> Color {
    match kind {
        PortKind::Float => Color::srgb(0.45, 0.8, 0.35),
        PortKind::Bool => Color::srgb(0.85, 0.3, 0.3),
        PortKind::Vec2 => Color::srgb(0.95, 0.8, 0.3),
        PortKind::Vec3 => Color::srgb(0.95, 0.55, 0.25),
        PortKind::State => Color::srgb(0.85, 0.85, 0.9),
        PortKind::Easing => Color::srgb(0.7, 0.45, 0.95),
        PortKind::Text => Color::srgb(0.9, 0.45, 0.7),
        PortKind::Entity => Color::srgb(0.35, 0.75, 0.9),
    }
}

/// A node's title bar: its first output's colour (its first input's for a
/// sink), dimmed.
pub fn header_color(node: &FlowNode) -> Color {
    let Some(port) = node.outputs.first().or(node.inputs.first()) else {
        return Color::srgb_u8(60, 63, 71);
    };
    let color = port_color(port.kind).to_srgba();
    Color::srgb(color.red * 0.45, color.green * 0.45, color.blue * 0.45)
}

/// Draw `graph` as a canvas filling `parent`, with a node library on its
/// left listing `catalog`, and the catalog's arrow kinds drawn as arrows
/// (see [`FlowArrows`]). Returns the canvas.
pub fn spawn_flow_graph_editor(
    commands: &mut Commands,
    parent: Entity,
    path: impl Into<String>,
    graph: FlowGraph,
    catalog: &FlowNodeCatalog,
) -> Entity {
    commands.insert_resource(FlowArrows::from_catalog(catalog));
    let row = commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                // The library, spawned last (it needs the canvas), on the left.
                flex_direction: FlexDirection::RowReverse,
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    let area = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: px(0),
                height: percent(100),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let canvas = spawn_flow_graph_view(commands, area, path, graph);
    spawn_wire_buttons(commands, area, canvas);
    spawn_node_library(commands, row, canvas, catalog);
    canvas
}

/// Draw `graph` as a canvas filling `parent`. Returns the canvas.
pub fn spawn_flow_graph_view(
    commands: &mut Commands,
    parent: Entity,
    path: impl Into<String>,
    graph: FlowGraph,
) -> Entity {
    let canvas = commands
        .spawn_scene(bsn! { @kit::canvas() ChildOf(parent) })
        .insert((
            DotGrid::default(),
            WireShape::default(),
            AvoidNodes::default(),
            TieSelection::default(),
        ))
        .id();
    let path = path.into();
    draw_level(commands, canvas, &path, "", &graph);
    commands.entity(canvas).insert(FlowGraphView {
        path,
        graph,
        outer: Vec::new(),
        dirty: false,
    });
    spawn_breadcrumb(commands, parent, canvas);
    canvas
}

/// Draw `graph`'s nodes and links on `canvas`, its nodes playing back under
/// `prefix` (for the previews inside them).
pub(crate) fn draw_level(
    commands: &mut Commands,
    canvas: Entity,
    path: &str,
    prefix: &str,
    graph: &FlowGraph,
) {
    for node in &graph.nodes {
        spawn_node(commands, canvas, node, Some((path, prefix)));
    }
    let (path, graph) = (path.to_owned(), graph.clone());
    commands.queue(move |world: &mut World| {
        _ = world.run_system_cached_with(connect_links, (canvas, path, graph));
    });
}

/// Draw `node` on `canvas`, as an arrow if it is of a [`FlowArrows`] kind.
/// With `preview` (the view's path and the prefix of the level shown) a
/// node holding a graph shows it inside, and a blend space its samples'
/// axis; without, the node is a preview's: previews do not nest.
fn spawn_node(
    commands: &mut Commands,
    canvas: Entity,
    node: &FlowNode,
    preview: Option<(&str, &str)>,
) {
    let node = node.clone();
    let preview = preview.map(|(path, prefix)| (path.to_owned(), prefix.to_owned()));
    commands.queue(move |world: &mut World| {
        let arrows = world.get_resource::<FlowArrows>();
        if arrows.is_some_and(|arrows| arrows.is_arrow(&node.kind)) {
            spawn_arrow(world, canvas, &node);
            return;
        }
        let preview = preview.as_ref().map(|(a, b)| (a.as_str(), b.as_str()));
        spawn_plain_node(&mut world.commands(), canvas, &node, preview);
        world.flush();
    });
}

fn spawn_plain_node(
    commands: &mut Commands,
    canvas: Entity,
    node: &FlowNode,
    preview: Option<(&str, &str)>,
) {
    let row = |port: &PortDef, make: fn(PortType) -> Port| {
        kit::row(
            port.name.clone(),
            make(port_type(port.kind)),
            port_color(port.kind),
        )
    };
    // Input rows get their value fields from `crate::fields`.
    let inputs: Vec<_> = node
        .inputs
        .iter()
        .enumerate()
        .map(|(index, port)| {
            // Many transitions lead into one state.
            let make = match port.kind {
                PortKind::State => |kind| Port::input(kind).with_capacity(Capacity::Unlimited),
                _ => Port::input,
            };
            let row = row(port, make);
            bsn! { @row InputRow(index) }
        })
        .collect();
    let outputs: Vec<_> = node
        .outputs
        .iter()
        .map(|port| row(port, Port::output))
        .collect();
    let (title, header) = (node.title.clone(), header_color(node));
    let drawn = commands
        .spawn_scene(bsn! {
            @kit::node(node.position) ChildOf(canvas)
            Children [ @kit::title(title) BackgroundColor(header) -- {inputs} -- {outputs} ]
        })
        .insert(ViewNode(node.id.clone()))
        .id();
    if let (Some((path, prefix)), Some(inner)) = (preview, &node.graph) {
        let prefix = format!("{prefix}{}/", node.id);
        spawn_preview(commands, drawn, path, prefix, inner);
    }
    if preview.is_some() && has_samples(&node.kind) {
        spawn_blend_axis(commands, drawn, is_2d(&node.kind));
    }
}

/// A preview of `graph`, the graph inside the canvas node `node`.
fn spawn_preview(
    commands: &mut Commands,
    node: Entity,
    path: &str,
    prefix: String,
    graph: &FlowGraph,
) {
    let preview = commands
        .spawn_scene(bsn! {
            NodeCanvas
            Node {
                width: px(PREVIEW_SIZE.x),
                height: px(PREVIEW_SIZE.y),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(px(6)),
            }
            EdgeStyle
            CanvasGrid { background: Color::srgb_u8(30, 32, 37) }
            ChildOf(node)
        })
        .insert((
            GraphPreview {
                path: path.to_owned(),
                prefix,
                graph: graph.clone(),
            },
            FitOnceLaidOut::default(),
        ))
        .id();
    for inner in &graph.nodes {
        spawn_node(commands, preview, inner, None);
    }
    let (path, graph) = (path.to_owned(), graph.clone());
    commands.queue(move |world: &mut World| {
        _ = world.run_system_cached_with(connect_links, (preview, path, graph));
    });
}

/// Add a node like `template` (a catalog node) at `position`, in graph
/// space, to the graph `canvas` draws, under a fresh id.
pub fn add_flow_node(commands: &mut Commands, canvas: Entity, template: FlowNode, position: Vec2) {
    commands.queue(move |world: &mut World| {
        insert_flow_node(world, canvas, template, position);
    });
}

/// [`add_flow_node`] right away: the node added, drawn.
pub(crate) fn insert_flow_node(
    world: &mut World,
    canvas: Entity,
    template: FlowNode,
    position: Vec2,
) -> Option<FlowNode> {
    let mut view = world.get_mut::<FlowGraphView>(canvas)?;
    let base = template.kind.replace('/', "_");
    let id = (1..)
        .map(|n| match n {
            1 => base.clone(),
            n => format!("{base}_{n}"),
        })
        .find(|id| view.graph.node(id).is_none())
        .unwrap_or(base);
    let node = FlowNode {
        id,
        position,
        ..template
    };
    view.graph.nodes.push(node.clone());
    view.dirty = true;
    let (path, prefix) = (view.path.clone(), view.prefix());
    spawn_node(&mut world.commands(), canvas, &node, Some((&path, &prefix)));
    world.flush();
    Some(node)
}

/// Delete deletes the selection of the canvas under the pointer.
fn delete_selection(
    keys: Res<ButtonInput<KeyCode>>,
    hovers: Option<Res<HoverMap>>,
    views: Query<(), With<FlowGraphView>>,
    graph: GraphQuery,
    focus: Option<Res<InputFocus>>,
    texts: Query<(), With<EditableText>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Delete) || focus.is_some_and(|focus| typing(&focus, &texts)) {
        return;
    }
    let Some(hovers) = hovers else {
        return;
    };
    for canvas in canvases_under_pointer(&hovers, &graph, |canvas| views.contains(canvas)) {
        let items: Vec<Entity> = graph.selected_in(canvas).collect();
        if !items.is_empty() {
            commands.graph_edit(canvas, GraphEdit::Delete { items });
        }
    }
}

/// The canvases `is_view` accepts that are under a pointer, themselves or
/// through what is on them.
pub(crate) fn canvases_under_pointer(
    hovers: &HoverMap,
    graph: &GraphQuery,
    is_view: impl Fn(Entity) -> bool,
) -> Vec<Entity> {
    let mut canvases: Vec<Entity> = hovers
        .values()
        .flat_map(|hits| hits.keys())
        .filter_map(|&hovered| {
            [Some(hovered), graph.canvas_of(hovered)]
                .into_iter()
                .flatten()
                .find(|&canvas| is_view(canvas))
        })
        .collect();
    canvases.sort();
    canvases.dedup();
    canvases
}

/// The canvas node drawing graph node `id`.
fn view_node(
    canvas: Entity,
    id: &str,
    graph: &GraphQuery,
    nodes: &Query<&ViewNode>,
) -> Option<Entity> {
    graph
        .nodes_in(canvas)
        .find(|&node| nodes.get(node).is_ok_and(|view| view.0 == id))
}

/// Draw `shown`'s links between the nodes drawn on `canvas`.
pub(crate) fn connect_links(
    In((canvas, path, shown)): In<(Entity, String, FlowGraph)>,
    graph: GraphQuery,
    nodes: Query<&ViewNode>,
    mut commands: Commands,
) {
    let port_index = |node: &str, name: &str, outputs: bool| {
        let node = shown.node(node)?;
        let ports = if outputs { &node.outputs } else { &node.inputs };
        ports.iter().position(|port| port.name == name)
    };
    for link in &shown.links {
        let from = view_node(canvas, &link.from, &graph, &nodes).and_then(|node| {
            graph
                .outputs_of(node)
                .nth(port_index(&link.from, &link.output, true)?)
        });
        let to = view_node(canvas, &link.to, &graph, &nodes).and_then(|node| {
            graph
                .inputs_of(node)
                .nth(port_index(&link.to, &link.input, false)?)
        });
        match (from, to) {
            (Some(from), Some(to)) => commands.graph_edit(canvas, GraphEdit::Connect { from, to }),
            _ => warn!("flow graph {path}: no ports for link {link:?}"),
        }
    }
}

/// The graph node id and port name a canvas port draws, `shown` being the
/// graph its canvas draws.
pub(crate) fn port_ref<'a>(
    port: Entity,
    shown: &'a FlowGraph,
    graph: &GraphQuery,
    nodes: &'a Query<&ViewNode>,
) -> Option<(&'a str, &'a str)> {
    let node = graph.node_of(port)?;
    let id = nodes.get(node).ok()?.0.as_str();
    let def = shown.node(id)?;
    let (ports, index) = match graph.inputs_of(node).position(|p| p == port) {
        Some(index) => (&def.inputs, index),
        None => (
            &def.outputs,
            graph.outputs_of(node).position(|p| p == port)?,
        ),
    };
    Some((id, ports.get(index)?.name.as_str()))
}

/// The graph link a canvas edge draws, `shown` being the graph its canvas
/// draws.
pub(crate) fn link_of(
    edge: Entity,
    shown: &FlowGraph,
    graph: &GraphQuery,
    nodes: &Query<&ViewNode>,
) -> Option<FlowLink> {
    let ports = graph.edge_ports(edge)?;
    let (from, output) = port_ref(ports.output, shown, graph, nodes)?;
    let (to, input) = port_ref(ports.input, shown, graph, nodes)?;
    Some(FlowLink::new(from, output, to, input))
}

/// The graph as `canvas` now draws it: its links and node positions, minus
/// deleted nodes, and its ties on the links left.
fn graph_from_canvas(
    canvas: Entity,
    view: &FlowGraphView,
    graph: &GraphQuery,
    nodes: &Query<&ViewNode>,
    positions: &Query<&NodePosition>,
) -> FlowGraph {
    let drawn: Vec<(Entity, &str)> = graph
        .nodes_in(canvas)
        .filter_map(|node| Some((node, nodes.get(node).ok()?.0.as_str())))
        .collect();
    let nodes_out = view
        .graph
        .nodes
        .iter()
        .filter_map(|node| {
            let (entity, _) = drawn.iter().find(|(_, id)| *id == node.id)?;
            let mut node = node.clone();
            if let Ok(position) = positions.get(*entity) {
                node.position = position.0;
            }
            Some(node)
        })
        .collect();
    let links: Vec<FlowLink> = graph
        .edges_in(canvas)
        .filter_map(|edge| link_of(edge, &view.graph, graph, nodes))
        .collect();
    let ties = view
        .graph
        .ties
        .iter()
        .filter_map(|tie| {
            let tied: Vec<FlowLink> = tie
                .links
                .iter()
                .filter(|link| links.contains(link))
                .cloned()
                .collect();
            (!tied.is_empty()).then(|| FlowTie {
                links: tied,
                ..tie.clone()
            })
        })
        .collect();
    FlowGraph {
        nodes: nodes_out,
        links,
        ties,
        variables: view.graph.variables.clone(),
    }
}

pub(crate) fn write_back_edits(
    mut edits: MessageReader<EditApplied>,
    mut views: Query<&mut FlowGraphView>,
    graph: GraphQuery,
    nodes: Query<&ViewNode>,
    positions: Query<&NodePosition>,
) {
    let mut canvases: Vec<Entity> = edits
        .read()
        .filter(|edit| !edit.change.is_drag_step())
        .map(|edit| edit.canvas)
        .collect();
    canvases.dedup();
    for canvas in canvases {
        let Ok(mut view) = views.get_mut(canvas) else {
            continue;
        };
        let edited = graph_from_canvas(canvas, &view, &graph, &nodes, &positions);
        if edited != view.graph {
            view.graph = edited;
            view.dirty = true;
        }
    }
}

/// Whether a playback recorded under `played` is for the file at `viewed`:
/// the same path, give or take separators and a leading directory (one may
/// be relative to `assets/`, the other to the project).
pub fn same_graph_path(played: &str, viewed: &str) -> bool {
    let played = played.replace('\\', "/");
    let viewed = viewed.replace('\\', "/");
    !played.is_empty()
        && (played == viewed
            || viewed.ends_with(&format!("/{played}"))
            || played.ends_with(&format!("/{viewed}")))
}

/// Light up nodes and wires from the playback of each view's graph, and of
/// each preview's, and mark the states machines are in [`CurrentState`].
#[allow(clippy::too_many_arguments, reason = "system parameters")]
pub(crate) fn light_up(
    views: Query<(Entity, &FlowGraphView)>,
    previews: Query<(Entity, &GraphPreview)>,
    playbacks: Query<&FlowGraphPlayback>,
    graph: GraphQuery,
    nodes: Query<&ViewNode>,
    mut borders: Query<&mut SelectedBorderColor>,
    marks: Query<(Has<CurrentState>, Has<ArrowNode>)>,
    lit_edges: Query<(), With<LitEdge>>,
    arrow_edges: Query<(), With<ArrowEdge>>,
    mut commands: Commands,
) {
    let views = views
        .iter()
        .map(|(canvas, view)| (canvas, &view.path, view.prefix(), &view.graph));
    let previews = previews.iter().map(|(canvas, preview)| {
        let prefix = preview.prefix.clone();
        (canvas, &preview.path, prefix, &preview.graph)
    });
    for (canvas, path, prefix, shown) in views.chain(previews) {
        let playback = playbacks
            .iter()
            .find(|playback| same_graph_path(&playback.graph, path));
        let state = |id: &str| playback?.node(&format!("{prefix}{id}"));
        for node in graph.nodes_in(canvas) {
            let (Ok(id), Ok(mut border), Ok((marked, arrow))) =
                (nodes.get(node), borders.get_mut(node), marks.get(node))
            else {
                continue;
            };
            let current = !arrow
                && shown
                    .node(&id.0)
                    .zip(state(&id.0))
                    .is_some_and(|(node, state)| is_current_state(node, state));
            if current != marked {
                match current {
                    true => commands.entity(node).insert(CurrentState),
                    false => commands.entity(node).remove::<CurrentState>(),
                };
            }
            // A current state's ring says it all.
            let normal = if !current && state(&id.0).is_some_and(|state| state.is_active()) {
                ACTIVE
            } else {
                SelectedBorderColor::default().normal
            };
            if border.normal != normal {
                border.normal = normal;
            }
        }
        for edge in graph.edges_in(canvas) {
            let carries = graph
                .edge_ports(edge)
                .and_then(|ports| port_ref(ports.output, shown, &graph, &nodes))
                .and_then(|(id, output)| {
                    let index = shown
                        .node(id)?
                        .outputs
                        .iter()
                        .position(|p| p.name == output)?;
                    state(id)?.outputs.get(index).cloned()
                })
                .is_some_and(|value| value.is_active());
            let (lit, arrow) = (lit_edges.contains(edge), arrow_edges.contains(edge));
            if carries && !lit {
                let style = EdgeStyle {
                    color: Some(ACTIVE),
                    width: 5.0,
                    flow_speed: 160.0,
                    ..default()
                };
                let style = if arrow {
                    arrow_style(Some(style))
                } else {
                    style
                };
                commands.entity(edge).insert((LitEdge, style));
            } else if !carries && lit && arrow {
                commands
                    .entity(edge)
                    .remove::<LitEdge>()
                    .insert(arrow_style(None));
            } else if !carries && lit {
                commands.entity(edge).remove::<(LitEdge, EdgeStyle)>();
            }
        }
    }
}

/// Whether `node` is a state (a `State` port in and out) its machine is in:
/// one the playback says flows through (an Entry starts a machine, it is
/// not one of its states).
fn is_current_state(node: &FlowNode, state: &FlowNodeState) -> bool {
    let is_state = |port: &PortDef| port.kind == PortKind::State;
    let flows = |ports: &[PortDef], values: &[FlowValue]| {
        ports
            .iter()
            .zip(values)
            .any(|(port, value)| is_state(port) && *value == FlowValue::State(true))
    };
    node.inputs.iter().any(is_state)
        && node.outputs.iter().any(is_state)
        && (flows(&node.inputs, &state.inputs) || flows(&node.outputs, &state.outputs))
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::scene::ScenePlugin;

    use super::*;
    use crate::model::PortDef;
    use crate::run::FlowNodeState;
    use crate::wires::lay_out_wires;

    fn graph() -> FlowGraph {
        let node = |id: &str, inputs: Vec<PortDef>, outputs: Vec<PortDef>| FlowNode {
            id: id.into(),
            kind: format!("test/{id}"),
            title: id.into(),
            position: Vec2::ZERO,
            inputs,
            outputs,
            ..default()
        };
        FlowGraph {
            nodes: vec![
                node("a", vec![], vec![PortDef::new("out", PortKind::Float)]),
                node(
                    "b",
                    vec![
                        PortDef::new("first", PortKind::Float),
                        PortDef::new("second", PortKind::Float),
                        PortDef::new("name", PortKind::Text),
                    ],
                    vec![],
                ),
            ],
            links: vec![FlowLink::new("a", "out", "b", "second")],
            ..default()
        }
    }

    /// Headless: the noodle core and our systems, no drawing.
    fn app() -> (App, Entity) {
        app_with(graph())
    }

    /// Headless, drawing `graph`.
    fn app_with(graph: FlowGraph) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
        ))
        .init_resource::<InputFocus>()
        // The default style's, drawing the easing fields' curves.
        .init_asset::<bevy_noodle::style::WireMaterial>()
        .add_plugins(ValueFieldsPlugin)
        .add_systems(Update, (write_back_edits, light_up).chain());
        let root = app.world_mut().spawn(Node::default()).id();
        let canvas = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(
                    &mut commands,
                    root,
                    "flow/test.flowgraph.bsn",
                    graph.clone(),
                )
            })
            .unwrap();
        app.update();
        (app, canvas)
    }

    fn view(app: &App, canvas: Entity) -> FlowGraphView {
        app.world().get::<FlowGraphView>(canvas).unwrap().clone()
    }

    #[test]
    fn links_are_drawn_on_the_named_ports() {
        let (mut app, canvas) = app();
        let drawn = app
            .world_mut()
            .run_system_once(
                move |graph: GraphQuery,
                      nodes: Query<&ViewNode>,
                      views: Query<&FlowGraphView>,
                      positions: Query<&NodePosition>| {
                    graph_from_canvas(
                        canvas,
                        views.get(canvas).unwrap(),
                        &graph,
                        &nodes,
                        &positions,
                    )
                },
            )
            .unwrap();
        assert_eq!(drawn, graph());
        assert!(!view(&app, canvas).dirty);
    }

    #[test]
    fn cutting_a_wire_writes_the_graph_back() {
        let (mut app, canvas) = app();
        let edge = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery| graph.edges_in(canvas).next())
            .unwrap()
            .expect("the link is drawn");
        app.world_mut()
            .graph_edit(canvas, GraphEdit::Disconnect { edge })
            .unwrap();
        app.update();
        let view = view(&app, canvas);
        assert!(view.dirty);
        assert!(view.graph.links.is_empty());
    }

    #[test]
    fn a_playback_lights_active_nodes_and_wires() {
        let (mut app, canvas) = app();
        let state = |id: &str, inputs: Vec<FlowValue>, outputs: Vec<FlowValue>| FlowNodeState {
            id: id.into(),
            inputs,
            outputs,
        };
        app.world_mut().spawn(FlowGraphPlayback {
            graph: "test.flowgraph.bsn".into(),
            nodes: vec![
                state("a", vec![], vec![FlowValue::Float(1.0)]),
                state(
                    "b",
                    vec![FlowValue::Float(0.0), FlowValue::Float(1.0)],
                    vec![],
                ),
            ],
        });
        app.update();
        app.update();
        let (borders, lit) = app
            .world_mut()
            .run_system_once(
                move |graph: GraphQuery,
                      borders: Query<&SelectedBorderColor>,
                      lit: Query<&EdgeStyle, With<LitEdge>>| {
                    let borders: Vec<Color> = graph
                        .nodes_in(canvas)
                        .map(|node| borders.get(node).unwrap().normal)
                        .collect();
                    let lit = graph.edges_in(canvas).filter(|&e| lit.contains(e)).count();
                    (borders, lit)
                },
            )
            .unwrap();
        assert_eq!(borders, [ACTIVE, ACTIVE]);
        assert_eq!(lit, 1);
    }

    #[test]
    fn the_states_machines_are_in_are_marked_current() {
        let (mut app, _) = app();
        let state = |id: &str| FlowNode {
            id: id.into(),
            kind: "test/state".into(),
            title: id.into(),
            inputs: vec![PortDef::new("enter", PortKind::State)],
            outputs: vec![
                PortDef::new("exit", PortKind::State),
                PortDef::new("phase", PortKind::Float),
            ],
            ..default()
        };
        let entry = FlowNode {
            id: "entry".into(),
            kind: "test/entry".into(),
            outputs: vec![PortDef::new("start", PortKind::State)],
            ..default()
        };
        let machine = FlowGraph {
            nodes: vec![entry, state("idle"), state("walk")],
            links: vec![FlowLink::new("entry", "start", "idle", "enter")],
            ..default()
        };
        let root = app.world_mut().spawn(Node::default()).id();
        let canvas = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(
                    &mut commands,
                    root,
                    "flow/machine.flowgraph.bsn",
                    machine.clone(),
                )
            })
            .unwrap();
        let played = |id: &str, current: bool| FlowNodeState {
            id: id.into(),
            inputs: vec![FlowValue::State(current)],
            outputs: vec![FlowValue::State(current), FlowValue::Float(0.4)],
        };
        let playback = app
            .world_mut()
            .spawn(FlowGraphPlayback {
                graph: "machine.flowgraph.bsn".into(),
                nodes: vec![
                    FlowNodeState {
                        id: "entry".into(),
                        inputs: vec![],
                        outputs: vec![FlowValue::State(true)],
                    },
                    played("idle", true),
                    played("walk", false),
                ],
            })
            .id();
        app.update();
        app.update();
        let marked = |app: &mut App| {
            app.world_mut()
                .run_system_once(
                    move |graph: GraphQuery,
                          nodes: Query<(&ViewNode, &SelectedBorderColor, Has<CurrentState>)>| {
                        graph
                            .nodes_in(canvas)
                            .filter_map(|node| nodes.get(node).ok())
                            .map(|(id, border, current)| (id.0.clone(), current, border.normal))
                            .collect::<Vec<_>>()
                    },
                )
                .unwrap()
        };
        let normal = SelectedBorderColor::default().normal;
        assert_eq!(
            marked(&mut app),
            [
                ("entry".into(), false, ACTIVE),
                ("idle".into(), true, normal),
                // Its phase flows, but the machine is not in it.
                ("walk".into(), false, ACTIVE),
            ]
        );

        // The machine moves on.
        let mut moved = app
            .world()
            .get::<FlowGraphPlayback>(playback)
            .unwrap()
            .clone();
        moved.nodes[1] = played("idle", false);
        moved.nodes[2] = played("walk", true);
        app.world_mut().entity_mut(playback).insert(moved);
        app.update();
        let current: Vec<_> = marked(&mut app)
            .into_iter()
            .map(|(id, current, _)| (id, current))
            .collect();
        assert_eq!(
            current,
            [
                ("entry".into(), false),
                ("idle".into(), false),
                ("walk".into(), true)
            ]
        );
    }

    fn template(kind: &str) -> FlowNode {
        FlowNode {
            kind: kind.into(),
            title: kind.into(),
            outputs: vec![PortDef::new("result", PortKind::Float)],
            ..default()
        }
    }

    #[test]
    fn added_nodes_get_fresh_ids_and_are_drawn() {
        let (mut app, canvas) = app();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                for _ in 0..2 {
                    add_flow_node(&mut commands, canvas, template("float/add"), Vec2::ONE);
                }
            })
            .unwrap();
        app.update();
        let view = view(&app, canvas);
        assert!(view.dirty);
        let ids: Vec<&str> = view.graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "float_add", "float_add_2"]);
        let drawn = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery| graph.nodes_in(canvas).count())
            .unwrap();
        assert_eq!(drawn, 4);
    }

    #[test]
    fn deleting_a_node_drops_it_and_its_links() {
        let (mut app, canvas) = app();
        let a = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery, nodes: Query<&ViewNode>| {
                view_node(canvas, "a", &graph, &nodes)
            })
            .unwrap()
            .expect("a is drawn");
        app.world_mut()
            .graph_edit(canvas, GraphEdit::Delete { items: vec![a] })
            .unwrap();
        app.update();
        let view = view(&app, canvas);
        assert!(view.graph.node("a").is_none());
        assert!(view.graph.links.is_empty());
    }

    #[test]
    fn the_library_lists_the_catalog_by_category() {
        use crate::library::{LibraryItem, category_title, spawn_node_library};
        let (mut app, canvas) = app();
        let catalog = FlowNodeCatalog {
            nodes: vec![
                template("vec2/add"),
                template("float/add"),
                template("float/min"),
            ],
            ..default()
        };
        let library = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                let parent = commands.spawn(Node::default()).id();
                spawn_node_library(&mut commands, parent, canvas, &catalog)
            })
            .unwrap();
        app.update();
        let mut items = app
            .world_mut()
            .query::<&LibraryItem>()
            .iter(app.world())
            .map(|item| {
                assert_eq!(item.library, library);
                item.template.kind.clone()
            })
            .collect::<Vec<_>>();
        items.sort();
        assert_eq!(items, ["float/add", "float/min", "vec2/add"]);
        assert_eq!(category_title("vec3"), "Vector 3");
        assert_eq!(category_title("leafwing"), "Leafwing");
    }

    fn first_edge(app: &mut App, canvas: Entity) -> Entity {
        app.world_mut()
            .run_system_once(move |graph: GraphQuery| graph.edges_in(canvas).next())
            .unwrap()
            .expect("the link is drawn")
    }

    fn tie_on_the_link() -> FlowTie {
        FlowTie {
            position: Vec2::new(100.0, 300.0),
            angle: 0.0,
            links: vec![FlowLink::new("a", "out", "b", "second")],
            color: None,
        }
    }

    /// The shape sets noodle's curvature, lit edges too; tied wires are
    /// routed through their tie.
    #[test]
    fn wires_take_the_canvas_shape_and_go_through_their_ties() {
        let (mut app, canvas) = app();
        let edge = first_edge(&mut app, canvas);
        app.world_mut().entity_mut(edge).insert((
            EdgeGeometry::between((Vec2::ZERO, Vec2::X), (Vec2::new(200.0, 0.0), Vec2::NEG_X)),
            EdgeStyle::default(),
        ));
        app.world_mut().entity_mut(canvas).insert((
            EdgeStyle::default(),
            WireShape::Straight,
            AvoidNodes(false),
        ));
        let mut drawn = app.world_mut().get_mut::<FlowGraphView>(canvas).unwrap();
        drawn.graph.ties.push(tie_on_the_link());
        app.world_mut().run_system_once(lay_out_wires).unwrap();

        for entity in [canvas, edge] {
            assert_eq!(app.world().get::<EdgeStyle>(entity).unwrap().curvature, 0.0);
        }
        let route = app.world().get::<EdgeRoute>(edge).expect("routed");
        assert_eq!(
            route.0,
            [RoutePoint {
                position: Vec2::new(100.0, 300.0),
                direction: Vec2::X,
            }]
        );

        // Untied: straight from port to port again.
        app.world_mut()
            .get_mut::<FlowGraphView>(canvas)
            .unwrap()
            .graph
            .ties
            .clear();
        app.world_mut().run_system_once(lay_out_wires).unwrap();
        assert!(app.world().get::<EdgeRoute>(edge).is_none());
    }

    #[test]
    fn ties_are_kept_while_their_links_are() {
        let (mut app, canvas) = app();
        let edge = first_edge(&mut app, canvas);
        let mut drawn = app.world_mut().get_mut::<FlowGraphView>(canvas).unwrap();
        drawn.graph.ties.push(tie_on_the_link());
        let moved = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery, nodes: Query<&ViewNode>| {
                view_node(canvas, "a", &graph, &nodes)
            })
            .unwrap()
            .unwrap();
        app.world_mut()
            .graph_edit(
                canvas,
                GraphEdit::MoveNodes {
                    nodes: vec![moved],
                    delta: Vec2::ONE,
                    drag: None,
                },
            )
            .unwrap();
        app.update();
        assert_eq!(view(&app, canvas).graph.ties, [tie_on_the_link()]);

        app.world_mut()
            .graph_edit(canvas, GraphEdit::Disconnect { edge })
            .unwrap();
        app.update();
        assert!(view(&app, canvas).graph.ties.is_empty());
    }

    #[test]
    fn ties_go_with_the_nodes_of_their_wires_moved_together() {
        let (mut app, canvas) = app();
        app.add_systems(Update, crate::ties::follow_moved_nodes);
        let mut drawn = app.world_mut().get_mut::<FlowGraphView>(canvas).unwrap();
        drawn.graph.ties.push(tie_on_the_link());
        let ends = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery, nodes: Query<&ViewNode>| {
                ["a", "b"].map(|id| view_node(canvas, id, &graph, &nodes).unwrap())
            })
            .unwrap();
        let delta = Vec2::new(40.0, -20.0);
        for nodes in [vec![ends[0]], ends.to_vec()] {
            app.world_mut()
                .graph_edit(
                    canvas,
                    GraphEdit::MoveNodes {
                        nodes,
                        delta,
                        drag: None,
                    },
                )
                .unwrap();
            app.update();
        }
        // Not with "a" alone; with both.
        let tie = &view(&app, canvas).graph.ties[0];
        assert_eq!(tie.position, tie_on_the_link().position + delta);
    }

    /// The text of `field` and whether its input's fields show.
    fn field_state(app: &mut App, field: Entity) -> (String, bool) {
        let world = app.world_mut();
        let text = world
            .get::<EditableText>(field)
            .unwrap()
            .value()
            .to_string();
        let fields = world.get::<ChildOf>(field).unwrap().parent();
        let shown = world.get::<Node>(fields).unwrap().display != Display::None;
        (text, shown)
    }

    /// Type `text` in `field` and let go of it.
    fn type_in(app: &mut App, field: Entity, text: &str) {
        let focus = bevy::input_focus::FocusCause::Pressed;
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, focus);
        app.update();
        // The field with the keyboard is outlined.
        let border = app.world().get::<BorderColor>(field).unwrap();
        assert_eq!(*border, BorderColor::all(crate::fields::FOCUSED_BORDER));
        let mut editable = app.world_mut().get_mut::<EditableText>(field).unwrap();
        editable.editor_mut().set_text(text);
        app.world_mut().resource_mut::<InputFocus>().clear();
        app.update();
    }

    #[test]
    fn unwired_inputs_show_and_edit_their_value() {
        use crate::fields::NumberField;
        let (mut app, canvas) = app();
        app.update();
        // b's inputs, in order: "first" unwired, "second" wired.
        let mut fields: Vec<(Entity, usize)> = app
            .world_mut()
            .query_filtered::<(Entity, &ChildOf), With<NumberField>>()
            .iter(app.world())
            .map(|(field, child_of)| (field, child_of.parent()))
            .map(|(field, fields)| {
                let row = app.world().get::<ChildOf>(fields).unwrap().parent();
                (field, app.world().get::<InputRow>(row).unwrap().0)
            })
            .collect();
        fields.sort_by_key(|&(_, input)| input);
        let [(first, 0), (second, 1)] = fields[..] else {
            panic!("one field per float input: {fields:?}");
        };
        assert_eq!(field_state(&mut app, first), ("0".into(), true));
        assert_eq!(field_state(&mut app, second), ("0".into(), false));

        type_in(&mut app, first, "2,5");
        let edited = view(&app, canvas);
        assert!(edited.dirty);
        let b = edited.graph.node("b").unwrap();
        assert_eq!(b.inputs[0].value, Some(FlowValue::Float(2.5)));
        assert_eq!(field_state(&mut app, first), ("2.5".into(), true));

        // Not a number: the value stays and shows again.
        type_in(&mut app, first, "-");
        let b = view(&app, canvas).graph.node("b").unwrap().clone();
        assert_eq!(b.inputs[0].value, Some(FlowValue::Float(2.5)));
        assert_eq!(field_state(&mut app, first).0, "2.5");
    }

    #[test]
    fn a_text_input_shows_and_edits_its_text() {
        use crate::fields::TextField;
        let (mut app, canvas) = app();
        app.update();
        let field = app
            .world_mut()
            .query_filtered::<Entity, With<TextField>>()
            .single(app.world())
            .unwrap();
        assert_eq!(field_state(&mut app, field), (String::new(), true));
        type_in(&mut app, field, " Player ");
        let b = view(&app, canvas).graph.node("b").unwrap().clone();
        assert_eq!(b.inputs[2].value, Some(FlowValue::Text("Player".into())));
        assert_eq!(field_state(&mut app, field).0, "Player");
        // Emptied: back to rest, no value in the file.
        type_in(&mut app, field, "");
        let b = view(&app, canvas).graph.node("b").unwrap().clone();
        assert_eq!(b.inputs[2].value, None);
    }

    #[test]
    fn a_bone_is_picked_from_its_skeleton_in_a_dropdown() {
        use crate::fields::{
            Choice, ChoiceField, ChoiceList, Target, choices, open_choice_list, pick,
        };

        let (mut app, canvas) = app_with(crate::nodes::tests::bone_graph("Hero", ""));
        crate::nodes::tests::hero(app.world_mut());
        app.update();
        // The Entity's and the Bone's names; the Skeleton has none.
        let world = app.world_mut();
        let fields: Vec<Entity> = world
            .query_filtered::<Entity, With<ChoiceField>>()
            .iter(world)
            .collect();
        assert_eq!(fields.len(), 2);
        let bone = Target {
            canvas,
            node: "bone".into(),
            input: 1,
        };
        // Its skeleton is the one found under the named entity.
        assert_eq!(choices(app.world(), &bone).unwrap(), ["head", "neck"]);
        let entity = Target {
            node: "entity".into(),
            input: 0,
            ..bone.clone()
        };
        assert_eq!(choices(app.world(), &entity).unwrap(), ["Hero"]);

        let field = fields
            .into_iter()
            .find(|&field| {
                let fields = app.world().get::<ChildOf>(field).unwrap().parent();
                let row = app.world().get::<ChildOf>(fields).unwrap().parent();
                app.world().get::<InputRow>(row) == Some(&InputRow(1))
            })
            .unwrap();
        open_choice_list(app.world_mut(), field, &bone);
        let world = app.world_mut();
        let listed: Vec<Choice> = world
            .query_filtered::<&Choice, With<ChildOf>>()
            .iter(world)
            .cloned()
            .collect();
        assert_eq!(
            listed,
            [
                Choice(None),
                Choice(Some("head".into())),
                Choice(Some("neck".into()))
            ]
        );
        assert_eq!(
            world.query::<&ChoiceList>().single(world).unwrap().field,
            field
        );
        let neck = Choice(Some("neck".into()));
        let target = bone.clone();
        world
            .run_system_once(move |mut views: Query<&mut FlowGraphView>| {
                pick(&mut views, &target, &neck)
            })
            .unwrap();
        let picked = view(&app, canvas).graph.node("bone").unwrap().clone();
        assert_eq!(picked.inputs[1].value, Some(FlowValue::Text("neck".into())));
        assert!(view(&app, canvas).dirty);
        app.update();
        let world = app.world_mut();
        let labels: Vec<String> = world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect();
        assert!(labels.contains(&"neck".to_owned()), "{labels:?}");
    }

    #[test]
    fn graph_paths_match_across_roots_and_separators() {
        assert!(same_graph_path(
            "flow/movement.flowgraph.bsn",
            r"assets\flow\movement.flowgraph.bsn"
        ));
        assert!(!same_graph_path(
            "flow/movement.flowgraph.bsn",
            "assets/flow/other.flowgraph.bsn"
        ));
        assert!(!same_graph_path("", "x.flowgraph.bsn"));
    }
}
