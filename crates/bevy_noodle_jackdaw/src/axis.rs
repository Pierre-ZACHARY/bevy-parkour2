//! Blend space axes: a blend space node shows its samples as points on its
//! axis (1D) or plane (2D), rather than as nodes. Hovering a point names
//! its sample; dragging it moves the sample; a right click removes it. A
//! click away from the points probes the blend there: how much of each
//! sample would play, as the points light up. Dropping a library item on
//! the axis adds a sample playing it. A double click zooms onto the axis.
//!
//! While the game plays, the blend space's input is marked where it is,
//! and the points light up by how much they play.
//!
//! Under the axis, a selected node with a probe (or a sample picked) shows
//! its blend on a character: the probe's, the picked sample's, or, while
//! the pointer is over the axis, the blend under it. Drawing it is a
//! preview plugin's job: the node asks with a [`BlendPreview`] on its
//! [`BlendPreviewSlot`], hidden without one.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::window::SystemCursorIcon;
use bevy_noodle::prelude::*;
use bevy_noodle::style::{WireMaterial, stroke_material};

use crate::blend::{SELECT_2D, axis_range, sample_weights, triangulate};
use crate::fields::{FIELD, FIELD_BORDER, format_number};
use crate::levels::focus_on;
use crate::library::local_point;
use crate::model::{BlendSample, FlowNode};
use crate::run::FlowGraphPlayback;
use crate::view::{ACTIVE, FlowGraphView, ViewNode, same_graph_path};

/// The plot's size on the node: an axis, a plane.
const SIZE_1D: Vec2 = Vec2::new(200.0, 40.0);
const SIZE_2D: Vec2 = Vec2::new(150.0, 150.0);
/// The preview's height under the plot, as wide.
const PREVIEW_HEIGHT: f32 = 200.0;
/// Between the plot's edges and the samples furthest out.
const PAD: f32 = 10.0;
/// Where the axis runs across a 1D plot.
const LINE_Y: f32 = 16.0;
const DOT: f32 = 9.0;
const SELECTED_DOT: f32 = 12.0;
const MARK: f32 = 11.0;
const LINE_WIDTH: f32 = 1.0;
const DOT_COLOR: Color = Color::srgb_u8(214, 218, 224);
const LINE: Color = Color::srgb_u8(90, 95, 108);
const TICK: Color = Color::srgb_u8(150, 154, 163);
const PROBE: Color = Color::srgb(0.45, 0.7, 1.0);
const LABEL: Color = Color::srgb_u8(20, 21, 25);

/// The plot of a blend space node's samples. Focusable: drags on it are its
/// own, not the node's.
#[derive(Component, Debug, Default)]
pub(crate) struct BlendAxis {
    /// The sample under the pointer.
    hovered: Option<usize>,
    selected: Option<usize>,
    /// Where a click away from the samples probes the blend.
    probe: Option<Vec2>,
    /// The values the plot runs between, as last drawn.
    range: Option<Rect>,
    /// The range held while a sample is dragged, so it does not slide.
    frozen: Option<Rect>,
    /// The value under the pointer while it is over the plot.
    pointer: Option<Vec2>,
    /// Where the blend shows on a character.
    slot: Option<Entity>,
}

/// Where a blend space node shows its blend on a character, under its axis:
/// drawn by a preview plugin, which shows it while it holds a
/// [`BlendPreview`]. Without the plugin, it stays hidden.
#[derive(Component, Debug, Default)]
pub struct BlendPreviewSlot;

/// What a [`BlendPreviewSlot`] is to show: blend space `space`'s blend with
/// its input at `at`.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct BlendPreview {
    pub space: FlowNode,
    pub at: Vec2,
}

/// A sample's point, by its index in the node's samples.
#[derive(Component)]
struct SampleDot(usize);

/// A line of the plot (the axis, or an edge of the samples' triangles),
/// drawn by the wires' shader so it stays smooth at any zoom.
#[derive(Component)]
struct AxisStroke(usize);

/// A range end's value: which axis, and its high end or not.
#[derive(Component)]
struct TickLabel {
    axis: usize,
    high: bool,
}

/// Where the blend space's input is while it plays.
#[derive(Component)]
struct InputMark;

/// Where the probe is.
#[derive(Component)]
struct ProbeMark;

/// Names the hovered sample, or the probe's weights.
#[derive(Component)]
struct AxisLabel;

pub(crate) struct BlendAxisPlugin;

impl Plugin for BlendAxisPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(hover_dot)
            .add_observer(leave_dot)
            .add_observer(start_dot_drag)
            .add_observer(drag_dot)
            .add_observer(end_dot_drag)
            .add_observer(click_dot)
            .add_observer(click_plot)
            .add_observer(drag_probe)
            .add_observer(point_at_plot)
            .add_observer(leave_plot)
            .add_systems(Update, sync_axes);
    }
}

fn is_2d(node: &FlowNode) -> bool {
    crate::blend::is_2d(&node.kind)
}

fn plot_size(two_d: bool) -> Vec2 {
    if two_d { SIZE_2D } else { SIZE_1D }
}

