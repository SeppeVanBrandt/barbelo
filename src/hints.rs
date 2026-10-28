//! Hint mode, in the spirit of tmux-fingers: label interesting strings on
//! screen (URLs, paths, hashes, ...) and copy one by typing its label.

use std::collections::HashMap;
use std::sync::LazyLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::Regex;

use crate::copy::Line;

/// Patterns in priority order: when matches overlap, the earlier pattern
/// wins. A capture group named `m` narrows the match to that group.
static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // URL
        r#"(?:https?|ftp|file|ssh|git)://[^\s<>"'`]+"#,
        // File from `git status`
        r"(?:modified|deleted|new file|renamed|copied|both modified):\s+(?<m>\S+)",
        // UUID
        r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        // IPv4 address, optionally with a port
        r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?\b",
        // Path, optionally followed by :line:column
        r"(?:~|\.{1,2}|[\w.\-@+]+)?(?:/[\w.\-@+]+)+/?(?::\d+){0,2}",
        // Hex number
        r"\b0x[0-9a-fA-F]+\b",
        // Git SHA and other hex hashes
        r"\b[0-9a-f]{7,40}\b",
        // Long number
        r"\b\d{4,}\b",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid pattern"))
    .collect()
});

pub struct Match {
    pub text: String,
    pub hint: String,
    /// Screen cells covered by the match, as (row, col), in order.
    pub cells: Vec<(u16, u16)>,
}

pub enum Action {
    None,
    Exit,
    Copy(String),
    CopyAndPaste(String),
}

/// What a screen cell shows while hints are up.
#[derive(Clone, Copy)]
pub enum Overlay<'a> {
    /// A character of a hint label.
    Label(&'a str),
    /// Part of a match whose hint still fits what has been typed.
    Match,
    /// Part of a match ruled out by what has been typed, or not a match.
    Plain,
}

pub struct HintMode {
    /// Frozen copy of the visible rows.
    pub lines: Vec<Line>,
    pub matches: Vec<Match>,
    typed: String,
    /// Match index and position within the match, per cell.
    grid: HashMap<(u16, u16), (usize, usize)>,
}

impl HintMode {
    /// Labels the matches in `lines` with hints made from `alphabet`.
    pub fn new(lines: Vec<Line>, alphabet: &[u8]) -> Self {
        let mut matches = find_matches(&lines);
        assign_hints(&mut matches, alphabet);
        let mut grid = HashMap::new();
        for (i, m) in matches.iter().enumerate() {
            for (k, cell) in m.cells.iter().enumerate() {
                grid.insert(*cell, (i, k));
            }
        }
        Self {
            lines,
            matches,
            typed: String::new(),
            grid,
        }
    }

