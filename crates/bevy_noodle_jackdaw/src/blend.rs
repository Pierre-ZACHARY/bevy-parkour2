//! Blend spaces: one state made of several samples (clips, say) placed on
//! an axis or a plane, mixed by where an input is.
//!
//! A Blend Space 1D node (`blend/space_1d`) or 2D (`blend/space_2d`) holds
//! its samples in [`FlowNode::samples`]: each names the kind it plays (any
//! kind of the catalog, a clip's usually) and where it sits. In 1D, input
//! `x` picks the two samples around it and mixes them linearly; beyond the
//! samples, the end one plays alone. In 2D, inputs `x` and `y` pick the
//! triangle around them in the samples' Delaunay triangulation and mix its
//! corners by their barycentric weights; outside, the nearest point on the
//! samples' hull.
//!
//! Samples play in phase. A clip holds sync cycles (for a walk: a left
//! step then a right one), the first starting some way through it (see
//! [`SyncCycles`]): what the clip says of itself, not the blend. A walk and
//! a jog mix with the same foot ahead, even with their steps placed
//! differently. The blend space's phase goes from 0 to 1 over one cycle.
//!
//! Playing the samples is the player's job (an animation crate): these
//! nodes only describe them. The player keeps the samples in phase (see
//! [`synced_duration`], [`SyncCycles::phase_at`]) and says where the blend
//! space is in its cycle with [`BlendSpacePhase`], which the node's `phase`
//! output reads.
//!
//! A Select 2D node (`blend/select_2d`) holds samples on a plane too, but
//! picks one instead of mixing them: the nearest to its inputs `x` and `y`
//! as they were when it was entered, measured on the plane as drawn (each
//! axis scaled to its range, see [`axis_range`]). Its player plays the
//! sample's clip once and says how far it is with [`BlendSelection`],
//! which the node's `time` and `finished` outputs read. A stop, say: `x` a
//! speed and `y` the gait's phase, picks the stop for that speed on the
//! foot ahead.
//!
//! Outside the game (in the editor), a blend space or select node previews
//! its blend on a character: [`BlendPreviewAssets`], which the game lists
//! in its node catalog, says which and where each sample's clip is.

use bevy::math::DVec2;
use bevy::prelude::*;

use crate::model::{BlendSample, FlowNode, FlowValue, PortKind};
use crate::registry::{FlowGraphAppExt, FlowNodeType};

pub const BLEND_SPACE_1D: &str = "blend/space_1d";
pub const BLEND_SPACE_2D: &str = "blend/space_2d";
pub const SELECT_2D: &str = "blend/select_2d";

/// Whether nodes of `kind` are blend spaces.
pub fn is_blend_space(kind: &str) -> bool {
    kind == BLEND_SPACE_1D || kind == BLEND_SPACE_2D
}

/// Whether nodes of `kind` hold samples: blend spaces and selects.
pub fn has_samples(kind: &str) -> bool {
    is_blend_space(kind) || kind == SELECT_2D
}

/// Whether nodes of `kind` place their samples on a plane, not an axis.
pub fn is_2d(kind: &str) -> bool {
    kind == BLEND_SPACE_2D || kind == SELECT_2D
}

/// How far through its sync cycle (0 to 1) the blend space playing on this
/// entity is; set by its player.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendSpacePhase(pub f32);

/// How far the clip the Select node playing on this entity picked is: 0 to
/// 1, and whether it is over. Set by its player.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendSelection {
    pub time: f32,
    pub finished: bool,
}

/// A clip's sync cycles: how many it holds, and where the first starts (0
/// to 1 through the clip).
#[derive(Reflect, Clone, Copy, Debug, PartialEq)]
#[reflect(Default)]
pub struct SyncCycles {
    pub cycles: f32,
    pub offset: f32,
}

impl Default for SyncCycles {
    /// One cycle, from the start.
    fn default() -> Self {
        Self {
            cycles: 1.0,
            offset: 0.0,
        }
    }
}

impl SyncCycles {
    pub fn new(cycles: f32, offset: f32) -> Self {
        Self {
            cycles: cycles.max(1e-3),
            offset,
        }
    }

