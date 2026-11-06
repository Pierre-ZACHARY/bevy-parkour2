//! The camera: [`CameraController`] runs its graph every frame. What the
//! cameras share: tags, the aim they leave on what they follow, the cursor,
//! and how look inputs turn them.

use bevy::input::mouse::{AccumulatedMouseMotion, MouseMotion};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorMoved, CursorOptions, PrimaryWindow, RawHandleWrapper};
use bevy_noodle_jackdaw::{FlowGraphRunner, RunEveryFrame};

use crate::graph::THIRD_PERSON_GRAPH;
use crate::input::default_camera_input_map;

/// A camera driven by a graph, run every frame. Adding it fills in the
/// graph's runner and the default camera input map. Its graph's sinks say
/// what kind of camera it is (Third Person, Fly).
#[derive(Component, Reflect, Clone, Debug, PartialEq)]
#[reflect(Component, Default)]
pub struct CameraController {
    /// Asset path of the graph it runs.
    pub graph: String,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            graph: THIRD_PERSON_GRAPH.into(),
        }
    }
}

/// A camera the player sees the world through, active or not: what is in
/// its view must stay visible, so culling will keep it. A camera without
/// the tag (the fly camera) is ignored by culling: through it, what the
/// player's cameras do not see may be missing. Nothing culls yet; the tag
/// only tells them apart.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct CullingCamera;

/// What third-person cameras follow (the first there is).
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct CameraTarget;

/// Where the camera following this entity aims, in radians: turned around
/// world Y (0 looks down -Z) and up (above 0) or down. Its camera keeps it
/// up to date; the Camera Aim node reads it.
#[derive(Component, Reflect, Clone, Copy, Debug, Default, PartialEq)]
#[reflect(Component, Default)]
pub struct CameraAim {
    pub yaw: f32,
    pub pitch: f32,
}

impl CameraAim {
    /// Horizontal, where the aim turns.
    pub fn forward(&self) -> Vec3 {
        Quat::from_rotation_y(self.yaw) * Vec3::NEG_Z
    }
}

pub(crate) fn build_camera(
    add: On<Add<CameraController>>,
    cameras: Query<&CameraController>,
    mut commands: Commands,
    assets: Res<AssetServer>,
) {
    let Ok(camera) = cameras.get(add.entity) else {
        return;
    };
    commands.entity(add.entity).insert((
        FlowGraphRunner {
            graph: assets.load(&camera.graph),
        },
        RunEveryFrame,
        default_camera_input_map(),
    ));
}

/// Whether the cursor turns the cameras: once the window has it (left
/// click; Escape lets it go). Without a window (tests, a windowless game),
/// always.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CameraCursor {
    /// Whether a click may take the cursor: the game clears it while the
    /// cursor is needed elsewhere (a panel over the game), which lets the
    /// cursor go.
    pub grab: bool,
}

impl Default for CameraCursor {
    fn default() -> Self {
        Self { grab: true }
    }
}

pub(crate) fn grab_cursor(
    cursor: Res<CameraCursor>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    for mut options in &mut windows {
        let grabbed = options.grab_mode != CursorGrabMode::None;
        let grab = if !cursor.grab || keys.just_pressed(KeyCode::Escape) {
            false
        } else {
            grabbed || mouse.just_pressed(MouseButton::Left)
        };
        if grab != grabbed {
            options.grab_mode = if grab {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::None
            };
            options.visible = !grab;
        }
    }
}

/// What tells how far the mouse moved, for the cameras to turn.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseLook {
    /// The cursor under Remote Desktop, the mouse's motion elsewhere.
    #[default]
    Auto,
    /// The mouse's motion, as the OS reports it.
    Motion,
    /// The cursor's way between its positions, put back at the window's
    /// center once it strays far from it. Under Remote Desktop, Windows
    /// reports where the mouse is on the desktop rather than how far it
    /// moved, and winit passes that on as motion: the cameras spin and look
    /// at the ground.
    Cursor,
}