/// Draw the axis of blend space node `node` (a canvas node) inside it, and
/// under it, the slot of its preview.
pub(crate) fn spawn_blend_axis(commands: &mut Commands, node: Entity, two_d: bool) {
    let size = plot_size(two_d);
    let plot = commands
        .spawn((
            TabIndex(0),
            Node {
                width: px(size.x),
                height: px(size.y),
                margin: UiRect::new(px(8), px(8), px(4), px(if two_d { 18 } else { 6 })),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(FIELD_BORDER),
            EntityCursor::System(SystemCursorIcon::Crosshair),
            ChildOf(node),
        ))
        .id();
    let slot = commands
        .spawn((
            BlendPreviewSlot,
            Node {
                width: px(size.x),
                height: px(PREVIEW_HEIGHT),
                margin: UiRect::new(px(8), px(8), px(0), px(8)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::clip(),
                display: Display::None,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(node),
        ))
        .id();
    commands.entity(plot).insert(BlendAxis {
        slot: Some(slot),
        ..default()
    });
    // The range ends: under the axis, or round the plane.
    let ticks: &[(usize, bool, UiRect)] = if two_d {
        &[
            (0, false, UiRect::new(px(0), auto(), px(size.y), auto())),
            (0, true, UiRect::new(auto(), px(0), px(size.y), auto())),
            (1, true, UiRect::new(px(2), auto(), px(1), auto())),
        ]
    } else {
        &[
            (
                0,
                false,
                UiRect::new(px(2), auto(), px(LINE_Y + 5.0), auto()),
            ),
            (
                0,
                true,
                UiRect::new(auto(), px(2), px(LINE_Y + 5.0), auto()),
            ),
        ]
    };
    for &(axis, high, at) in ticks {
        commands.spawn((
            TickLabel { axis, high },
            Text::default(),
            TextFont::from_font_size(10.0),
            TextColor(TICK),
            Node {
                position_type: PositionType::Absolute,
                left: at.left,
                right: at.right,
                top: at.top,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(plot),
        ));
    }
    let ring = |color: Color| {
        (
            Node {
                position_type: PositionType::Absolute,
                width: px(MARK),
                height: px(MARK),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(MARK / 2.0)),
                display: Display::None,
                ..default()
            },
            BorderColor::all(color),
            ZIndex(1),
            Pickable::IGNORE,
            ChildOf(plot),
        )
    };
    commands.spawn((InputMark, ring(ACTIVE)));
    commands.spawn((ProbeMark, ring(PROBE)));
    commands.spawn((
        AxisLabel,
        Text::default(),
        TextFont::from_font_size(11.0),
        TextColor(LABEL),
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(4), px(1)),
            border_radius: BorderRadius::all(px(3)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(DOT_COLOR),
        ZIndex(2),
        Pickable::IGNORE,
        ChildOf(plot),
    ));
}

/// The values a plot of `samples` runs between: theirs, from 0.
fn range_of(samples: &[BlendSample]) -> Rect {
    let (x, y) = (axis_range(samples, 0), axis_range(samples, 1));
    Rect::new(x.0, y.0, x.1, y.1)
}

/// Where value `at` is on a plot of `size`, in its pixels: 1D on the axis
/// line, 2D up the plane.
fn on_plot(at: Vec2, range: Rect, size: Vec2, two_d: bool) -> Vec2 {
    let t = (at - range.min) / range.size();
    let inner = size - 2.0 * PAD;
    if two_d {
        Vec2::new(PAD + t.x * inner.x, PAD + (1.0 - t.y) * inner.y)
    } else {
        Vec2::new(PAD + t.x * inner.x, LINE_Y)
    }
}

/// The value at `point` (the plot's pixels), the inverse of [`on_plot`]. 1D
/// values have no y.
fn from_plot(point: Vec2, range: Rect, size: Vec2, two_d: bool) -> Vec2 {
    let t = (point - PAD) / (size - 2.0 * PAD);
    let x = range.min.x + t.x * range.width();
    if two_d {
        Vec2::new(x, range.min.y + (1.0 - t.y) * range.height())
    } else {
        Vec2::new(x, 0.0)
    }
}

/// `value` rounded to a step suiting `span`: 0.05 for a few units.
fn snap(value: f32, span: f32) -> f32 {
    let step = 10f32.powf(span.max(1e-3).log10().floor()) / 20.0;
    (value / step).round() * step
}

fn snap_point(at: Vec2, range: Rect) -> Vec2 {
    Vec2::new(snap(at.x, range.width()), snap(at.y, range.height()))
}

/// What a plot edits and reads: its canvas, and its node's id.
fn plot_target(
    plot: Entity,
    graph: &GraphQuery,
    view_nodes: &Query<&ViewNode>,
) -> Option<(Entity, String)> {
    let node = graph.node_of(plot)?;
    Some((graph.canvas_of(node)?, view_nodes.get(node).ok()?.0.clone()))
}

/// Edit the samples of node `id` in `canvas`'s view with `edit`, marking
/// the graph edited if they changed.
fn edit_samples(
    views: &mut Query<&mut FlowGraphView>,
    canvas: Entity,
    id: &str,
    edit: impl FnOnce(&mut Vec<BlendSample>),
) {
    let Ok(mut view) = views.get_mut(canvas) else {
        return;
    };
    let Some(node) = view.graph.nodes.iter().find(|node| node.id == id) else {
        return;
    };
    let mut samples = node.samples.clone();
    edit(&mut samples);
    if samples != node.samples {
        let node = view.graph.nodes.iter_mut().find(|node| node.id == id);
        if let Some(node) = node {
            node.samples = samples;
            view.dirty = true;
        }
    }
}

type Layout<'a> = (&'a ComputedNode, &'a UiGlobalTransform);

/// Window position `position` → the value under it on `plot`, and whether
/// it is inside the plot.
fn value_under(
    plot: Entity,
    position: Vec2,
    range: Rect,
    two_d: bool,
    layouts: &Query<Layout>,
    ui_scale: f32,
) -> Option<(Vec2, bool)> {
    let (computed, transform) = layouts.get(plot).ok()?;
    let (local, inside) = local_point(computed, transform, ui_scale, position)?;
    Some((from_plot(local, range, plot_size(two_d), two_d), inside))
}

/// Drops library items on blend space axes.
#[derive(SystemParam)]
pub(crate) struct AxisDrops<'w, 's> {
    plots: Query<'w, 's, (Entity, &'static BlendAxis, Layout<'static>)>,
    graph: GraphQuery<'w, 's>,
    view_nodes: Query<'w, 's, &'static ViewNode>,
    views: Query<'w, 's, &'static mut FlowGraphView>,
    ui_scale: Res<'w, UiScale>,
}

impl AxisDrops<'_, '_> {
    /// Add a sample playing `template`'s kind to the blend space axis of
    /// `canvas` under window position `position`, if there is one there.
    /// Returns whether it did.
    pub(crate) fn drop(&mut self, canvas: Entity, template: &FlowNode, position: Vec2) -> bool {
        let ui_scale = self.ui_scale.0;
        let hit = self
            .plots
            .iter()
            .find_map(|(plot, axis, (computed, transform))| {
                let (local, inside) = local_point(computed, transform, ui_scale, position)?;
                let (in_canvas, id) = plot_target(plot, &self.graph, &self.view_nodes)?;
                (inside && in_canvas == canvas).then_some((id, local, axis.range?))
            });
        let Some((id, local, range)) = hit else {
            return false;
        };
        let two_d = self
            .views
            .get(canvas)
            .ok()
            .and_then(|view| view.graph.node(&id).map(is_2d))
            .unwrap_or(false);
        let at = snap_point(from_plot(local, range, plot_size(two_d), two_d), range);
        edit_samples(&mut self.views, canvas, &id, |samples| {
            samples.push(BlendSample {
                kind: template.kind.clone(),
                title: template.title.clone(),
                at,
            });
        });
        true
    }
}

fn place(node: &mut Node, center: Vec2, size: f32) {
    let corner = center - size / 2.0;
    let (left, top) = (px(corner.x), px(corner.y));
    if node.left != left {
        node.left = left;
    }
    if node.top != top {
        node.top = top;
    }
}

fn set_display(node: &mut Node, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
}

/// The lines a plot draws: the axis in 1D; the samples' triangles' edges
/// in a 2D blend space; none for a select, which mixes nothing.
fn plot_lines(node: &FlowNode, range: Rect, size: Vec2) -> Vec<[Vec2; 2]> {
    if !is_2d(node) {
        return vec![[Vec2::new(PAD, LINE_Y), Vec2::new(size.x - PAD, LINE_Y)]];
    }
    if node.kind == SELECT_2D {
        return Vec::new();
    }
    let mut unique: Vec<Vec2> = Vec::new();
    for sample in &node.samples {
        if !unique.contains(&sample.at) {
            unique.push(sample.at);
        }
    }
    let mut edges: Vec<[usize; 2]> = Vec::new();
    for [a, b, c] in triangulate(&unique) {
        for [from, to] in [[a, b], [b, c], [c, a]] {
            let edge = [from.min(to), from.max(to)];
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
    }
    edges
        .iter()
        .map(|edge| edge.map(|i| on_plot(unique[i], range, size, true)))
        .collect()
}

/// How the probe at `at` mixes `node`'s samples, for the label:
/// `1.5: Walk 75%, Jog 25%`.
fn weights_text(node: &FlowNode, at: Vec2, weights: &[f32]) -> String {
    let mut shown: Vec<(f32, &str)> = node
        .samples
        .iter()
        .zip(weights)
        .filter(|(_, w)| **w > 0.005)
        .map(|(sample, &w)| (w, sample.title.as_str()))
        .collect();
    shown.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mix = shown
        .iter()
        .map(|(w, title)| format!("{title} {:.0}%", w * 100.0))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}: {mix}", value_text(at, is_2d(node)))
}

/// A sample's value, for the label: `1.4`, `1.4, -2`.
fn value_text(at: Vec2, two_d: bool) -> String {
    if two_d {
        format!("{}, {}", format_number(at.x), format_number(at.y))
    } else {
        format_number(at.x)
    }
}

/// What a plot draws with.
type AxisParts<'a> = (
    &'a mut Node,
    Option<&'a SampleDot>,
    Option<(&'a AxisStroke, &'a MaterialNode<WireMaterial>)>,
    Option<&'a TickLabel>,
    Option<&'a mut Text>,
    Option<&'a mut BackgroundColor>,
    Has<InputMark>,
    Has<ProbeMark>,
    Has<AxisLabel>,
);

/// Plots show their node's samples, as many points and lines as it takes,
/// lit by the probe's weights or else the playing input's.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn sync_axes(
    mut plots: Query<(Entity, &mut BlendAxis, Option<&Children>)>,
    mut parts: Query<AxisParts>,
    selected: Query<(), With<Selected>>,
    previews: Query<&BlendPreview>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    views: Query<&FlowGraphView>,
    playbacks: Query<&FlowGraphPlayback>,
    mut materials: ResMut<Assets<WireMaterial>>,
    mut commands: Commands,
) {
    for (plot, mut axis, children) in &mut plots {
        let Some((canvas, id)) = plot_target(plot, &graph, &view_nodes) else {
            continue;
        };
        let Ok(view) = views.get(canvas) else {
            continue;
        };
        let Some(node) = view.graph.node(&id) else {
            continue;
        };
        let two_d = is_2d(node);
        let size = plot_size(two_d);
        let range = axis.frozen.unwrap_or_else(|| range_of(&node.samples));
        if axis.range != Some(range) {
            axis.range = Some(range);
        }
        let count = node.samples.len();
        if axis.hovered.is_some_and(|i| i >= count) {
            axis.hovered = None;
        }
        if axis.selected.is_some_and(|i| i >= count) {
            axis.selected = None;
        }
        // The input, while the blend space plays.
        let live = playbacks
            .iter()
            .find(|playback| same_graph_path(&playback.graph, &view.path))
            .and_then(|playback| playback.node(&format!("{}{}", view.prefix(), id)))
            .filter(|state| state.is_active())
            .map(|state| {
                let input = |i: usize| state.inputs.get(i).map_or(0.0, |v| v.as_f32());
                Vec2::new(input(1), if two_d { input(2) } else { 0.0 })
            });
        let lit_by = axis.probe.or(live);
        let weights = lit_by
            .map(|at| sample_weights(node, at))
            .unwrap_or_default();
        let lines = plot_lines(node, range, size);
        if let Some(slot) = axis.slot {
            let preview = preview_of(
                &axis,
                node,
                graph.node_of(plot).is_some_and(|n| selected.contains(n)),
            );
            match preview {
                Some(preview) if previews.get(slot) != Ok(&preview) => {
                    commands.entity(slot).insert(preview);
                }
                None if previews.contains(slot) => {
                    commands.entity(slot).remove::<BlendPreview>();
                }
                _ => {}
            }
        }

        let mut dots = vec![false; count];
        let mut strokes = vec![false; lines.len()];
        for &child in children.into_iter().flatten() {
            let Ok((mut part, dot, stroke, tick, text, background, input, probe, label)) =
                parts.get_mut(child)
            else {
                continue;
            };
            if let Some(SampleDot(i)) = dot {
                let Some(sample) = node.samples.get(*i) else {
                    commands.entity(child).despawn();
                    continue;
                };
                dots[*i] = true;
                let selected = axis.selected == Some(*i);
                let size_px = if selected { SELECTED_DOT } else { DOT };
                if part.width != px(size_px) {
                    (part.width, part.height) = (px(size_px), px(size_px));
                    part.border_radius = BorderRadius::all(px(size_px / 2.0));
                }
                place(&mut part, on_plot(sample.at, range, size, two_d), size_px);
                let weight = weights.get(*i).copied().unwrap_or(0.0);
                let color = DOT_COLOR.mix(&ACTIVE, weight.clamp(0.0, 1.0));
                if let Some(mut background) = background {
                    background.set_if_neq(BackgroundColor(color));
                }
            }
            if let Some((AxisStroke(i), material)) = stroke {
                let Some(&[from, to]) = lines.get(*i) else {
                    commands.entity(child).despawn();
                    continue;
                };
                strokes[*i] = true;
                draw_line(&mut part, &material.0, &mut materials, from, to);
            }
            if let Some(tick) = tick {
                let end = if tick.high { range.max } else { range.min };
                let value = format_number(end[tick.axis]);
                if let Some(mut text) = text
                    && text.0 != value
                {
                    text.0 = value;
                }
                continue;
            }
            if input {
                set_display(&mut part, live.is_some());
                if let Some(at) = live {
                    place(&mut part, on_plot(at, range, size, two_d), MARK);
                }
            }
            if probe {
                set_display(&mut part, axis.probe.is_some());
                if let Some(at) = axis.probe {
                    place(&mut part, on_plot(at, range, size, two_d), MARK);
                }
            }
            if label && let Some(mut text) = text {
                // The hovered sample, else what the probe mixes.
                let (said, at) = match axis.hovered.and_then(|i| node.samples.get(i)) {
                    Some(sample) => (
                        format!("{}  {}", sample.title, value_text(sample.at, two_d)),
                        Some(sample.at),
                    ),
                    None => (
                        axis.probe
                            .map_or_else(String::new, |at| weights_text(node, at, &weights)),
                        axis.probe,
                    ),
                };
                set_display(&mut part, !said.is_empty());
                if text.0 != said {
                    text.0 = said;
                }
                if let Some(at) = at {
                    let corner = on_plot(at, range, size, two_d) + Vec2::new(8.0, -22.0);
                    let (left, top) = (px(corner.x), px(corner.y));
                    if (part.left, part.top) != (left, top) {
                        (part.left, part.top) = (left, top);
                    }
                }
            }
        }
        for (i, _) in dots.iter().enumerate().filter(|(_, drawn)| !**drawn) {
            commands.spawn((
                SampleDot(i),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(DOT),
                    height: px(DOT),
                    border_radius: BorderRadius::all(px(DOT / 2.0)),
                    ..default()
                },
                BackgroundColor(DOT_COLOR),
                EntityCursor::System(SystemCursorIcon::Grab),
                ChildOf(plot),
            ));
        }
        for (i, _) in strokes.iter().enumerate().filter(|(_, drawn)| !**drawn) {
            let material = materials.add(stroke_material([Vec2::ZERO; 4], LINE, LINE_WIDTH).1);
            commands.spawn((
                AxisStroke(i),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                MaterialNode(material),
                Pickable::IGNORE,
                ChildOf(plot),
            ));
        }
    }
}

