use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::render::Style;

/// Time constant of the scroll animation: larger is slower.
const SCROLL_TAU: f32 = 0.035;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Visual,
    VisualLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: u16,
}

pub enum Action {
    None,
    Exit,
    Yank(String),
}

#[derive(Clone)]
pub struct Cell {
    pub text: String,
    pub style: Style,
    pub wide: bool,
    pub continuation: bool,
}

#[derive(Clone)]
pub struct Line {
    pub cells: Vec<Cell>,
    pub wrapped: bool,
}

/// A frozen snapshot of a pane's scrollback and screen, navigated with
/// vi-style keys. The pane keeps running underneath; its new output shows up
/// again once copy mode is left.
pub struct CopyMode {
    lines: Vec<Line>,
    rows: u16,
    cols: u16,
    pub mode: Mode,
    pub cursor: Pos,
    anchor: Pos,
    /// Column the cursor tries to return to when moving vertically.
    want_col: u16,
    /// Line at the top of the view once scrolling finishes.
    top: usize,
    /// Line at the top of the view right now, mid-animation.
    shown: f32,
    count: Option<usize>,
    pending_g: bool,
}

impl CopyMode {
    pub fn new(screen: &mut vt100::Screen) -> Self {
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());

        // Page through the scrollback a screenful at a time, oldest first.
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        let total = history + rows as usize;
        let mut lines = Vec::with_capacity(total);
        while lines.len() < total {
            let offset = history.saturating_sub(lines.len());
            screen.set_scrollback(offset);
            let first_row = lines.len() - (history - offset);
            for row in first_row as u16..rows {
                lines.push(read_line(screen, row, cols));
            }
        }
        screen.set_scrollback(0);

