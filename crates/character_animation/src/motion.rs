//! How far a library's clips go: their motions, measured on their feet.
//!
//! Clips play in place, yet their feet say how fast the body would go: a
//! planted foot slides back under it at the body's speed. So each clip's
//! distance curve follows the lowest of the planted balls of the feet
//! (`ball_l`, `ball_r`, within [`PLANT`] of their lowest), holding the last
//! speed while neither is planted (and the first before either is),
//! smoothed, below [`STANDING`] none (feet shuffling in place), and summed. In the library's units, its up being +Y as in any
//! glTF.
//!
//! A select matches a stop clip with these: it plays the clip where what is
//! left of the clip's distance is what the character has left to go.
//!
//! [`ClipMotions`] loads from a library's glb by type
//! (`asset_server.load::<ClipMotions>(path)`): its loader claims no
//! extension, to leave the glb's other loads to the glTF loader.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext};
use bevy::gltf::gltf::{self, Document, animation::Property, animation::util::ReadOutputs};
use bevy::math::Affine3A;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// The feet's balls, as UE5 Manny names them.
const BALLS: [&str; 2] = ["ball_l", "ball_r"];
/// A ball this close to its lowest (m) is planted.
const PLANT: f32 = 0.04;
/// How often the feet are sampled (s).
const STEP: f32 = 1.0 / 60.0;
/// How many samples the speed is averaged over.
const SMOOTHING: usize = 7;
/// Slower than this (m/s), the body stands: a stop's feet settling in place
/// do not take it anywhere.
const STANDING: f32 = 0.1;

/// Each animation's motion in a library, by its name in the glb.
#[derive(Asset, TypePath, Clone, Debug, Default)]
pub struct ClipMotions(HashMap<String, ClipMotion>);

impl ClipMotions {
    /// The animations of a binary glTF, measured. None measure without the
    /// feet's balls.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, gltf::Error> {
        let glb = gltf::Gltf::from_slice(bytes)?;
        let blob = glb.blob.as_deref().unwrap_or_default();
        Ok(Self(measure(&glb.document, blob)))
    }

    pub fn get(&self, animation: &str) -> Option<&ClipMotion> {
        self.0.get(animation)
    }
}

/// How far a clip has gone by each sample, [`STEP`] apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipMotion {
    gone: Vec<f32>,
}

impl ClipMotion {
    /// How far the whole clip goes.
    pub fn total(&self) -> f32 {
        self.gone.last().copied().unwrap_or_default()
    }

    /// How long the measure runs (s): the clip's duration.
    pub fn duration(&self) -> f32 {
        self.gone.len().saturating_sub(1) as f32 * STEP
    }

    /// How far it has gone at `time` (s).
    pub fn gone_at(&self, time: f32) -> f32 {
        let Some(last) = self.gone.len().checked_sub(1) else {
            return 0.0;
        };
        let at = (time / STEP).clamp(0.0, last as f32);
        let k = (at.floor() as usize).min(last.saturating_sub(1));
        let next = (k + 1).min(last);
        self.gone[k].lerp(self.gone[next], at - k as f32)
    }

    /// The first time (s) with `left` or less of the clip's distance to go.
    pub fn time_left(&self, left: f32) -> f32 {
        let total = self.total();
        let Some(k) = self.gone.iter().position(|gone| total - gone <= left) else {
            return self.duration();
        };
        if k == 0 {
            return 0.0;
        }
        // Between the sample before, with more than `left` to go, and this.
        let (before, after) = (total - self.gone[k - 1], total - self.gone[k]);
        let f = (before - left) / (before - after).max(1e-6);
        (k as f32 - 1.0 + f) * STEP
    }
}

/// Loads [`ClipMotions`] from a glb, by type only.
#[derive(Default, TypePath)]
pub(crate) struct ClipMotionsLoader;

impl AssetLoader for ClipMotionsLoader {
    type Asset = ClipMotions;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<ClipMotions, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        Ok(ClipMotions::from_glb(&bytes)?)
    }
}

/// A channel's keys: times and values.
struct Track<T> {
    times: Vec<f32>,
    values: Vec<T>,
}

