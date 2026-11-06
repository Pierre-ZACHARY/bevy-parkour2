//! The Gait node: how fast the character goes, eased from one pace to the
//! next. Walk without Shift, jog with it held, sprint with it tapped twice
//! and held; let go of the stick and it stops, along the way it went.
//!
//! Each change of pace eases the speed with its own curve and duration,
//! an input of the node: `walk_to_jog`, `jog_to_sprint`, `walk_to_sprint`,
//! `jog_to_walk`, `sprint_to_walk` (sprint to jog too, which the keys do
//! not do), `walk_stop`, `jog_stop` and `sprint_stop` (to a stop, from each
//! pace) and `start` (from a stop). A stop is from the pace the speed is
//! nearest, not the one last asked for: a sprint let down to a walk just
//! before the stick is let go still stops as a sprint. The node puts
//! out a velocity in m/s, for Tnua Walk: the controller's walk speed is
//! 1 m/s, so a motion is a velocity.
//!
//! It also puts out which of its inputs is easing the speed (`easing`, -1
//! once settled) and how far along (`progress`, 0 to 1): what the view
//! marks on that curve while the game plays.
//!
//! And how far the character goes before it is at rest (`stop_distance`,
//! in m): the rest of the stop under way, else the stop it would make were
//! the stick let go now. Set Stop Distance keeps it on the character, for
//! its animation to match its stop with (see `state.rs`).

use bevy::prelude::*;
use bevy_noodle_jackdaw::{Easing, FlowGraphAppExt, FlowNodeType, FlowValue, PortKind};

pub const GAIT: &str = "character/gait";

pub const WALK_SPEED: f32 = 1.4;
pub const JOG_SPEED: f32 = 3.5;
pub const SPRINT_SPEED: f32 = 6.0;

/// A stick pushed less than this is let go.
const DEADZONE: f32 = 0.05;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gait {
    Stop,
    Walk,
    Jog,
    Sprint,
}

impl Gait {
    fn from_index(index: f32) -> Self {
        match index as i32 {
            1 => Self::Walk,
            2 => Self::Jog,
            3 => Self::Sprint,
            _ => Self::Stop,
        }
    }

    fn index(self) -> f32 {
        self as i32 as f32
    }
}

/// The node's inputs, in order.
const MOTION: usize = 0;
const JOG: usize = 1;
const SPRINT: usize = 2;
/// Walk, jog and sprint speeds, from here.
const SPEEDS: usize = 3;
/// The easings, from here, in [`EASINGS`]' order.
const FIRST_EASING: usize = 6;
const EASINGS: [&str; 9] = [
    "start",
    "walk_to_jog",
    "jog_to_sprint",
    "walk_to_sprint",
    "jog_to_walk",
    "sprint_to_walk",
    "walk_stop",
    "jog_stop",
    "sprint_stop",
];

/// Which of [`EASINGS`] eases from `from` to `to`.
fn easing_between(from: Gait, to: Gait) -> usize {
    use Gait::*;
    let name = match (from, to) {
        (Stop, _) => "start",
        (Walk, Stop) => "walk_stop",
        (Jog, Stop) => "jog_stop",
        (Sprint, Stop) => "sprint_stop",
        (Walk, Jog) => "walk_to_jog",
        (Jog, Sprint) => "jog_to_sprint",
        (Walk, Sprint) => "walk_to_sprint",
        (Jog, Walk) => "jog_to_walk",
        (Sprint, Walk | Jog) => "sprint_to_walk",
        (Walk, Walk) | (Jog, Jog) | (Sprint, Sprint) => "start",
    };
    EASINGS.iter().position(|easing| *easing == name).unwrap()
}

/// The easings' defaults: ease in and out between paces, quick to start.
/// A walk stops as Yummy's walk stop clip does once it leaves the walk,
/// measured on its feet (0.83 m in 0.93 s, most of the speed kept for half
/// of it, then the last step planted); a sprint slows at once, then glides
/// on, nearly logarithmically (`ln(1 + 9t) / ln 10` within 0.004: 1.9 m
/// from its speed in 1 s); a jog settles at once.
fn default_easing(name: &str) -> Easing {
    match name {
        "start" => Easing::ease_out(0.2),
        "walk_to_jog" | "jog_to_walk" => Easing::ease_in_out(0.4),
        "jog_to_sprint" => Easing::ease_in_out(0.5),
        "walk_to_sprint" | "sprint_to_walk" => Easing::ease_in_out(0.7),
        "walk_stop" => Easing::new(0.93, Vec2::new(0.2, 0.2), Vec2::new(0.5, 0.0)),
        "sprint_stop" => Easing::new(1.0, Vec2::new(0.15, 0.6), Vec2::new(0.6, 0.85)),
        _ => Easing::ease_out(0.3),
    }
}

