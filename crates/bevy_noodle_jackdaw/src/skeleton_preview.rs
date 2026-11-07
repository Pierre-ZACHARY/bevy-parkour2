//! Skeleton previews: a selected Skeleton or Bone node shows the game's
//! character (its [`BlendPreviewAssets`] model) at rest under its ports,
//! its skeleton drawn over it, the bones picked lit: the Bone node's, or
//! those of the Bone nodes the Skeleton node feeds. Its slot asks for it
//! (a [`SkeletonPreview`]) and goes when the node is let go; drawing it is
//! the preview plugin's job, as for blend spaces.
//!
//! Views run nodes for no entity, so a Skeleton node finds no skeleton in
//! them: Bone nodes then pick their names from [`PreviewBones`], the
//! character's, filled by the preview plugin.

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy_noodle::prelude::*;

use crate::axis::BlendPreviewSlot;
use crate::blend::BlendPreviewAssets;
use crate::model::{FlowGraph, FlowNode};
use crate::nodes::{BONE, SKELETON};
use crate::view::{FlowGraphView, ViewNode};

/// The preview's size: a clip's, a little larger to make bones out.
const SIZE: Vec2 = Vec2::new(200.0, 260.0);

pub(crate) struct SkeletonPreviewPlugin;

impl Plugin for SkeletonPreviewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PreviewBones>()
            .add_systems(Update, sync_skeleton_slots);
    }
}

/// What a [`BlendPreviewSlot`] is to show instead of a blend: the
/// character at rest, its skeleton drawn, the bones named `lit` lit.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct SkeletonPreview {
    pub lit: Vec<String>,
}

/// The names of the preview character's bones, sorted; empty until the
/// preview plugin has read them.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct PreviewBones(pub Vec<String>);

/// On a Skeleton or Bone node's preview slot.
#[derive(Component)]
struct SkeletonSlot;

/// The bones `node` (in `graph`) lights: a Bone node's own, a Skeleton
/// node's Bone nodes'; none for another node.
pub(crate) fn lit_bones(graph: &FlowGraph, node: &FlowNode) -> Option<Vec<String>> {
    match node.kind.as_str() {
        BONE => Some(bone_name(node).into_iter().collect()),
        SKELETON => Some(
            graph
                .links
                .iter()
                .filter(|link| link.from == node.id)
                .filter_map(|link| graph.node(&link.to))
                .filter(|to| to.kind == BONE)
                .filter_map(bone_name)
                .collect(),
        ),
        _ => None,
    }
}

/// The name a Bone node picks, unless none is.
fn bone_name(node: &FlowNode) -> Option<String> {
    let name = node
        .inputs
        .iter()
        .find(|port| port.name == "name")?
        .value
        .as_ref()?
        .as_text();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Give each selected Skeleton or Bone node a preview slot under its ports,
/// lighting its bones as they are picked; take it away once the node is let
/// go.
fn sync_skeleton_slots(
    nodes: Query<(Entity, &ViewNode, Has<Selected>, Option<&Children>)>,
    mut slots: Query<&mut SkeletonPreview, With<SkeletonSlot>>,
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
        let lit = (selected && !assets.model.is_empty())
            .then(|| {
                let view = views.get(graph.canvas_of(drawn)?).ok()?;
                lit_bones(&view.graph, view.graph.node(id)?)
            })
            .flatten();
        match (lit, slot) {
            (Some(lit), None) => {
                commands.spawn((
                    SkeletonSlot,
                    BlendPreviewSlot,
                    SkeletonPreview { lit },
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
            (Some(lit), Some(slot)) => {
                if let Ok(mut preview) = slots.get_mut(slot)
                    && preview.lit != lit
                {
                    preview.lit = lit;
                }
            }
            (None, Some(slot)) => {
                commands.entity(slot).despawn();
            }
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FlowLink, FlowValue, PortDef, PortKind};

    fn bone(id: &str, name: &str) -> FlowNode {
        FlowNode {
            id: id.into(),
            kind: BONE.into(),
            inputs: vec![
                PortDef {
                    name: "skeleton".into(),
                    kind: PortKind::Entity,
                    value: None,
                },
                PortDef {
                    name: "name".into(),
                    kind: PortKind::Text,
                    value: Some(FlowValue::Text(name.into())),
                },
            ],
            ..default()
        }
    }

    fn link(from: &str, to: &str) -> FlowLink {
        FlowLink {
            from: from.into(),
            output: "skeleton".into(),
            to: to.into(),
            input: "skeleton".into(),
        }
    }

    #[test]
    fn a_skeleton_lights_the_bones_picked_from_it() {
        let skeleton = FlowNode {
            id: "skeleton".into(),
            kind: SKELETON.into(),
            ..default()
        };
        let graph = FlowGraph {
            nodes: vec![
                skeleton.clone(),
                bone("split", "spine_01"),
                bone("chest", "spine_05"),
                bone("unpicked", ""),
                bone("elsewhere", "head"),
            ],
            links: vec![
                link("skeleton", "split"),
                link("skeleton", "chest"),
                link("skeleton", "unpicked"),
            ],
            ..default()
        };
        assert_eq!(
            lit_bones(&graph, &skeleton),
            Some(vec!["spine_01".into(), "spine_05".into()])
        );
        assert_eq!(
            lit_bones(&graph, &graph.nodes[4]),
            Some(vec!["head".into()])
        );
        assert_eq!(lit_bones(&graph, &graph.nodes[3]), Some(vec![]));
        let other = FlowNode {
            kind: "float/add".into(),
            ..default()
        };
        assert_eq!(lit_bones(&graph, &other), None);
    }
}
