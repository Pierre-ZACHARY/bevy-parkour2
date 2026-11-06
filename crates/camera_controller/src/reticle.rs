//! The reticle: a dot at the middle of the screen, where a third-person
//! camera aims. Each camera running a Third Person node gets one, drawn on
//! it alone, so it shows only while that camera is the active one; it hides
//! while the camera looks around freely, away from the aim.

use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::controller::angle_between;
use crate::third_person::ThirdPersonView;

/// The reticle's dot, `camera`'s: under a full-screen node centering it.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reticle {
    pub camera: Entity,
}

/// The reticle's look: its diameter and outline, logical pixels.
const SIZE: f32 = 8.0;
const OUTLINE: f32 = 1.0;
const COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);
const OUTLINE_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);

/// How far the look may be from the aim, radians, for the reticle to show.
const ON_AIM: f32 = 0.01;

pub(crate) struct ReticlePlugin;

impl Plugin for ReticlePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(spawn_reticle)
            .add_systems(Update, show_reticles);
    }
}

fn spawn_reticle(add: On<Add<ThirdPersonView>>, mut commands: Commands) {
    let camera = add.entity;
    commands
        .spawn((
            Name::new("Reticle"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            UiTargetCamera(camera),
            Pickable::IGNORE,
        ))
        .with_child((
            Reticle { camera },
            Node {
                width: px(SIZE),
                height: px(SIZE),
                border: UiRect::all(px(OUTLINE)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(COLOR),
            BorderColor::all(OUTLINE_COLOR),
            Pickable::IGNORE,
        ));
}

/// Show each reticle while its camera aims where it looks; despawn the
/// ones whose camera is gone.
fn show_reticles(
    mut reticles: Query<(&Reticle, &ChildOf, &mut Visibility)>,
    views: Query<&ThirdPersonView>,
    mut commands: Commands,
) {
    for (reticle, child_of, mut visibility) in &mut reticles {
        let Ok(view) = views.get(reticle.camera) else {
            commands.entity(child_of.parent()).despawn();
            continue;
        };
        let shown = if on_aim(view) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        visibility.set_if_neq(shown);
    }
}

/// Whether the camera looks where it aims (not looking around freely).
pub fn on_aim(view: &ThirdPersonView) -> bool {
    let yaw = angle_between(view.aim.x, view.look.x);
    Vec2::new(yaw, view.look.y - view.aim.y).length() < ON_AIM
}