pub(crate) fn register(app: &mut App) {
    let mut inputs = vec![
        ("motion", PortKind::Vec3),
        ("jog", PortKind::Bool),
        ("sprint", PortKind::Bool),
        ("walk_speed", PortKind::Float),
        ("jog_speed", PortKind::Float),
        ("sprint_speed", PortKind::Float),
    ];
    inputs.extend(EASINGS.iter().map(|name| (*name, PortKind::Easing)));
    let mut node = FlowNodeType::step(
        GAIT,
        "Gait",
        &inputs,
        &[
            ("velocity", PortKind::Vec3),
            ("speed", PortKind::Float),
            ("gait", PortKind::Float),
            ("easing", PortKind::Float),
            ("progress", PortKind::Float),
            ("stop_distance", PortKind::Float),
        ],
        |world, _, inputs, memory| {
            let dt = world.get_resource::<Time>().map_or(0.0, Time::delta_secs);
            let out = step(memory, inputs, dt);
            vec![
                FlowValue::Vec3(out.velocity),
                FlowValue::Float(out.velocity.length()),
                FlowValue::Float(out.gait.index()),
                FlowValue::Float(out.easing.map_or(-1.0, |i| (FIRST_EASING + i) as f32)),
                FlowValue::Float(out.progress),
                FlowValue::Float(out.stop_distance),
            ]
        },
    )
    .with_default("walk_speed", FlowValue::Float(WALK_SPEED))
    .with_default("jog_speed", FlowValue::Float(JOG_SPEED))
    .with_default("sprint_speed", FlowValue::Float(SPRINT_SPEED));
    for name in EASINGS {
        node = node.with_default(name, FlowValue::Easing(default_easing(name)));
    }
    app.register_flow_node(node);
}

/// What one step of the node gives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GaitStep {
    pub velocity: Vec3,
    pub gait: Gait,
    /// The easing of [`EASINGS`] under way, if one is.
    pub easing: Option<usize>,
    pub progress: f32,
    /// How far (m) the character goes before it is at rest.
    pub stop_distance: f32,
}

/// The pace asked for: a stop with the stick let go, else sprint over jog
/// over walk.
fn wanted(inputs: &[FlowValue]) -> Gait {
    if inputs[MOTION].as_vec3().length() < DEADZONE {
        Gait::Stop
    } else if inputs[SPRINT].as_bool() {
        Gait::Sprint
    } else if inputs[JOG].as_bool() {
        Gait::Jog
    } else {
        Gait::Walk
    }
}

/// The speed (m/s) `gait` settles at.
fn speed_of(gait: Gait, inputs: &[FlowValue]) -> f32 {
    match gait {
        Gait::Stop => 0.0,
        Gait::Walk => inputs[SPEEDS].as_f32(),
        Gait::Jog => inputs[SPEEDS + 1].as_f32(),
        Gait::Sprint => inputs[SPEEDS + 2].as_f32(),
    }
}

/// The pace whose speed `speed` is nearest: the one a stop is from, as the
/// animation picks its stop clip by the speed too.
fn pace_at(speed: f32, inputs: &[FlowValue]) -> Gait {
    [Gait::Walk, Gait::Jog, Gait::Sprint]
        .into_iter()
        .min_by(|a, b| {
            let off = |gait| (speed_of(gait, inputs) - speed).abs();
            off(*a).total_cmp(&off(*b))
        })
        .unwrap()
}

