//! Value fields: an unwired input shows its value on the node, to edit in
//! place (`y × 2`: type 2 in the multiply's unwired `b`). A number field per
//! axis, a box to tick for a bool, a wider field for a text. Wiring the input hides them; the input
//! then reads its wire, and keeps the value for when it is unwired again.
//!
//! An easing shows its curve and its duration (see `crate::curves`). A text
//! picked from names (see [`FlowNodeType::with_choices`]) shows a dropdown:
//! a click lists the names, read from the world the view is in.
//!
//! The fields are Bevy text inputs, their text selected on focus so typing
//! replaces it. The editor's and the canvas's shortcuts stand aside while
//! one has the keyboard. Enter or a click elsewhere commits; a number that
//! does not parse puts the value back.

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::picking::Pickable;
use bevy::picking::cursor::EntityCursor;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, LineBreak, TextCursorStyle, TextLayout};
use bevy::ui::Selected;
use bevy::ui_widgets::{SelectAllOnFocus, TextInput};
use bevy::window::SystemCursorIcon;
use bevy_noodle::prelude::*;

use bevy_noodle::style::WireMaterial;

use crate::arrows::ArrowNode;
use crate::curves::spawn_easing_plot;
use crate::model::{FlowValue, PortKind};
#[cfg(doc)]
use crate::registry::FlowNodeType;
use crate::registry::FlowNodeTypes;
use crate::run::{FlowNodeMemory, FlowVariables, evaluate};
use crate::view::{FlowGraphView, ViewNode};

pub(crate) struct ValueFieldsPlugin;

impl Plugin for ValueFieldsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ViewNodeTypes(crate::builtin_types()))
            .add_observer(enter_commits)
            .add_observer(toggle_bool)
            .add_observer(toggle_choice_list)
            .add_observer(pick_choice)
            .add_observer(scroll_choice_list)
            .add_systems(
                Update,
                (
                    add_value_fields,
                    commit_fields,
                    sync_value_fields,
                    close_choice_lists,
                    highlight_choices,
                )
                    .chain(),
            );
    }
}

/// The node types views know: the built-in ones, which the editor's views
/// know too (it runs no game). Their dropdowns list names from them, and a
/// node's inputs evaluate with them to tell what to list (a Bone node's,
/// from the skeleton wired into it).
#[derive(Resource)]
pub(crate) struct ViewNodeTypes(pub FlowNodeTypes);

/// On a drawn node's input row: which of the node's inputs it is.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputRow(pub usize);

/// Holds an input row's fields; hidden while the input is wired.
#[derive(Component)]
pub(crate) struct ValueFields;

/// A number field: one axis of its input's value (0 for a float).
#[derive(Component)]
pub(crate) struct NumberField {
    pub axis: usize,
}

/// A text input's field.
#[derive(Component)]
pub(crate) struct TextField;

/// A box ticked while its bool input's value is true.
#[derive(Component)]
pub(crate) struct BoolField;

/// A text input picked from names: shows its name; a click lists the names.
#[derive(Component)]
pub(crate) struct ChoiceField;

/// A choice field's name.
#[derive(Component)]
struct ChoiceLabel;

/// The names a choice field lists, open under it.
#[derive(Component)]
pub(crate) struct ChoiceList {
    pub field: Entity,
}

/// A name in a choice list; `None` clears the input.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Choice(pub Option<String>);

/// What a choice field shows, and lists first, for no name.
const NO_CHOICE: &str = "(none)";

pub(crate) const FIELD: Color = Color::srgb_u8(30, 32, 37);
pub(crate) const FIELD_BORDER: Color = Color::srgb_u8(70, 74, 84);
const TICKED: Color = Color::srgb(0.85, 0.3, 0.3);
/// The border of the field that has the keyboard.
pub(crate) const FOCUSED_BORDER: Color = Color::srgb_u8(110, 160, 230);
const TEXT: Color = Color::srgb_u8(214, 218, 224);

