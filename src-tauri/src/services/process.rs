use std::path::Path;
use std::process::{Command, Output, Stdio};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn hidden_command(program: impl AsRef<Path>) -> Command {
    let mut command = Command::new(program.as_ref());
    hide_window(&mut command);
    command
}

pub fn hidden_piped_command(program: impl AsRef<Path>) -> Command {
    let mut command = hidden_command(program);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

pub fn run_hidden(
    program: impl AsRef<Path>,
    arguments: &[String],
    path_env: &str,
) -> Result<Output, std::io::Error> {
    let mut command = hidden_command(program);
    command
        .args(arguments)
        .env("PATH", path_env)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .output()
}

#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_command: &mut Command) {}