/// What `axis`'s node previews: nothing unless it is `selected` and has a
/// probe or a sample picked; then the blend at the hovered sample, else
/// under the pointer, else at the picked sample, else at the probe.
fn preview_of(axis: &BlendAxis, node: &FlowNode, selected: bool) -> Option<BlendPreview> {
    if !selected || (axis.probe.is_none() && axis.selected.is_none()) {
        return None;
    }
    let sample = |i: Option<usize>| Some(node.samples.get(i?)?.at);
    let at = sample(axis.hovered)
        .or(axis.pointer)
        .or(sample(axis.selected))
        .or(axis.probe)?;
    Some(BlendPreview {
        space: node.clone(),
        at,
    })
}

/// Draw a line from `from` to `to` (the plot's pixels) with `node` and
/// `material`.
fn draw_line(
    node: &mut Node,
    material: &Handle<WireMaterial>,
    materials: &mut Assets<WireMaterial>,
    from: Vec2,
    to: Vec2,
) {
    let (rect, drawn) = stroke_material([from, from, to, to], LINE, LINE_WIDTH);
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

/// The plot a sample's point is on, and the sample's index.
fn dot_on(dot: Entity, dots: &Query<(&SampleDot, &ChildOf)>) -> Option<(Entity, usize)> {
    let (SampleDot(i), child_of) = dots.get(dot).ok()?;
    Some((child_of.parent(), *i))
}

fn hover_dot(
    over: On<PointerOver>,
    dots: Query<(&SampleDot, &ChildOf)>,
    mut axes: Query<&mut BlendAxis>,
) {
    if let Some((plot, i)) = dot_on(over.entity, &dots)
        && let Ok(mut axis) = axes.get_mut(plot)
    {
        axis.hovered = Some(i);
    }
}

fn leave_dot(
    out: On<PointerOut>,
    dots: Query<(&SampleDot, &ChildOf)>,
    mut axes: Query<&mut BlendAxis>,
) {
    if let Some((plot, i)) = dot_on(out.entity, &dots)
        && let Ok(mut axis) = axes.get_mut(plot)
        && axis.hovered == Some(i)
    {
        axis.hovered = None;
    }
}

/// Dragging a point holds the plot's range still until it is let go.
fn start_dot_drag(
    mut start: On<PointerDragStart>,
    dots: Query<(&SampleDot, &ChildOf)>,
    mut axes: Query<&mut BlendAxis>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    if let Some((plot, i)) = dot_on(start.entity, &dots)
        && let Ok(mut axis) = axes.get_mut(plot)
    {
        start.propagate(false);
        axis.frozen = axis.range;
        axis.selected = Some(i);
    }
}

fn end_dot_drag(
    mut end: On<PointerDragEnd>,
    dots: Query<(&SampleDot, &ChildOf)>,
    mut axes: Query<&mut BlendAxis>,
) {
    if let Some((plot, _)) = dot_on(end.entity, &dots)
        && let Ok(mut axis) = axes.get_mut(plot)
    {
        end.propagate(false);
        axis.frozen = None;
    }
}

/// Dragging a point moves its sample, snapped to round values.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn drag_dot(
    mut drag: On<PointerDrag>,
    dots: Query<(&SampleDot, &ChildOf)>,
    axes: Query<&BlendAxis>,
    layouts: Query<Layout>,
    ui_scale: Res<UiScale>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    mut views: Query<&mut FlowGraphView>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some((plot, i)) = dot_on(drag.entity, &dots) else {
        return;
    };
    drag.propagate(false);
    let (Ok(axis), Some((canvas, id))) = (axes.get(plot), plot_target(plot, &graph, &view_nodes))
    else {
        return;
    };
    let Some(range) = axis.frozen.or(axis.range) else {
        return;
    };
    let two_d = views
        .get(canvas)
        .ok()
        .and_then(|view| view.graph.node(&id).map(is_2d))
        .unwrap_or(false);
    let position = drag.pointer.position;
    let Some((at, _)) = value_under(plot, position, range, two_d, &layouts, ui_scale.0) else {
        return;
    };
    let at = snap_point(at, range);
    edit_samples(&mut views, canvas, &id, |samples| {
        if let Some(sample) = samples.get_mut(i) {
            sample.at = at;
        }
    });
}

