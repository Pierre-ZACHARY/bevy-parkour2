//! A node library beside a canvas: the catalog's nodes by category, dragged
//! onto the canvas to add them. Its `<` button folds it to a strip, and a
//! category's title folds the category.

use bevy::picking::Pickable;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use bevy::ui::ui_transform::UiGlobalTransform;
use bevy::window::SystemCursorIcon;
use bevy_noodle::prelude::*;

use crate::axis::AxisDrops;
use crate::model::{FlowNode, FlowNodeCatalog};
use crate::view::{add_flow_node, header_color};

const OPEN_WIDTH: f32 = 210.0;
const FOLDED_WIDTH: f32 = 30.0;
const PANEL: Color = Color::srgb(0.118, 0.122, 0.137);
const ITEM: Color = Color::srgb(0.173, 0.18, 0.204);
const ITEM_HOVERED: Color = Color::srgb(0.235, 0.247, 0.278);

pub(crate) struct NodeLibraryPlugin;

impl Plugin for NodeLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(start_drag)
            .add_observer(move_ghost)
            .add_observer(drop_item)
            .add_observer(fold_library)
            .add_observer(fold_category)
            .add_observer(scroll_list)
            .add_systems(Update, highlight_items);
    }
}

/// A library adding nodes to `canvas`.
#[derive(Component, Debug)]
pub struct NodeLibrary {
    pub canvas: Entity,
    pub open: bool,
}

/// One entry of a library: a node like `template` is added where it is
/// dropped.
#[derive(Component, Clone, Debug)]
pub struct LibraryItem {
    pub library: Entity,
    pub template: FlowNode,
}

/// What a folded library hides.
#[derive(Component)]
struct LibraryBody;

/// The library's fold button.
#[derive(Component)]
struct FoldButton(Entity);

/// A category's title, folding its items.
#[derive(Component)]
struct CategoryTitle(Entity);

/// The scrolling list of categories.
#[derive(Component)]
struct LibraryList;

/// Follows the pointer while an item is dragged.
#[derive(Component)]
struct DragGhost;

/// How a library names a kind prefix.
pub fn category_title(category: &str) -> String {
    match category {
        "" => "Other".to_string(),
        "float" => "Float".to_string(),
        "vec2" => "Vector 2".to_string(),
        "vec3" => "Vector 3".to_string(),
        other => {
            let other = other.replace('_', " ");
            let mut chars = other.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        }
    }
}

/// Spawn a library for `canvas` under `parent`, listing `catalog`. Put it
/// before the canvas's own container in a row for it to sit on its left.
pub fn spawn_node_library(
    commands: &mut Commands,
    parent: Entity,
    canvas: Entity,
    catalog: &FlowNodeCatalog,
) -> Entity {
    let library = commands
        .spawn((
            NodeLibrary { canvas, open: true },
            Node {
                flex_direction: FlexDirection::Column,
                width: px(OPEN_WIDTH),
                height: percent(100),
                flex_shrink: 0.0,
                border: UiRect::right(px(1)),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(Color::srgb_u8(55, 57, 64)),
            ChildOf(parent),
        ))
        .id();
    let header = commands
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(6), px(5)),
                ..default()
            },
            ChildOf(library),
        ))
        .id();
    commands.spawn((
        LibraryBody,
        Text::new("Nodes"),
        TextFont::from_font_size(13.0),
        ChildOf(header),
    ));
    commands.spawn((
        FoldButton(library),
        Text::new("<"),
        TextFont::from_font_size(13.0),
        Node {
            padding: UiRect::axes(px(6), px(1)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BackgroundColor(ITEM),
        EntityCursor::System(SystemCursorIcon::Pointer),
        ChildOf(header),
    ));
    let list = commands
        .spawn((
            LibraryBody,
            LibraryList,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_height: px(0),
                overflow: Overflow::scroll_y(),
                padding: UiRect::new(px(6), px(6), px(0), px(6)),
                row_gap: px(2),
                ..default()
            },
            ScrollPosition::default(),
            ChildOf(library),
        ))
        .id();

    let mut nodes: Vec<&FlowNode> = catalog.nodes.iter().collect();
    nodes.sort_by_cached_key(|node| (category_title(node.category()), node.title.clone()));
    let mut current: Option<(String, Entity)> = None;
    for node in nodes {
        let title = category_title(node.category());
        let items = match &current {
            Some((open, items)) if *open == title => *items,
            _ => {
                let items = spawn_category(commands, list, &title);
                current = Some((title, items));
                items
            }
        };
        let (swatch, name) = (header_color(node), node.title.clone());
        commands.spawn((
            LibraryItem {
                library,
                template: node.clone(),
            },
            Node {
                align_items: AlignItems::Center,
                column_gap: px(7),
                padding: UiRect::axes(px(7), px(4)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(ITEM),
            EntityCursor::System(SystemCursorIcon::Grab),
            ChildOf(items),
            children![
                (
                    Node {
                        width: px(10),
                        height: px(10),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(swatch),
                    Pickable::IGNORE,
                ),
                (
                    Text::new(name),
                    TextFont::from_font_size(13.0),
                    Pickable::IGNORE
                ),
            ],
        ));
    }
    library
}

/// A category's title and the column of its items, returned.
fn spawn_category(commands: &mut Commands, list: Entity, title: &str) -> Entity {
    let section = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            },
            ChildOf(list),
        ))
        .id();
    let title = commands
        .spawn((
            Text::new(title),
            TextFont::from_font_size(12.0),
            TextColor(Color::srgb_u8(150, 154, 163)),
            Node {
                margin: UiRect::top(px(8)),
                ..default()
            },
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(section),
        ))
        .id();
    let items = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            },
            ChildOf(section),
        ))
        .id();
    commands.entity(title).insert(CategoryTitle(items));
    items
}

