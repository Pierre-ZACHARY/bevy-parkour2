//! The cameras' actions, mapped by leafwing-input-manager. Every camera
//! gets them all; its graph reads those it uses.
//!
//! Keys are bound by physical position, as the character's are: `KeyW`,
//! `KeyA`, `KeyS`, `KeyD` are ZQSD on an AZERTY keyboard.

use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum CameraAction {
    /// The mouse's motion over the frame, in pixels (+Y down the screen).
    #[actionlike(DualAxis)]
    Look,
    /// The right stick: how fast to turn, up to 1 (+Y up).
    #[actionlike(DualAxis)]
    LookStick,
    /// Held: look around the character, which keeps facing where it
    /// aimed.
    FreeLook,
    /// Flying: +Y forward, +X right.
    #[actionlike(DualAxis)]
    FlyMove,
    FlyUp,
    FlyDown,
    /// Flying: the wheel's notches over the frame, up to go faster.
    #[actionlike(Axis)]
    FlySpeed,
    /// Pressed: see through the next camera.
    Switch,
}

/// Default bindings: the mouse and the right stick to look, the middle
/// button (or a press of the right stick) to look around freely; ZQSD and
/// the left stick to fly, Space and left Shift (the triggers) up and down,
/// the wheel for speed; F (Select) to switch cameras.
pub fn default_camera_input_map() -> InputMap<CameraAction> {
    InputMap::default()
        .with_dual_axis(CameraAction::Look, MouseMove::default())
        .with_dual_axis(
            CameraAction::LookStick,
            GamepadStick::RIGHT.with_circle_deadzone(0.1),
        )
        .with(CameraAction::FreeLook, MouseButton::Middle)
        .with(CameraAction::FreeLook, GamepadButton::RightThumb)
        .with_dual_axis(CameraAction::FlyMove, VirtualDPad::wasd())
        .with_dual_axis(
            CameraAction::FlyMove,
            GamepadStick::LEFT.with_circle_deadzone(0.1),
        )
        .with(CameraAction::FlyUp, KeyCode::Space)
        .with(CameraAction::FlyUp, GamepadButton::RightTrigger2)
        .with(CameraAction::FlyDown, KeyCode::ShiftLeft)
        .with(CameraAction::FlyDown, GamepadButton::LeftTrigger2)
        .with_axis(CameraAction::FlySpeed, MouseScrollAxis::Y)
        .with(CameraAction::Switch, KeyCode::KeyF)
        .with(CameraAction::Switch, GamepadButton::Select)
}