    pub fn overlay(&self, row: u16, col: u16) -> Overlay<'_> {
        let Some(&(i, k)) = self.grid.get(&(row, col)) else {
            return Overlay::Plain;
        };
        let m = &self.matches[i];
        if !m.hint.starts_with(&self.typed) {
            return Overlay::Plain;
        }
        match m.hint.get(k..k + 1) {
            Some(c) => Overlay::Label(c),
            None => Overlay::Match,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => Action::Exit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Exit,
            KeyCode::Backspace => {
                self.typed.pop();
                Action::None
            }
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                self.typed.push(c.to_ascii_lowercase());
                if let Some(m) = self.matches.iter().find(|m| m.hint == self.typed) {
                    let text = m.text.clone();
                    return if c.is_ascii_uppercase() {
                        Action::CopyAndPaste(text)
                    } else {
                        Action::Copy(text)
                    };
                }
                if !self.matches.iter().any(|m| m.hint.starts_with(&self.typed)) {
                    self.typed.pop();
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

/// Text of a logical line (soft-wrapped rows joined) and the screen cell of
/// every byte in it.
fn logical_lines(lines: &[Line]) -> Vec<(String, Vec<(u16, u16)>)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut cells = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        for (col, cell) in line.cells.iter().enumerate() {
            if cell.continuation {
                continue;
            }
            let s = if cell.text.is_empty() { " " } else { &cell.text };
            text.push_str(s);
            cells.extend(std::iter::repeat_n((row as u16, col as u16), s.len()));
        }
        if !line.wrapped {
            out.push((std::mem::take(&mut text), std::mem::take(&mut cells)));
        }
    }
    if !text.is_empty() {
        out.push((text, cells));
    }
    out
}

fn find_matches(lines: &[Line]) -> Vec<Match> {
    let mut matches = Vec::new();
    for (text, cells) in logical_lines(lines) {
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for pattern in PATTERNS.iter() {
            for caps in pattern.captures_iter(&text) {
                let m = caps.name("m").or_else(|| caps.get(0)).expect("group 0 exists");
                let s = trim_trailing(m.as_str());
                let (start, end) = (m.start(), m.start() + s.len());
                if s.len() < 2 || taken.iter().any(|&(a, b)| start < b && a < end) {
                    continue;
                }
                taken.push((start, end));
                let mut covered: Vec<(u16, u16)> = cells[start..end].to_vec();
                covered.dedup();
                matches.push(Match {
                    text: s.to_string(),
                    hint: String::new(),
                    cells: covered,
                });
            }
        }
    }
    matches.sort_by_key(|m| m.cells[0]);
    matches
}

/// Strips punctuation that usually ends a sentence rather than the match.
fn trim_trailing(s: &str) -> &str {
    let mut s = s.trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']);
    // Keep a closing bracket only if the match also opened one.
    while let Some(stripped) = s.strip_suffix([')', ']', '}', '>']) {
        let close = s.chars().last().expect("non-empty");
        let open = match close {
            ')' => '(',
            ']' => '[',
            '}' => '{',
            _ => '<',
        };
        if s.matches(open).count() >= s.matches(close).count() {
            break;
        }
        s = stripped.trim_end_matches(['.', ',', ';', ':']);
    }
    s
}

/// Gives every distinct text a hint. Matches nearest the bottom of the screen
/// get the shortest hints, and identical texts share one.
fn assign_hints(matches: &mut [Match], alphabet: &[u8]) {
    let mut texts: Vec<&str> = Vec::new();
    for m in matches.iter().rev() {
        if !texts.contains(&m.text.as_str()) {
            texts.push(&m.text);
        }
    }
    let hints = generate_hints(texts.len(), alphabet);
    let by_text: HashMap<String, String> = texts
        .into_iter()
        .zip(hints)
        .map(|(t, h)| (t.to_string(), h))
        .collect();
    for m in matches.iter_mut() {
        m.hint = by_text.get(&m.text).cloned().unwrap_or_default();
    }
}

/// Prefix-free hints: single letters first, then two-letter hints starting
/// with letters taken off the end of the alphabet.
fn generate_hints(n: usize, alphabet: &[u8]) -> Vec<String> {
    let len = alphabet.len();
    let prefixes = if n <= len { 0 } else { (n - len).div_ceil(len - 1).min(len) };
    let singles = &alphabet[..len - prefixes];
    let mut hints: Vec<String> = singles.iter().map(|&c| (c as char).to_string()).collect();
    for &p in &alphabet[len - prefixes..] {
        for &c in alphabet {
            hints.push(format!("{}{}", p as char, c as char));
        }
    }
    hints.truncate(n);
    hints
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy::Cell;

    const ALPHABET: &[u8] = b"asdfqwerzxcvjklmiuopghtybn";

    fn lines(rows: &[&str], cols: usize) -> Vec<Line> {
        rows.iter()
            .map(|row| {
                let mut cells: Vec<Cell> = row
                    .chars()
                    .map(|c| Cell {
                        text: c.to_string(),
                        style: Default::default(),
                        wide: false,
                        continuation: false,
                    })
                    .collect();
                cells.resize(cols, blank());
                Line {
                    wrapped: row.chars().count() >= cols,
                    cells,
                }
            })
            .collect()
    }

    fn blank() -> Cell {
        Cell {
            text: String::new(),
            style: Default::default(),
            wide: false,
            continuation: false,
        }
    }

    fn texts(rows: &[&str]) -> Vec<String> {
        HintMode::new(lines(rows, 80), ALPHABET).matches.into_iter().map(|m| m.text).collect()
    }

    #[test]
    fn finds_common_patterns() {
        assert_eq!(
            texts(&["see https://example.com/a?b=1. or (http://x.org/y)"]),
            ["https://example.com/a?b=1", "http://x.org/y"]
        );
        assert_eq!(texts(&["error at src/app.rs:42:7: oops"]), ["src/app.rs:42:7"]);
        assert_eq!(texts(&["b779539 Answer terminal queries"]), ["b779539"]);
        assert_eq!(texts(&["listening on 127.0.0.1:8080"]), ["127.0.0.1:8080"]);
        assert_eq!(texts(&["\tmodified:   Cargo.toml"]), ["Cargo.toml"]);
        assert_eq!(
            texts(&["id 123e4567-e89b-12d3-a456-426614174000 pid 31337"]),
            ["123e4567-e89b-12d3-a456-426614174000", "31337"]
        );
        assert!(texts(&["nothing to see here"]).is_empty());
    }

    #[test]
    fn joins_soft_wrapped_rows() {
        let mode = HintMode::new(lines(&["go to https://exa", "mple.com now"], 17), ALPHABET);
        assert_eq!(mode.matches.len(), 1);
        assert_eq!(mode.matches[0].text, "https://example.com");
        assert_eq!(mode.matches[0].cells.last(), Some(&(1, 7)));
    }

    #[test]
    fn hints_are_prefix_free_and_shared() {
        let hints = generate_hints(60, ALPHABET);
        assert_eq!(hints.len(), 60);
        for a in &hints {
            for b in &hints {
                assert!(a == b || !b.starts_with(a.as_str()), "{a} is a prefix of {b}");
            }
        }
        assert_eq!(generate_hints(3, b"jk"), ["j", "kj", "kk"]);
        let mode = HintMode::new(lines(&["/tmp/a /tmp/b", "/tmp/a"], 80), ALPHABET);
        let hints: Vec<&str> = mode.matches.iter().map(|m| m.hint.as_str()).collect();
        // The bottom match gets the first hint; duplicates share it.
        assert_eq!(hints, ["a", "s", "a"]);
    }

    #[test]
    fn typing_a_hint_copies() {
        let mut mode = HintMode::new(lines(&["/tmp/a /tmp/b"], 80), ALPHABET);
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        assert!(matches!(mode.handle_key(key('S')), Action::CopyAndPaste(t) if t == "/tmp/a"));
        assert!(matches!(mode.handle_key(key('z')), Action::None));
    }
}
