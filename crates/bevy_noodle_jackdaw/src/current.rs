//! A ring of dashes running round each node [`light_up`](crate::view)
//! marks [`CurrentState`]: the state a running machine is in. One fragment
//! shader on a node over the state's border, as the dotted grid is drawn.

use bevy::asset::embedded_asset;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// The ring's colour.
pub const CURRENT: Color = Color::srgb(0.3, 0.9, 0.45);

/// How far the ring's node reaches out of the state's padding box: over its
/// border, and a little beyond.
const OUT: f32 = 4.0;
/// Kit nodes' corner radius, outside their border.
const RADIUS: f32 = 8.0;
/// Twice kit nodes' border.
const WIDTH: f32 = 3.0;
const DASH: f32 = 10.0;
/// A dash and a gap.
const PERIOD: f32 = 16.0;
/// Pixels per second.
const SPEED: f32 = 24.0;

pub(crate) struct CurrentStatePlugin;

impl Plugin for CurrentStatePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "current.wesl");
        app.add_plugins(UiMaterialPlugin::<RingMaterial>::default())
            .add_systems(PostUpdate, draw_rings);
    }
}

/// On a canvas node: it draws the state its machine is in.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CurrentState;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, PartialEq)]
struct RingMaterial {
    #[uniform(0)]
    ring: Ring,
}

#[derive(ShaderType, Clone, Debug, PartialEq)]
struct Ring {
    color: Vec4,
    shape: Vec4,
    pattern: Vec4,
}

impl UiMaterial for RingMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_noodle_jackdaw/current.wesl".into()
    }
}

/// On a current state's node: the node drawing its ring.
#[derive(Component)]
struct RingVisual(Entity);

/// Give current states a ring over their border, and take it off the
/// others.
fn draw_rings(
    mut commands: Commands,
    states: Query<(Entity, &ComputedNode, Option<&RingVisual>), With<CurrentState>>,
    former: Query<(Entity, &RingVisual), Without<CurrentState>>,
    visuals: Query<(&MaterialNode<RingMaterial>, &ComputedNode)>,
    mut materials: ResMut<Assets<RingMaterial>>,
) {
    for (node, visual) in &former {
        commands.entity(visual.0).try_despawn();
        commands.entity(node).remove::<RingVisual>();
    }
    for (node, computed, visual) in &states {
        let Some((handle, drawn)) = visual.and_then(|v| visuals.get(v.0).ok()) else {
            let over = Node {
                position_type: PositionType::Absolute,
                left: px(-OUT),
                top: px(-OUT),
                right: px(-OUT),
                bottom: px(-OUT),
                ..default()
            };
            let material = materials.add(RingMaterial {
                ring: ring(Vec2::ZERO, 0.0),
            });
            let visual = commands
                .spawn((over, MaterialNode(material), Pickable::IGNORE))
                .id();
            // First, so the ports along the border draw over it.
            commands
                .entity(node)
                .insert_children(0, &[visual])
                .insert(RingVisual(visual));
            continue;
        };
        let logical = |computed: &ComputedNode| computed.size() * computed.inverse_scale_factor();
        let size = logical(drawn);
        // From the ring's edge to the border's outer edge: the stroke covers
        // the border and no more of the node, whose title bar would hide it.
        let inset = (size.x - logical(computed).x) / 2.0;
        let material = RingMaterial {
            ring: ring(size, inset),
        };
        if materials.get(&handle.0) != Some(&material)
            && let Some(mut current) = materials.get_mut(&handle.0)
        {
            *current = material;
        }
    }
}

/// A ring `size` big, its stroke `inset` in from its edge.
fn ring(size: Vec2, inset: f32) -> Ring {
    Ring {
        color: CURRENT.to_linear().to_vec4(),
        shape: Vec4::new(size.x, size.y, inset, RADIUS),
        pattern: Vec4::new(WIDTH, DASH, PERIOD, SPEED),
    }
}