    /// Where a blend space is in its sync cycle, the clip `time` (0 to 1)
    /// through.
    pub fn phase_at(&self, time: f32) -> f32 {
        ((time - self.offset) * self.cycles).rem_euclid(1.0)
    }

    /// How far through the clip (0 to 1) the blend space's cycle starts.
    pub fn start(&self) -> f32 {
        self.offset.rem_euclid(1.0)
    }
}

/// What blend space previews need, outside the game: the character to pose
/// and the clips samples play. The game fills it as it registers them and
/// lists it in its node catalog (see
/// [`FlowNodeCatalogPlugin`](crate::FlowNodeCatalogPlugin)).
#[derive(Resource, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Resource, Default)]
pub struct BlendPreviewAssets {
    /// The character's glTF asset path, its bones named like UE5 Manny's.
    /// Empty: no preview.
    pub model: String,
    pub clips: Vec<PreviewClip>,
}

impl BlendPreviewAssets {
    pub fn clip(&self, kind: &str) -> Option<&PreviewClip> {
        self.clips.iter().find(|clip| clip.kind == kind)
    }
}

/// A clip samples can play, for previews.
#[derive(Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Default)]
pub struct PreviewClip {
    /// Its node kind.
    pub kind: String,
    /// The glb holding it, on a UE5 Manny skeleton.
    pub library: String,
    /// The animation's name in the glb.
    pub animation: String,
    /// In seconds.
    pub duration: f32,
    pub sync: SyncCycles,
}

pub(crate) fn register(app: &mut App) {
    app.register_type::<BlendPreviewAssets>()
        .register_type::<PreviewClip>()
        .register_type::<SyncCycles>();
    // Lit by their player while current.
    let lit = |world: &World, entity: Entity, _: &[FlowValue]| {
        let phase = world.get::<BlendSpacePhase>(entity).map_or(0.0, |p| p.0);
        vec![FlowValue::State(false), FlowValue::Float(phase)]
    };
    let outputs = &[("exit", PortKind::State), ("phase", PortKind::Float)];
    app.register_flow_node(FlowNodeType::compute(
        BLEND_SPACE_1D,
        "Blend Space 1D",
        &[("enter", PortKind::State), ("x", PortKind::Float)],
        outputs,
        lit,
    ))
    .register_flow_node(FlowNodeType::compute(
        BLEND_SPACE_2D,
        "Blend Space 2D",
        &[
            ("enter", PortKind::State),
            ("x", PortKind::Float),
            ("y", PortKind::Float),
        ],
        outputs,
        lit,
    ))
    .register_flow_node(FlowNodeType::compute(
        SELECT_2D,
        "Select 2D",
        &[
            ("enter", PortKind::State),
            ("x", PortKind::Float),
            ("y", PortKind::Float),
            // How far what the clip plays has left to go, for a player to
            // match the clip with.
            ("distance", PortKind::Float),
        ],
        &[
            ("exit", PortKind::State),
            ("time", PortKind::Float),
            ("finished", PortKind::Bool),
        ],
        // Lit by their player while current.
        |world, entity, _| {
            let selection = world
                .get::<BlendSelection>(entity)
                .copied()
                .unwrap_or_default();
            vec![
                FlowValue::State(false),
                FlowValue::Float(selection.time),
                FlowValue::Bool(selection.finished),
            ]
        },
    ));
}

/// Each of blend space `space`'s samples' weight with its input at
/// `input` (`x` only, in 1D); a select's: 1 for the one it picks.
pub fn sample_weights(space: &FlowNode, input: Vec2) -> Vec<f32> {
    let points: Vec<Vec2> = space.samples.iter().map(|s| s.at).collect();
    if space.kind == SELECT_2D {
        let scale = Vec2::new(axis_span(&space.samples, 0), axis_span(&space.samples, 1));
        weights_nearest(&points, input, scale)
    } else if space.kind == BLEND_SPACE_2D {
        weights_2d(&points, input)
    } else {
        weights_1d(&points.iter().map(|p| p.x).collect::<Vec<_>>(), input.x)
    }
}

/// Where a blend space's axis `axis` (0: x, 1: y) runs, to draw it: from 0
/// or below to the samples' furthest. Never empty.
pub fn axis_range(samples: &[BlendSample], axis: usize) -> (f32, f32) {
    let low = samples.iter().map(|s| s.at[axis]).fold(0.0, f32::min);
    let high = samples.iter().map(|s| s.at[axis]).fold(0.0, f32::max);
    if high - low < 1e-3 {
        (low, low + 1.0)
    } else {
        (low, high)
    }
}

