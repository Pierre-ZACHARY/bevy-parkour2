//! How a canvas lays its wires out: curved or straight ([`WireShape`]),
//! around the nodes in their way ([`AvoidNodes`], like
//! react-flow-smart-edge) and through the zip ties of their links. Both go
//! into bevy_noodle's [`EdgeStyle`] curvature and [`EdgeRoute`]s, between
//! its geometry and its drawing.

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::curve::cubic_splines::CubicSegment;
use bevy::picking::cursor::EntityCursor;
use bevy::prelude::*;
use bevy::window::SystemCursorIcon;
use bevy_noodle::NoodleSystems;
use bevy_noodle::prelude::*;

use crate::arrows::ArrowNode;
use crate::routing::find_path;
use crate::ties::tie_strands;
use crate::view::{FlowGraphView, ViewNode, link_of};

/// Clearance a wire keeps around nodes, in graph units.
const MARGIN: f32 = 14.0;
/// How far a detouring wire runs straight out of its port first.
const LEAD: f32 = MARGIN + 12.0;

pub(crate) struct WireLayoutPlugin;

impl Plugin for WireLayoutPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(press_button).add_systems(
            PostUpdate,
            lay_out_wires
                .after(NoodleSystems::Sync)
                .before(NoodleSystems::Render),
        );
    }
}

/// On a canvas: how its wires are drawn.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WireShape {
    #[default]
    Curved,
    Straight,
}

impl WireShape {
    /// bevy_noodle's curvature for the shape.
    pub fn curvature(self) -> f32 {
        match self {
            Self::Curved => 0.5,
            Self::Straight => 0.0,
        }
    }
}

/// On a canvas: whether its wires go around the nodes in their way.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AvoidNodes(pub bool);

impl Default for AvoidNodes {
    fn default() -> Self {
        Self(true)
    }
}

/// A button switching a canvas's wire layout.
#[derive(Component, Clone, Copy)]
enum WireButton {
    Shape(Entity),
    Avoid(Entity),
}

fn shape_label(shape: WireShape) -> &'static str {
    match shape {
        WireShape::Curved => "Curved wires",
        WireShape::Straight => "Straight wires",
    }
}

fn avoid_label(avoid: AvoidNodes) -> &'static str {
    if avoid.0 {
        "Around nodes"
    } else {
        "Through nodes"
    }
}

/// Buttons over `canvas`'s top-right corner, spawned under `parent` (the
/// canvas's container), switching its wires' shape and routing.
pub(crate) fn spawn_wire_buttons(commands: &mut Commands, parent: Entity, canvas: Entity) {
    let bar = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(8),
                right: px(8),
                column_gap: px(6),
                ..default()
            },
            ZIndex(5),
            ChildOf(parent),
        ))
        .id();
    for (button, label) in [
        (WireButton::Shape(canvas), shape_label(WireShape::default())),
        (
            WireButton::Avoid(canvas),
            avoid_label(AvoidNodes::default()),
        ),
    ] {
        commands.spawn((
            button,
            Text::new(label),
            TextFont::from_font_size(12.0),
            Node {
                padding: UiRect::axes(px(8), px(3)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(44, 46, 52, 230)),
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(bar),
        ));
    }
}

fn press_button(
    click: On<PointerClick>,
    buttons: Query<&WireButton>,
    mut layouts: Query<(&mut WireShape, &mut AvoidNodes)>,
    mut labels: Query<&mut Text>,
) {
    let Ok(&button) = buttons.get(click.entity) else {
        return;
    };
    let (WireButton::Shape(canvas) | WireButton::Avoid(canvas)) = button;
    let Ok((mut shape, mut avoid)) = layouts.get_mut(canvas) else {
        return;
    };
    let label = match button {
        WireButton::Shape(_) => {
            *shape = match *shape {
                WireShape::Curved => WireShape::Straight,
                WireShape::Straight => WireShape::Curved,
            };
            shape_label(*shape)
        }
        WireButton::Avoid(_) => {
            avoid.0 = !avoid.0;
            avoid_label(*avoid)
        }
    };
    if let Ok(mut text) = labels.get_mut(click.entity) {
        text.0 = label.to_string();
    }
}

/// On a canvas: a hash of what its routes were computed from.
#[derive(Component, PartialEq)]
pub(crate) struct RoutedFrom(u64);

