//! Nodes drawn as arrows: a state machine's transitions. A node of a kind
//! the [`FlowArrows`] name keeps its place in the graph (wired from a
//! state's `State` output, wired into another's `State` input), but the view
//! draws it as a pill of its other ports halfway along an arrow between the
//! two, placed and turned by [`place_arrows`]. Wiring a state into another
//! puts one in between; cutting either half of the arrow, or deleting a
//! state it links, deletes it.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::asset::RenderAssetUsages;
use bevy::curve::cubic_splines::CubicSegment;
use bevy::picking::Pickable;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_noodle::prelude::*;
use bevy_noodle::style::kit;

use crate::fields::InputRow;
use crate::model::{FlowGraph, FlowLink, FlowNode, FlowNodeCatalog, PortKind};
use crate::view::{
    FlowGraphView, ViewNode, connect_links, insert_flow_node, port_color, port_type,
};
use crate::wires::{AvoidNodes, Obstacles, WireShape, node_rects, route};

/// Room between pills.
const GAP: f32 = 8.0;
/// How many places a pill tries, off its arrow's middle, to clear the others.
const MOVES_ASIDE: i32 = 13;
/// A hub's side, in graph units.
const HUB: f32 = 2.0;
/// The arrowhead's side, in graph units.
const HEAD: f32 = 12.0;

pub(crate) struct ArrowsPlugin;

impl Plugin for ArrowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlowArrows>()
            .add_observer(edit_arrows)
            .add_systems(Update, (place_arrows, show_heads));
    }
}

/// The kinds views draw as arrows, as template nodes: what wiring two
/// states adds between them (the first one).
#[derive(Resource, Clone, Debug, Default)]
pub struct FlowArrows {
    pub templates: Vec<FlowNode>,
}

impl FlowArrows {
    /// The catalog's arrow kinds.
    pub fn from_catalog(catalog: &FlowNodeCatalog) -> Self {
        let templates = catalog
            .nodes
            .iter()
            .filter(|node| catalog.arrows.contains(&node.kind))
            .cloned()
            .collect();
        Self { templates }
    }

    pub fn is_arrow(&self, kind: &str) -> bool {
        self.templates.iter().any(|node| node.kind == kind)
    }
}

/// On a canvas node drawing an arrow's pill.
#[derive(Component)]
pub(crate) struct ArrowNode;

/// On a pill's `State` ports, at its center: where the arrow's halves meet.
#[derive(Component)]
struct ArrowHub;

/// On an edge between a pill and a node: half an arrow.
#[derive(Component)]
pub(crate) struct ArrowEdge;

/// The head of the arrows into a port, a child of the port.
#[derive(Component)]
struct ArrowHead;

/// How an arrow's halves are drawn: under the nodes, so they run on through
/// the pill; lit (by [`light_up`](crate::view::light_up)) like any wire.
pub(crate) fn arrow_style(lit: Option<EdgeStyle>) -> EdgeStyle {
    EdgeStyle {
        below_nodes: true,
        ..lit.unwrap_or_default()
    }
}

