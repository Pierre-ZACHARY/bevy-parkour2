//! Zip ties, like comfyui-zip-ties: hold Z and drag across wires to bundle
//! them through one point, side by side; Z-drag across a tie (or
//! double-click it) to cut it. Drag a tie to move it, Shift-drag to turn it;
//! click selects it, Ctrl-click adds it to the selection (moved and deleted
//! together), right-click opens a menu (turn, colour, remove). A tie goes
//! with its nodes when they all move together.
//! Ties are part of the graph ([`FlowGraph::ties`](crate::FlowGraph)); the
//! wires go through them by [`crate::wires`]'s routes.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};

use bevy::curve::cubic_splines::CubicSegment;
use bevy::input_focus::InputFocus;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::Pickable;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::ui_transform::UiGlobalTransform;
use bevy::window::SystemCursorIcon;
use bevy_noodle::prelude::*;

use crate::fields::typing;
use crate::library::local_point;
use crate::model::{FlowLink, FlowTie};
use crate::view::{FlowGraphView, ViewNode, canvases_under_pointer, link_of};

/// Between the strands of a tie, in graph units.
const SPACING: f32 = 8.0;
/// Past the outer strands, on each side.
const PAD: f32 = 6.0;
/// How thick a tie is drawn, along its wires.
const BAND: f32 = 8.0;

pub(crate) struct ZipTiesPlugin;

impl Plugin for ZipTiesPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(press_tie)
            .add_observer(press_menus)
            .add_observer(grab_tie)
            .add_observer(drag_tie)
            .add_observer(drop_tie)
            .add_observer(click_tie)
            .add_observer(click_canvas)
            .add_observer(click_menu_item)
            .add_observer(start_cut)
            .add_observer(drag_cut)
            .add_observer(end_cut)
            .add_systems(
                Update,
                (
                    zip_mode,
                    delete_selected_ties,
                    follow_moved_nodes,
                    draw_ties,
                    draw_cuts,
                )
                    .chain(),
            );
    }
}

/// Where each of `tie`'s links passes: `(link, point, way through)`.
pub fn tie_strands(tie: &FlowTie) -> Vec<(FlowLink, Vec2, Vec2)> {
    let way = tie.direction();
    let middle = (tie.links.len() as f32 - 1.0) / 2.0;
    tie.links
        .iter()
        .enumerate()
        .map(|(i, link)| {
            let offset = way.perp() * (i as f32 - middle) * SPACING;
            (link.clone(), tie.position + offset, way)
        })
        .collect()
}

/// How long `tie` is across its wires.
fn span(tie: &FlowTie) -> f32 {
    tie.links.len().saturating_sub(1) as f32 * SPACING + 2.0 * PAD
}

/// Where segments `a`–`b` and `c`–`d` cross, if they do.
fn crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<Vec2> {
    let (r, s) = (b - a, d - c);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-6 {
        return None;
    }
    let t = (c - a).perp_dot(s) / denom;
    let u = (c - a).perp_dot(r) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| a + r * t)
}

/// Where a wire drawn as `curves` first crosses `a`–`b`, and which way it
/// runs there.
fn wire_crossing(a: Vec2, b: Vec2, curves: &[[Vec2; 4]]) -> Option<(Vec2, Vec2)> {
    curves.iter().find_map(|&curve| {
        let points: Vec<Vec2> = CubicSegment::new_bezier(curve).iter_positions(24).collect();
        points.windows(2).find_map(|leg| {
            let point = crossing(a, b, leg[0], leg[1])?;
            Some((point, (leg[1] - leg[0]).normalize_or_zero()))
        })
    })
}

