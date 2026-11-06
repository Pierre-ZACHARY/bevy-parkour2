//! A dotted canvas background (grey, white dots), in place of bevy_noodle's
//! line grid: one fragment shader on a UI node filling the canvas, as
//! bevy_noodle draws its own grid.

use bevy::asset::embedded_asset;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy_noodle::prelude::*;
use bevy_noodle::style::CanvasGrid;

pub(crate) struct DotGridPlugin;

impl Plugin for DotGridPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "dots.wesl");
        app.add_plugins(UiMaterialPlugin::<DotGridMaterial>::default())
            .add_systems(PostUpdate, draw_dot_grids);
    }
}

/// On a canvas: a dotted background, replacing its [`CanvasGrid`].
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct DotGrid {
    /// Between dots, in graph units.
    pub spacing: f32,
    /// Dot radius in pixels at zoom 1.
    pub radius: f32,
    pub dot: Color,
    pub background: Color,
}

impl Default for DotGrid {
    fn default() -> Self {
        Self {
            spacing: 22.0,
            radius: 1.2,
            dot: Color::srgba(1.0, 1.0, 1.0, 0.3),
            background: Color::srgb_u8(40, 41, 46),
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, PartialEq)]
struct DotGridMaterial {
    #[uniform(0)]
    dots: Dots,
}

#[derive(ShaderType, Clone, Debug, PartialEq)]
struct Dots {
    background: Vec4,
    dot: Vec4,
    view: Vec4,
    extent: Vec4,
}

impl UiMaterial for DotGridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_noodle_jackdaw/dots.wesl".into()
    }
}

/// The node drawing a canvas's dots.
#[derive(Component)]
struct DotVisual(Entity);

type DottedCanvas<'a> = (
    Entity,
    &'a DotGrid,
    &'a CanvasView,
    &'a ComputedNode,
    Option<&'a DotVisual>,
    Has<CanvasGrid>,
);

fn draw_dot_grids(
    mut commands: Commands,
    canvases: Query<DottedCanvas>,
    visuals: Query<&MaterialNode<DotGridMaterial>>,
    mut materials: ResMut<Assets<DotGridMaterial>>,
) {
    for (canvas, grid, view, computed, visual, lines) in &canvases {
        if lines {
            commands.entity(canvas).remove::<CanvasGrid>();
        }
        let size = (computed.size() * computed.inverse_scale_factor()).max(Vec2::ONE);
        let linear = |c: Color| c.to_linear().to_vec4();
        let radius = (grid.radius * view.zoom).clamp(0.6, grid.radius * 2.0);
        let material = DotGridMaterial {
            dots: Dots {
                background: linear(grid.background),
                dot: linear(grid.dot),
                view: Vec4::new(view.pan.x, view.pan.y, view.zoom, grid.spacing.max(1.0)),
                extent: Vec4::new(size.x, size.y, radius, 0.0),
            },
        };
        match visual.and_then(|v| visuals.get(v.0).ok()) {
            Some(handle) => {
                if materials.get(&handle.0) != Some(&material)
                    && let Some(mut current) = materials.get_mut(&handle.0)
                {
                    *current = material;
                }
            }
            None => {
                let fill = Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                };
                let child = commands
                    .spawn((
                        fill,
                        MaterialNode(materials.add(material)),
                        ZIndex(-1),
                        Pickable::IGNORE,
                        ChildOf(canvas),
                    ))
                    .id();
                commands.entity(canvas).insert(DotVisual(child));
            }
        }
    }
}
