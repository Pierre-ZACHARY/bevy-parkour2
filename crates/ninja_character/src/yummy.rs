//! Yummy Games' Core Locomotion, in place: the clips
//! `tools/convert_yummy.py` puts in [`LIBRARY`], on their UE5 Manny
//! skeleton, as animation graph nodes.

use bevy::prelude::*;
use character_animation::RootMotion as Root;
use character_animation::{Clip, ClipLibrary, ClipLibraryAppExt, SyncCycles};

/// The clips' glb, under the app's assets (third-party, not redistributed).
pub const LIBRARY: &str = "animations/yummy/locomotion.glb";
/// Standing idle, the first take.
pub const IDLE: &str = "standing_idle/ip.standing.idle.01";
/// Standing walk forward.
pub const WALK_FORWARD: &str = "standing_walk/ip.standing.walk.f";
/// Standing jog forward.
pub const JOG_FORWARD: &str = "standing_jog/ip.standing.jog.f";
/// Standing walks by direction, round from forward clockwise seen from
/// above: forward, forward right, right, back right, back, back left, left,
/// forward left.
pub const WALKS: [&str; 8] = [
    WALK_FORWARD,
    "standing_walk/ip.standing.walk.fr",
    "standing_walk/ip.standing.walk.r",
    "standing_walk/ip.standing.walk.br",
    "standing_walk/ip.standing.walk.b",
    "standing_walk/ip.standing.walk.bl",
    "standing_walk/ip.standing.walk.l",
    "standing_walk/ip.standing.walk.fl",
];
/// Standing jogs by direction, as [`WALKS`].
pub const JOGS: [&str; 8] = [
    JOG_FORWARD,
    "standing_jog/ip.standing.jog.fr",
    "standing_jog/ip.standing.jog.r",
    "standing_jog/ip.standing.jog.br",
    "standing_jog/ip.standing.jog.b",
    "standing_jog/ip.standing.jog.bl",
    "standing_jog/ip.standing.jog.l",
    "standing_jog/ip.standing.jog.fl",
];
/// Standing sprint.
pub const SPRINT: &str = "standing_sprint/ip.standing.sprint";
/// Stopping from a walk.
pub const WALK_STOP: &str = "standing_walk/ip.standing.walk.f.stop";
/// Stopping from a jog, the left foot ahead.
pub const JOG_STOP_LF: &str = "standing_jog/ip.standing.jog.f.stop.lf";
/// Stopping from a jog, the right foot ahead.
pub const JOG_STOP_RF: &str = "standing_jog/ip.standing.jog.f.stop.rf";
/// Stopping from a sprint.
pub const SPRINT_STOP: &str = "standing_sprint/ip.standing.sprint.stop";
/// Standing in the air, looping: falling.
pub const FALL: &str = "standing_jump/ip.standing.jump.loop";
/// Landing from a standing jump, back to standing.
pub const LAND: &str = "standing_jump/ip.standing.jump.end";
/// Turning in place, each with how far (degrees, left positive): 45, 90,
/// 135 and 180 to the left, then to the right.
pub const TURNS: [(&str, f32); 8] = [
    ("standing_turn/rm.standing.turn.l.45", 45.0),
    ("standing_turn/rm.standing.turn.l.90", 90.0),
    ("standing_turn/rm.standing.turn.l.135", 135.0),
    ("standing_turn/rm.standing.turn.l.180", 180.0),
    ("standing_turn/rm.standing.turn.r.45", -45.0),
    ("standing_turn/rm.standing.turn.r.90", -90.0),
    ("standing_turn/rm.standing.turn.r.135", -135.0),
    ("standing_turn/rm.standing.turn.r.180", -180.0),
];

/// One converted clip.
pub(crate) struct YummyClip {
    /// The source file's name, as `ip.standing.walk.f`; the glb's animation
    /// name.
    pub id: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    pub looping: bool,
    pub duration: f32,
    /// A turn's root motion, taken out of its poses.
    /// Samples are loaded from the private GLB, never compiled into public source.
    pub root: bool,
}

/// The steps of the clips blended by velocity, measured on their feet: how
/// many each holds, and where the first (the left foot's) starts. Other
/// clips hold one cycle, from their start. The diagonals step as their
/// forward or backward.
const SYNC: &[(&str, f32, f32)] = &[
    (WALKS[0], 2.0, 0.485),
    (WALKS[1], 2.0, 0.485),
    (WALKS[2], 3.0, 0.189),
    (WALKS[3], 2.0, 0.458),
    (WALKS[4], 2.0, 0.458),
    (WALKS[5], 2.0, 0.458),
    (WALKS[6], 3.0, 0.024),
    (WALKS[7], 2.0, 0.485),
    (JOGS[0], 2.0, 0.215),
    (JOGS[1], 2.0, 0.215),
    (JOGS[2], 2.0, 0.479),
    (JOGS[3], 2.0, 0.245),
    (JOGS[4], 2.0, 0.245),
    (JOGS[5], 2.0, 0.245),
    (JOGS[6], 2.0, 0.219),
    (JOGS[7], 2.0, 0.215),
    (SPRINT, 3.0, 0.32),
];

/// The sync cycles of the clip of `kind`.
fn sync(kind: &str) -> SyncCycles {
    SYNC.iter()
        .find(|(clip, ..)| *clip == kind)
        .map_or_else(SyncCycles::default, |&(_, cycles, offset)| {
            SyncCycles::new(cycles, offset)
        })
}

/// Registers every Yummy clip, as `category/id` (see [`IDLE`]).
pub struct YummyClipsPlugin;

impl Plugin for YummyClipsPlugin {
    fn build(&self, app: &mut App) {
        app.register_clip_library(ClipLibrary {
            path: LIBRARY.to_owned(),
            clips: crate::yummy_clips::CLIPS
                .iter()
                .map(|clip| {
                    let kind = format!("{}/{}", clip.category, clip.id);
                    Clip {
                        sync: sync(&kind),
                        kind,
                        title: clip.title.to_owned(),
                        name: clip.id.to_owned(),
                        looping: clip.looping,
                        duration: clip.duration,
                        root: clip.root.then(Root::default),
                    }
                })
                .collect(),
        });
    }
}