/// A Z-cut from `a` to `b` across `wires` (each link with its drawn curves):
/// cuts the ties it crosses, or else ties the wires it crosses together
/// (out of the ties they were in). Whether `ties` changed.
pub fn zip(
    ties: &mut Vec<FlowTie>,
    a: Vec2,
    b: Vec2,
    wires: &[(FlowLink, Vec<[Vec2; 4]>)],
) -> bool {
    let before = ties.len();
    ties.retain(|tie| {
        let half = tie.direction().perp() * span(tie) / 2.0;
        crossing(a, b, tie.position - half, tie.position + half).is_none()
    });
    if ties.len() != before {
        return true;
    }
    let mut crossed: Vec<(FlowLink, Vec2, Vec2)> = wires
        .iter()
        .filter_map(|(link, curves)| {
            let (point, way) = wire_crossing(a, b, curves)?;
            Some((link.clone(), point, way))
        })
        .collect();
    let Some(first) = crossed.first() else {
        return false;
    };
    let position = crossed.iter().map(|c| c.1).sum::<Vec2>() / crossed.len() as f32;
    let way = crossed
        .iter()
        .map(|c| c.2)
        .sum::<Vec2>()
        .try_normalize()
        .unwrap_or(first.2);
    // Across the tie in the order the wires cross it.
    crossed.sort_by(|x, y| {
        let across = |c: &(FlowLink, Vec2, Vec2)| (c.1 - position).dot(way.perp());
        across(x).total_cmp(&across(y))
    });
    let links: Vec<FlowLink> = crossed.into_iter().map(|c| c.0).collect();
    for tie in ties.iter_mut() {
        tie.links.retain(|link| !links.contains(link));
    }
    ties.retain(|tie| !tie.links.is_empty());
    ties.push(FlowTie {
        position,
        angle: way.to_angle(),
        links,
        color: None,
    });
    true
}

/// Colours the tie menu offers, besides the default.
pub const TIE_COLORS: [Color; 6] = [
    Color::srgb(0.85, 0.3, 0.3),
    Color::srgb(0.95, 0.55, 0.25),
    Color::srgb(0.95, 0.8, 0.3),
    Color::srgb(0.45, 0.8, 0.35),
    Color::srgb(0.35, 0.6, 0.95),
    Color::srgb(0.7, 0.45, 0.9),
];
const TIE_FILL: Color = Color::srgb(0.094, 0.098, 0.11);
const TIE_BORDER: Color = Color::srgb(0.8, 0.816, 0.84);
/// bevy_noodle's selected border.
const TIE_SELECTED: Color = Color::srgb(0.98, 0.8, 0.36);

/// What the tie menu does to the ties it was opened on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TieAction {
    /// Turn by this angle, in radians (clockwise on screen).
    Turn(f32),
    /// Draw in this colour; `None` for the default.
    Color(Option<Color>),
    Remove,
}

/// Whether `tie` is among `targets`, ties named by their links (a link is
/// in one tie at most).
fn is_target(tie: &FlowTie, targets: &[FlowLink]) -> bool {
    tie.links.iter().any(|link| targets.contains(link))
}

/// Applies `action` to the ties among `targets`. Whether `ties` changed.
pub fn act_on_ties(ties: &mut Vec<FlowTie>, targets: &[FlowLink], action: TieAction) -> bool {
    let before = ties.clone();
    match action {
        TieAction::Turn(angle) => {
            for tie in ties.iter_mut().filter(|tie| is_target(tie, targets)) {
                tie.angle = Vec2::from_angle(tie.angle + angle).to_angle();
            }
        }
        TieAction::Color(color) => {
            for tie in ties.iter_mut().filter(|tie| is_target(tie, targets)) {
                tie.color = color;
            }
        }
        TieAction::Remove => ties.retain(|tie| !is_target(tie, targets)),
    }
    *ties != before
}

/// Moves by `delta` the ties whose links all run between `moved` nodes
/// (ids): a tie goes with its wires when they move as one. Whether any did.
pub fn follow_nodes(ties: &mut [FlowTie], moved: &[&str], delta: Vec2) -> bool {
    let mut followed = false;
    for tie in ties {
        let carried = tie
            .links
            .iter()
            .all(|link| moved.contains(&link.from.as_str()) && moved.contains(&link.to.as_str()));
        if carried && !tie.links.is_empty() {
            tie.position += delta;
            followed = true;
        }
    }
    followed
}

/// On a graph canvas: its selected ties, named by their links.
#[derive(Component, Default, Debug)]
pub(crate) struct TieSelection(Vec<FlowLink>);

impl TieSelection {
    fn contains(&self, tie: &FlowTie) -> bool {
        is_target(tie, &self.0)
    }

    fn toggle(&mut self, tie: &FlowTie) {
        if self.contains(tie) {
            self.0.retain(|link| !tie.links.contains(link));
        } else {
            self.0.extend(tie.links.iter().cloned());
        }
    }

