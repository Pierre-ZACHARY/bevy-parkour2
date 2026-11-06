//! Bundle launcher: pass loader/asset paths to the child before dyld starts.
//! No global environment or shell launcher is required.
use std::{env, process::{Command, ExitCode}};
fn main() -> ExitCode {
    match launch() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => { eprintln!("Bevy Parkour: {e}"); ExitCode::FAILURE }
    }
}
fn launch() -> Result<i32, Box<dyn std::error::Error>> {
    let executable = env::current_exe()?;
    let macos = executable.parent().ok_or("missing bundle directory")?;
    let contents = macos.parent().ok_or("missing Contents")?;
    let mut game = Command::new(macos.join("game"));
    game.args(env::args_os().skip(1))
        .env("DYLD_LIBRARY_PATH", contents.join("Frameworks"))
        .env("BEVY_ASSET_ROOT", contents.join("Resources"));
    if env::var_os("WGPU_BACKEND").is_none() { game.env("WGPU_BACKEND","metal"); }
    Ok(game.status()?.code().unwrap_or(1))
}
