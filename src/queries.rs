//! Answers the queries programs send to their terminal. Editors like helix and
//! neovim block at startup until these are answered or time out.

use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long to wait for the outer terminal to answer at startup.
const QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// Default colors of the outer terminal, in the form it reported them
/// (e.g. `rgb:ffff/ffff/ffff`).
#[derive(Debug, Default)]
pub struct TerminalColors {
    pub foreground: Option<String>,
    pub background: Option<String>,
}

impl TerminalColors {
    /// Asks the outer terminal for its colors. Must run in raw mode, before
    /// anything else reads stdin. The trailing device attributes query is
    /// answered by virtually every terminal, so it marks the end of the replies.
    pub fn query() -> Self {
        let mut stdout = std::io::stdout();
        if stdout
            .write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[c")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return Self::default();
        }

        let fd = std::io::stdin().as_raw_fd();
        let deadline = Instant::now() + QUERY_TIMEOUT;
        let mut buf = Vec::new();
        while !has_device_attributes(&buf) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let mut pollfd = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: `pollfd` is a valid, initialised pollfd for the duration of the call.
            let ready = unsafe { libc::poll(&mut pollfd, 1, remaining.as_millis() as libc::c_int) };
            if ready <= 0 {
                break;
            }
            let mut chunk = [0u8; 256];
            // SAFETY: `chunk` is a writable buffer of the given length.
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n as usize]);
        }

        Self {
            foreground: osc_reply(&buf, b"10"),
            background: osc_reply(&buf, b"11"),
        }
    }
}

/// Whether `buf` contains a primary device attributes reply, `ESC [ ? ... c`.
fn has_device_attributes(buf: &[u8]) -> bool {
    buf.windows(3)
        .enumerate()
        .filter(|(_, w)| *w == b"\x1b[?")
        .any(|(i, _)| {
            buf[i + 3..]
                .iter()
                .find(|b| !(b.is_ascii_digit() || **b == b';'))
                .is_some_and(|b| *b == b'c')
        })
}

/// Extracts the value of an OSC reply such as `ESC ] 11 ; rgb:... BEL`.
fn osc_reply(buf: &[u8], code: &[u8]) -> Option<String> {
    let prefix = [b"\x1b]".as_slice(), code, b";"].concat();
    let start = buf.windows(prefix.len()).position(|w| w == prefix)? + prefix.len();
    let rest = &buf[start..];
    let end = rest.iter().position(|b| *b == 0x07 || *b == 0x1b)?;
    let value = std::str::from_utf8(&rest[..end]).ok()?;
    (!value.is_empty() && value != "?").then(|| value.to_string())
}

/// Parser callbacks that collect replies to be written back to the pane.
pub struct Responder {
    pub replies: Vec<u8>,
    colors: Arc<TerminalColors>,
}

impl Responder {
    pub fn new(colors: Arc<TerminalColors>) -> Self {
        Self {
            replies: Vec::new(),
            colors,
        }
    }

    fn reply(&mut self, s: &str) {
        self.replies.extend_from_slice(s.as_bytes());
    }
}

impl vt100::Callbacks for Responder {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, i2, c) {
            // Primary device attributes: a VT220 with ANSI color.
            (None, None, 'c') if first == 0 => self.reply("\x1b[?62;22c"),
            // Secondary device attributes.
            (Some(b'>'), None, 'c') if first == 0 => self.reply("\x1b[>0;10;1c"),
            // Device status report.
            (None, None, 'n') if first == 5 => self.reply("\x1b[0n"),
            (None, None, 'n') if first == 6 => {
                let (row, col) = screen.cursor_position();
                self.reply(&format!("\x1b[{};{}R", row + 1, col + 1));
            }
            // Request DEC private mode: 1 = set, 2 = reset, 0 = not recognised.
            (Some(b'?'), Some(b'$'), 'p') => {
                let state = match first {
                    1 => Some(screen.application_cursor()),
                    25 => Some(!screen.hide_cursor()),
                    1049 => Some(screen.alternate_screen()),
                    2004 => Some(screen.bracketed_paste()),
                    _ => None,
                };
                let state = state.map_or(0, |set| if set { 1 } else { 2 });
                self.reply(&format!("\x1b[?{first};{state}$y"));
            }
            // Request ANSI mode: none are recognised.
            (None, Some(b'$'), 'p') => self.reply(&format!("\x1b[{first};0$y")),
            // Terminal name and version (XTVERSION).
            (Some(b'>'), None, 'q') => self.reply(concat!(
                "\x1bP>|barbelo ",
                env!("CARGO_PKG_VERSION"),
                "\x1b\\"
            )),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let color = match params {
            [b"10", b"?"] => self.colors.foreground.as_deref().map(|c| ("10", c)),
            [b"11", b"?"] => self.colors.background.as_deref().map(|c| ("11", c)),
            _ => None,
        };
        if let Some((code, value)) = color {
            let reply = format!("\x1b]{code};{value}\x1b\\");
            self.reply(&reply);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    fn replies(input: &[u8]) -> String {
        let colors = Arc::new(TerminalColors {
            foreground: Some("rgb:ffff/ffff/ffff".into()),
            background: Some("rgb:1414/1616/1b1b".into()),
        });
        let mut parser = vt100::Parser::new_with_callbacks(
            NonZeroU16::new(24).unwrap(),
            NonZeroU16::new(80).unwrap(),
            0,
            Responder::new(colors),
        );
        parser.process(input);
        String::from_utf8(std::mem::take(&mut parser.callbacks_mut().replies)).unwrap()
    }

    #[test]
    fn answers_startup_queries() {
        assert_eq!(replies(b"\x1b[c"), "\x1b[?62;22c");
        assert_eq!(replies(b"\x1b[5n"), "\x1b[0n");
        assert_eq!(replies(b"abc\x1b[6n"), "\x1b[1;4R");
        assert_eq!(replies(b"\x1b]11;?\x07"), "\x1b]11;rgb:1414/1616/1b1b\x1b\\");
        assert_eq!(replies(b"\x1b[?2004h\x1b[?2004$p"), "\x1b[?2004;1$y");
        assert_eq!(replies(b"\x1b[?2026$p"), "\x1b[?2026;0$y");
        // Kitty keyboard protocol is unsupported, so stay silent.
        assert_eq!(replies(b"\x1b[?u"), "");
    }

    #[test]
    fn parses_outer_terminal_replies() {
        let buf = b"\x1b]10;rgb:aaaa/bbbb/cccc\x1b\\\x1b]11;rgb:0000/0000/0000\x07\x1b[?62;22c";
        assert!(has_device_attributes(buf));
        assert_eq!(osc_reply(buf, b"10").as_deref(), Some("rgb:aaaa/bbbb/cccc"));
        assert_eq!(osc_reply(buf, b"11").as_deref(), Some("rgb:0000/0000/0000"));
        assert!(!has_device_attributes(b"\x1b[?62;22"));
    }
}
