//! Yummy Games' Core Locomotion, in place: the clips
//! `tools/convert_yummy.py` puts in [`LIBRARY`], on their UE5 Manny
//! skeleton, as animation graph nodes.

use bevy::prelude::*;
use character_animation::{Clip, ClipLibrary, ClipLibraryAppExt, SyncCycles};

/// The clips' glb, under the app's assets (third-party, not redistributed).
pub const LIBRARY: &str = "animations/yummy/locomotion.glb";
/// Standing idle, the first take.
pub const IDLE: &str = "standing_idle/ip.standing.idle.01";
/// Standing walk forward.
pub const WALK_FORWARD: &str = "standing_walk/ip.standing.walk.f";
/// Standing jog forward.
pub const JOG_FORWARD: &str = "standing_jog/ip.standing.jog.f";
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

/// One converted clip.
pub(crate) struct YummyClip {
    /// The source file's name, as `ip.standing.walk.f`; the glb's animation
    /// name.
    pub id: &'static str,
    pub category: &'static str,
    pub title: &'static str,
    pub looping: bool,
    pub duration: f32,
}

/// The steps of the clips blended by speed, measured on their feet: how
/// many each holds, and where the first (the left foot's) starts. Other
/// clips hold one cycle, from their start.
const SYNC: &[(&str, f32, f32)] = &[
    (WALK_FORWARD, 2.0, 0.485),
    (JOG_FORWARD, 2.0, 0.215),
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
                    }
                })
                .collect(),
        });
    }
}