impl<T: Copy> Track<T> {
    /// Its value at `time`, mixing the keys around it with `mix`.
    fn at(&self, time: f32, mix: impl Fn(T, T, f32) -> T) -> T {
        let last = self.times.len() - 1;
        if time <= self.times[0] {
            return self.values[0];
        }
        if time >= self.times[last] {
            return self.values[last];
        }
        let k = self.times.partition_point(|t| *t <= time) - 1;
        let f = (time - self.times[k]) / (self.times[k + 1] - self.times[k]).max(1e-6);
        mix(self.values[k], self.values[k + 1], f)
    }
}

/// One animation's channels, by node.
#[derive(Default)]
struct Channels {
    translations: HashMap<usize, Track<Vec3>>,
    rotations: HashMap<usize, Track<Quat>>,
    scales: HashMap<usize, Track<Vec3>>,
    end: f32,
}

fn measure(document: &Document, blob: &[u8]) -> HashMap<String, ClipMotion> {
    let mut parents = vec![None; document.nodes().len()];
    for node in document.nodes() {
        for child in node.children() {
            parents[child.index()] = Some(node.index());
        }
    }
    let rest: Vec<Transform> = document
        .nodes()
        .map(|node| {
            let (translation, rotation, scale) = node.transform().decomposed();
            Transform {
                translation: translation.into(),
                rotation: Quat::from_array(rotation),
                scale: scale.into(),
            }
        })
        .collect();
    // Each ball's nodes, from the top down.
    let chains: Vec<Vec<usize>> = BALLS
        .iter()
        .filter_map(|name| {
            let ball = document.nodes().find(|node| node.name() == Some(*name))?;
            let mut chain = vec![ball.index()];
            while let Some(parent) = parents[*chain.last().unwrap()] {
                chain.push(parent);
            }
            chain.reverse();
            Some(chain)
        })
        .collect();
    if chains.len() < BALLS.len() {
        return HashMap::default();
    }
    document
        .animations()
        .filter_map(|animation| {
            let channels = channels(&animation, blob);
            let feet: Vec<Vec<Vec3>> = chains
                .iter()
                .map(|chain| positions(chain, &rest, &channels))
                .collect();
            let name = animation.name()?.to_owned();
            Some((name, ClipMotion { gone: gone(&feet) }))
        })
        .collect()
}

fn channels(animation: &gltf::Animation, blob: &[u8]) -> Channels {
    let mut channels = Channels::default();
    for channel in animation.channels() {
        let reader = channel.reader(|buffer| match buffer.source() {
            gltf::buffer::Source::Bin => Some(blob),
            gltf::buffer::Source::Uri(_) => None,
        });
        let (Some(times), Some(outputs)) = (reader.read_inputs(), reader.read_outputs()) else {
            continue;
        };
        let times: Vec<f32> = times.collect();
        let Some(end) = times.last() else {
            continue;
        };
        channels.end = channels.end.max(*end);
        let cubic =
            channel.sampler().interpolation() == gltf::animation::Interpolation::CubicSpline;
        let node = channel.target().node().index();
        match (channel.target().property(), outputs) {
            (Property::Translation, ReadOutputs::Translations(values)) => {
                let values = keys(values.map(Vec3::from).collect(), cubic);
                channels.translations.insert(node, Track { times, values });
            }
            (Property::Rotation, ReadOutputs::Rotations(values)) => {
                let values = keys(values.into_f32().map(Quat::from_array).collect(), cubic);
                channels.rotations.insert(node, Track { times, values });
            }
            (Property::Scale, ReadOutputs::Scales(values)) => {
                let values = keys(values.map(Vec3::from).collect(), cubic);
                channels.scales.insert(node, Track { times, values });
            }
            _ => {}
        }
    }
    channels
}

/// A channel's keyed values: a cubic spline's come with their tangents.
fn keys<T>(values: Vec<T>, cubic: bool) -> Vec<T> {
    if cubic {
        values.into_iter().skip(1).step_by(3).collect()
    } else {
        values
    }
}

/// Where the last node of `chain` is at each sample.
fn positions(chain: &[usize], rest: &[Transform], channels: &Channels) -> Vec<Vec3> {
    let samples = (channels.end / STEP + 1e-3).floor() as usize + 1;
    (0..samples)
        .map(|k| {
            let time = k as f32 * STEP;
            let world = chain.iter().fold(Affine3A::IDENTITY, |world, node| {
                let mut local = rest[*node];
                if let Some(track) = channels.translations.get(node) {
                    local.translation = track.at(time, Vec3::lerp);
                }
                if let Some(track) = channels.rotations.get(node) {
                    local.rotation = track.at(time, Quat::slerp);
                }
                if let Some(track) = channels.scales.get(node) {
                    local.scale = track.at(time, Vec3::lerp);
                }
                world * local.compute_affine()
            });
            world.translation.into()
        })
        .collect()
}