/// The number of number fields a port kind shows.
fn axes(kind: PortKind) -> usize {
    match kind {
        PortKind::Bool | PortKind::State | PortKind::Text | PortKind::Entity => 0,
        // Its duration.
        PortKind::Float | PortKind::Easing => 1,
        PortKind::Vec2 => 2,
        PortKind::Vec3 => 3,
    }
}

/// Axis `axis` of a number value.
pub(crate) fn axis_of(value: &FlowValue, axis: usize) -> f32 {
    match value {
        FlowValue::Float(x) => *x,
        FlowValue::Vec2(v) => v.to_array()[axis],
        FlowValue::Vec3(v) => v.to_array()[axis],
        FlowValue::Easing(easing) => easing.duration,
        FlowValue::Bool(_) | FlowValue::State(_) | FlowValue::Text(_) | FlowValue::Entity(_) => 0.0,
    }
}

/// `value` with axis `axis` set to `x`.
pub(crate) fn with_axis(value: FlowValue, axis: usize, x: f32) -> FlowValue {
    match value {
        FlowValue::Float(_) => FlowValue::Float(x),
        FlowValue::Vec2(mut v) => {
            v[axis] = x;
            FlowValue::Vec2(v)
        }
        FlowValue::Vec3(mut v) => {
            v[axis] = x;
            FlowValue::Vec3(v)
        }
        FlowValue::Easing(mut easing) => {
            easing.duration = x.max(0.0);
            FlowValue::Easing(easing)
        }
        other @ (FlowValue::Bool(_)
        | FlowValue::State(_)
        | FlowValue::Text(_)
        | FlowValue::Entity(_)) => other,
    }
}

/// A number typed in a field: `,` reads as a decimal point.
pub(crate) fn parse_number(text: &str) -> Option<f32> {
    text.trim()
        .replace(',', ".")
        .parse::<f32>()
        .ok()
        .filter(|x| x.is_finite())
}

/// How a field shows a number: `2`, `0.5`.
pub(crate) fn format_number(x: f32) -> String {
    // Rounded so float noise (0.1 + 0.2) does not show.
    let rounded = (x * 1e4).round() / 1e4;
    format!("{}", rounded + 0.0)
}

/// Where a field's edits go: its canvas, node id and input index.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub canvas: Entity,
    pub node: String,
    pub input: usize,
}

/// Finds the input a field edits: the field is two levels under its row,
/// the row a child of its node.
#[derive(SystemParam)]
pub(crate) struct Targets<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    rows: Query<'w, 's, &'static InputRow>,
    view_nodes: Query<'w, 's, &'static ViewNode>,
    graph: GraphQuery<'w, 's>,
}

impl Targets<'_, '_> {
    pub(crate) fn of(&self, field: Entity) -> Option<Target> {
        let fields = self.parents.get(field).ok()?.parent();
        self.of_fields(fields)
    }

    fn of_fields(&self, fields: Entity) -> Option<Target> {
        let node = self.node_of_fields(fields)?;
        let row = self.parents.get(fields).ok()?.parent();
        Some(Target {
            canvas: self.graph.canvas_of(node)?,
            node: self.view_nodes.get(node).ok()?.0.clone(),
            input: self.rows.get(row).ok()?.0,
        })
    }

    /// The canvas node holding a row's `fields`.
    fn node_of_fields(&self, fields: Entity) -> Option<Entity> {
        let row = self.parents.get(fields).ok()?.parent();
        Some(self.parents.get(row).ok()?.parent())
    }
}

/// Set the value of `target`'s input with `edit`, marking the graph edited.
pub(crate) fn edit_value(
    views: &mut Query<&mut FlowGraphView>,
    target: &Target,
    edit: impl FnOnce(FlowValue) -> FlowValue,
) {
    let Ok(mut view) = views.get_mut(target.canvas) else {
        return;
    };
    let Some(port) = view
        .graph
        .nodes
        .iter()
        .find(|node| node.id == target.node)
        .and_then(|node| node.inputs.get(target.input))
    else {
        return;
    };
    let value = edit(port.unwired_value());
    // At rest, the file needs no value.
    let value = value.is_active().then_some(value);
    if value == port.value {
        return;
    }
    let node = view.graph.nodes.iter_mut().find(|n| n.id == target.node);
    if let Some(port) = node.and_then(|node| node.inputs.get_mut(target.input)) {
        port.value = value;
        view.dirty = true;
    }
}

