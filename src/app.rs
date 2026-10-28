use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use portable_pty::CommandBuilder;

use crate::clipboard;
use crate::clock;
use crate::config::Config;
use crate::copy::{Action, Mode};
use crate::hints;
use crate::keys;
use crate::layout::{Direction, Rect, Split};
use crate::pane::{Pane, PaneContext, PaneId};
use crate::queries::TerminalColors;
use crate::render;
use crate::window::Window;

/// Frame interval while a scroll animation is running.
const FRAME: Duration = Duration::from_millis(8);

pub enum AppEvent {
    Input(Event),
    PaneOutput,
    /// The time shown in the status bar changed.
    Clock,
    PaneExited(PaneId),
}

pub struct App {
    pub config: Config,
    pub windows: Vec<Window>,
    pub active: usize,
    /// Terminal size as (columns, rows).
    pub size: (u16, u16),
    pub prefix_pending: bool,
    pub message: Option<String>,
    /// New name being typed for the active window.
    pub rename: Option<String>,
    /// The screensaver while it runs, drawn over everything else.
    pub screensaver: Option<(PaneId, Pane)>,
    /// Time of the last key press or paste, for starting the screensaver.
    last_input: Instant,
    /// Text from the last yank, pasted with prefix + `]`.
    paste_buffer: Option<String>,
    /// Escape sequences for the outer terminal, written before the next frame.
    pending_output: Vec<u8>,
    next_pane_id: PaneId,
    ctx: PaneContext,
    quit: bool,
}