/// Draw the arrow node `node` on `canvas`: a pill of its ports but the
/// `State` ones, which sit at its center.
pub(crate) fn spawn_arrow(world: &mut World, canvas: Entity, node: &FlowNode) {
    let row =
        |name: &str, port: Port, kind: PortKind| kit::row(name.to_owned(), port, port_color(kind));
    let inputs: Vec<_> = node
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, port)| port.kind != PortKind::State)
        .map(|(index, port)| {
            let row = row(&port.name, Port::input(port_type(port.kind)), port.kind);
            bsn! { @row InputRow(index) }
        })
        .collect();
    let outputs: Vec<_> = node
        .outputs
        .iter()
        .filter(|port| port.kind != PortKind::State)
        .map(|port| row(&port.name, Port::output(port_type(port.kind)), port.kind))
        .collect();
    let pill = world
        .commands()
        .spawn_scene(bsn! {
            @kit::node(node.position)
            Node {
                min_width: px(0),
                row_gap: px(2),
                padding: UiRect::axes(px(10), px(6)),
                border_radius: BorderRadius::all(px(12)),
            }
            ChildOf(canvas)
            Children [ {inputs} -- {outputs} ]
        })
        .insert((ViewNode(node.id.clone()), ArrowNode))
        .id();
    world.flush();
    // In the order of the node's ports, which is the graph's: hubs go where
    // their ports are among the rows.
    let ports = node
        .inputs
        .iter()
        .map(|port| (port, Port::input as fn(_) -> _));
    let ports = ports.chain(
        node.outputs
            .iter()
            .map(|port| (port, Port::output as fn(_) -> _)),
    );
    for (index, (port, make)) in ports.enumerate() {
        if port.kind != PortKind::State {
            continue;
        }
        let hub = world
            .spawn((
                make(port_type(port.kind)),
                ArrowHub,
                // Sized, or it is never measured; too small to see.
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(50),
                    top: percent(50),
                    width: px(HUB),
                    height: px(HUB),
                    margin: UiRect::all(px(-HUB / 2.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        world.entity_mut(pill).insert_children(index, &[hub]);
    }
}

/// A pill's two hubs: into it, out of it.
fn hubs(pill: Entity, graph: &GraphQuery, hubs: &Query<(), With<ArrowHub>>) -> Option<[Entity; 2]> {
    Some([
        graph.inputs_of(pill).find(|&p| hubs.contains(p))?,
        graph.outputs_of(pill).find(|&p| hubs.contains(p))?,
    ])
}

/// The port across the edge at `port`, if it has one.
fn peer(port: Entity, graph: &GraphQuery) -> Option<Entity> {
    let edge = graph.edges_of(port).next()?;
    Some(graph.edge_ports(edge)?.other(port))
}

/// Where an arrow from `start` to `end` has its pill: halfway along the wire
/// it would be, curved by `curvature` and routed around `nodes` if given,
/// and the way the wire runs there.
pub(crate) fn halfway(start: Vec2, end: Vec2, curvature: f32, nodes: Option<&[Rect]>) -> Half {
    // Out of a state's right side, into another's left one.
    let geometry = EdgeGeometry::between((start, Vec2::X), (end, Vec2::NEG_X));
    let points = route(&geometry, &[], curvature, nodes);
    let samples: Vec<Vec2> = geometry
        .segments(curvature, &points)
        .into_iter()
        .flat_map(|curve| {
            let curve = CubicSegment::new_bezier(curve);
            curve.iter_positions(32).collect::<Vec<_>>()
        })
        .collect();
    let half = samples.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>() / 2.0;
    let mut gone = 0.0;
    for pair in samples.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        let length = a.distance(b);
        if length > 0.0 && gone + length >= half {
            return (a.lerp(b, (half - gone) / length), (b - a) / length);
        }
        gone += length;
    }
    (start.midpoint(end), (end - start).normalize_or(Vec2::X))
}

/// A point halfway along an arrow, and the way the arrow runs there.
type Half = (Vec2, Vec2);

/// What a pill's place depends on, hashed.
fn placed_from(ends: [Vec2; 2], curvature: f32, nodes: Option<&[Rect]>) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut floats = |values: &[f32]| values.iter().for_each(|v| v.to_bits().hash(&mut hasher));
    floats(&[ends[0].x, ends[0].y, ends[1].x, ends[1].y, curvature]);
    for rect in nodes.unwrap_or_default() {
        floats(&[rect.min.x, rect.min.y, rect.max.x, rect.max.y]);
    }
    hasher.finish()
}

/// Put each pill halfway along its arrow, clear of the others, and turn
/// its hubs along the arrow.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn place_arrows(
    graph: GraphQuery,
    pills: Query<(Entity, &ComputedNode), With<ArrowNode>>,
    hub_ports: Query<(), With<ArrowHub>>,
    layouts: Query<(&WireShape, &AvoidNodes)>,
    obstacles: Obstacles,
    mut positions: Query<&mut NodePosition, With<ArrowNode>>,
    mut tangents: Query<&mut PortTangent>,
    mut halves: Local<HashMap<Entity, (u64, Half)>>,
    mut commands: Commands,
) {
    struct Laid {
        pill: Entity,
        size: Vec2,
        canvas: Entity,
        hubs: [Entity; 2],
        half: Half,
    }
    let mut rects: HashMap<Entity, Option<Vec<Rect>>> = HashMap::default();
    let mut placed = HashMap::default();
    let mut laid: Vec<Laid> = pills
        .iter()
        .filter_map(|(pill, computed)| {
            let hubs = hubs(pill, &graph, &hub_ports)?;
            let [from, to] = hubs.map(|hub| peer(hub, &graph));
            let (from, to) = (from?, to?);
            let ends = [graph.port_position(from)?, graph.port_position(to)?];
            // Routed like the canvas's wires (previews' are not).
            let canvas = graph.canvas_of(pill)?;
            let (shape, avoid) = layouts
                .get(canvas)
                .map_or((WireShape::Curved, false), |(s, a)| (*s, a.0));
            let nodes = rects
                .entry(canvas)
                .or_insert_with(|| avoid.then(|| node_rects(canvas, &graph, &obstacles)))
                .as_deref();
            let key = placed_from(ends, shape.curvature(), nodes);
            let half = match halves.get(&pill) {
                Some(&(known, half)) if known == key => half,
                _ => halfway(ends[0], ends[1], shape.curvature(), nodes),
            };
            placed.insert(pill, (key, half));
            Some(Laid {
                pill,
                size: computed.size() * computed.inverse_scale_factor(),
                canvas,
                hubs,
                half,
            })
        })
        .collect();
    *halves = placed;
    laid.sort_by_key(|arrow| arrow.pill);
    // Each pill halfway along its arrow, or moved aside, across it, off the
    // nodes and the pills placed before it on its canvas.
    let mut placed: Vec<(Entity, Rect)> = Vec::new();
    let mut canvases: Vec<Entity> = laid.iter().map(|arrow| arrow.canvas).collect();
    canvases.sort();
    canvases.dedup();
    let nodes: Vec<(Entity, Rect)> = canvases
        .into_iter()
        .flat_map(|canvas| {
            let rects = node_rects(canvas, &graph, &obstacles);
            rects.into_iter().map(move |rect| (canvas, rect))
        })
        .collect();
    for arrow in &laid {
        let (middle, way) = arrow.half;
        let step = arrow.size.y + GAP;
        let places: Vec<Rect> = (0..MOVES_ASIDE)
            .map(|n| {
                let side = if n % 2 == 0 { n / 2 } else { -(n + 1) / 2 };
                let center = middle + way.perp() * step * side as f32;
                Rect::from_center_size(center, arrow.size)
            })
            .collect();
        let clear = |rect: &Rect, of: &[(Entity, Rect)]| {
            let apart = |other: &Rect| rect.inflate(GAP / 2.0).intersect(*other).is_empty();
            of.iter()
                .all(|(canvas, other)| *canvas != arrow.canvas || apart(other))
        };
        // Clear of pills at least, if nodes leave no room.
        let rect = places
            .iter()
            .find(|rect| clear(rect, &placed) && clear(rect, &nodes))
            .or_else(|| places.iter().find(|rect| clear(rect, &placed)))
            .copied()
            .unwrap_or(places[0]);
        placed.push((arrow.canvas, rect));
        if let Ok(mut position) = positions.get_mut(arrow.pill) {
            position.set_if_neq(NodePosition(rect.min));
        }
        for (hub, tangent) in arrow.hubs.into_iter().zip([-way, way]) {
            match tangents.get_mut(hub) {
                Ok(mut current) => _ = current.set_if_neq(PortTangent(tangent)),
                Err(_) => _ = commands.entity(hub).try_insert(PortTangent(tangent)),
            }
        }
    }
}