/// How long axis `axis` of [`axis_range`] is.
fn axis_span(samples: &[BlendSample], axis: usize) -> f32 {
    let (low, high) = axis_range(samples, axis);
    high - low
}

/// Each sample's weight for `p`, samples at `points`: 1 for the nearest
/// (the first of those as near), each axis' distance divided by `scale`'s.
pub fn weights_nearest(points: &[Vec2], p: Vec2, scale: Vec2) -> Vec<f32> {
    let mut weights = vec![0.0; points.len()];
    let distance = |point: &Vec2| ((*point - p) / scale.max(Vec2::splat(1e-6))).length_squared();
    let nearest = points
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)));
    if let Some((index, _)) = nearest {
        weights[index] = 1.0;
    }
    weights
}

/// Each sample's weight for `x`, samples at `points` (in any order): the
/// two around `x` share 1 by how near each is; beyond the ends, the end
/// sample has it all. Samples at the same point share theirs evenly.
pub fn weights_1d(points: &[f32], x: f32) -> Vec<f32> {
    let mut weights = vec![0.0; points.len()];
    let below = nearest(points, |at| at <= x, |a, b| a > b);
    let above = nearest(points, |at| at >= x, |a, b| a < b);
    let (low, high) = match (below, above) {
        (Some(low), Some(high)) => (low, high),
        (Some(end), None) | (None, Some(end)) => (end, end),
        (None, None) => return weights,
    };
    let span = high - low;
    let toward_high = if span > 0.0 { (x - low) / span } else { 0.0 };
    let share = |weights: &mut Vec<f32>, point: f32, weight: f32| {
        let at: Vec<usize> = (0..points.len()).filter(|&i| points[i] == point).collect();
        for &i in &at {
            weights[i] += weight / at.len() as f32;
        }
    };
    share(&mut weights, low, 1.0 - toward_high);
    if span > 0.0 {
        share(&mut weights, high, toward_high);
    }
    weights
}

/// The point of `points` passing `keep` that `better` prefers.
fn nearest(
    points: &[f32],
    keep: impl Fn(f32) -> bool,
    better: impl Fn(f32, f32) -> bool,
) -> Option<f32> {
    points
        .iter()
        .copied()
        .filter(|&at| keep(at))
        .fold(None, |best, at| match best {
            Some(best) if !better(at, best) => Some(best),
            _ => Some(at),
        })
}

/// Each sample's weight for `p`, samples at `points`: the corners of the
/// Delaunay triangle around `p` by their barycentric weights; outside the
/// samples' hull, the two ends of the nearest hull edge, linearly. Samples
/// in a line mix as in 1D along it; samples at the same point share theirs
/// evenly.
pub fn weights_2d(points: &[Vec2], p: Vec2) -> Vec<f32> {
    let mut weights = vec![0.0; points.len()];
    // One point per place: the first sample there stands for the others.
    let mut unique: Vec<Vec2> = Vec::new();
    let place: Vec<usize> = points
        .iter()
        .map(|point| match unique.iter().position(|u| u == point) {
            Some(index) => index,
            None => {
                unique.push(*point);
                unique.len() - 1
            }
        })
        .collect();
    for (index, weight) in mix_2d(&unique, p) {
        let shared: Vec<usize> = (0..points.len()).filter(|&i| place[i] == index).collect();
        for &i in &shared {
            weights[i] += weight / shared.len() as f32;
        }
    }
    weights
}