    /// The ties an action on `tie` is for: the selection if it holds
    /// `tie`, else `tie` alone.
    fn targets(&self, tie: &FlowTie) -> Vec<FlowLink> {
        match self.contains(tie) {
            true => self.0.clone(),
            false => tie.links.clone(),
        }
    }
}

/// The UI node drawing tie `index` of `canvas`'s graph.
#[derive(Component, Clone, Copy)]
struct TieVisual {
    canvas: Entity,
    index: usize,
}

/// On a tie being moved: from the pointer to the tie, in graph units.
#[derive(Component)]
struct TieGrab(Vec2);

/// A tie's right-click menu, for these ties.
#[derive(Component)]
struct TieMenu {
    canvas: Entity,
    targets: Vec<FlowLink>,
}

/// An entry of a [`TieMenu`].
#[derive(Component, Clone, Copy)]
struct TieMenuItem(TieAction);

/// On a canvas while Z is held: the buttons it gave up to cut wires.
#[derive(Component)]
struct ZipMode {
    select: Option<PointerButton>,
    drag: Option<PointerButton>,
}

/// On a canvas being Z-cut: from where to where, in graph units.
#[derive(Component)]
struct Cut {
    from: Vec2,
    to: Vec2,
}

/// The line drawing a [`Cut`].
#[derive(Component)]
struct CutLine(Entity);

fn ctrl_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ])
}

/// Holding Z (without Ctrl, which is undo) turns pointer drags on graph
/// canvases into cuts.
fn zip_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut canvases: Query<(Entity, &mut CanvasInteraction, Option<&ZipMode>), With<FlowGraphView>>,
    mut commands: Commands,
) {
    let held = keys.pressed(KeyCode::KeyZ) && !ctrl_held(&keys);
    for (canvas, mut interaction, mode) in &mut canvases {
        match (held, mode) {
            (true, None) => {
                commands.entity(canvas).insert(ZipMode {
                    select: interaction.select_button.take(),
                    drag: interaction.drag_button.take(),
                });
            }
            (false, Some(mode)) => {
                interaction.select_button = mode.select;
                interaction.drag_button = mode.drag;
                commands.entity(canvas).remove::<(ZipMode, Cut)>();
            }
            _ => {}
        }
    }
}

type Layouts<'w, 's> = Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform)>;

/// Window position → `canvas`-local logical pixels.
fn canvas_point(
    canvas: Entity,
    position: Vec2,
    layouts: &Layouts,
    ui_scale: &UiScale,
) -> Option<Vec2> {
    let (computed, transform) = layouts.get(canvas).ok()?;
    Some(local_point(computed, transform, ui_scale.0, position)?.0)
}

/// Window position → graph space on `canvas`.
fn graph_point(
    canvas: Entity,
    position: Vec2,
    (layouts, views): &(Layouts, Query<&CanvasView>),
    ui_scale: &UiScale,
) -> Option<Vec2> {
    let local = canvas_point(canvas, position, layouts, ui_scale)?;
    Some(views.get(canvas).ok()?.canvas_to_graph(local))
}

/// The canvas a Z-drag on `target` cuts: the canvas itself, or a wire's
/// (wires have no parent, so their events never reach it).
fn cut_canvas(
    target: Entity,
    graph: &GraphQuery,
    zipping: &Query<(), With<ZipMode>>,
) -> Option<Entity> {
    let canvas = match graph.edge_ports(target) {
        Some(_) => graph.canvas_of(target)?,
        None => target,
    };
    zipping.contains(canvas).then_some(canvas)
}

fn start_cut(
    mut start: On<PointerDragStart>,
    graph: GraphQuery,
    zipping: Query<(), With<ZipMode>>,
    layouts: (Layouts, Query<&CanvasView>),
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let Some(canvas) = cut_canvas(start.entity, &graph, &zipping) else {
        return;
    };
    start.propagate(false);
    if let Some(from) = graph_point(canvas, start.pointer.position, &layouts, &ui_scale) {
        commands.entity(canvas).insert(Cut { from, to: from });
    }
}