/// A click on a point selects its sample; a right click removes it.
fn click_dot(
    mut click: On<PointerClick>,
    dots: Query<(&SampleDot, &ChildOf)>,
    mut axes: Query<&mut BlendAxis>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    mut views: Query<&mut FlowGraphView>,
) {
    let Some((plot, i)) = dot_on(click.entity, &dots) else {
        return;
    };
    let Ok(mut axis) = axes.get_mut(plot) else {
        return;
    };
    click.propagate(false);
    match click.button {
        PointerButton::Primary => {
            axis.selected = Some(i);
            axis.probe = None;
        }
        PointerButton::Secondary => {
            let Some((canvas, id)) = plot_target(plot, &graph, &view_nodes) else {
                return;
            };
            edit_samples(&mut views, canvas, &id, |samples| {
                if i < samples.len() {
                    samples.remove(i);
                }
            });
            (axis.hovered, axis.selected) = (None, None);
        }
        _ => {}
    }
}

/// A click away from the points probes the blend there; a right click
/// takes the probe away; a double click zooms onto the plot.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn click_plot(
    mut click: On<PointerClick>,
    mut axes: Query<&mut BlendAxis>,
    layouts: Query<Layout>,
    ui_scale: Res<UiScale>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    views: Query<&FlowGraphView>,
    mut commands: Commands,
) {
    let plot = click.entity;
    let Ok(mut axis) = axes.get_mut(plot) else {
        return;
    };
    click.propagate(false);
    let Some((canvas, id)) = plot_target(plot, &graph, &view_nodes) else {
        return;
    };
    match (click.button, click.count) {
        (PointerButton::Primary, 2) => focus_on(&mut commands, canvas, plot),
        (PointerButton::Primary, _) => {
            let two_d = views
                .get(canvas)
                .ok()
                .and_then(|view| view.graph.node(&id).map(is_2d))
                .unwrap_or(false);
            let Some(range) = axis.range else {
                return;
            };
            let position = click.pointer.position;
            if let Some((at, true)) =
                value_under(plot, position, range, two_d, &layouts, ui_scale.0)
            {
                axis.probe = Some(snap_point(at, range));
                axis.selected = None;
            }
        }
        (PointerButton::Secondary, _) => axis.probe = None,
        _ => {}
    }
}