impl App {
    pub fn new(config: Config, colors: TerminalColors) -> Result<(Self, Receiver<AppEvent>)> {
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            config,
            windows: Vec::new(),
            active: 0,
            size: crossterm::terminal::size()?,
            prefix_pending: false,
            message: None,
            rename: None,
            screensaver: None,
            last_input: Instant::now(),
            paste_buffer: None,
            pending_output: Vec::new(),
            next_pane_id: 0,
            ctx: PaneContext {
                events: tx,
                colors: Arc::new(colors),
            },
            quit: false,
        };
        app.new_window()?;
        Ok((app, rx))
    }

    pub fn run(mut self, rx: Receiver<AppEvent>) -> Result<()> {
        let input = self.ctx.events.clone();
        thread::spawn(move || {
            while let Ok(ev) = event::read() {
                if input.send(AppEvent::Input(ev)).is_err() {
                    break;
                }
            }
        });

        if self.config.status.clock {
            clock::spawn(self.config.status.clock_format.clone(), self.ctx.events.clone());
        }

        let mut out = BufWriter::with_capacity(1 << 16, io::stdout());
        render::draw(&self, &mut out)?;
        let mut last_frame = Instant::now();
        loop {
            let ev = match self.wakeup() {
                Some(timeout) => match rx.recv_timeout(timeout) {
                    Ok(ev) => Some(ev),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(ev) => Some(ev),
                    Err(_) => break,
                },
            };
            if let Some(ev) = ev {
                self.handle(ev);
                // Coalesce bursts of output into a single redraw.
                while let Ok(ev) = rx.try_recv() {
                    self.handle(ev);
                }
            }
            if self.quit {
                break;
            }
            if self.screensaver_deadline().is_some_and(|t| t <= Instant::now()) {
                self.start_screensaver();
            }

            let now = Instant::now();
            // Cap the step so an animation starting after idle time doesn't jump.
            self.tick((now - last_frame).min(FRAME * 2));
            last_frame = now;

            if !self.pending_output.is_empty() {
                out.write_all(&std::mem::take(&mut self.pending_output))?;
            }
            render::draw(&self, &mut out)?;
        }
        Ok(())
    }

    /// How long the event loop may sleep without an event, or `None` for as
    /// long as it takes.
    fn wakeup(&self) -> Option<Duration> {
        // While scrolling is animating, wake up every frame.
        let frame = self.animating().then_some(FRAME);
        let idle = self
            .screensaver_deadline()
            .map(|t| t.saturating_duration_since(Instant::now()));
        match (frame, idle) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// When the screensaver starts unless there is input before then.
    fn screensaver_deadline(&self) -> Option<Instant> {
        let timeout = self.config.screensaver.timeout;
        (timeout > 0 && self.screensaver.is_none())
            .then(|| self.last_input + Duration::from_secs(timeout))
    }

    fn start_screensaver(&mut self) {
        // Count this as input, so a screensaver that fails to start isn't
        // retried until the timeout has passed again.
        self.last_input = Instant::now();
        if self.screensaver.is_some() {
            return;
        }
        let config = &self.config.screensaver;
        if !command_exists(&config.command) {
            self.message = Some(format!(
                "{} not found: install it or set screensaver.command",
                config.command
            ));
            return;
        }
        let mut cmd = CommandBuilder::new(&config.command);
        cmd.args(config.args());
        let id = self.alloc_pane_id();
        let (cols, rows) = self.size;
        match Pane::spawn_command(id, rows, cols, self.ctx.clone(), cmd) {
            Ok(pane) => self.screensaver = Some((id, pane)),
            Err(e) => self.message = Some(format!("can't start screensaver: {e}")),
        }
    }

    fn animating(&self) -> bool {
        self.windows
            .iter()
            .flat_map(|w| w.panes.values())
            .any(|p| p.copy.as_ref().is_some_and(|c| c.animating()))
    }

    fn tick(&mut self, dt: Duration) {
        for pane in self.windows.iter_mut().flat_map(|w| w.panes.values_mut()) {
            if let Some(copy) = &mut pane.copy {
                copy.tick(dt);
            }
        }
    }

    /// Mode of the active pane, or `None` in insert mode.
    pub fn mode(&self) -> Option<Mode> {
        let window = self.active_window();
        window.panes.get(&window.active)?.copy.as_ref().map(|c| c.mode)
    }

    /// Whether the active pane is showing hints.
    pub fn hinting(&self) -> bool {
        let window = self.active_window();
        window.panes.get(&window.active).is_some_and(|p| p.hints.is_some())
    }

    /// Cursor line (1-based) and line count of the active pane in copy mode.
    pub fn copy_position(&self) -> Option<(usize, usize)> {
        let window = self.active_window();
        let copy = window.panes.get(&window.active)?.copy.as_ref()?;
        Some((copy.cursor.line + 1, copy.len()))
    }

    pub fn pane_area(&self) -> Rect {
        // A status bar in the bottom border doesn't need a line of its own.
        let status_rows = if self.config.status_in_border() { 0 } else { 1 };
        Rect {
            x: 0,
            y: 0,
            w: self.size.0,
            h: self.size.1.saturating_sub(status_rows),
        }
    }

    pub fn active_window(&self) -> &Window {
        &self.windows[self.active]
    }

    fn active_window_mut(&mut self) -> &mut Window {
        &mut self.windows[self.active]
    }

    fn handle(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::PaneOutput | AppEvent::Clock => {}
            AppEvent::PaneExited(id) if self.screensaver.as_ref().is_some_and(|(s, _)| *s == id) => {
                self.screensaver = None;
                self.last_input = Instant::now();
                self.message = Some("screensaver exited".into());
            }
            AppEvent::PaneExited(id) => self.close_pane(id),
            AppEvent::Input(Event::Key(key)) if key.kind != KeyEventKind::Release => {
                self.last_input = Instant::now();
                // Any key ends the screensaver, and does nothing else.
                if self.screensaver.take().is_none() {
                    self.handle_key(key);
                }
            }
            AppEvent::Input(Event::Paste(text)) => {
                self.last_input = Instant::now();
                if self.screensaver.take().is_none() {
                    self.paste(&text);
                }
            }
            AppEvent::Input(Event::Resize(cols, rows)) => {
                self.size = (cols, rows);
                if let Some((_, saver)) = &mut self.screensaver {
                    saver.resize(rows, cols);
                }
                let area = self.pane_area();
                for window in &mut self.windows {
                    window.resize(area);
                }
            }
            AppEvent::Input(_) => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        self.message = None;
        let prefix = self.config.prefix.0;
        if self.rename.is_some() {
            self.rename_key(key);
        } else if self.prefix_pending {
            self.prefix_pending = false;
            if let Err(e) = self.command(key) {
                self.message = Some(e.to_string());
            }
        } else if key.code == prefix.code && key.modifiers == prefix.modifiers {
            self.prefix_pending = true;
        } else if let Some(hints) = &mut self.active_window_mut().active_pane().hints {
            let action = hints.handle_key(key);
            if !matches!(action, hints::Action::None) {
                self.active_window_mut().active_pane().hints = None;
            }
            match action {
                hints::Action::None | hints::Action::Exit => {}
                hints::Action::Copy(text) => self.yank(text),
                hints::Action::CopyAndPaste(text) => {
                    self.yank(text.clone());
                    self.paste(&text);
                }
            }
        } else if let Some(copy) = &mut self.active_window_mut().active_pane().copy {
            match copy.handle_key(key) {
                Action::None => {}
                Action::Exit => self.active_window_mut().active_pane().copy = None,
                Action::Yank(text) => self.yank(text),
            }
        } else {
            self.send_key(key);
        }
    }

    /// Edits the window name being typed. An empty name brings back the
    /// automatic one.
    fn rename_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(text) = &mut self.rename else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                let name = text.trim().to_string();
                self.active_window_mut().custom_name = (!name.is_empty()).then_some(name);
                self.rename = None;
            }
            KeyCode::Esc => self.rename = None,
            KeyCode::Char('c') if ctrl => self.rename = None,
            KeyCode::Char('u') if ctrl => text.clear(),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char(c) if !ctrl => text.push(c),
            _ => {}
        }
    }

    fn yank(&mut self, text: String) {
        let lines = text.lines().count().max(1);
        if !clipboard::copy(&text) {
            self.pending_output.extend_from_slice(clipboard::osc52(&text).as_bytes());
        }
        self.message = Some(match lines {
            1 => format!("copied {} chars", text.chars().count()),
            n => format!("copied {n} lines"),
        });
        self.paste_buffer = Some(text);
    }

    fn paste(&mut self, text: &str) {
        let pane = self.active_window_mut().active_pane();
        if pane.copy.is_some() {
            return;
        }
        let bracketed = pane.parser.lock().unwrap().screen().bracketed_paste();
        if bracketed {
            pane.write(b"\x1b[200~");
            pane.write(text.as_bytes());
            pane.write(b"\x1b[201~");
        } else {
            pane.write(text.as_bytes());
        }
    }

    fn send_key(&mut self, key: KeyEvent) {
        let pane = self.active_window_mut().active_pane();
        let app_cursor = pane.parser.lock().unwrap().screen().application_cursor();
        let bytes = keys::to_bytes(key, app_cursor);
        if !bytes.is_empty() {
            pane.write(&bytes);
        }
    }

    /// Runs the command bound to `key` after the prefix.
    fn command(&mut self, key: KeyEvent) -> Result<()> {
        let area = self.pane_area();
        match key.code {
            _ if key.code == self.config.prefix.0.code => self.send_key(self.config.prefix.0),
            KeyCode::Char('c') => self.new_window()?,
            KeyCode::Char('n') => self.active = (self.active + 1) % self.windows.len(),
            KeyCode::Char('p') => {
                self.active = (self.active + self.windows.len() - 1) % self.windows.len()
            }
            // Windows are numbered from 1; 0 selects the tenth.
            KeyCode::Char(c @ '0'..='9') => {
                let i = (c as usize - '0' as usize + 9) % 10;
                if i < self.windows.len() {
                    self.active = i;
                }
            }
            KeyCode::Char('%' | '|') => self.split(Split::Horizontal)?,
            KeyCode::Char('"' | '-') => self.split(Split::Vertical)?,
            KeyCode::Left | KeyCode::Char('h') => self.active_window_mut().focus(Direction::Left, area),
            KeyCode::Right | KeyCode::Char('l') => self.active_window_mut().focus(Direction::Right, area),
            KeyCode::Up | KeyCode::Char('k') => self.active_window_mut().focus(Direction::Up, area),
            KeyCode::Down | KeyCode::Char('j') => self.active_window_mut().focus(Direction::Down, area),
            KeyCode::Char('o') => self.active_window_mut().focus_next(area),
            KeyCode::Char(';' | ':') => self.active_window_mut().focus_last(area),
            KeyCode::Char('z') => self.active_window_mut().toggle_zoom(area),
            KeyCode::Char(',') => {
                self.rename = Some(self.active_window().custom_name.clone().unwrap_or_default())
            }
            KeyCode::Char('x') => {
                let id = self.active_window().active;
                self.close_pane(id);
            }
            KeyCode::Char('&') => self.close_window(self.active),
            KeyCode::Char('[') | KeyCode::Esc => self.active_window_mut().active_pane().enter_copy_mode(),
            KeyCode::Char('f') => {
                let pane = self.windows[self.active].active_pane();
                if !pane.enter_hint_mode(&self.config.hints.alphabet.0) {
                    self.message = Some("nothing to copy".into());
                }
            }
            KeyCode::Char(']') => {
                if let Some(text) = self.paste_buffer.clone() {
                    self.paste(&text);
                }
            }
            KeyCode::Char('s') => self.start_screensaver(),
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
        Ok(())
    }

    fn alloc_pane_id(&mut self) -> PaneId {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }

    fn new_window(&mut self) -> Result<()> {
        let id = self.alloc_pane_id();
        let style = self.config.windows.borders;
        let window = Window::new(id, self.pane_area(), style, self.ctx.clone())?;
        self.windows.push(window);
        self.active = self.windows.len() - 1;
        Ok(())
    }

    fn split(&mut self, split: Split) -> Result<()> {
        let id = self.alloc_pane_id();
        let area = self.pane_area();
        let ctx = self.ctx.clone();
        self.active_window_mut().split(id, split, area, ctx)
    }

    fn close_pane(&mut self, id: PaneId) {
        let area = self.pane_area();
        let Some(index) = self.windows.iter().position(|w| w.panes.contains_key(&id)) else {
            return;
        };
        if self.windows[index].remove_pane(id, area) {
            self.close_window(index);
        }
    }

    fn close_window(&mut self, index: usize) {
        self.windows.remove(index);
        if self.windows.is_empty() {
            self.quit = true;
            return;
        }
        if self.active > index || self.active >= self.windows.len() {
            self.active -= 1;
        }
    }
}

/// Whether `command` is a path to a file, or a program found in `$PATH`.
fn command_exists(command: &str) -> bool {
    if command.contains('/') {
        return Path::new(command).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(command).is_file()))
}