fn drag_cut(
    mut drag: On<PointerDrag>,
    graph: GraphQuery,
    zipping: Query<(), With<ZipMode>>,
    layouts: (Layouts, Query<&CanvasView>),
    ui_scale: Res<UiScale>,
    mut cuts: Query<&mut Cut>,
) {
    let Some(canvas) = cut_canvas(drag.entity, &graph, &zipping) else {
        return;
    };
    drag.propagate(false);
    if let (Ok(mut cut), Some(to)) = (
        cuts.get_mut(canvas),
        graph_point(canvas, drag.pointer.position, &layouts, &ui_scale),
    ) {
        cut.to = to;
    }
}

#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn end_cut(
    mut end: On<PointerDragEnd>,
    graph: GraphQuery,
    zipping: Query<(), With<ZipMode>>,
    cuts: Query<&Cut>,
    hitboxes: Query<&EdgeHitbox>,
    view_nodes: Query<&ViewNode>,
    mut views: Query<&mut FlowGraphView>,
    mut commands: Commands,
) {
    let Some(canvas) = cut_canvas(end.entity, &graph, &zipping) else {
        return;
    };
    end.propagate(false);
    let (Ok(cut), Ok(mut view)) = (cuts.get(canvas), views.get_mut(canvas)) else {
        return;
    };
    commands.entity(canvas).remove::<Cut>();
    let wires: Vec<(FlowLink, Vec<[Vec2; 4]>)> = graph
        .edges_in(canvas)
        .filter_map(|edge| {
            let link = link_of(edge, &view.graph, &graph, &view_nodes)?;
            Some((link, hitboxes.get(edge).ok()?.segments.clone()))
        })
        .collect();
    if zip(&mut view.graph.ties, cut.from, cut.to, &wires) {
        view.dirty = true;
    }
}

/// Presses on a tie are the tie's: no selecting under it.
fn press_tie(mut press: On<PointerPress>, ties: Query<(), With<TieVisual>>) {
    if ties.contains(press.entity) {
        press.propagate(false);
    }
}

/// A press in a tie menu is the menu's; anywhere else closes the menus.
fn press_menus(
    mut press: On<PointerPress>,
    menus: Query<Entity, With<TieMenu>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let target = press.event_target();
    if target != press.original_event_target() || menus.is_empty() {
        return;
    }
    let inside = std::iter::once(target)
        .chain(parents.iter_ancestors(target))
        .any(|entity| menus.contains(entity));
    if inside {
        press.propagate(false);
    } else {
        menus
            .iter()
            .for_each(|menu| commands.entity(menu).despawn());
    }
}

fn grab_tie(
    mut start: On<PointerDragStart>,
    ties: Query<&TieVisual>,
    graphs: Query<&FlowGraphView>,
    layouts: (Layouts, Query<&CanvasView>),
    ui_scale: Res<UiScale>,
    mut commands: Commands,
) {
    let Ok(&TieVisual { canvas, index }) = ties.get(start.entity) else {
        return;
    };
    start.propagate(false);
    let tie = graphs
        .get(canvas)
        .ok()
        .and_then(|v| v.graph.ties.get(index));
    let point = graph_point(canvas, start.pointer.position, &layouts, &ui_scale);
    if let (Some(tie), Some(point)) = (tie, point) {
        commands
            .entity(start.entity)
            .insert(TieGrab(tie.position - point));
    }
}

/// Drag moves a tie, with the others selected if it is; Shift-drag turns
/// it toward the pointer.
fn drag_tie(
    mut drag: On<PointerDrag>,
    ties: Query<(&TieVisual, Option<&TieGrab>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut graphs: Query<(&mut FlowGraphView, &TieSelection)>,
    layouts: (Layouts, Query<&CanvasView>),
    ui_scale: Res<UiScale>,
) {
    let Ok((&TieVisual { canvas, index }, grab)) = ties.get(drag.entity) else {
        return;
    };
    drag.propagate(false);
    if drag.button != PointerButton::Primary {
        return;
    }
    let point = graph_point(canvas, drag.pointer.position, &layouts, &ui_scale);
    let (Some(point), Ok((mut view, selection))) = (point, graphs.get_mut(canvas)) else {
        return;
    };
    let Some(tie) = view.graph.ties.get(index).cloned() else {
        return;
    };
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let Some(way) = (point - tie.position).try_normalize() else {
            return;
        };
        view.graph.ties[index].angle = way.to_angle();
    } else if let Some(grab) = grab {
        let delta = point + grab.0 - tie.position;
        let moving = selection.targets(&tie);
        for tie in view.graph.ties.iter_mut() {
            if is_target(tie, &moving) {
                tie.position += delta;
            }
        }
    }
}