/// Window position (logical pixels) → `node`'s local logical pixels from
/// its top-left corner, and whether it is inside the node.
pub(crate) fn local_point(
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    ui_scale: f32,
    position: Vec2,
) -> Option<(Vec2, bool)> {
    // As bevy_noodle does: the node's scale factor is the window's times
    // `UiScale`; window positions are scaled by the window's only.
    let scale = computed.inverse_scale_factor();
    let physical = position / (scale * ui_scale);
    let normalized = computed.normalize_point(*transform, physical)?;
    let inside = normalized.abs().max_element() <= 0.5;
    Some(((normalized + 0.5) * computed.size() * scale, inside))
}

type Layout<'a> = (&'a ComputedNode, &'a UiGlobalTransform);

fn start_drag(
    start: On<PointerDragStart>,
    items: Query<&LibraryItem>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Ok(item) = items.get(start.entity) else {
        return;
    };
    if start.button != PointerButton::Primary {
        return;
    }
    let Ok(parent) = parents.get(item.library) else {
        return;
    };
    commands.spawn((
        DragGhost,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(10), px(5)),
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(header_color(&item.template)),
        BorderColor::all(Color::srgb_u8(250, 204, 92)),
        // Over the canvas, its sibling.
        ZIndex(10),
        Pickable::IGNORE,
        ChildOf(parent.parent()),
        children![(
            Text::new(item.template.title.clone()),
            TextFont::from_font_size(13.0),
            Pickable::IGNORE,
        )],
    ));
}

fn move_ghost(
    drag: On<PointerDrag>,
    items: Query<&LibraryItem>,
    parents: Query<&ChildOf>,
    layouts: Query<Layout>,
    mut ghosts: Query<&mut Node, With<DragGhost>>,
    ui_scale: Res<UiScale>,
) {
    let Ok(item) = items.get(drag.entity) else {
        return;
    };
    let Some((computed, transform)) = parents
        .get(item.library)
        .ok()
        .and_then(|parent| layouts.get(parent.parent()).ok())
    else {
        return;
    };
    let position = drag.pointer.position;
    let Some((local, _)) = local_point(computed, transform, ui_scale.0, position) else {
        return;
    };
    for mut ghost in &mut ghosts {
        ghost.left = px(local.x + 8.0);
        ghost.top = px(local.y + 8.0);
    }
}

#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn drop_item(
    end: On<PointerDragEnd>,
    items: Query<&LibraryItem>,
    libraries: Query<&NodeLibrary>,
    canvases: Query<(&CanvasView, &ComputedNode, &UiGlobalTransform)>,
    ghosts: Query<Entity, With<DragGhost>>,
    ui_scale: Res<UiScale>,
    mut axes: AxisDrops,
    mut commands: Commands,
) {
    let Ok(item) = items.get(end.entity) else {
        return;
    };
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let Ok(library) = libraries.get(item.library) else {
        return;
    };
    let Ok((view, computed, transform)) = canvases.get(library.canvas) else {
        return;
    };
    let position = end.pointer.position;
    // On a blend space's axis: a sample playing it.
    if axes.drop(library.canvas, &item.template, position) {
        return;
    }
    if let Some((local, true)) = local_point(computed, transform, ui_scale.0, position) {
        // The node's corner a little up-left of the pointer.
        let at = view.canvas_to_graph(local) - Vec2::new(20.0, 12.0);
        add_flow_node(&mut commands, library.canvas, item.template.clone(), at);
    }
}

fn fold_library(
    click: On<PointerClick>,
    buttons: Query<&FoldButton>,
    mut libraries: Query<(&mut NodeLibrary, &mut Node)>,
    children: Query<&Children>,
    mut bodies: Query<&mut Node, (With<LibraryBody>, Without<NodeLibrary>)>,
    mut labels: Query<&mut Text>,
) {
    let Ok(&FoldButton(library)) = buttons.get(click.entity) else {
        return;
    };
    let Ok((mut state, mut node)) = libraries.get_mut(library) else {
        return;
    };
    state.open = !state.open;
    node.width = px(if state.open { OPEN_WIDTH } else { FOLDED_WIDTH });
    for entity in children.iter_descendants(library) {
        if let Ok(mut body) = bodies.get_mut(entity) {
            body.display = if state.open {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    if let Ok(mut label) = labels.get_mut(click.entity) {
        label.0 = if state.open { "<" } else { ">" }.to_string();
    }
}

fn fold_category(
    click: On<PointerClick>,
    titles: Query<&CategoryTitle>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(&CategoryTitle(items)) = titles.get(click.entity) else {
        return;
    };
    if let Ok(mut node) = nodes.get_mut(items) {
        node.display = match node.display {
            Display::None => Display::Flex,
            _ => Display::None,
        };
    }
}

fn scroll_list(
    mut scroll: On<PointerScroll>,
    mut lists: Query<&mut ScrollPosition, With<LibraryList>>,
) {
    let Ok(mut position) = lists.get_mut(scroll.entity) else {
        return;
    };
    let lines = match scroll.unit {
        bevy::input::mouse::MouseScrollUnit::Line => scroll.y * 24.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => scroll.y,
    };
    position.y = (position.y - lines).max(0.0);
    scroll.propagate(false);
}

/// Brighten the item under the pointer.
fn highlight_items(
    hovers: Res<HoverMap>,
    mut items: Query<(Entity, &mut BackgroundColor), With<LibraryItem>>,
) {
    for (item, mut background) in &mut items {
        let hovered = hovers.values().any(|hits| hits.contains_key(&item));
        let color = if hovered { ITEM_HOVERED } else { ITEM };
        if background.0 != color {
            background.0 = color;
        }
    }
}