/// One step of `dt` seconds. Remembers the pace, the speed the easing
/// started from, which easing and for how long, the speed and the way it
/// goes.
fn step(memory: &mut Vec<FlowValue>, inputs: &[FlowValue], dt: f32) -> GaitStep {
    if memory.len() != 6 {
        *memory = vec![
            FlowValue::Float(Gait::Stop.index()),
            FlowValue::Float(0.0),
            FlowValue::Float(easing_between(Gait::Stop, Gait::Stop) as f32),
            FlowValue::Float(f32::INFINITY),
            FlowValue::Float(0.0),
            FlowValue::Vec3(Vec3::ZERO),
        ];
    }
    let mut gait = Gait::from_index(memory[0].as_f32());
    let mut from = memory[1].as_f32();
    let mut easing = memory[2].as_f32() as usize;
    let mut elapsed = memory[3].as_f32();
    let speed = memory[4].as_f32();
    let mut direction = memory[5].as_vec3();

    let motion = inputs[MOTION].as_vec3();
    if motion.length() >= DEADZONE {
        direction = motion.normalize();
    }
    let wanted = wanted(inputs);
    if wanted != gait {
        let leaving = if wanted == Gait::Stop {
            pace_at(speed, inputs)
        } else {
            gait
        };
        easing = easing_between(leaving, wanted);
        from = speed;
        elapsed = 0.0;
        gait = wanted;
    } else {
        elapsed += dt;
    }
    let target = speed_of(gait, inputs);
    let curve = inputs
        .get(FIRST_EASING + easing)
        .map_or(Easing::LINEAR, |value| value.as_easing());
    let progress = if curve.duration > 0.0 {
        (elapsed / curve.duration).min(1.0)
    } else {
        1.0
    };
    let speed = from + (target - from) * curve.after(elapsed);
    let stop_distance = if gait == Gait::Stop {
        distance_to_rest(from, curve, elapsed)
    } else {
        let stop = inputs
            .get(FIRST_EASING + easing_between(pace_at(speed, inputs), Gait::Stop))
            .map_or(Easing::LINEAR, |value| value.as_easing());
        distance_to_rest(speed, stop, 0.0)
    };
    *memory = vec![
        FlowValue::Float(gait.index()),
        FlowValue::Float(from),
        FlowValue::Float(easing as f32),
        FlowValue::Float(elapsed),
        FlowValue::Float(speed),
        FlowValue::Vec3(direction),
    ];
    GaitStep {
        velocity: direction * speed.max(0.0),
        gait,
        easing: (progress < 1.0).then_some(easing),
        progress,
        stop_distance,
    }
}