/// The graph is edited once the drag ends, as for nodes: not at every step.
fn drop_tie(
    mut end: On<PointerDragEnd>,
    ties: Query<&TieVisual>,
    mut graphs: Query<&mut FlowGraphView>,
    mut commands: Commands,
) {
    let Ok(&TieVisual { canvas, .. }) = ties.get(end.entity) else {
        return;
    };
    end.propagate(false);
    commands.entity(end.entity).remove::<TieGrab>();
    if let Ok(mut view) = graphs.get_mut(canvas) {
        view.dirty = true;
    }
}

/// Click selects a tie, Ctrl-click adds it to the selection or takes it
/// out, double-click cuts it, right-click opens its menu.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn click_tie(
    mut click: On<PointerClick>,
    ties: Query<&TieVisual>,
    keys: Res<ButtonInput<KeyCode>>,
    mut graphs: Query<(&mut FlowGraphView, &mut TieSelection)>,
    layouts: Layouts,
    ui_scale: Res<UiScale>,
    menus: Query<Entity, With<TieMenu>>,
    mut commands: Commands,
) {
    let Ok(&TieVisual { canvas, index }) = ties.get(click.entity) else {
        return;
    };
    click.propagate(false);
    let Ok((mut view, mut selection)) = graphs.get_mut(canvas) else {
        return;
    };
    let Some(tie) = view.graph.ties.get(index).cloned() else {
        return;
    };
    match click.button {
        PointerButton::Primary if click.count >= 2 => {
            if act_on_ties(&mut view.graph.ties, &tie.links, TieAction::Remove) {
                view.dirty = true;
            }
            selection.0.retain(|link| !tie.links.contains(link));
        }
        PointerButton::Primary if ctrl_held(&keys) => selection.toggle(&tie),
        PointerButton::Primary => selection.0 = tie.links.clone(),
        PointerButton::Secondary => {
            menus
                .iter()
                .for_each(|menu| commands.entity(menu).despawn());
            if let Some(at) = canvas_point(canvas, click.pointer.position, &layouts, &ui_scale) {
                spawn_tie_menu(&mut commands, canvas, at, selection.targets(&tie));
            }
        }
        PointerButton::Middle => {}
    }
}

/// A click on a graph canvas (or on a node of it) deselects its ties,
/// unless Ctrl is held.
fn click_canvas(
    click: On<PointerClick>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selections: Query<&mut TieSelection>,
) {
    if click.button != PointerButton::Primary || ctrl_held(&keys) {
        return;
    }
    if let Ok(mut selection) = selections.get_mut(click.entity)
        && !selection.0.is_empty()
    {
        selection.0.clear();
    }
}

/// Delete cuts the selected ties of the canvas under the pointer.
fn delete_selected_ties(
    keys: Res<ButtonInput<KeyCode>>,
    hovers: Option<Res<HoverMap>>,
    graph: GraphQuery,
    mut graphs: Query<(&mut FlowGraphView, &mut TieSelection)>,
    focus: Option<Res<InputFocus>>,
    texts: Query<(), With<EditableText>>,
) {
    let typing = focus.is_some_and(|focus| typing(&focus, &texts));
    let Some(hovers) = hovers.filter(|_| keys.just_pressed(KeyCode::Delete) && !typing) else {
        return;
    };
    for canvas in canvases_under_pointer(&hovers, &graph, |c| graphs.contains(c)) {
        let Ok((mut view, mut selection)) = graphs.get_mut(canvas) else {
            continue;
        };
        if act_on_ties(&mut view.graph.ties, &selection.0, TieAction::Remove) {
            view.dirty = true;
        }
        selection.0.clear();
    }
}

