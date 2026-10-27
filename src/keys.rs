use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Translates a key press into the bytes a terminal would send for it.
pub fn to_bytes(key: KeyEvent, application_cursor: bool) -> Vec<u8> {
    let mods = key.modifiers;
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);

    let mut out = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if alt {
                out.push(0x1b);
            }
            if ctrl {
                if let Some(b) = ctrl_byte(c) {
                    out.push(b);
                    return out;
                }
            }
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        KeyCode::Enter => prefixed(&mut out, alt, b"\r"),
        KeyCode::Tab => prefixed(&mut out, alt, b"\t"),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => prefixed(&mut out, alt, if ctrl { b"\x08" } else { b"\x7f" }),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => cursor_key(&mut out, b'A', mods, application_cursor),
        KeyCode::Down => cursor_key(&mut out, b'B', mods, application_cursor),
        KeyCode::Right => cursor_key(&mut out, b'C', mods, application_cursor),
        KeyCode::Left => cursor_key(&mut out, b'D', mods, application_cursor),
        KeyCode::Home => cursor_key(&mut out, b'H', mods, application_cursor),
        KeyCode::End => cursor_key(&mut out, b'F', mods, application_cursor),
        KeyCode::Insert => tilde_key(&mut out, 2, mods),
        KeyCode::Delete => tilde_key(&mut out, 3, mods),
        KeyCode::PageUp => tilde_key(&mut out, 5, mods),
        KeyCode::PageDown => tilde_key(&mut out, 6, mods),
        KeyCode::F(n @ 1..=4) => {
            let c = b'P' + (n - 1);
            match modifier_param(mods) {
                Some(m) => out.extend_from_slice(format!("\x1b[1;{m}{}", c as char).as_bytes()),
                None => out.extend_from_slice(&[0x1b, b'O', c]),
            }
        }
        KeyCode::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize];
            tilde_key(&mut out, code, mods);
        }
        _ => {}
    }
    out
}

fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '/' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn prefixed(out: &mut Vec<u8>, alt: bool, bytes: &[u8]) {
    if alt {
        out.push(0x1b);
    }
    out.extend_from_slice(bytes);
}

/// The xterm modifier parameter (`1 + shift + 2*alt + 4*ctrl`), if any.
fn modifier_param(mods: KeyModifiers) -> Option<u8> {
    let mut m = 0;
    if mods.contains(KeyModifiers::SHIFT) {
        m |= 1;
    }
    if mods.contains(KeyModifiers::ALT) {
        m |= 2;
    }
    if mods.contains(KeyModifiers::CONTROL) {
        m |= 4;
    }
    (m != 0).then_some(m + 1)
}

fn cursor_key(out: &mut Vec<u8>, c: u8, mods: KeyModifiers, application_cursor: bool) {
    match modifier_param(mods) {
        Some(m) => out.extend_from_slice(format!("\x1b[1;{m}{}", c as char).as_bytes()),
        None if application_cursor => out.extend_from_slice(&[0x1b, b'O', c]),
        None => out.extend_from_slice(&[0x1b, b'[', c]),
    }
}

fn tilde_key(out: &mut Vec<u8>, code: u8, mods: KeyModifiers) {
    match modifier_param(mods) {
        Some(m) => out.extend_from_slice(format!("\x1b[{code};{m}~").as_bytes()),
        None => out.extend_from_slice(format!("\x1b[{code}~").as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn basic_keys() {
        assert_eq!(to_bytes(key(KeyCode::Char('a'), KeyModifiers::NONE), false), b"a");
        assert_eq!(to_bytes(key(KeyCode::Char('c'), KeyModifiers::CONTROL), false), [3]);
        assert_eq!(to_bytes(key(KeyCode::Char('x'), KeyModifiers::ALT), false), b"\x1bx");
        assert_eq!(to_bytes(key(KeyCode::Up, KeyModifiers::NONE), false), b"\x1b[A");
        assert_eq!(to_bytes(key(KeyCode::Up, KeyModifiers::NONE), true), b"\x1bOA");
        assert_eq!(to_bytes(key(KeyCode::Left, KeyModifiers::CONTROL), false), b"\x1b[1;5D");
        assert_eq!(to_bytes(key(KeyCode::F(5), KeyModifiers::NONE), false), b"\x1b[15~");
    }
}
