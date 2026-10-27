mod app;
mod clipboard;
mod copy;
mod keys;
mod layout;
mod pane;
mod render;
mod window;

use std::io;

use anyhow::Result;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use app::{App, Config};

fn main() -> Result<()> {
    if std::env::var_os("BARBELO").is_some() {
        anyhow::bail!("already running inside barbelo");
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;

    let result = App::new(Config::default()).and_then(|(app, rx)| app.run(rx));

    restore_terminal();
    result
}

fn restore_terminal() {
    let _ = execute!(
        io::stdout(),
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}