/// Ties go with the nodes their wires join when those move together.
pub(crate) fn follow_moved_nodes(
    mut edits: MessageReader<EditApplied>,
    mut graphs: Query<&mut FlowGraphView>,
    view_nodes: Query<&ViewNode>,
) {
    for edit in edits.read() {
        let GraphChange::Moved { nodes, delta, .. } = &edit.change else {
            continue;
        };
        let Ok(mut view) = graphs.get_mut(edit.canvas) else {
            continue;
        };
        let moved: Vec<&str> = nodes
            .iter()
            .filter_map(|&node| Some(view_nodes.get(node).ok()?.0.as_str()))
            .collect();
        if follow_nodes(&mut view.graph.ties, &moved, *delta) && !edit.change.is_drag_step() {
            view.dirty = true;
        }
    }
}

/// A menu at `at` (`canvas`-local pixels) acting on the ties `targets`.
fn spawn_tie_menu(commands: &mut Commands, canvas: Entity, at: Vec2, targets: Vec<FlowLink>) {
    let menu = commands
        .spawn((
            TieMenu { canvas, targets },
            Node {
                position_type: PositionType::Absolute,
                left: px(at.x),
                top: px(at.y),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(4)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(36, 38, 43)),
            BorderColor::all(Color::srgb_u8(70, 73, 81)),
            // Over everything; a control, so wires under it are not picked.
            ZIndex(i32::MAX),
            TabIndex(-1),
            ChildOf(canvas),
        ))
        .id();
    let entry = |commands: &mut Commands, label: &str, action: TieAction| {
        commands.spawn((
            TieMenuItem(action),
            Text::new(label),
            TextFont::from_font_size(12.0),
            Node {
                padding: UiRect::axes(px(8), px(3)),
                ..default()
            },
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(menu),
        ));
    };
    // The default font has no degree sign.
    entry(commands, "Rotate 90 deg", TieAction::Turn(FRAC_PI_2));
    entry(commands, "Rotate 45 deg", TieAction::Turn(FRAC_PI_4));
    let swatches = commands
        .spawn((
            Node {
                column_gap: px(4),
                padding: UiRect::axes(px(8), px(3)),
                ..default()
            },
            ChildOf(menu),
        ))
        .id();
    for color in std::iter::once(None).chain(TIE_COLORS.map(Some)) {
        commands.spawn((
            TieMenuItem(TieAction::Color(color)),
            Node {
                width: px(14),
                height: px(14),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(color.unwrap_or(TIE_FILL)),
            BorderColor::all(TIE_BORDER),
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(swatches),
        ));
    }
    entry(commands, "Remove", TieAction::Remove);
}

fn click_menu_item(
    mut click: On<PointerClick>,
    items: Query<&TieMenuItem>,
    parents: Query<&ChildOf>,
    menus: Query<&TieMenu>,
    mut graphs: Query<(&mut FlowGraphView, &mut TieSelection)>,
    mut commands: Commands,
) {
    let Ok(&TieMenuItem(action)) = items.get(click.entity) else {
        return;
    };
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    let Some((menu, open)) = parents
        .iter_ancestors(click.entity)
        .find_map(|entity| Some((entity, menus.get(entity).ok()?)))
    else {
        return;
    };
    if let Ok((mut view, mut selection)) = graphs.get_mut(open.canvas) {
        if act_on_ties(&mut view.graph.ties, &open.targets, action) {
            view.dirty = true;
        }
        if action == TieAction::Remove {
            selection.0.retain(|link| !open.targets.contains(link));
        }
    }
    commands.entity(menu).despawn();
}

/// What a tie's visual shows of it, for comparing before writing.
type Shown<'a> = (
    Entity,
    &'a TieVisual,
    &'a mut Node,
    &'a mut UiTransform,
    &'a mut BackgroundColor,
    &'a mut BorderColor,
);