/// The weights of `points` (all apart) for `p`, by index.
fn mix_2d(points: &[Vec2], p: Vec2) -> Vec<(usize, f32)> {
    match points.len() {
        0 => return Vec::new(),
        1 => return vec![(0, 1.0)],
        _ => {}
    }
    let triangles = triangulate(points);
    for &[a, b, c] in &triangles {
        if let Some([u, v, w]) = barycentric(p, points[a], points[b], points[c])
            && u.min(v).min(w) >= -1e-5
        {
            return vec![(a, u), (b, v), (c, w)];
        }
    }
    // Outside: along the nearest edge of the hull, or of the line.
    let edges = if triangles.is_empty() {
        line_edges(points)
    } else {
        hull_edges(&triangles)
    };
    let along = |[a, b]: [usize; 2]| {
        let (from, to) = (points[a], points[b]);
        let t =
            ((p - from).dot(to - from) / (to - from).length_squared().max(1e-9)).clamp(0.0, 1.0);
        (from.lerp(to, t).distance_squared(p), [(a, 1.0 - t), (b, t)])
    };
    edges
        .into_iter()
        .map(along)
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map_or_else(Vec::new, |(_, weights)| weights.to_vec())
}

/// `p`'s barycentric weights in triangle `a b c`; none if it is flat.
fn barycentric(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> Option<[f32; 3]> {
    let area = (b - a).perp_dot(c - a);
    if area.abs() < 1e-9 {
        return None;
    }
    let u = (b - p).perp_dot(c - p) / area;
    let v = (c - p).perp_dot(a - p) / area;
    Some([u, v, 1.0 - u - v])
}

/// The Delaunay triangulation of `points` (all apart), by index, corners
/// counter-clockwise; none if they are in a line. Bowyer-Watson: blend
/// spaces have few samples, so the plain way.
pub fn triangulate(points: &[Vec2]) -> Vec<[usize; 3]> {
    let n = points.len();
    if n < 3 {
        return Vec::new();
    }
    let mut all: Vec<DVec2> = points.iter().map(|p| p.as_dvec2()).collect();
    let min = all.iter().copied().fold(DVec2::MAX, DVec2::min);
    let max = all.iter().copied().fold(DVec2::MIN, DVec2::max);
    let size = (max - min).max_element().max(1e-3);
    let mid = (min + max) / 2.0;
    // A triangle holding every point, dropped at the end.
    all.extend([
        mid + DVec2::new(-20.0 * size, -size),
        mid + DVec2::new(20.0 * size, -size),
        mid + DVec2::new(0.0, 20.0 * size),
    ]);
    let mut triangles: Vec<[usize; 3]> = vec![[n, n + 1, n + 2]];
    for (i, &point) in all.iter().enumerate().take(n) {
        let (bad, kept): (Vec<[usize; 3]>, Vec<[usize; 3]>) = triangles
            .into_iter()
            .partition(|&t| in_circumcircle(point, t.map(|c| all[c])));
        let edges: Vec<[usize; 2]> = bad
            .iter()
            .flat_map(|&[a, b, c]| [[a, b], [b, c], [c, a]])
            .collect();
        // The hole's boundary: the edges of one bad triangle only.
        let boundary = edges
            .iter()
            .filter(|&&[a, b]| !edges.contains(&[b, a]))
            .map(|&[a, b]| ccw([a, b, i], &all));
        triangles = kept;
        triangles.extend(boundary);
    }
    triangles.retain(|t| {
        let [a, b, c] = t.map(|c| all[c]);
        t.iter().all(|&c| c < n) && (b - a).perp_dot(c - a).abs() > 1e-9 * size * size
    });
    triangles
}

/// `triangle`'s corners, counter-clockwise.
fn ccw([a, b, c]: [usize; 3], points: &[DVec2]) -> [usize; 3] {
    if (points[b] - points[a]).perp_dot(points[c] - points[a]) < 0.0 {
        [a, c, b]
    } else {
        [a, b, c]
    }
}

/// Whether `p` is inside the circle through counter-clockwise `a b c`.
fn in_circumcircle(p: DVec2, [a, b, c]: [DVec2; 3]) -> bool {
    let (a, b, c) = (a - p, b - p, c - p);
    let det = a.length_squared() * b.perp_dot(c) - b.length_squared() * a.perp_dot(c)
        + c.length_squared() * a.perp_dot(b);
    det > 0.0
}

/// The edges of one triangle only: the hull's.
fn hull_edges(triangles: &[[usize; 3]]) -> Vec<[usize; 2]> {
    let edges: Vec<[usize; 2]> = triangles
        .iter()
        .flat_map(|&[a, b, c]| [[a, b], [b, c], [c, a]])
        .collect();
    edges
        .iter()
        .copied()
        .filter(|&[a, b]| !edges.contains(&[b, a]))
        .collect()
}

/// Points in a line: the edges between neighbours along it.
fn line_edges(points: &[Vec2]) -> Vec<[usize; 2]> {
    let first = points[0];
    let far = points
        .iter()
        .copied()
        .max_by(|a, b| {
            a.distance_squared(first)
                .total_cmp(&b.distance_squared(first))
        })
        .unwrap_or(first);
    let along = |i: usize| (points[i] - first).dot(far - first);
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&a, &b| along(a).total_cmp(&along(b)));
    order.windows(2).map(|pair| [pair[0], pair[1]]).collect()
}