impl MouseLook {
    fn reads_cursor(self) -> bool {
        match self {
            Self::Auto => remote_session(),
            Self::Motion => false,
            Self::Cursor => true,
        }
    }
}

/// Whether the game runs in a Remote Desktop session. Asked every frame:
/// a session can be taken over remotely while the game runs.
#[cfg(windows)]
fn remote_session() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};
    // SAFETY: GetSystemMetrics only reads a system setting.
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

#[cfg(not(windows))]
fn remote_session() -> bool {
    false
}

/// The primary window, if the OS draws it.
type NativePrimaryWindow = (With<PrimaryWindow>, With<RawHandleWrapper>);

/// When [`MouseLook`] reads the cursor and the window has it: the mouse's
/// motion over the frame becomes the cursor's way between its positions
/// (see [`CursorTrack`]). A frame the cursor did not move, it is the raw
/// motion's, where the absolute positions Remote Desktop sends count from
/// one to the next (see [`RawTrack`]): there, once the window has hidden
/// and confined it, the cursor no longer moves. The cursor goes back to the window's center
/// once it is a quarter of the window away. Not every frame: under Remote
/// Desktop, each move is sent to the remote client, which answers with
/// late or rounded positions, and the camera only drifted down. The frame
/// the window takes the cursor only centers it, not to turn towards where
/// the click was. Until then, and while the window lacks focus, there is no
/// motion: what Windows reports there is where the mouse is, which threw
/// the camera to the ground (the click that takes the cursor comes after
/// this, in the frame the cameras turn). A window without a native handle
/// (the editor's Play In Editor view) has no cursor to move: its motion
/// stays as it comes.
pub(crate) fn cursor_look(
    look: Res<MouseLook>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut moves: MessageReader<CursorMoved>,
    mut raw_moves: MessageReader<MouseMotion>,
    mut windows: Query<(Entity, &mut Window, &CursorOptions), NativePrimaryWindow>,
    mut track: Local<Option<CursorTrack>>,
    mut raw_track: Local<RawTrack>,
) {
    let moves: Vec<&CursorMoved> = moves.read().collect();
    let raw_moves: Vec<Vec2> = raw_moves.read().map(|moved| moved.delta).collect();
    let Ok((entity, mut window, options)) = windows.single_mut() else {
        *track = None;
        return;
    };
    if !look.reads_cursor() {
        *track = None;
        return;
    }
    if options.grab_mode == CursorGrabMode::None || !window.focused {
        motion.delta = Vec2::ZERO;
        *track = None;
        *raw_track = RawTrack::default();
        return;
    }
    let center = (window.physical_size() / 2).as_vec2();
    let scale = window.scale_factor();
    let desktop = desktop_size().unwrap_or(window.physical_size().as_vec2());
    let raw: Vec2 = raw_moves
        .iter()
        .map(|&moved| raw_track.step(moved, desktop))
        .sum();
    let moves: Vec<&CursorMoved> = moves
        .into_iter()
        .filter(|moved| moved.window == entity)
        .collect();
    motion.delta = match track.as_mut() {
        Some(track) if !moves.is_empty() => moves
            .iter()
            .map(|moved| track.step(moved.position * scale))
            .sum(),
        Some(_) => raw,
        None => Vec2::ZERO,
    };
    let track = track.get_or_insert_default();
    let strayed = track
        .last
        .is_none_or(|last| (last - center).abs().max_element() > center.min_element() / 2.0);
    if strayed {
        // Bevy moves the OS cursor in `Last`: the position asked differs
        // from the last one the window had.
        window.set_physical_cursor_position(Some(center.as_dvec2()));
        track.warp(center);
    }
    if !moves.is_empty() || !raw_moves.is_empty() || strayed {
        debug!(
            "cursor look: cursor {:?}, raw {raw_moves:?} on a {desktop} desktop -> {}{}",
            moves
                .iter()
                .map(|moved| moved.position * scale)
                .collect::<Vec<_>>(),
            motion.delta,
            if strayed { ", back to the center" } else { "" }
        );
    }
}