/// One UI node per tie, in the canvas content, across its strands.
fn draw_ties(
    mut commands: Commands,
    graph: GraphQuery,
    graphs: Query<(Entity, &FlowGraphView, Option<&TieSelection>)>,
    mut visuals: Query<Shown>,
) {
    for (canvas, view, selection) in &graphs {
        let Some(content) = graph.content_of(canvas) else {
            continue;
        };
        let ties = &view.graph.ties;
        let mut drawn = vec![None; ties.len()];
        for (entity, visual, ..) in &visuals {
            if visual.canvas != canvas {
                continue;
            }
            match drawn.get_mut(visual.index) {
                Some(slot) => *slot = Some(entity),
                None => commands.entity(entity).despawn(),
            }
        }
        for (index, (tie, visual)) in ties.iter().zip(drawn).enumerate() {
            let size = Vec2::new(BAND, span(tie));
            let corner = tie.position - size / 2.0;
            let placed = Node {
                position_type: PositionType::Absolute,
                left: px(corner.x),
                top: px(corner.y),
                width: px(size.x),
                height: px(size.y),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            };
            let turned = UiTransform {
                rotation: Rot2::radians(tie.angle),
                ..default()
            };
            let fill = BackgroundColor(tie.color.unwrap_or(TIE_FILL));
            let selected = selection.is_some_and(|s| s.contains(tie));
            let border = BorderColor::all(if selected { TIE_SELECTED } else { TIE_BORDER });
            let Some((_, _, mut node, mut transform, mut background, mut outline)) =
                visual.and_then(|v| visuals.get_mut(v).ok())
            else {
                commands.spawn((
                    TieVisual { canvas, index },
                    placed,
                    turned,
                    fill,
                    border,
                    // Above the wires; a control, so wires under it are not picked.
                    ZIndex(i32::MAX),
                    TabIndex(-1),
                    EntityCursor::System(SystemCursorIcon::Move),
                    ChildOf(content),
                ));
                continue;
            };
            if *node != placed {
                *node = placed;
            }
            transform.set_if_neq(turned);
            background.set_if_neq(fill);
            outline.set_if_neq(border);
        }
    }
}

/// A line along each cut in progress.
fn draw_cuts(
    mut commands: Commands,
    graph: GraphQuery,
    cuts: Query<(Entity, &Cut, Option<&CutLine>)>,
    lines: Query<(Entity, &ChildOf), With<CutLineOf>>,
    mut placed: Query<(&mut Node, &mut UiTransform)>,
) {
    for (line, child_of) in &lines {
        let canvas = graph.canvas_of(child_of.parent());
        if !canvas.is_some_and(|c| cuts.contains(c)) {
            commands.entity(line).despawn();
        }
    }
    for (canvas, cut, line) in &cuts {
        let Some(content) = graph.content_of(canvas) else {
            continue;
        };
        let length = cut.from.distance(cut.to);
        let middle = cut.from.midpoint(cut.to);
        let node = Node {
            position_type: PositionType::Absolute,
            left: px(middle.x - length / 2.0),
            top: px(middle.y - 1.0),
            width: px(length),
            height: px(2),
            ..default()
        };
        let transform = UiTransform {
            rotation: Rot2::radians((cut.to - cut.from).to_angle()),
            ..default()
        };
        match line.and_then(|l| placed.get_mut(l.0).ok()) {
            Some((mut current, mut turned)) => {
                *current = node;
                turned.set_if_neq(transform);
            }
            None => {
                let line = commands
                    .spawn((
                        CutLineOf,
                        node,
                        transform,
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
                        ZIndex(i32::MAX),
                        Pickable::IGNORE,
                        ChildOf(content),
                    ))
                    .id();
                commands.entity(canvas).insert(CutLine(line));
            }
        }
    }
}

/// Marks the line drawing a cut.
#[derive(Component)]
struct CutLineOf;

#[cfg(test)]
mod tests {
    use super::*;

    fn link(name: &str) -> FlowLink {
        FlowLink::new(name, "out", "sink", name)
    }

    /// A straight wire left to right at height `y`.
    fn wire(name: &str, y: f32) -> (FlowLink, Vec<[Vec2; 4]>) {
        let (a, b) = (Vec2::new(0.0, y), Vec2::new(300.0, y));
        (
            link(name),
            vec![[a, a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b]],
        )
    }

    #[test]
    fn a_cut_across_wires_ties_them_in_order() {
        let wires = [wire("low", 60.0), wire("high", 0.0), wire("away", 500.0)];
        let mut ties = vec![];
        assert!(zip(
            &mut ties,
            Vec2::new(150.0, -20.0),
            Vec2::new(150.0, 80.0),
            &wires
        ));
        assert_eq!(ties.len(), 1);
        let tie = &ties[0];
        assert_eq!(tie.links, [link("high"), link("low")]);
        assert!(tie.position.distance(Vec2::new(150.0, 30.0)) < 1e-3);
        assert!(tie.direction().distance(Vec2::X) < 1e-3);
        // The strands keep the order, side by side through the tie.
        let strands = tie_strands(tie);
        assert!(strands[0].1.y < strands[1].1.y);
        assert_eq!(strands[1].1.y - strands[0].1.y, SPACING);
    }