/// The nodes wires go around: all but arrows' pills, which they run through.
pub(crate) type Obstacles<'w, 's> =
    Query<'w, 's, (&'static NodePosition, &'static ComputedNode), Without<ArrowNode>>;

/// Nodes as rectangles, in graph units.
pub(crate) fn node_rects(canvas: Entity, graph: &GraphQuery, nodes: &Obstacles) -> Vec<Rect> {
    graph
        .nodes_in(canvas)
        .filter_map(|node| {
            let (position, computed) = nodes.get(node).ok()?;
            let size = computed.size() * computed.inverse_scale_factor();
            Some(Rect::from_corners(position.0, position.0 + size))
        })
        .collect()
}

/// The waypoints of a wire from `geometry`'s output to its input through
/// `strands` (point, way through), detouring around `nodes` when given.
pub fn route(
    geometry: &EdgeGeometry,
    strands: &[(Vec2, Vec2)],
    curvature: f32,
    nodes: Option<&[Rect]>,
) -> Vec<RoutePoint> {
    let ends: Vec<(Vec2, Vec2)> = std::iter::once((geometry.start, geometry.start_tangent))
        .chain(strands.iter().copied())
        .chain(std::iter::once((geometry.end, -geometry.end_tangent)))
        .collect();
    let mut points = Vec::new();
    for (leg, pair) in ends.windows(2).enumerate() {
        let [(a, leaving), (b, entering)] = [pair[0], pair[1]];
        if leg > 0 {
            points.push(RoutePoint {
                position: a,
                direction: leaving,
            });
        }
        let Some(nodes) = nodes else {
            continue;
        };
        // The leg as it would be drawn, away from its ends (which sit on
        // the nodes' edges).
        let direct = EdgeGeometry::between((a, leaving), (b, -entering)).bezier(curvature);
        let blocked = CubicSegment::new_bezier(direct)
            .iter_positions(32)
            .filter(|p| p.distance(a) > LEAD && p.distance(b) > LEAD)
            .any(|p| nodes.iter().any(|rect| rect.contains(p)));
        if !blocked {
            continue;
        }
        let (out, into) = (a + leaving * LEAD, b - entering * LEAD);
        // Clear of the nodes by the margin, unless that would swallow an
        // end (a tie close to a node): then just clear of that node.
        let grown: Vec<Rect> = nodes
            .iter()
            .map(|rect| {
                let grown = rect.inflate(MARGIN);
                match grown.contains(out) || grown.contains(into) {
                    true => *rect,
                    false => grown,
                }
            })
            .collect();
        let Some(path) = find_path(out, into, &grown) else {
            continue;
        };
        points.push(RoutePoint {
            position: out,
            direction: leaving,
        });
        let corners = path
            .get(1..path.len().saturating_sub(1))
            .unwrap_or_default();
        points.extend(corners.iter().map(|&corner| RoutePoint::at(corner)));
        points.push(RoutePoint {
            position: into,
            direction: entering,
        });
    }
    points
}

/// An edge to lay out: its geometry, current route and tie strands.
type Laid<'a> = (
    Entity,
    EdgeGeometry,
    Option<&'a EdgeRoute>,
    Vec<(Vec2, Vec2)>,
);

/// What `edges`' routes on a canvas depend on, hashed.
fn routed_from(layout: (&WireShape, &AvoidNodes), rects: &[Rect], edges: &[Laid]) -> RoutedFrom {
    let mut hasher = DefaultHasher::new();
    layout.hash(&mut hasher);
    let floats = |hasher: &mut DefaultHasher, values: &[f32]| {
        values.iter().for_each(|v| v.to_bits().hash(hasher));
    };
    for rect in rects {
        floats(
            &mut hasher,
            &[rect.min.x, rect.min.y, rect.max.x, rect.max.y],
        );
    }
    for (edge, geometry, _, strands) in edges {
        edge.hash(&mut hasher);
        let (s, e) = (geometry.start, geometry.end);
        let (st, et) = (geometry.start_tangent, geometry.end_tangent);
        floats(&mut hasher, &[s.x, s.y, e.x, e.y, st.x, st.y, et.x, et.y]);
        for (point, way) in strands {
            floats(&mut hasher, &[point.x, point.y, way.x, way.y]);
        }
    }
    RoutedFrom(hasher.finish())
}

