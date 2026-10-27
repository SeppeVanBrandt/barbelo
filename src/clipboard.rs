use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;

use base64::Engine;

/// Copies `text` to the system clipboard using whichever helper is available.
/// Returns `false` if none worked.
pub fn copy(text: &str) -> bool {
    let mut helpers: Vec<&[&str]> = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        helpers.push(&["wl-copy"]);
    }
    if std::env::var_os("DISPLAY").is_some() {
        helpers.push(&["xclip", "-selection", "clipboard"]);
        helpers.push(&["xsel", "--clipboard", "--input"]);
    }
    helpers.into_iter().any(|cmd| run(cmd, text))
}

fn run(cmd: &[&str], text: &str) -> bool {
    let child = Command::new(cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let written = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // Some helpers stay around to serve the selection; reap them off-thread.
    thread::spawn(move || child.wait());
    written
}

/// OSC 52 escape sequence asking the outer terminal to set its clipboard.
pub fn osc52(text: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;c;{encoded}\x07")
}