/// How far the body has gone at each sample, by `feet`'s positions.
fn gone(feet: &[Vec<Vec3>]) -> Vec<f32> {
    let samples = feet.first().map_or(0, Vec::len);
    // Each foot's height above its lowest, and speed along the ground.
    let feet: Vec<(Vec<f32>, Vec<f32>)> = feet
        .iter()
        .map(|positions| {
            let lowest = positions.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
            let heights = positions.iter().map(|p| p.y - lowest).collect();
            (heights, planar_speeds(positions))
        })
        .collect();
    let planted: Vec<Option<f32>> = (0..samples)
        .map(|k| {
            feet.iter()
                .filter(|(heights, _)| heights[k] < PLANT)
                .min_by(|a, b| a.0[k].total_cmp(&b.0[k]))
                .map(|(_, speeds)| speeds[k])
        })
        .collect();
    let first = planted.iter().flatten().next().copied().unwrap_or_default();
    let mut last = first;
    let speeds: Vec<f32> = planted
        .iter()
        .map(|speed| {
            last = speed.unwrap_or(last);
            last
        })
        .collect();
    let speeds: Vec<f32> = smoothed(&speeds)
        .into_iter()
        .map(|speed| if speed < STANDING { 0.0 } else { speed })
        .collect();
    let mut gone = Vec::with_capacity(samples);
    let mut total = 0.0;
    for k in 0..samples {
        if k > 0 {
            total += (speeds[k - 1] + speeds[k]) / 2.0 * STEP;
        }
        gone.push(total);
    }
    gone
}

/// Speeds along the ground (XZ), by central differences.
fn planar_speeds(positions: &[Vec3]) -> Vec<f32> {
    let n = positions.len();
    if n < 2 {
        return vec![0.0; n];
    }
    (0..n)
        .map(|k| {
            let (a, b) = (k.saturating_sub(1), (k + 1).min(n - 1));
            let moved = positions[b] - positions[a];
            Vec2::new(moved.x, moved.z).length() / ((b - a) as f32 * STEP)
        })
        .collect()
}

/// Averaged over [`SMOOTHING`] samples, the ends held.
fn smoothed(values: &[f32]) -> Vec<f32> {
    let n = values.len() as isize;
    let half = (SMOOTHING / 2) as isize;
    (0..n)
        .map(|k| {
            (k - half..=k + half)
                .map(|i| values[i.clamp(0, n - 1) as usize])
                .sum::<f32>()
                / SMOOTHING as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sample a step, `speed` m/s throughout.
    fn steady(speed: f32, samples: usize) -> ClipMotion {
        ClipMotion {
            gone: (0..samples).map(|k| k as f32 * STEP * speed).collect(),
        }
    }

    #[test]
    fn the_time_left_finds_where_the_rest_of_the_clip_is_that_far() {
        let motion = steady(1.0, 61);
        assert!((motion.total() - 1.0).abs() < 1e-5);
        assert!((motion.duration() - 1.0).abs() < 1e-5);
        assert_eq!(motion.time_left(2.0), 0.0);
        assert!((motion.time_left(0.25) - 0.75).abs() < 1e-4);
        assert!((motion.gone_at(0.75) - 0.75).abs() < 1e-4);
        assert!((motion.time_left(0.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_planted_foot_sliding_back_moves_the_body() {
        // One foot down sliding back at 2 m/s, the other up: 2 m/s ahead.
        let down: Vec<Vec3> = (0..61)
            .map(|k| Vec3::new(0.0, 0.0, -2.0 * k as f32 * STEP))
            .collect();
        let up: Vec<Vec3> = (0..61)
            .map(|k| Vec3::new(0.2, if k == 0 { 0.0 } else { 0.3 }, 0.0))
            .collect();
        let gone = gone(&[down, up]);
        assert!((gone[60] - 2.0).abs() < 1e-3, "{}", gone[60]);
    }
}