    #[test]
    fn a_cut_across_a_tie_cuts_it_and_retying_moves_links() {
        let wires = [wire("a", 0.0), wire("b", 40.0)];
        let mut ties = vec![];
        zip(
            &mut ties,
            Vec2::new(100.0, -10.0),
            Vec2::new(100.0, 50.0),
            &wires,
        );
        // Tying "b" again elsewhere takes it out of the first tie.
        zip(
            &mut ties,
            Vec2::new(250.0, 30.0),
            Vec2::new(250.0, 50.0),
            &wires,
        );
        assert_eq!(ties.len(), 2);
        assert_eq!(ties[0].links, [link("a")]);
        assert_eq!(ties[1].links, [link("b")]);
        // Across the first tie (along the wires, through its band): cut.
        let at = ties[0].position;
        assert!(zip(
            &mut ties,
            at - Vec2::new(10.0, 0.0),
            at + Vec2::new(10.0, 0.0),
            &wires
        ));
        assert_eq!(ties.len(), 1);
        assert_eq!(ties[0].links, [link("b")]);
        // Across nothing: no change.
        let before = ties.clone();
        assert!(!zip(
            &mut ties,
            Vec2::new(0.0, 900.0),
            Vec2::new(10.0, 900.0),
            &wires
        ));
        assert_eq!(ties, before);
    }

    fn tie(links: &[&str], angle: f32) -> FlowTie {
        FlowTie {
            position: Vec2::ZERO,
            angle,
            links: links.iter().map(|name| link(name)).collect(),
            color: None,
        }
    }

    #[test]
    fn the_menu_acts_on_its_ties_only() {
        let mut ties = vec![tie(&["a", "b"], 0.0), tie(&["c"], 3.0)];
        let first = ties[0].links[..1].to_vec();
        assert!(act_on_ties(&mut ties, &first, TieAction::Turn(FRAC_PI_2)));
        assert!((ties[0].angle - FRAC_PI_2).abs() < 1e-5);
        assert_eq!(ties[1].angle, 3.0);
        // Turning past a half turn wraps.
        act_on_ties(&mut ties, &[link("c")], TieAction::Turn(FRAC_PI_4));
        assert!((ties[1].angle - (3.0 + FRAC_PI_4 - std::f32::consts::TAU)).abs() < 1e-5);

        let blue = Some(TIE_COLORS[4]);
        assert!(act_on_ties(&mut ties, &[link("c")], TieAction::Color(blue)));
        assert_eq!((ties[0].color, ties[1].color), (None, blue));
        assert!(!act_on_ties(
            &mut ties,
            &[link("c")],
            TieAction::Color(blue)
        ));

        assert!(act_on_ties(&mut ties, &first, TieAction::Remove));
        assert_eq!(ties.len(), 1);
        assert!(!act_on_ties(&mut ties, &[link("away")], TieAction::Remove));
    }

    #[test]
    fn ties_follow_nodes_moved_together() {
        // Links run from their name to "sink".
        let mut ties = vec![tie(&["a", "b"], 0.0), tie(&["c"], 0.0)];
        let delta = Vec2::new(30.0, -10.0);
        assert!(!follow_nodes(&mut ties, &["a", "b"], delta), "sink stayed");
        assert!(follow_nodes(&mut ties, &["a", "b", "sink"], delta));
        assert_eq!(ties[0].position, delta);
        assert_eq!(ties[1].position, Vec2::ZERO, "c stayed");
    }

    #[test]
    fn selection_names_ties_by_their_links() {
        let ties = [tie(&["a", "b"], 0.0), tie(&["c"], 0.0)];
        let mut selection = TieSelection::default();
        assert_eq!(selection.targets(&ties[0]), ties[0].links);
        selection.toggle(&ties[0]);
        selection.toggle(&ties[1]);
        assert!(selection.contains(&ties[0]) && selection.contains(&ties[1]));
        assert_eq!(selection.targets(&ties[1]).len(), 3);
        selection.toggle(&ties[0]);
        assert!(!selection.contains(&ties[0]));
        assert_eq!(selection.targets(&ties[0]), ties[0].links);
    }
}