/// Give the ports arrows lead into a head, and the edges of arrows their
/// style; take heads away from ports no arrow leads into any more.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn show_heads(
    graph: GraphQuery,
    edges: Query<Entity, (With<Edge>, Without<ArrowEdge>)>,
    styles: Query<&EdgeStyle>,
    hub_ports: Query<Entity, With<ArrowHub>>,
    heads: Query<(Entity, &ChildOf), With<ArrowHead>>,
    colors: Query<&PortColor>,
    mut image: Local<Option<Handle<Image>>>,
    mut images: Option<ResMut<Assets<Image>>>,
    mut commands: Commands,
) {
    for edge in &edges {
        let Some(ports) = graph.edge_ports(edge) else {
            continue;
        };
        if [ports.output, ports.input]
            .iter()
            .any(|&p| hub_ports.contains(p))
        {
            let style = arrow_style(styles.get(edge).ok().copied());
            commands.entity(edge).try_insert((ArrowEdge, style));
        }
    }
    let into_arrows = |port: Entity| {
        graph.edges_of(port).any(|edge| {
            graph
                .edge_ports(edge)
                .is_some_and(|ports| ports.input == port && hub_ports.contains(ports.output))
        })
    };
    let mut headed = Vec::new();
    for (head, child_of) in &heads {
        match into_arrows(child_of.parent()) {
            true => headed.push(child_of.parent()),
            false => commands.entity(head).despawn(),
        }
    }
    let Some(images) = images.as_mut() else {
        return;
    };
    let image = image
        .get_or_insert_with(|| images.add(head_image()))
        .clone();
    let targets = hub_ports.iter().filter_map(|hub| {
        let port = peer(hub, &graph)?;
        (graph.port(port)?.direction == PortDirection::Input).then_some(port)
    });
    for port in targets {
        if headed.contains(&port) {
            continue;
        }
        headed.push(port);
        let color = colors.get(port).map_or(Color::WHITE, |c| c.0);
        commands.spawn((
            ArrowHead,
            ImageNode::new(image.clone()).with_color(color),
            Node {
                position_type: PositionType::Absolute,
                right: percent(100),
                top: percent(50),
                width: px(HEAD),
                height: px(HEAD),
                margin: UiRect::top(px(-HEAD / 2.0)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(port),
        ));
    }
}

/// A white triangle pointing right, its edges smoothed.
fn head_image() -> Image {
    const SIZE: u32 = 32;
    const SAMPLES: u32 = 4;
    let inside = |x: f32, y: f32| {
        let half = SIZE as f32 / 2.0;
        // Tip at the right edge's middle, base along the left edge.
        x >= 0.0 && (y - half).abs() <= half * (1.0 - x / SIZE as f32)
    };
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let hits = (0..SAMPLES * SAMPLES)
                .filter(|sample| {
                    let dx = (sample % SAMPLES) as f32 + 0.5;
                    let dy = (sample / SAMPLES) as f32 + 0.5;
                    let at = |pixel: u32, d: f32| pixel as f32 + d / SAMPLES as f32;
                    inside(at(x, dx), at(y, dy))
                })
                .count();
            let alpha = (hits * 255 / (SAMPLES * SAMPLES) as usize) as u8;
            data.extend_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// The first `State` port's name among `ports`.
fn state_port(ports: &[crate::model::PortDef]) -> Option<&str> {
    ports
        .iter()
        .find(|port| port.kind == PortKind::State)
        .map(|port| port.name.as_str())
}

/// Wiring a state's `State` output into another node's `State` input puts
/// an arrow between them; cutting half an arrow, or deleting a node it
/// links, deletes it.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn edit_arrows(
    mut request: On<EditRequested>,
    graph: GraphQuery,
    pills: Query<(), With<ArrowNode>>,
    hub_ports: Query<(), With<ArrowHub>>,
    arrow_edges: Query<(), With<ArrowEdge>>,
    views: Query<&FlowGraphView>,
    view_nodes: Query<&ViewNode>,
    arrows: Res<FlowArrows>,
    mut commands: Commands,
) {
    let canvas = request.canvas;
    // The pill an arrow edge leads from or to.
    let pill_of = |edge: Entity| {
        let ports = graph.edge_ports(edge)?;
        [ports.output, ports.input]
            .into_iter()
            .find(|&p| hub_ports.contains(p))
            .and_then(|hub| graph.node_of(hub))
    };
    match &mut request.edit {
        GraphEdit::Disconnect { edge } if arrow_edges.contains(*edge) => {
            if let Some(pill) = pill_of(*edge) {
                request.edit = GraphEdit::Delete { items: vec![pill] };
            }
        }
        GraphEdit::Delete { items } => {
            let mut more = Vec::new();
            for &item in items.iter() {
                if arrow_edges.contains(item) {
                    more.extend(pill_of(item));
                } else if graph.nodes_in(canvas).any(|node| node == item) && !pills.contains(item) {
                    let edges = graph.ports_of(item).flat_map(|port| graph.edges_of(port));
                    more.extend(edges.filter_map(pill_of));
                }
            }
            more.retain(|pill| !items.contains(pill));
            more.sort();
            more.dedup();
            items.extend(more);
        }
        &mut GraphEdit::Connect { from, to } => {
            let Ok(view) = views.get(canvas) else {
                return;
            };
            let Some(template) = arrows.templates.first() else {
                return;
            };
            let state = port_type(PortKind::State);
            let is_state = |port: Entity| graph.port(port).is_some_and(|p| p.port_type == state);
            let (output, input) = match graph.port(from).map(|p| p.direction) {
                Some(PortDirection::Output) => (from, to),
                _ => (to, from),
            };
            let (Some(source), Some(target)) = (graph.node_of(output), graph.node_of(input)) else {
                return;
            };
            // From a state (a node a state leads into: not an entry), into
            // a node; arrows themselves are wired as they are.
            let a_state = graph.inputs_of(source).any(is_state);
            if !is_state(output)
                || !is_state(input)
                || !a_state
                || pills.contains(source)
                || pills.contains(target)
            {
                return;
            }
            let name = |port: Entity| crate::view::port_ref(port, &view.graph, &graph, &view_nodes);
            let (Some((from_id, exit)), Some((to_id, enter))) = (name(output), name(input)) else {
                return;
            };
            let (Some(arrow_in), Some(arrow_out)) =
                (state_port(&template.inputs), state_port(&template.outputs))
            else {
                return;
            };
            request.reject();
            let ends = [graph.port_position(output), graph.port_position(input)];
            let position = match ends {
                [Some(a), Some(b)] => a.midpoint(b),
                _ => Vec2::ZERO,
            };
            let ends = [
                view.graph.node(from_id).cloned(),
                view.graph.node(to_id).cloned(),
            ];
            let [Some(source), Some(target)] = ends else {
                return;
            };
            let (template, path) = (template.clone(), view.path.clone());
            let (exit, enter) = (exit.to_owned(), enter.to_owned());
            let (arrow_in, arrow_out) = (arrow_in.to_owned(), arrow_out.to_owned());
            commands.queue(move |world: &mut World| {
                let Some(arrow) = insert_flow_node(world, canvas, template, position) else {
                    return;
                };
                let links = vec![
                    FlowLink::new(&source.id, &exit, &arrow.id, &arrow_in),
                    FlowLink::new(&arrow.id, &arrow_out, &target.id, &enter),
                ];
                let shown = FlowGraph {
                    nodes: vec![source, arrow, target],
                    links,
                    ..default()
                };
                _ = world.run_system_cached_with(connect_links, (canvas, path, shown));
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input_focus::InputFocus;
    use bevy::scene::ScenePlugin;
    use bevy::ui::Selected;
    use bevy_noodle::style::WireMaterial;

    use super::*;
    use crate::fields::{ValueFields, ValueFieldsPlugin};
    use crate::model::PortDef;
    use crate::view::{spawn_flow_graph_view, write_back_edits};

    const TRANSITION: &str = "test/transition";

    fn node(id: &str, kind: &str, inputs: Vec<PortDef>, outputs: Vec<PortDef>) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: kind.into(),
            title: id.into(),
            inputs,
            outputs,
            ..default()
        }
    }

    fn state(id: &str) -> FlowNode {
        let port = |name: &str| vec![PortDef::new(name, PortKind::State)];
        node(id, "test/state", port("enter"), port("exit"))
    }

    fn transition(id: &str) -> FlowNode {
        let inputs = vec![
            PortDef::new("from", PortKind::State),
            PortDef::new("condition", PortKind::Bool),
            PortDef::new("duration", PortKind::Float),
        ];
        let outputs = vec![PortDef::new("to", PortKind::State)];
        node(id, TRANSITION, inputs, outputs)
    }

    /// Entry → idle, and idle → walk through a transition.
    fn graph() -> FlowGraph {
        let entry = node(
            "entry",
            "test/entry",
            vec![],
            vec![PortDef::new("start", PortKind::State)],
        );
        FlowGraph {
            nodes: vec![entry, state("idle"), state("walk"), transition("go")],
            links: vec![
                FlowLink::new("entry", "start", "idle", "enter"),
                FlowLink::new("idle", "exit", "go", "from"),
                FlowLink::new("go", "to", "walk", "enter"),
            ],
            ..default()
        }
    }

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
            ArrowsPlugin,
        ))
        .init_resource::<InputFocus>()
        .init_asset::<WireMaterial>()
        .add_plugins(ValueFieldsPlugin)
        .insert_resource(FlowArrows {
            templates: vec![transition("")],
        })
        .add_systems(Update, write_back_edits);
        let root = app.world_mut().spawn(Node::default()).id();
        let canvas = app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(&mut commands, root, "flow/test.flowgraph.bsn", graph())
            })
            .unwrap();
        app.update();
        app.update();
        (app, canvas)
    }

    fn view(app: &App, canvas: Entity) -> FlowGraph {
        app.world()
            .get::<FlowGraphView>(canvas)
            .unwrap()
            .graph
            .clone()
    }

    /// The canvas node drawing `id`.
    fn drawn(app: &mut App, canvas: Entity, id: &'static str) -> Entity {
        app.world_mut()
            .run_system_once(move |graph: GraphQuery, nodes: Query<&ViewNode>| {
                graph
                    .nodes_in(canvas)
                    .find(|&node| nodes.get(node).is_ok_and(|n| n.0 == id))
            })
            .unwrap()
            .unwrap_or_else(|| panic!("{id} is drawn"))
    }

    /// The `State` port of the node drawing `id`, out of it or into it.
    fn state_port(app: &mut App, canvas: Entity, id: &'static str, output: bool) -> Entity {
        let node = drawn(app, canvas, id);
        app.world_mut()
            .run_system_once(move |graph: GraphQuery| match output {
                true => graph.outputs_of(node).next(),
                false => graph.inputs_of(node).next(),
            })
            .unwrap()
            .unwrap()
    }

    fn kinds(graph: &FlowGraph) -> Vec<&str> {
        graph.nodes.iter().map(|node| node.kind.as_str()).collect()
    }

    #[test]
    fn transitions_are_pills_on_arrows_under_the_nodes() {
        let (mut app, canvas) = app();
        let pill = drawn(&mut app, canvas, "go");
        assert!(app.world().get::<ArrowNode>(pill).is_some());
        // The pill's rows: condition and duration, not its `State` ports.
        let rows = app
            .world_mut()
            .run_system_once(move |children: Query<&Children>, rows: Query<&InputRow>| {
                let rows = children
                    .iter_descendants(pill)
                    .filter_map(|e| rows.get(e).ok());
                rows.map(|row| row.0).collect::<Vec<_>>()
            })
            .unwrap();
        assert_eq!(rows, [1, 2]);
        let (edges, styles) = app
            .world_mut()
            .run_system_once(
                move |graph: GraphQuery, styles: Query<&EdgeStyle, With<ArrowEdge>>| {
                    let edges: Vec<Entity> = graph.edges_in(canvas).collect();
                    let styles: Vec<EdgeStyle> = edges
                        .iter()
                        .filter_map(|&e| styles.get(e).ok())
                        .copied()
                        .collect();
                    (edges.len(), styles)
                },
            )
            .unwrap();
        assert_eq!(edges, 3);
        assert_eq!(styles.len(), 2, "both halves of the arrow");
        assert!(styles.iter().all(|style| style.below_nodes));
        // Drawn on the ports they were saved on.
        assert!(!app.world().get::<FlowGraphView>(canvas).unwrap().dirty);
        assert_eq!(view(&app, canvas), graph());
    }

    #[test]
    fn a_pill_shows_its_fields_while_selected() {
        let (mut app, canvas) = app();
        let pill = drawn(&mut app, canvas, "go");
        let shown = |app: &mut App| {
            app.world_mut()
                .run_system_once(
                    move |children: Query<&Children>, fields: Query<&Node, With<ValueFields>>| {
                        let fields = children
                            .iter_descendants(pill)
                            .filter_map(|e| fields.get(e).ok());
                        fields
                            .map(|node| node.display != Display::None)
                            .collect::<Vec<_>>()
                    },
                )
                .unwrap()
        };
        // Condition's box, duration's number.
        assert_eq!(shown(&mut app), [false, false], "ports alone");
        app.world_mut().entity_mut(pill).insert(Selected);
        app.update();
        assert_eq!(shown(&mut app), [true, true]);
    }

    #[test]
    fn wiring_a_state_into_another_puts_a_transition_between() {
        let (mut app, canvas) = app();
        let from = state_port(&mut app, canvas, "walk", true);
        let to = state_port(&mut app, canvas, "idle", false);
        _ = app
            .world_mut()
            .graph_edit(canvas, GraphEdit::Connect { from, to });
        app.update();
        app.update();
        let graph = view(&app, canvas);
        let added = graph.nodes.last().unwrap().id.clone();
        assert_eq!(added, "test_transition");
        for link in [
            FlowLink::new("walk", "exit", &added, "from"),
            FlowLink::new(&added, "to", "idle", "enter"),
        ] {
            assert!(graph.links.contains(&link), "{link:?} in {:?}", graph.links);
        }
        assert!(
            !graph
                .links
                .contains(&FlowLink::new("walk", "exit", "idle", "enter"))
        );

        // Not from an entry: it is wired straight.
        let from = state_port(&mut app, canvas, "entry", true);
        let to = state_port(&mut app, canvas, "walk", false);
        app.world_mut()
            .graph_edit(canvas, GraphEdit::Connect { from, to })
            .unwrap();
        app.update();
        let graph = view(&app, canvas);
        assert!(
            graph
                .links
                .contains(&FlowLink::new("entry", "start", "walk", "enter"))
        );
    }

    #[test]
    fn cutting_half_an_arrow_deletes_its_transition() {
        let (mut app, canvas) = app();
        let into = state_port(&mut app, canvas, "walk", false);
        let edge = app
            .world_mut()
            .run_system_once(move |graph: GraphQuery| graph.edges_of(into).next())
            .unwrap()
            .unwrap();
        _ = app
            .world_mut()
            .graph_edit(canvas, GraphEdit::Disconnect { edge });
        app.update();
        let graph = view(&app, canvas);
        assert_eq!(kinds(&graph), ["test/entry", "test/state", "test/state"]);
        assert_eq!(
            graph.links,
            [FlowLink::new("entry", "start", "idle", "enter")]
        );
    }

    #[test]
    fn deleting_a_state_deletes_its_arrows() {
        let (mut app, canvas) = app();
        let walk = drawn(&mut app, canvas, "walk");
        _ = app
            .world_mut()
            .graph_edit(canvas, GraphEdit::Delete { items: vec![walk] });
        app.update();
        let graph = view(&app, canvas);
        assert_eq!(kinds(&graph), ["test/entry", "test/state"]);
        assert_eq!(
            graph.links,
            [FlowLink::new("entry", "start", "idle", "enter")]
        );
    }

    #[test]
    fn the_head_points_right() {
        let image = head_image();
        let alpha = |x: u32, y: u32| image.get_color_at(x, y).unwrap().alpha();
        assert!(alpha(1, 16) > 0.9, "base");
        assert!(alpha(28, 16) > 0.5, "near the tip");
        assert!(
            alpha(28, 2) < 0.1 && alpha(28, 30) < 0.1,
            "clear around the tip"
        );
    }
}