/// From where raw motion reads as a position: Windows' absolute raw input
/// spans 0 to 65535 across the desktop, a relative motion is a few pixels.
const ABSOLUTE_FROM: f32 = 2048.0;

/// The raw motion's way, in pixels. Absolute motions (past
/// [`ABSOLUTE_FROM`] on an axis) are where the mouse is on the desktop:
/// each counts from the last; relative ones count as they come.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RawTrack {
    /// The last absolute position, in pixels.
    last: Option<Vec2>,
}

impl RawTrack {
    /// How far the mouse went with `raw`, on a desktop `desktop` pixels big.
    pub(crate) fn step(&mut self, raw: Vec2, desktop: Vec2) -> Vec2 {
        if raw.abs().max_element() < ABSOLUTE_FROM {
            return raw;
        }
        let position = raw / 65535.0 * desktop;
        let step = self.last.map_or(Vec2::ZERO, |last| position - last);
        self.last = Some(position);
        step
    }
}

/// The virtual desktop's size in pixels, which absolute raw input spans.
#[cfg(windows)]
fn desktop_size() -> Option<Vec2> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    };
    // SAFETY: GetSystemMetrics only reads a system setting.
    let size = unsafe {
        Vec2::new(
            GetSystemMetrics(SM_CXVIRTUALSCREEN) as f32,
            GetSystemMetrics(SM_CYVIRTUALSCREEN) as f32,
        )
    };
    (size.min_element() > 0.0).then_some(size)
}

#[cfg(not(windows))]
fn desktop_size() -> Option<Vec2> {
    None
}

/// The cursor's way, from its positions (physical pixels). After a warp,
/// positions from before it may still come: a Remote Desktop client moves
/// its own cursor once the warp reaches it, and until then tells where the
/// mouse took it from where it was. Those count from the last position
/// before the warp, the others from the warp's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorTrack {
    /// The last position, since the last warp.
    last: Option<Vec2>,
    /// The last one from before the warp, until the cursor moves from
    /// where the warp put it.
    before_warp: Option<Vec2>,
}

impl CursorTrack {
    /// How far the cursor went to reach `position`.
    pub(crate) fn step(&mut self, position: Vec2) -> Vec2 {
        let Some(last) = self.last else {
            self.last = Some(position);
            return Vec2::ZERO;
        };
        if let Some(before) = self
            .before_warp
            .filter(|&before| position.distance(before) < position.distance(last))
        {
            self.before_warp = Some(position);
            return position - before;
        }
        self.last = Some(position);
        let step = position - last;
        if step != Vec2::ZERO {
            self.before_warp = None;
        }
        step
    }

    /// The cursor is put at `position`.
    pub(crate) fn warp(&mut self, position: Vec2) {
        if self.before_warp.is_none() {
            self.before_warp = self.last;
        }
        self.last = Some(position);
    }
}

/// Whether the mouse turns the cameras now (see [`CameraCursor`]).
pub(crate) fn mouse_looks(world: &World) -> bool {
    let Some(mut windows) = world.try_query_filtered::<&CursorOptions, With<PrimaryWindow>>()
    else {
        return true;
    };
    windows
        .iter(world)
        .next()
        .is_none_or(|options| options.grab_mode != CursorGrabMode::None)
}

/// How look inputs turn a camera, in radians (yaw, pitch): the mouse's
/// motion (pixels) times `sensitivity`, the stick (up to 1) times `speed`
/// per second.
pub fn look_turn(mouse: Vec2, stick: Vec2, sensitivity: f32, speed: f32, dt: f32) -> Vec2 {
    // Right turns towards -yaw; the mouse goes up the screen to -Y.
    Vec2::new(-mouse.x, -mouse.y) * sensitivity + Vec2::new(-stick.x, stick.y) * speed * dt
}