/// Moving the pointer over a plot (its points too) previews the blend
/// under it.
fn point_at_plot(
    pointer: On<PointerMove>,
    mut axes: Query<&mut BlendAxis>,
    layouts: Query<Layout>,
    ui_scale: Res<UiScale>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    views: Query<&FlowGraphView>,
) {
    let plot = pointer.entity;
    let Ok(mut axis) = axes.get_mut(plot) else {
        return;
    };
    let (Some(range), Some((canvas, id))) = (axis.range, plot_target(plot, &graph, &view_nodes))
    else {
        return;
    };
    let two_d = views
        .get(canvas)
        .ok()
        .and_then(|view| view.graph.node(&id).map(is_2d))
        .unwrap_or(false);
    let position = pointer.pointer.position;
    if let Some((at, _)) = value_under(plot, position, range, two_d, &layouts, ui_scale.0) {
        axis.pointer = Some(at.clamp(range.min, range.max));
    }
}

/// Leaving a plot (or one of its points) stops previewing under the
/// pointer, until it moves over it again.
fn leave_plot(out: On<PointerOut>, mut axes: Query<&mut BlendAxis>) {
    if let Ok(mut axis) = axes.get_mut(out.entity)
        && axis.pointer.is_some()
    {
        axis.pointer = None;
    }
}