/// Give each new input row its fields.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn add_value_fields(
    rows: Query<(Entity, &InputRow, &ChildOf), Added<InputRow>>,
    view_nodes: Query<&ViewNode>,
    views: Query<&FlowGraphView>,
    graph: GraphQuery,
    types: Res<ViewNodeTypes>,
    mut materials: ResMut<Assets<WireMaterial>>,
    mut commands: Commands,
) {
    for (row, input, child_of) in &rows {
        let node = child_of.parent();
        let Some((kind, port)) = graph
            .canvas_of(node)
            .and_then(|canvas| views.get(canvas).ok())
            .zip(view_nodes.get(node).ok())
            .and_then(|(view, id)| {
                let node = view.graph.node(&id.0)?;
                Some((node.kind.clone(), node.inputs.get(input.0)?.clone()))
            })
        else {
            continue;
        };
        let fields = commands
            .spawn((
                ValueFields,
                Node {
                    column_gap: px(3),
                    margin: UiRect::left(px(8)),
                    ..default()
                },
                ChildOf(row),
            ))
            .id();
        let value = port.unwired_value();
        if port.kind == PortKind::Easing {
            spawn_easing_plot(&mut commands, &mut materials, fields);
        }
        if port.kind == PortKind::Bool {
            commands.spawn((
                BoolField,
                Node {
                    width: px(14),
                    height: px(14),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(if value.as_bool() { TICKED } else { FIELD }),
                BorderColor::all(FIELD_BORDER),
                EntityCursor::System(SystemCursorIcon::Pointer),
                ChildOf(fields),
            ));
        }
        for axis in 0..axes(port.kind) {
            let field = spawn_field(
                &mut commands,
                format_number(axis_of(&value, axis)),
                40.0,
                fields,
            );
            commands.entity(field).insert((
                NumberField { axis },
                EditableTextFilter::new(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')),
            ));
        }
        let picked = types
            .0
            .get(&kind)
            .is_some_and(|node_type| node_type.choices_for(&port.name).is_some());
        if port.kind == PortKind::Text && picked {
            spawn_choice_field(&mut commands, fields);
        } else if port.kind == PortKind::Text {
            let field = spawn_field(&mut commands, value.as_text().to_owned(), 110.0, fields);
            commands.entity(field).insert(TextField);
        }
    }
}

/// A choice field in `fields`: its name (shown by `sync_value_fields`) and
/// a chevron.
fn spawn_choice_field(commands: &mut Commands, fields: Entity) {
    commands
        .spawn((
            ChoiceField,
            Node {
                width: px(110),
                height: px(18),
                padding: UiRect::horizontal(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                align_items: AlignItems::Center,
                column_gap: px(4),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(FIELD_BORDER),
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(fields),
        ))
        .with_children(|field| {
            field.spawn((
                ChoiceLabel,
                Text::new(NO_CHOICE),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT),
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                Node {
                    flex_grow: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            // A corner turned down.
            field.spawn((
                Node {
                    width: px(5),
                    height: px(5),
                    margin: UiRect::bottom(px(3)),
                    border: UiRect {
                        right: px(1.5),
                        bottom: px(1.5),
                        ..default()
                    },
                    ..default()
                },
                BorderColor::all(TEXT),
                UiTransform::from_rotation(Rot2::degrees(45.0)),
                Pickable::IGNORE,
            ));
        });
}

/// The names `target`'s input may be picked from, as its node type lists
/// them in `world`: the node's inputs evaluated there, the view's graph
/// run with the view's node types. `None` if it is not picked from names.
pub(crate) fn choices(world: &World, target: &Target) -> Option<Vec<String>> {
    let view = world.get::<FlowGraphView>(target.canvas)?;
    let node = view.graph.node(&target.node)?;
    let port = node.inputs.get(target.input)?;
    let types = &world.resource::<ViewNodeTypes>().0;
    let choices = types.get(&node.kind)?.choices_for(&port.name)?;
    let evaluation = evaluate(
        world,
        Entity::PLACEHOLDER,
        &view.graph,
        types,
        &FlowVariables::default(),
        &FlowNodeMemory::default(),
    );
    let state = evaluation
        .states
        .iter()
        .find(|state| state.id == target.node)?;
    Some(choices(world, &state.inputs))
}

/// List the names under choice field `field`, which edits `target`: no
/// name first, then `target`'s choices, the current one highlighted.
pub(crate) fn open_choice_list(world: &mut World, field: Entity, target: &Target) {
    let names = choices(world, target).unwrap_or_default();
    let current = world
        .get::<FlowGraphView>(target.canvas)
        .and_then(|view| view.graph.node(&target.node)?.inputs.get(target.input))
        .map(|port| port.unwired_value().as_text().to_owned())
        .unwrap_or_default();
    let list = world
        .spawn((
            ChoiceList { field },
            Node {
                position_type: PositionType::Absolute,
                top: px(18),
                left: px(-1),
                min_width: px(110),
                max_height: px(240),
                flex_direction: FlexDirection::Column,
                padding: UiRect::vertical(px(2)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(FIELD),
            BorderColor::all(FIELD_BORDER),
            // Over the nodes drawn after this one.
            GlobalZIndex(1000),
            ChildOf(field),
        ))
        .id();
    let choices = std::iter::once(None).chain(names.into_iter().map(Some));
    for choice in choices {
        let color = if choice.as_deref().unwrap_or_default() == current {
            FOCUSED_BORDER
        } else {
            TEXT
        };
        world.spawn((
            Text::new(choice.as_deref().unwrap_or(NO_CHOICE)),
            Choice(choice),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(color),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            Node {
                padding: UiRect::axes(px(6), px(2)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            EntityCursor::System(SystemCursorIcon::Pointer),
            ChildOf(list),
        ));
    }
}

/// A click on a choice field opens its list, or closes it.
fn toggle_choice_list(
    mut click: On<PointerClick>,
    fields: Query<Option<&Children>, With<ChoiceField>>,
    lists: Query<(), With<ChoiceList>>,
    targets: Targets,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let field = click.entity;
    let Ok(children) = fields.get(field) else {
        return;
    };
    // The node under it would take the click as a selection.
    click.propagate(false);
    let open = children
        .into_iter()
        .flatten()
        .copied()
        .find(|&child| lists.contains(child));
    if let Some(list) = open {
        commands.entity(list).despawn();
        return;
    }
    if let Some(target) = targets.of(field) {
        commands.queue(move |world: &mut World| open_choice_list(world, field, &target));
    }
}

/// A click on a name picks it, closing the list.
fn pick_choice(
    mut click: On<PointerClick>,
    choices: Query<(&Choice, &ChildOf)>,
    lists: Query<&ChoiceList>,
    targets: Targets,
    mut views: Query<&mut FlowGraphView>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((choice, child_of)) = choices.get(click.entity) else {
        return;
    };
    click.propagate(false);
    let list = child_of.parent();
    if let Some(target) = lists.get(list).ok().and_then(|list| targets.of(list.field)) {
        pick(&mut views, &target, choice);
    }
    commands.entity(list).despawn();
}

/// Set `target`'s input to `choice`.
pub(crate) fn pick(views: &mut Query<&mut FlowGraphView>, target: &Target, choice: &Choice) {
    let name = choice.0.clone().unwrap_or_default();
    edit_value(views, target, |_| FlowValue::Text(name));
}

/// The wheel over a list scrolls it, rather than zooming the canvas.
fn scroll_choice_list(
    mut scroll: On<PointerScroll>,
    mut lists: Query<&mut ScrollPosition, With<ChoiceList>>,
) {
    let Ok(mut position) = lists.get_mut(scroll.entity) else {
        return;
    };
    let lines = match scroll.unit {
        bevy::input::mouse::MouseScrollUnit::Line => scroll.y * 20.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => scroll.y,
    };
    position.y = (position.y - lines).max(0.0);
    scroll.propagate(false);
}

/// Escape, or a press away from its field, closes a list.
fn close_choice_lists(
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    hovers: Option<Res<HoverMap>>,
    lists: Query<(Entity, &ChoiceList)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let escape = keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    let pressed = mouse.is_some_and(|mouse| mouse.get_just_pressed().next().is_some());
    if !escape && !pressed {
        return;
    }
    for (list, &ChoiceList { field }) in &lists {
        let over_field = hovers.as_ref().is_some_and(|hovers| {
            hovers
                .values()
                .flat_map(|hits| hits.keys())
                .any(|&hit| hit == field || parents.iter_ancestors(hit).any(|up| up == field))
        });
        if escape || !over_field {
            commands.entity(list).despawn();
        }
    }
}

/// Brighten the name under the pointer.
fn highlight_choices(
    hovers: Option<Res<HoverMap>>,
    mut choices: Query<(Entity, &mut BackgroundColor), With<Choice>>,
) {
    for (choice, mut background) in &mut choices {
        let hovered = hovers
            .as_ref()
            .is_some_and(|hovers| hovers.values().any(|hits| hits.contains_key(&choice)));
        let color = if hovered { FIELD_BORDER } else { Color::NONE };
        background.set_if_neq(BackgroundColor(color));
    }
}

/// A text input showing `text`, `width` wide, in `fields`.
fn spawn_field(commands: &mut Commands, text: String, width: f32, fields: Entity) -> Entity {
    commands
        .spawn((
            TextInput,
            SelectAllOnFocus,
            EditableText::new(text),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(TEXT),
            // Bevy's default caret is dark: unseen on a dark field.
            TextCursorStyle {
                color: TEXT,
                selection_color: Color::srgb_u8(50, 90, 150),
                unfocused_selection_color: Color::NONE,
                ..default()
            },
            // Over the node's own move cursor.
            EntityCursor::System(SystemCursorIcon::Text),
            TabIndex(0),
            Node {
                width: px(width),
                height: px(18),
                padding: UiRect::horizontal(px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(FIELD),
            BorderColor::all(FIELD_BORDER),
            ChildOf(fields),
        ))
        .id()
}

/// A number field or a text field.
type Field = Or<(With<NumberField>, With<TextField>)>;

/// Enter in a field commits it: the field lets go of the keyboard.
fn enter_commits(
    input: On<FocusedInput<KeyboardInput>>,
    fields: Query<(), Field>,
    mut focus: ResMut<InputFocus>,
) {
    if fields.contains(input.focused_entity)
        && input.input.state.is_pressed()
        && matches!(input.input.logical_key, Key::Enter)
    {
        focus.clear();
    }
}

/// A field that lost the keyboard writes its number, or its text, to its
/// input.
fn commit_fields(
    focus: Res<InputFocus>,
    mut last: Local<Option<Entity>>,
    fields: Query<(Option<&NumberField>, &EditableText), Field>,
    targets: Targets,
    mut views: Query<&mut FlowGraphView>,
) {
    let focused = focus.get();
    if *last == focused {
        return;
    }
    let left = std::mem::replace(&mut *last, focused);
    let Some(left) = left else {
        return;
    };
    let Ok((field, text)) = fields.get(left) else {
        return;
    };
    let Some(target) = targets.of(left) else {
        return;
    };
    let text = text.value().to_string();
    let Some(field) = field else {
        let text = text.trim().to_owned();
        edit_value(&mut views, &target, |_| FlowValue::Text(text));
        return;
    };
    // Not a number: `sync_value_fields` puts the value back.
    if let Some(x) = parse_number(&text) {
        edit_value(&mut views, &target, |value| with_axis(value, field.axis, x));
    }
}

/// A click on a bool box flips its input.
fn toggle_bool(
    mut click: On<PointerClick>,
    boxes: Query<(), With<BoolField>>,
    targets: Targets,
    mut views: Query<&mut FlowGraphView>,
) {
    if click.button != PointerButton::Primary || !boxes.contains(click.entity) {
        return;
    }
    // The node under it would take the click as a selection.
    click.propagate(false);
    if let Some(target) = targets.of(click.entity) {
        edit_value(&mut views, &target, |value| {
            FlowValue::Bool(!value.as_bool())
        });
    }
}

/// Fields show their input's value, and only while it is unwired; the
/// one with the keyboard is outlined. An easing's, and an arrow's pill's,
/// show only while their node is selected (or one has the keyboard): else
/// the node shows its ports alone.
#[allow(clippy::too_many_arguments, reason = "system parameters")]
fn sync_value_fields(
    mut containers: Query<(Entity, &mut Node, &Children), With<ValueFields>>,
    mut numbers: Query<(Option<&NumberField>, &mut EditableText, &mut BorderColor)>,
    mut boxes: Query<&mut BackgroundColor, With<BoolField>>,
    choice_fields: Query<&Children, With<ChoiceField>>,
    mut labels: Query<&mut Text, With<ChoiceLabel>>,
    canvas_nodes: Query<(Has<Selected>, Has<ArrowNode>)>,
    targets: Targets,
    views: Query<&FlowGraphView>,
    focus: Res<InputFocus>,
) {
    for (container, mut node, fields) in &mut containers {
        let Some(target) = targets.of_fields(container) else {
            continue;
        };
        let Some((view, port)) = views.get(target.canvas).ok().and_then(|view| {
            let port = view.graph.node(&target.node)?.inputs.get(target.input)?;
            Some((view, port))
        }) else {
            continue;
        };
        let (selected, pill) = targets
            .node_of_fields(container)
            .and_then(|node| canvas_nodes.get(node).ok())
            .unwrap_or_default();
        let typing = fields.iter().any(|field| focus.get() == Some(field));
        let on_selection = pill || port.kind == PortKind::Easing;
        let wired = view.graph.link_into(&target.node, &port.name).is_some();
        let display = if wired || (on_selection && !selected && !typing) {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        let value = port.unwired_value();
        for &field in fields {
            if let Ok((number, mut text, mut border)) = numbers.get_mut(field) {
                let focused = focus.get() == Some(field);
                let shown = match number {
                    Some(number) => format_number(axis_of(&value, number.axis)),
                    None => value.as_text().to_owned(),
                };
                if !focused && text.value().to_string() != shown {
                    text.editor_mut().set_text(&shown);
                }
                let color = if focused {
                    FOCUSED_BORDER
                } else {
                    FIELD_BORDER
                };
                border.set_if_neq(BorderColor::all(color));
            }
            if let Ok(mut background) = boxes.get_mut(field) {
                let color = if value.as_bool() { TICKED } else { FIELD };
                background.set_if_neq(BackgroundColor(color));
            }
            let shown = match value.as_text() {
                "" => NO_CHOICE,
                name => name,
            };
            for &child in choice_fields.get(field).into_iter().flatten() {
                if let Ok(mut label) = labels.get_mut(child)
                    && label.0 != shown
                {
                    label.0 = shown.to_owned();
                }
            }
        }
    }
}

/// Whether the keyboard belongs to a text field: canvas shortcuts stand
/// aside.
pub(crate) fn typing(focus: &InputFocus, texts: &Query<(), With<EditableText>>) -> bool {
    focus.get().is_some_and(|focused| texts.contains(focused))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_and_show_plainly() {
        assert_eq!(parse_number(" 2 "), Some(2.0));
        assert_eq!(parse_number("0,5"), Some(0.5));
        assert_eq!(parse_number("-1.25"), Some(-1.25));
        assert_eq!(parse_number("2-"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(-0.0), "0");
    }

    #[test]
    fn axes_edit_one_component() {
        let v = FlowValue::Vec3(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(axis_of(&v, 1), 2.0);
        assert_eq!(
            with_axis(v, 2, 5.0),
            FlowValue::Vec3(Vec3::new(1.0, 2.0, 5.0))
        );
        assert_eq!(
            with_axis(FlowValue::Float(1.0), 0, 4.0),
            FlowValue::Float(4.0)
        );
    }
}