        let (row, col) = screen.cursor_position();
        let cursor = Pos {
            line: history + row as usize,
            col,
        };
        Self {
            lines,
            rows,
            cols,
            mode: Mode::Normal,
            cursor,
            anchor: cursor,
            want_col: col,
            top: history,
            shown: history as f32,
            count: None,
            pending_g: false,
        }
    }

    pub fn line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }

    /// The rows currently in view.
    pub fn visible_lines(&self) -> Vec<Line> {
        let top = self.view_top();
        let end = (top + self.rows as usize).min(self.lines.len());
        self.lines[top..end].to_vec()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Line currently at the top of the view.
    pub fn view_top(&self) -> usize {
        self.shown.round() as usize
    }

    pub fn animating(&self) -> bool {
        self.shown != self.top as f32
    }

    /// Advances the scroll animation by `dt`.
    pub fn tick(&mut self, dt: Duration) {
        let target = self.top as f32;
        let diff = target - self.shown;
        if diff.abs() <= 1.0 {
            self.shown = target;
            return;
        }
        let k = 1.0 - (-dt.as_secs_f32() / SCROLL_TAU).exp();
        let step = (diff.abs() * k).max(1.0).min(diff.abs());
        self.shown += step * diff.signum();
    }

    pub fn is_selected(&self, line: usize, col: u16) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        (start..=end).contains(&Pos { line, col })
    }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let (a, b) = if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        match self.mode {
            Mode::Normal => None,
            Mode::Visual => Some((a, b)),
            Mode::VisualLine => Some((
                Pos { line: a.line, col: 0 },
                Pos {
                    line: b.line,
                    col: self.cols - 1,
                },
            )),
        }
    }

    fn selected_text(&self) -> String {
        let Some((start, end)) = self.selection() else {
            return String::new();
        };
        let mut out = String::new();
        for line in start.line..=end.line {
            let from = if line == start.line { start.col } else { 0 };
            let to = if line == end.line { end.col } else { self.cols - 1 };
            let mut text = String::new();
            for cell in &self.lines[line].cells[from as usize..=to as usize] {
                if !cell.continuation {
                    text.push_str(if cell.text.is_empty() { " " } else { &cell.text });
                }
            }
            // Soft-wrapped lines continue on the next row without a newline.
            if self.lines[line].wrapped && line != end.line && to == self.cols - 1 {
                out.push_str(&text);
            } else {
                out.push_str(text.trim_end());
                if line != end.line {
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let KeyCode::Char(c @ '0'..='9') = key.code
            && !ctrl
            && (c != '0' || self.count.is_some())
        {
            let digit = c as usize - '0' as usize;
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(digit));
            return Action::None;
        }
        let explicit_count = self.count.take();
        let count = explicit_count.unwrap_or(1);
        let pending_g = std::mem::take(&mut self.pending_g);
        let visual = self.mode != Mode::Normal;
        let half_page = (self.rows as usize / 2).max(1);
        let page = (self.rows as usize).saturating_sub(2).max(1);

        match (key.code, ctrl) {
            (KeyCode::Char('g'), false) if pending_g => {
                self.goto_line(explicit_count.map_or(0, |n| n.saturating_sub(1)))
            }
            (KeyCode::Char('g'), false) => self.pending_g = true,
            (KeyCode::Char('G'), false) => {
                let last = self.lines.len() - 1;
                self.goto_line(explicit_count.map_or(last, |n| n.saturating_sub(1)))
            }
            (KeyCode::Char('j') | KeyCode::Down, false) => {
                self.goto_line(self.cursor.line.saturating_add(count))
            }
            (KeyCode::Char('k') | KeyCode::Up, false) => {
                self.goto_line(self.cursor.line.saturating_sub(count))
            }
            (KeyCode::Char('h') | KeyCode::Left, false) => {
                self.set_col(self.cursor.col.saturating_sub(count as u16))
            }
            (KeyCode::Char('l') | KeyCode::Right, false) => {
                let end = self.line_end(self.cursor.line);
                self.set_col(self.cursor.col.saturating_add(count as u16).min(end))
            }
            (KeyCode::Char('0') | KeyCode::Home, false) => self.set_col(0),
            (KeyCode::Char('^'), false) => self.set_col(self.first_non_blank(self.cursor.line)),
            (KeyCode::Char('$') | KeyCode::End, false) => {
                self.set_col(self.line_end(self.cursor.line));
                self.want_col = u16::MAX;
            }
            (KeyCode::Char('w'), false) => self.repeat(count, Self::word_forward),
            (KeyCode::Char('b'), false) => self.repeat(count, Self::word_backward),
            (KeyCode::Char('e'), false) => self.repeat(count, Self::word_end),
            (KeyCode::Char('H'), false) => self.goto_line(self.top),
            (KeyCode::Char('M'), false) => {
                let last = (self.top + self.rows as usize - 1).min(self.lines.len() - 1);
                self.goto_line((self.top + last) / 2)
            }
            (KeyCode::Char('L'), false) => {
                self.goto_line((self.top + self.rows as usize - 1).min(self.lines.len() - 1))
            }
            (KeyCode::Char('d'), true) => self.scroll_page(count * half_page, true),
            (KeyCode::Char('u'), true) => self.scroll_page(count * half_page, false),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => self.scroll_page(count * page, true),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => self.scroll_page(count * page, false),
            (KeyCode::Char('e'), true) => self.scroll_view(count, true),
            (KeyCode::Char('y'), true) => self.scroll_view(count, false),
            (KeyCode::Char('v'), false) => self.toggle_visual(Mode::Visual),
            (KeyCode::Char('V'), false) => self.toggle_visual(Mode::VisualLine),
            (KeyCode::Char('o'), false) if visual => {
                std::mem::swap(&mut self.anchor, &mut self.cursor);
                self.want_col = self.cursor.col;
                self.reveal_cursor();
            }
            (KeyCode::Char('y'), false) | (KeyCode::Enter, _) if visual => {
                let text = self.selected_text();
                self.mode = Mode::Normal;
                return Action::Yank(text);
            }
            (KeyCode::Esc, _) if visual => self.mode = Mode::Normal,
            (KeyCode::Char('i' | 'a' | 'q'), false) => return Action::Exit,
            _ => {}
        }
        Action::None
    }

    fn toggle_visual(&mut self, mode: Mode) {
        if self.mode == mode {
            self.mode = Mode::Normal;
        } else {
            if self.mode == Mode::Normal {
                self.anchor = self.cursor;
            }
            self.mode = mode;
        }
    }

    fn goto_line(&mut self, line: usize) {
        self.cursor.line = line.min(self.lines.len() - 1);
        self.cursor.col = self.want_col.min(self.line_end(self.cursor.line));
        self.reveal_cursor();
    }

    fn set_col(&mut self, col: u16) {
        self.cursor.col = col;
        self.want_col = col;
    }

    fn max_top(&self) -> usize {
        self.lines.len().saturating_sub(self.rows as usize)
    }

    /// Scrolls the view just enough to show the cursor.
    fn reveal_cursor(&mut self) {
        let rows = self.rows as usize;
        if self.cursor.line < self.top {
            self.top = self.cursor.line;
        } else if self.cursor.line >= self.top + rows {
            self.top = self.cursor.line + 1 - rows;
        }
    }

    /// Moves view and cursor together, like vim's ctrl-d / ctrl-u.
    fn scroll_page(&mut self, amount: usize, down: bool) {
        if down {
            self.top = (self.top + amount).min(self.max_top());
            self.goto_line(self.cursor.line + amount);
        } else {
            self.top = self.top.saturating_sub(amount);
            self.goto_line(self.cursor.line.saturating_sub(amount));
        }
    }

    /// Moves only the view, dragging the cursor along if it would leave it.
    fn scroll_view(&mut self, amount: usize, down: bool) {
        self.top = if down {
            (self.top + amount).min(self.max_top())
        } else {
            self.top.saturating_sub(amount)
        };
        let bottom = self.top + self.rows as usize - 1;
        let line = self.cursor.line.clamp(self.top, bottom);
        if line != self.cursor.line {
            self.goto_line(line);
        }
    }

    fn repeat(&mut self, count: usize, motion: fn(&mut Self)) {
        for _ in 0..count {
            motion(self);
        }
        self.want_col = self.cursor.col;
        self.reveal_cursor();
    }

    fn text_at(&self, pos: Pos) -> &str {
        self.lines[pos.line]
            .cells
            .get(pos.col as usize)
            .map_or("", |c| c.text.as_str())
    }

    /// 0 for blanks, 1 for word characters, 2 for punctuation.
    fn class(&self, pos: Pos) -> u8 {
        match self.text_at(pos).chars().next() {
            None => 0,
            Some(c) if c.is_whitespace() => 0,
            Some(c) if c.is_alphanumeric() || c == '_' => 1,
            Some(_) => 2,
        }
    }

    fn next(&self, pos: Pos) -> Option<Pos> {
        if pos.col + 1 < self.cols {
            Some(Pos { col: pos.col + 1, ..pos })
        } else if pos.line + 1 < self.lines.len() {
            Some(Pos { line: pos.line + 1, col: 0 })
        } else {
            None
        }
    }

    fn prev(&self, pos: Pos) -> Option<Pos> {
        if pos.col > 0 {
            Some(Pos { col: pos.col - 1, ..pos })
        } else if pos.line > 0 {
            Some(Pos {
                line: pos.line - 1,
                col: self.cols - 1,
            })
        } else {
            None
        }
    }

    fn word_forward(&mut self) {
        let mut pos = self.cursor;
        let class = self.class(pos);
        while class != 0 && self.class(pos) == class {
            match self.next(pos) {
                Some(p) => pos = p,
                None => return,
            }
        }
        while self.class(pos) == 0 {
            match self.next(pos) {
                Some(p) => pos = p,
                None => return,
            }
        }
        self.cursor = pos;
    }

    fn word_end(&mut self) {
        let Some(mut pos) = self.next(self.cursor) else {
            return;
        };
        while self.class(pos) == 0 {
            match self.next(pos) {
                Some(p) => pos = p,
                None => return,
            }
        }
        let class = self.class(pos);
        while let Some(p) = self.next(pos).filter(|p| self.class(*p) == class) {
            pos = p;
        }
        self.cursor = pos;
    }

    fn word_backward(&mut self) {
        let Some(mut pos) = self.prev(self.cursor) else {
            return;
        };
        while self.class(pos) == 0 {
            match self.prev(pos) {
                Some(p) => pos = p,
                None => return,
            }
        }
        let class = self.class(pos);
        while let Some(p) = self.prev(pos).filter(|p| self.class(*p) == class) {
            pos = p;
        }
        self.cursor = pos;
    }

    /// Column of the last non-blank character on `line`, or 0.
    fn line_end(&self, line: usize) -> u16 {
        self.lines[line]
            .cells
            .iter()
            .rposition(|c| !c.text.trim().is_empty())
            .map_or(0, |i| i as u16)
    }

    fn first_non_blank(&self, line: usize) -> u16 {
        self.lines[line]
            .cells
            .iter()
            .position(|c| !c.text.trim().is_empty())
            .map_or(0, |i| i as u16)
    }
}

/// The rows currently on `screen`.
pub fn visible_lines(screen: &vt100::Screen) -> Vec<Line> {
    let (rows, cols) = screen.size();
    (0..rows.get()).map(|row| read_line(screen, row, cols.get())).collect()
}

fn read_line(screen: &vt100::Screen, row: u16, cols: u16) -> Line {
    let cells = (0..cols)
        .map(|col| match screen.cell(row, col) {
            Some(cell) => Cell {
                text: cell.contents().to_string(),
                style: Style::of(cell),
                wide: cell.is_wide(),
                continuation: cell.is_wide_continuation(),
            },
            None => Cell {
                text: String::new(),
                style: Style::default(),
                wide: false,
                continuation: false,
            },
        })
        .collect();
    Line {
        cells,
        wrapped: screen.row_wrapped(row),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    fn copy_mode(output: &str, rows: u16, cols: u16) -> CopyMode {
        let mut parser = vt100::Parser::new(
            NonZeroU16::new(rows).unwrap(),
            NonZeroU16::new(cols).unwrap(),
            100,
        );
        parser.process(output.as_bytes());
        CopyMode::new(parser.screen_mut())
    }

    fn press(copy: &mut CopyMode, keys: &str) -> Option<String> {
        let mut yanked = None;
        for c in keys.chars() {
            if let Action::Yank(text) = copy.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)) {
                yanked = Some(text);
            }
        }
        yanked
    }

    #[test]
    fn snapshot_includes_scrollback() {
        let output: String = (0..10).map(|i| format!("line {i}\r\n")).collect();
        let copy = copy_mode(&output, 4, 20);
        // 7 lines scrolled off plus the 4 visible rows.
        assert_eq!(copy.len(), 11);
        assert_eq!(copy.line(0).unwrap().cells[5].text, "0");
        assert_eq!(copy.cursor, Pos { line: 10, col: 0 });
        assert_eq!(copy.view_top(), 7);
    }

    #[test]
    fn yank_charwise_and_linewise() {
        let mut copy = copy_mode("hello world\r\nfoo bar\r\n", 5, 20);
        assert_eq!(press(&mut copy, "ggwve").as_deref(), None);
        assert_eq!(press(&mut copy, "y").as_deref(), Some("world"));
        assert_eq!(press(&mut copy, "ggVjy").as_deref(), Some("hello world\nfoo bar"));
        assert_eq!(copy.mode, Mode::Normal);
    }

    #[test]
    fn wrapped_lines_join_without_newline() {
        let mut copy = copy_mode("abcdefghij\r\n", 5, 6);
        assert_eq!(press(&mut copy, "ggVjy").as_deref(), Some("abcdefghij"));
    }

    #[test]
    fn scrolling_animates_towards_target() {
        let output: String = (0..50).map(|i| format!("{i}\r\n")).collect();
        let mut copy = copy_mode(&output, 10, 10);
        press(&mut copy, "gg");
        assert!(copy.animating());
        let mut frames = 0;
        while copy.animating() {
            copy.tick(Duration::from_millis(8));
            frames += 1;
        }
        assert_eq!(copy.view_top(), 0);
        assert!(frames > 3, "should take several frames, took {frames}");
    }
}
