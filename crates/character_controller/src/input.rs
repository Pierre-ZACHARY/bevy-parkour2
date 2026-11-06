//! The character's actions, mapped by leafwing-input-manager.
//!
//! Keys are bound by physical position (`KeyCode` names the key at that
//! place on a US QWERTY layout), so `KeyW`/`KeyA`/`KeyS`/`KeyD` are ZQSD on
//! an AZERTY keyboard and WASD on a QWERTY one, with no layout detection.

use bevy::prelude::*;
use leafwing_input_manager::prelude::*;

#[derive(Actionlike, PartialEq, Eq, Clone, Copy, Hash, Debug, Reflect)]
pub enum PlayerAction {
    /// Move relative to the camera: +Y forward, +X right, length up to 1.
    #[actionlike(DualAxis)]
    Move,
    /// Held: jog. Tapped twice and held: sprint (the movement graph's
    /// Double Tap node tells them apart).
    Run,
}

/// Default bindings: ZQSD (WASD on QWERTY) and the left stick to move,
/// left Shift and a press of the left stick to run.
pub fn default_input_map() -> InputMap<PlayerAction> {
    InputMap::default()
        .with_dual_axis(PlayerAction::Move, VirtualDPad::wasd())
        .with_dual_axis(
            PlayerAction::Move,
            GamepadStick::LEFT.with_circle_deadzone(0.1),
        )
        .with(PlayerAction::Run, KeyCode::ShiftLeft)
        .with(PlayerAction::Run, GamepadButton::LeftThumb)
}
