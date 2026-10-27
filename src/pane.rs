use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::app::AppEvent;
use crate::copy::CopyMode;
use crate::queries::{Responder, TerminalColors};

pub type PaneId = usize;

const SCROLLBACK: usize = 1000;

/// What every new pane needs from the app.
#[derive(Clone)]
pub struct PaneContext {
    pub events: Sender<AppEvent>,
    pub colors: Arc<TerminalColors>,
}

/// A single shell running in a pseudo-terminal, together with the emulated
/// screen state built from its output.
pub struct Pane {
    pub parser: Arc<Mutex<vt100::Parser<Responder>>>,
    /// Set while the pane is in normal or visual mode.
    pub copy: Option<CopyMode>,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
}

impl Pane {
    pub fn spawn(id: PaneId, rows: u16, cols: u16, ctx: PaneContext) -> Result<Self> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let pair = native_pty_system()
            .openpty(pty_size(rows, cols))
            .context("failed to open pty")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        cmd.env("BARBELO", "1");
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn shell")?;
        drop(pair.slave);

        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            nz(rows),
            nz(cols),
            SCROLLBACK,
            Responder::new(ctx.colors),
        )));
        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        let reader_parser = Arc::clone(&parser);
        let reader_writer = Arc::clone(&writer);
        let events = ctx.events;
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let replies = {
                            let mut parser = reader_parser.lock().unwrap();
                            parser.process(&buf[..n]);
                            std::mem::take(&mut parser.callbacks_mut().replies)
                        };
                        // Answer terminal queries straight away; programs
                        // often block until they get a reply.
                        if !replies.is_empty() {
                            let mut writer = reader_writer.lock().unwrap();
                            let _ = writer.write_all(&replies);
                            let _ = writer.flush();
                        }
                        if events.send(AppEvent::PaneOutput).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = events.send(AppEvent::PaneExited(id));
        });

        Ok(Self {
            parser,
            copy: None,
            master: pair.master,
            writer,
            child,
            size: (rows, cols),
        })
    }

    pub fn enter_copy_mode(&mut self) {
        if self.copy.is_none() {
            let mut parser = self.parser.lock().unwrap();
            self.copy = Some(CopyMode::new(parser.screen_mut()));
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        // The snapshot no longer matches the pane, so drop back to insert mode.
        self.copy = None;
        let _ = self.master.resize(pty_size(rows, cols));
        self.parser.lock().unwrap().set_size(nz(rows), nz(cols));
    }

    /// Name of the process currently in the foreground of this pane.
    pub fn process_name(&self) -> Option<String> {
        let pid = self.master.process_group_leader()?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(comm.trim().to_string())
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}