/// How fast blend space `space`'s clips play for speed `x`, in 1D: as they
/// are between its samples, in proportion to `x` below the first or beyond
/// the last. In 2D, as they are.
pub fn playrate(space: &FlowNode, x: f32) -> f32 {
    if space.kind != BLEND_SPACE_1D {
        return 1.0;
    }
    let points: Vec<f32> = space.samples.iter().map(|s| s.at.x).collect();
    let low = points.iter().copied().fold(f32::INFINITY, f32::min);
    let high = points.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if x < low && low > 0.0 {
        x.max(0.0) / low
    } else if x > high && high > 0.0 {
        x / high
    } else {
        1.0
    }
}

/// How long one sync cycle of the blend lasts, its samples' cycles lasting
/// `durations`: their durations, weighted. Each sample then plays at `its
/// cycle's duration / this` times its speed.
pub fn synced_duration(durations: &[f32], weights: &[f32]) -> f32 {
    let total: f32 = weights.iter().sum();
    if total <= 0.0 {
        return durations.first().copied().unwrap_or(1.0);
    }
    durations
        .iter()
        .zip(weights)
        .map(|(duration, weight)| duration * weight)
        .sum::<f32>()
        / total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
    }

    #[test]
    fn the_two_samples_around_x_share_it() {
        let points = [3.5, 1.4, 6.0];
        assert_eq!(weights_1d(&points, 1.4), [0.0, 1.0, 0.0]);
        assert!(close(&weights_1d(&points, 2.45), &[0.5, 0.5, 0.0]));
        // Beyond the ends, the end sample.
        assert_eq!(weights_1d(&points, 0.2), [0.0, 1.0, 0.0]);
        assert_eq!(weights_1d(&points, 9.0), [0.0, 0.0, 1.0]);
        assert_eq!(weights_1d(&[], 1.0), Vec::<f32>::new());
        assert_eq!(weights_1d(&[2.0, 2.0], 1.0), [0.5, 0.5]);
    }

    #[test]
    fn a_select_picks_the_nearest_on_the_plane_as_drawn() {
        let sample = |title: &str, x: f32, y: f32| BlendSample {
            kind: title.into(),
            title: title.into(),
            at: Vec2::new(x, y),
        };
        // Stops by speed (m/s) and the foot ahead (a gait's phase).
        let select = FlowNode {
            kind: SELECT_2D.into(),
            samples: vec![
                sample("walk", 1.4, 0.25),
                sample("walk", 1.4, 0.75),
                sample("jog lf", 3.5, 0.25),
                sample("jog rf", 3.5, 0.75),
                sample("sprint", 6.0, 0.25),
                sample("sprint", 6.0, 0.75),
            ],
            ..default()
        };
        let picked = |x: f32, y: f32| {
            let weights = sample_weights(&select, Vec2::new(x, y));
            assert_eq!(weights.iter().sum::<f32>(), 1.0);
            let index = weights.iter().position(|w| *w == 1.0).unwrap();
            select.samples[index].title.as_str()
        };
        assert_eq!(picked(2.0, 0.9), "walk");
        assert_eq!(picked(3.0, 0.1), "jog lf");
        assert_eq!(picked(4.0, 0.6), "jog rf");
        assert_eq!(picked(5.0, 0.4), "sprint");
        // The phase only tells the jogs apart: halfway in speed, the nearer.
        assert_eq!(picked(2.4, 0.25), "walk");
        assert_eq!(picked(2.5, 0.25), "jog lf");
        assert!(
            sample_weights(
                &FlowNode {
                    kind: SELECT_2D.into(),
                    ..default()
                },
                Vec2::ZERO
            )
            .is_empty()
        );
    }

    #[test]
    fn a_blend_space_weighs_its_samples_by_its_kind() {
        let sample = |at: Vec2| BlendSample {
            kind: "clip".into(),
            title: "Clip".into(),
            at,
        };
        let mut space = FlowNode {
            kind: BLEND_SPACE_1D.into(),
            samples: vec![sample(Vec2::new(1.0, 5.0)), sample(Vec2::new(3.0, -5.0))],
            ..default()
        };
        // In 1D, y is not read.
        let weights = sample_weights(&space, Vec2::new(1.5, 9.0));
        assert!(close(&weights, &[0.75, 0.25]), "{weights:?}");
        space.kind = BLEND_SPACE_2D.into();
        let weights = sample_weights(&space, Vec2::new(2.0, 0.0));
        assert!(close(&weights, &[0.5, 0.5]), "{weights:?}");
        assert_eq!(axis_range(&space.samples, 0), (0.0, 3.0));
        assert_eq!(axis_range(&space.samples, 1), (-5.0, 5.0));
        assert_eq!(axis_range(&[], 0), (0.0, 1.0));
    }

    #[test]
    fn a_square_splits_into_two_delaunay_triangles() {
        let square = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
        assert_eq!(triangulate(&square).len(), 2);
        // A centre point: four triangles round it.
        let centred = [square.as_slice(), &[Vec2::splat(0.5)]].concat();
        assert_eq!(triangulate(&centred).len(), 4);
        // In a line: none.
        assert!(triangulate(&[Vec2::ZERO, Vec2::X, Vec2::X * 2.0]).is_empty());
    }

    #[test]
    fn inside_the_samples_their_triangle_mixes_them() {
        let points = [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)];
        assert!(close(&weights_2d(&points, Vec2::ZERO), &[1.0, 0.0, 0.0]));
        let edge = weights_2d(&points, Vec2::new(1.0, 0.0));
        assert!(close(&edge, &[0.5, 0.5, 0.0]), "{edge:?}");
        let mid = weights_2d(&points, Vec2::new(0.5, 0.5));
        assert!(close(&mid, &[0.5, 0.25, 0.25]), "{mid:?}");
    }

    #[test]
    fn outside_the_samples_the_nearest_edge_mixes_them() {
        let points = [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0)];
        // Below the bottom edge, a quarter along it.
        let below = weights_2d(&points, Vec2::new(0.5, -3.0));
        assert!(close(&below, &[0.75, 0.25, 0.0]), "{below:?}");
        // Past a corner: the corner.
        let past = weights_2d(&points, Vec2::new(-1.0, -1.0));
        assert!(close(&past, &[1.0, 0.0, 0.0]), "{past:?}");
        // In a line, as in 1D along it.
        let line = [Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(1.0, 0.0)];
        let along = weights_2d(&line, Vec2::new(1.5, 1.0));
        assert!(close(&along, &[0.0, 0.5, 0.5]), "{along:?}");
        // At one place, shared.
        let same = [Vec2::ONE, Vec2::ONE];
        assert!(close(&weights_2d(&same, Vec2::ZERO), &[0.5, 0.5]));
        assert!(weights_2d(&[], Vec2::ZERO).is_empty());
    }

    #[test]
    fn clips_meet_at_the_start_of_their_cycles() {
        let jog = SyncCycles::new(2.0, 0.225);
        assert_eq!(jog.start(), 0.225);
        assert!(jog.phase_at(0.225).abs() < 1e-5);
        // Half a step on: the other foot.
        assert!((jog.phase_at(0.35) - 0.25).abs() < 1e-5);
        // The second cycle of the clip.
        assert!(jog.phase_at(0.725).abs() < 1e-5);
        assert!((jog.phase_at(0.1) - 0.75).abs() < 1e-5);
    }

    #[test]
    fn a_synced_cycle_lasts_the_weighted_durations() {
        assert_eq!(synced_duration(&[2.0, 1.0], &[0.5, 0.5]), 1.5);
        assert_eq!(synced_duration(&[2.0, 1.0], &[0.0, 1.0]), 1.0);
        assert_eq!(synced_duration(&[2.0, 1.0], &[0.0, 0.0]), 2.0);
    }
}