pub(crate) fn lay_out_wires(
    mut commands: Commands,
    graph: GraphQuery,
    canvases: Query<(
        Entity,
        &FlowGraphView,
        &WireShape,
        &AvoidNodes,
        Option<&RoutedFrom>,
    )>,
    nodes: Obstacles,
    view_nodes: Query<&ViewNode>,
    geometries: Query<(&EdgeGeometry, Option<&EdgeRoute>)>,
    mut styles: Query<&mut EdgeStyle>,
) {
    for (canvas, view, shape, avoid, routed) in &canvases {
        let curvature = shape.curvature();
        // On the canvas, and on edges with a style of their own (lit ones).
        for entity in std::iter::once(canvas).chain(graph.edges_in(canvas)) {
            if let Ok(mut style) = styles.get_mut(entity)
                && style.curvature != curvature
            {
                style.curvature = curvature;
            }
        }

        let rects = node_rects(canvas, &graph, &nodes);
        let strands: Vec<_> = view.graph.ties.iter().flat_map(tie_strands).collect();
        let edges: Vec<Laid> = graph
            .edges_in(canvas)
            .filter_map(|edge| {
                let (geometry, current) = geometries.get(edge).ok()?;
                let link = link_of(edge, &view.graph, &graph, &view_nodes)?;
                let through = strands
                    .iter()
                    .filter(|(tied, ..)| *tied == link)
                    .map(|&(_, point, way)| (point, way))
                    .collect();
                Some((edge, *geometry, current, through))
            })
            .collect();
        let inputs = routed_from((shape, avoid), &rects, &edges);
        if routed == Some(&inputs) {
            continue;
        }
        commands.entity(canvas).insert(inputs);

        let obstacles = avoid.0.then_some(rects.as_slice());
        for (edge, geometry, current, through) in edges {
            let points = route(&geometry, &through, curvature, obstacles);
            match (points.is_empty(), current) {
                (true, Some(_)) => _ = commands.entity(edge).remove::<EdgeRoute>(),
                (true, None) => {}
                (false, Some(current)) if current.0 == points => {}
                (false, _) => _ = commands.entity(edge).insert(EdgeRoute(points)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> EdgeGeometry {
        EdgeGeometry::between((Vec2::ZERO, Vec2::X), (Vec2::new(400.0, 0.0), Vec2::NEG_X))
    }

    #[test]
    fn wires_with_nothing_in_the_way_are_not_routed() {
        let nodes = [Rect::new(100.0, 100.0, 200.0, 200.0)];
        assert!(route(&geometry(), &[], 0.5, Some(&nodes)).is_empty());
    }

    #[test]
    fn wires_detour_around_nodes_in_their_way() {
        let nodes = [Rect::new(150.0, -50.0, 250.0, 50.0)];
        let points = route(&geometry(), &[], 0.5, Some(&nodes));
        assert!(points.len() >= 3, "{points:?}");
        // Out of the port first, then around: no waypoint inside the node.
        assert_eq!(points[0].position, Vec2::new(LEAD, 0.0));
        let grown = nodes[0].inflate(MARGIN);
        assert!(points.iter().all(|p| !grown.contains(p.position)));
        // Not asked to: straight through.
        assert!(route(&geometry(), &[], 0.5, None).is_empty());
    }

    #[test]
    fn wires_out_of_a_tie_near_a_node_still_go_around_it() {
        let node = Rect::new(150.0, -50.0, 250.0, 50.0);
        // A tie just left of the node, closer than the margin once out of it.
        let strand = (Vec2::new(150.0 - LEAD - 5.0, 0.0), Vec2::X);
        let points = route(&geometry(), &[strand], 0.5, Some(&[node]));
        let leg: Vec<Vec2> = points.iter().map(|p| p.position).collect();
        assert!(leg.len() > 2, "{leg:?}");
        assert!(
            leg.windows(2)
                .all(|w| crate::routing::clear(w[0], w[1], &[node])),
            "{leg:?}"
        );
    }

    #[test]
    fn tied_wires_pass_their_strand_its_way() {
        let strand = (Vec2::new(200.0, 80.0), Vec2::Y);
        let points = route(&geometry(), &[strand], 0.5, None);
        assert_eq!(
            points,
            [RoutePoint {
                position: strand.0,
                direction: strand.1
            }]
        );
    }
}