/// How far a stop from `from` m/s along `curve` goes from `elapsed` seconds
/// in to its end.
fn distance_to_rest(from: f32, curve: Easing, elapsed: f32) -> f32 {
    const STEPS: usize = 32;
    let left = curve.duration - elapsed;
    if left <= 0.0 {
        return 0.0;
    }
    let speed = |t: f32| (from * (1.0 - curve.after(t))).max(0.0);
    let dt = left / STEPS as f32;
    (0..STEPS)
        .map(|i| {
            let t = elapsed + i as f32 * dt;
            (speed(t) + speed(t + dt)) / 2.0 * dt
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The node's inputs, its defaults but for the motion and the keys.
    fn inputs(motion: Vec3, jog: bool, sprint: bool) -> Vec<FlowValue> {
        let mut inputs = vec![
            FlowValue::Vec3(motion),
            FlowValue::Bool(jog),
            FlowValue::Bool(sprint),
            FlowValue::Float(WALK_SPEED),
            FlowValue::Float(JOG_SPEED),
            FlowValue::Float(SPRINT_SPEED),
        ];
        inputs.extend(EASINGS.map(|name| FlowValue::Easing(default_easing(name))));
        inputs
    }

    /// Steps of 1/60 s for `seconds`: the last one.
    fn run(memory: &mut Vec<FlowValue>, inputs: &[FlowValue], seconds: f32) -> GaitStep {
        let steps = (seconds * 60.0).round() as usize;
        (0..steps)
            .map(|_| step(memory, inputs, 1.0 / 60.0))
            .last()
            .unwrap()
    }

    #[test]
    fn each_pace_settles_at_its_speed() {
        let mut memory = Vec::new();
        let forward = Vec3::NEG_Z;
        for (jog, sprint, gait, speed) in [
            (false, false, Gait::Walk, WALK_SPEED),
            (true, false, Gait::Jog, JOG_SPEED),
            (true, true, Gait::Sprint, SPRINT_SPEED),
            (false, false, Gait::Walk, WALK_SPEED),
        ] {
            let last = run(&mut memory, &inputs(forward, jog, sprint), 1.0);
            assert_eq!((last.gait, last.easing), (gait, None));
            assert!(last.velocity.abs_diff_eq(forward * speed, 1e-4), "{last:?}");
        }
    }

    #[test]
    fn a_change_of_pace_eases_with_its_curve() {
        let mut memory = Vec::new();
        run(&mut memory, &inputs(Vec3::X, false, false), 1.0);
        let jog = inputs(Vec3::X, true, false);
        let first = step(&mut memory, &jog, 1.0 / 60.0);
        assert_eq!(first.easing, Some(easing_between(Gait::Walk, Gait::Jog)));
        // Ease in and out: slow off the mark, halfway at half the time.
        assert!(first.velocity.x < WALK_SPEED + 0.01);
        let half = run(&mut memory, &jog, 0.2);
        assert!((half.velocity.x - (WALK_SPEED + JOG_SPEED) / 2.0).abs() < 0.1);
        assert!((half.progress - 0.5).abs() < 0.05);
    }

    #[test]
    fn letting_go_stops_along_the_way_it_went() {
        let mut memory = Vec::new();
        run(&mut memory, &inputs(Vec3::X, true, false), 1.0);
        let stopping = step(&mut memory, &inputs(Vec3::ZERO, false, false), 1.0 / 60.0);
        assert_eq!(stopping.gait, Gait::Stop);
        assert!(stopping.velocity.x > 0.0 && stopping.velocity.y == 0.0);
        let stopped = run(&mut memory, &inputs(Vec3::ZERO, false, false), 1.0);
        assert_eq!(stopped.velocity, Vec3::ZERO);
    }

    /// Shift let go a moment before the stick: the sprint has barely begun
    /// to ease down to a walk, so it stops as a sprint.
    #[test]
    fn a_stop_is_from_the_pace_it_goes_at() {
        let mut memory = Vec::new();
        run(&mut memory, &inputs(Vec3::X, true, true), 2.0);
        let walking = run(&mut memory, &inputs(Vec3::X, false, false), 0.1);
        assert_eq!(walking.gait, Gait::Walk);
        let sprint_stop = default_easing("sprint_stop");
        assert!(
            (walking.stop_distance - distance_to_rest(walking.velocity.x, sprint_stop, 0.0)).abs()
                < 1e-4,
            "{walking:?}"
        );
        let stopping = step(&mut memory, &inputs(Vec3::ZERO, false, false), 1.0 / 60.0);
        assert_eq!(
            stopping.easing,
            Some(easing_between(Gait::Sprint, Gait::Stop))
        );

        // Once it is down to a walk, it stops as a walk.
        run(&mut memory, &inputs(Vec3::X, true, true), 2.0);
        run(&mut memory, &inputs(Vec3::X, false, false), 1.0);
        let stopping = step(&mut memory, &inputs(Vec3::ZERO, false, false), 1.0 / 60.0);
        assert_eq!(
            stopping.easing,
            Some(easing_between(Gait::Walk, Gait::Stop))
        );
    }

    /// The stop distance is the stop the character would make, then what is
    /// left of the one it makes, down to none at rest.
    #[test]
    fn the_stop_distance_is_what_is_left_to_go() {
        let linear = |duration| Easing {
            duration,
            ..Easing::LINEAR
        };
        // A linear stop goes half as far as at full speed.
        assert!((distance_to_rest(2.0, linear(0.5), 0.0) - 0.5).abs() < 1e-4);
        assert!((distance_to_rest(2.0, linear(0.5), 0.25) - 0.125).abs() < 1e-4);

        let mut memory = Vec::new();
        let walking = run(&mut memory, &inputs(Vec3::X, false, false), 1.0);
        let stop = default_easing("walk_stop");
        let ahead = distance_to_rest(WALK_SPEED, stop, 0.0);
        assert!((walking.stop_distance - ahead).abs() < 1e-4, "{walking:?}");
        assert!(ahead > 0.0 && ahead < WALK_SPEED * stop.duration);

        // What it goes while stopping is what was ahead.
        let let_go = inputs(Vec3::ZERO, false, false);
        let (mut went, mut left) = (0.0, ahead);
        for _ in 0..(stop.duration * 60.0).ceil() as usize + 1 {
            let out = step(&mut memory, &let_go, 1.0 / 60.0);
            assert!(out.stop_distance <= left + 1e-4, "{out:?}");
            left = out.stop_distance;
            went += out.velocity.x / 60.0;
        }
        assert_eq!(left, 0.0);
        assert!((went - ahead).abs() < 0.03, "went {went}, {ahead} ahead");
    }
}
