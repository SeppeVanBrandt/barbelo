mod app;
mod clipboard;
mod clock;
mod config;
mod copy;
mod hints;
mod keys;
mod layout;
mod menu;
mod pane;
mod queries;
mod render;
mod resize;
mod window;

use std::io;

use anyhow::Result;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};

use app::App;
use queries::TerminalColors;

fn main() -> Result<()> {
    if std::env::var_os("BARBELO").is_some() {
        anyhow::bail!("already running inside barbelo");
    }

    // Read the config before taking over the terminal, so errors show up.
    let config = config::load()?;

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    // Legacy terminals send F15 as Shift+F3, the same bytes as a cursor
    // position report, so it never arrives. Terminals with the kitty
    // keyboard protocol can tell them apart.
    if config.beta.function_keys && supports_keyboard_enhancement().unwrap_or(false) {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
        if execute!(io::stdout(), PushKeyboardEnhancementFlags(flags)).is_ok() {
            KEYBOARD_ENHANCED.store(true, Ordering::Relaxed);
        }
    }

    let colors = TerminalColors::query();
    let result = App::new(config, colors).and_then(|(app, rx)| app.run(rx));

    restore_terminal();
    result
}

/// Whether the kitty keyboard protocol was turned on, and needs turning off.
static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

fn restore_terminal() {
    if KEYBOARD_ENHANCED.swap(false, Ordering::Relaxed) {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        io::stdout(),
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}
