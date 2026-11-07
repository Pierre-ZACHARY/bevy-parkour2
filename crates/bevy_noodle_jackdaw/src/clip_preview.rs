//! Clip previews: a selected node playing a clip (one the game lists in its
//! [`BlendPreviewAssets`]) shows it on a character under its ports, as a
//! blend space shows its blend. Its slot asks for the clip alone, a blend
//! space of one sample, and goes when the node is let go; drawing it is the
//! preview plugin's job, as for blend spaces.

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;

use crate::axis::{BlendPreview, BlendPreviewSlot};
use crate::blend::{BLEND_SPACE_1D, BlendPreviewAssets};
use crate::model::{BlendSample, FlowNode};
use crate::view::{FlowGraphView, ViewNode};

/// The preview's size, as a 2D blend space's.
const SIZE: Vec2 = Vec2::new(150.0, 200.0);

pub(crate) struct ClipPreviewPlugin;

impl Plugin for ClipPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_clip_slots);
    }
}

/// On a clip node's preview slot.
#[derive(Component)]
struct ClipSlot;

/// What a node playing clip `kind` previews: the clip alone.
pub(crate) fn clip_preview(kind: &str, title: &str) -> BlendPreview {
    BlendPreview {
        space: FlowNode {
            kind: BLEND_SPACE_1D.into(),
            samples: vec![BlendSample {
                kind: kind.into(),
                title: title.into(),
                at: Vec2::ZERO,
            }],
            ..default()
        },
        at: Vec2::ZERO,
    }
}

/// Give each selected clip node a preview slot under its ports, asking for
/// its clip; take it away once the node is let go.
fn sync_clip_slots(
    nodes: Query<(Entity, &ViewNode, Has<Selected>, Option<&Children>)>,
    slots: Query<(), With<ClipSlot>>,
    graph: GraphQuery,
    views: Query<&FlowGraphView>,
    assets: Res<BlendPreviewAssets>,
    mut commands: Commands,
) {
    for (drawn, ViewNode(id), selected, children) in &nodes {
        let slot = children
            .into_iter()
            .flatten()
            .copied()
            .find(|child| slots.contains(*child));
        let clip = selected
            .then(|| {
                let view = views.get(graph.canvas_of(drawn)?).ok()?;
                let node = view.graph.node(id)?;
                assets.clip(&node.kind)?;
                Some(clip_preview(&node.kind, &node.title))
            })
            .flatten();
        match (clip, slot) {
            (Some(preview), None) => {
                commands.spawn((
                    ClipSlot,
                    BlendPreviewSlot,
                    preview,
                    Node {
                        width: px(SIZE.x),
                        height: px(SIZE.y),
                        margin: UiRect::new(px(8), px(8), px(4), px(8)),
                        border_radius: BorderRadius::all(px(3)),
                        overflow: Overflow::clip(),
                        display: Display::None,
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(drawn),
                ));
            }
            (None, Some(slot)) => {
                commands.entity(slot).despawn();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::{playrate, sample_weights};

    #[test]
    fn a_clip_previews_alone_at_its_own_speed() {
        let preview = clip_preview("idle", "Idle 01");
        assert_eq!(sample_weights(&preview.space, preview.at), [1.0]);
        assert_eq!(playrate(&preview.space, preview.at), 1.0);
    }
}