/// Dragging away from the points moves the probe along.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn drag_probe(
    mut drag: On<PointerDrag>,
    mut axes: Query<&mut BlendAxis>,
    layouts: Query<Layout>,
    ui_scale: Res<UiScale>,
    graph: GraphQuery,
    view_nodes: Query<&ViewNode>,
    views: Query<&FlowGraphView>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let plot = drag.entity;
    let Ok(mut axis) = axes.get_mut(plot) else {
        return;
    };
    drag.propagate(false);
    let (Some(range), Some((canvas, id))) = (axis.range, plot_target(plot, &graph, &view_nodes))
    else {
        return;
    };
    let two_d = views
        .get(canvas)
        .ok()
        .and_then(|view| view.graph.node(&id).map(is_2d))
        .unwrap_or(false);
    let position = drag.pointer.position;
    if let Some((at, _)) = value_under(plot, position, range, two_d, &layouts, ui_scale.0) {
        axis.probe = Some(snap_point(at.clamp(range.min, range.max), range));
        axis.selected = None;
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input_focus::InputFocus;
    use bevy::scene::ScenePlugin;

    use super::*;
    use crate::blend::{BLEND_SPACE_1D, BLEND_SPACE_2D};
    use crate::fields::ValueFieldsPlugin;
    use crate::model::{FlowGraph, FlowValue, PortDef, PortKind};
    use crate::run::FlowNodeState;
    use crate::view::spawn_flow_graph_view;

    const PATH: &str = "flow/test.flowgraph.bsn";

    fn sample(title: &str, x: f32, y: f32) -> BlendSample {
        BlendSample {
            kind: format!("clips/{title}"),
            title: title.into(),
            at: Vec2::new(x, y),
        }
    }

    /// A blend space of `kind` holding `samples`.
    fn graph(kind: &str, samples: Vec<BlendSample>) -> FlowGraph {
        FlowGraph {
            nodes: vec![FlowNode {
                id: "space".into(),
                kind: kind.into(),
                title: "Locomotion".into(),
                inputs: vec![
                    PortDef::new("enter", PortKind::State),
                    PortDef::new("x", PortKind::Float),
                    PortDef::new("y", PortKind::Float),
                ],
                samples,
                ..default()
            }],
            ..default()
        }
    }

    fn app(graph: FlowGraph) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            NoodleCorePlugin,
        ))
        .init_resource::<InputFocus>()
        // The default style's, drawing the lines.
        .init_asset::<WireMaterial>()
        .add_plugins((ValueFieldsPlugin, BlendAxisPlugin));
        let root = app.world_mut().spawn(Node::default()).id();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                spawn_flow_graph_view(&mut commands, root, PATH, graph.clone())
            })
            .unwrap();
        for _ in 0..3 {
            app.update();
        }
        app
    }

    fn walk_jog_sprint() -> Vec<BlendSample> {
        vec![
            sample("Walk", 1.5, 0.0),
            sample("Jog", 3.0, 0.0),
            sample("Sprint", 6.0, 0.0),
        ]
    }

    /// Each point's index and centre.
    fn dots(app: &mut App) -> Vec<(usize, Vec2)> {
        let mut dots: Vec<(usize, Vec2)> = app
            .world_mut()
            .query::<(&SampleDot, &Node)>()
            .iter(app.world())
            .map(|(SampleDot(i), node)| {
                let (Val::Px(left), Val::Px(top)) = (node.left, node.top) else {
                    panic!("placed in pixels");
                };
                (*i, Vec2::new(left, top) + DOT / 2.0)
            })
            .collect();
        dots.sort_by_key(|(i, _)| *i);
        dots
    }

    fn label(app: &mut App) -> (String, Display) {
        let (text, node) = app
            .world_mut()
            .query_filtered::<(&Text, &Node), With<AxisLabel>>()
            .single(app.world())
            .unwrap();
        (text.0.clone(), node.display)
    }

    fn axis(app: &mut App) -> Mut<'_, BlendAxis> {
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<BlendAxis>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<BlendAxis>(entity).unwrap()
    }

    #[test]
    fn values_and_plot_pixels_map_both_ways() {
        let range = Rect::new(0.0, -1.0, 6.0, 1.0);
        for two_d in [false, true] {
            let size = plot_size(two_d);
            let at = Vec2::new(1.5, if two_d { 0.5 } else { 0.0 });
            let back = from_plot(on_plot(at, range, size, two_d), range, size, two_d);
            assert!(back.distance(at) < 1e-4, "{back}");
        }
        assert_eq!(
            on_plot(Vec2::ZERO, range, SIZE_1D, false),
            Vec2::new(PAD, LINE_Y)
        );
        assert!((snap(1.43, 6.0) - 1.45).abs() < 1e-5);
        assert!((snap(42.0, 100.0) - 40.0).abs() < 1e-5);
    }

    #[test]
    fn a_1d_blend_space_shows_its_samples_on_its_axis() {
        let mut app = app(graph(BLEND_SPACE_1D, walk_jog_sprint()));
        let inner = SIZE_1D.x - 2.0 * PAD;
        // From 0 to the sprint's 6.
        let expected = [1.5, 3.0, 6.0].map(|x| Vec2::new(PAD + x / 6.0 * inner, LINE_Y));
        let placed = dots(&mut app);
        assert_eq!(placed.len(), 3);
        for ((_, at), expected) in placed.iter().zip(expected) {
            assert!(at.distance(expected) < 1e-3, "{at} vs {expected}");
        }
        // One line: the axis.
        let lines = app
            .world_mut()
            .query::<&AxisStroke>()
            .iter(app.world())
            .count();
        assert_eq!(lines, 1);
        let mut ticks: Vec<String> = app
            .world_mut()
            .query_filtered::<&Text, With<TickLabel>>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect();
        ticks.sort();
        assert_eq!(ticks, ["0", "6"]);
        assert_eq!(label(&mut app).1, Display::None);
    }

    #[test]
    fn a_probe_says_how_the_samples_mix_there() {
        let mut app = app(graph(BLEND_SPACE_1D, walk_jog_sprint()));
        axis(&mut app).probe = Some(Vec2::new(2.0, 0.0));
        app.update();
        let (text, display) = label(&mut app);
        assert_eq!(text, "2: Walk 67%, Jog 33%");
        assert_eq!(display, Display::Flex);
        // Hovering a point names it instead.
        axis(&mut app).hovered = Some(2);
        app.update();
        assert_eq!(label(&mut app).0, "Sprint  6");
    }

    #[test]
    fn a_2d_blend_space_draws_its_samples_triangles() {
        let square = vec![
            sample("Back", 0.0, -1.0),
            sample("Right", 1.0, 0.0),
            sample("Forward", 0.0, 1.0),
            sample("Left", -1.0, 0.0),
        ];
        let mut app = app(graph(BLEND_SPACE_2D, square));
        assert_eq!(dots(&mut app).len(), 4);
        // Two triangles: their four sides and the diagonal.
        let lines = app
            .world_mut()
            .query::<&AxisStroke>()
            .iter(app.world())
            .count();
        assert_eq!(lines, 5);
    }

    #[test]
    fn the_playing_input_is_marked_and_removed_samples_go() {
        let mut app = app(graph(BLEND_SPACE_1D, walk_jog_sprint()));
        app.world_mut().spawn(FlowGraphPlayback {
            graph: PATH.into(),
            nodes: vec![FlowNodeState {
                id: "space".into(),
                inputs: vec![
                    FlowValue::State(true),
                    FlowValue::Float(3.0),
                    FlowValue::Float(0.0),
                ],
                // Current: lit by its player.
                outputs: vec![FlowValue::State(true), FlowValue::Float(0.0)],
            }],
        });
        app.update();
        let mark = app
            .world_mut()
            .query_filtered::<&Node, With<InputMark>>()
            .single(app.world())
            .unwrap()
            .clone();
        assert_eq!(mark.display, Display::Flex);
        let jog = dots(&mut app)[1].1;
        assert_eq!(mark.left, px(jog.x - MARK / 2.0));

        let mut views = app.world_mut().query::<&mut FlowGraphView>();
        let mut view = views.single_mut(app.world_mut()).unwrap();
        view.graph.nodes[0].samples.remove(0);
        app.update();
        app.update();
        assert_eq!(dots(&mut app).len(), 2);
    }

    /// What the node's preview slot is asked to show.
    fn previewed(app: &mut App) -> Option<Vec2> {
        app.world_mut()
            .query_filtered::<Option<&BlendPreview>, With<BlendPreviewSlot>>()
            .single(app.world())
            .unwrap()
            .map(|preview| preview.at)
    }

    fn select(app: &mut App, selected: bool) {
        let node = app
            .world_mut()
            .query_filtered::<Entity, With<ViewNode>>()
            .single(app.world())
            .unwrap();
        match selected {
            true => _ = app.world_mut().entity_mut(node).insert(Selected),
            false => _ = app.world_mut().entity_mut(node).remove::<Selected>(),
        }
    }

    #[test]
    fn a_selected_node_with_a_probe_previews_its_blend() {
        let mut app = app(graph(BLEND_SPACE_1D, walk_jog_sprint()));
        // Neither selected nor probed: nothing.
        assert_eq!(previewed(&mut app), None);
        axis(&mut app).probe = Some(Vec2::new(2.0, 0.0));
        app.update();
        assert_eq!(previewed(&mut app), None);
        select(&mut app, true);
        app.update();
        assert_eq!(previewed(&mut app), Some(Vec2::new(2.0, 0.0)));
        // The pointer over the axis, then over a point.
        axis(&mut app).pointer = Some(Vec2::new(4.5, 0.0));
        app.update();
        assert_eq!(previewed(&mut app), Some(Vec2::new(4.5, 0.0)));
        axis(&mut app).hovered = Some(2);
        app.update();
        assert_eq!(previewed(&mut app), Some(Vec2::new(6.0, 0.0)));
        // A sample picked, the pointer away.
        {
            let mut axis = axis(&mut app);
            (axis.hovered, axis.pointer, axis.probe) = (None, None, None);
            axis.selected = Some(0);
        }
        app.update();
        assert_eq!(previewed(&mut app), Some(Vec2::new(1.5, 0.0)));
        select(&mut app, false);
        app.update();
        assert_eq!(previewed(&mut app), None);
    }
}