/// Whether the camera renders (one without `Camera` counts as active).
pub(crate) fn is_active(world: &World, camera: Entity) -> bool {
    world
        .get::<Camera>(camera)
        .is_none_or(|camera| camera.is_active)
}

/// `entity`'s transform in the world, from its parent's last propagated
/// one: up to date for a parent that does not move (the scene's root).
pub(crate) fn world_transform(world: &World, entity: Entity) -> Option<Transform> {
    let local = *world.get::<Transform>(entity)?;
    Some(match parent_transform(world, entity) {
        Some(parent) => (parent * GlobalTransform::from(local)).compute_transform(),
        None => local,
    })
}

/// Place `entity` at `transform` in the world (see [`world_transform`]).
pub(crate) fn set_world_transform(world: &mut World, entity: Entity, transform: Transform) {
    let local = match parent_transform(world, entity) {
        Some(parent) => GlobalTransform::from(transform).reparented_to(&parent),
        None => transform,
    };
    if let Some(mut own) = world.get_mut::<Transform>(entity) {
        own.set_if_neq(local);
    }
}

fn parent_transform(world: &World, entity: Entity) -> Option<GlobalTransform> {
    let parent = world.get::<ChildOf>(entity)?.parent();
    world.get::<GlobalTransform>(parent).copied()
}

/// The shortest turn from angle `from` to `to`, in -π..π.
pub(crate) fn angle_between(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(track: &mut CursorTrack, positions: &[(f32, f32)]) -> Vec2 {
        positions
            .iter()
            .map(|&(x, y)| track.step(Vec2::new(x, y)))
            .sum()
    }

    #[test]
    fn the_cursor_counts_its_way_between_positions() {
        let mut track = CursorTrack::default();
        track.warp(Vec2::new(640.0, 360.0));
        assert_eq!(
            steps(
                &mut track,
                &[(640.0, 360.0), (650.0, 355.0), (700.0, 340.0)]
            ),
            Vec2::new(60.0, -20.0)
        );
    }

    #[test]
    fn positions_from_before_a_warp_count_from_there() {
        let mut track = CursorTrack::default();
        track.warp(Vec2::new(640.0, 360.0));
        steps(&mut track, &[(900.0, 360.0)]);
        track.warp(Vec2::new(640.0, 360.0));
        // The warp's own move, then the remote client's late ones, from
        // its cursor still at 900, then from the center once it got there.
        let way = steps(
            &mut track,
            &[
                (640.0, 360.0),
                (910.0, 362.0),
                (920.0, 364.0),
                (650.0, 361.0),
            ],
        );
        assert_eq!(way, Vec2::new(30.0, 5.0));
        // Caught up: the old position no longer counts.
        assert_eq!(steps(&mut track, &[(890.0, 361.0)]), Vec2::new(240.0, 0.0));
    }

    #[test]
    fn absolute_raw_motion_counts_from_one_position_to_the_next() {
        let mut track = RawTrack::default();
        let desktop = Vec2::new(2560.0, 1440.0);
        // The middle, then 10 pixels right and 4 up; relative ones as is.
        assert_eq!(track.step(Vec2::new(32767.5, 32767.5), desktop), Vec2::ZERO);
        let step = track.step(Vec2::new(32767.5 + 256.0, 32767.5 - 182.0), desktop);
        assert!(step.distance(Vec2::new(10.0, -4.0)) < 0.01, "{step}");
        assert_eq!(
            track.step(Vec2::new(3.0, -2.0), desktop),
            Vec2::new(3.0, -2.0)
        );
    }

    #[test]
    fn a_rounded_echo_of_a_warp_moves_a_pixel_once() {
        let mut track = CursorTrack::default();
        track.warp(Vec2::new(640.0, 360.0));
        steps(&mut track, &[(900.0, 360.0)]);
        track.warp(Vec2::new(640.0, 360.0));
        assert_eq!(
            steps(&mut track, &[(640.0, 361.0), (640.0, 361.0)]),
            Vec2::new(0.0, 1.0)
        );
    }
}
