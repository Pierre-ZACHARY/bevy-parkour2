//! The movement graph over the game, toggled with Tab: lit where values
//! flow, with the node library on its left. Edits made there drive the
//! character at once but are not saved; edit and save the file in the
//! Jackdaw editor.

use bevy::prelude::*;
use bevy_noodle_jackdaw::view::{FlowGraphView, FlowGraphViewPlugin, spawn_flow_graph_editor};
use bevy_noodle_jackdaw::{FlowGraph, FlowGraphRunner, FlowNodeTypes};

use crate::{CharacterController, MOVEMENT_GRAPH};

/// The in-game view of the movement graph, toggled with Tab.
pub struct MovementGraphViewPlugin;

impl Plugin for MovementGraphViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FlowGraphViewPlugin)
            .add_systems(Startup, spawn_graph_panel)
            .add_systems(Update, (toggle_graph_panel, draw_graph, apply_view_edits));
    }
}

/// The overlay holding the graph's canvas: shown, `Node::display` is not
/// `Display::None`.
#[derive(Component)]
pub struct GraphPanel;

fn spawn_graph_panel(mut commands: Commands) {
    commands.spawn((
        GraphPanel,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            bottom: px(0),
            height: percent(38),
            ..default()
        },
        BackgroundColor(Color::srgba_u8(24, 25, 29, 230)),
    ));
}

fn toggle_graph_panel(
    keys: Res<ButtonInput<KeyCode>>,
    mut panels: Query<&mut Node, With<GraphPanel>>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    for mut node in &mut panels {
        node.display = match node.display {
            Display::None => Display::Flex,
            _ => Display::None,
        };
    }
}

/// (Re)draw the panel's canvas when the movement graph loads or its file
/// changes, unless it already shows that graph (its own edits, applied
/// below). Other graphs (animation) run on other entities.
fn draw_graph(
    mut events: MessageReader<AssetEvent<FlowGraph>>,
    graphs: Res<Assets<FlowGraph>>,
    runners: Query<&FlowGraphRunner, With<CharacterController>>,
    panels: Query<Entity, With<GraphPanel>>,
    views: Query<&FlowGraphView>,
    types: Res<FlowNodeTypes>,
    mut commands: Commands,
) {
    for event in events.read() {
        let (AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id }) = event
        else {
            continue;
        };
        if !runners.iter().any(|runner| runner.graph.id() == *id) {
            continue;
        }
        let (Some(graph), Ok(panel)) = (graphs.get(*id), panels.single()) else {
            continue;
        };
        if views.iter().any(|view| view.root_graph() == *graph) {
            continue;
        }
        commands.entity(panel).despawn_related::<Children>();
        spawn_flow_graph_editor(
            &mut commands,
            panel,
            MOVEMENT_GRAPH,
            graph.clone(),
            &types.catalog(),
        );
    }
}

/// Edits in the in-game view drive the running movement graph at once.
fn apply_view_edits(
    mut views: Query<&mut FlowGraphView>,
    runners: Query<&FlowGraphRunner, With<CharacterController>>,
    mut graphs: ResMut<Assets<FlowGraph>>,
) {
    for mut view in &mut views {
        // Other views (another graph's) are not this one's to apply.
        if !view.dirty || view.path != MOVEMENT_GRAPH {
            continue;
        }
        view.dirty = false;
        for runner in &runners {
            if let Some(mut graph) = graphs.get_mut(&runner.graph) {
                *graph = view.root_graph();
            }
        }
    }
}
