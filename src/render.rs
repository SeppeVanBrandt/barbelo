use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};

use crate::app::App;
use crate::copy::{CopyMode, Line, Mode};
use crate::hints::{HintMode, Overlay};
use crate::layout::{Border, Rect};

const ACTIVE_BORDER: Color = Color::Green;
const INACTIVE_BORDER: Color = Color::DarkGrey;
const STATUS_FG: Color = Color::Black;
const STATUS_BG: Color = Color::Green;
const NORMAL_BG: Color = Color::Blue;
const VISUAL_BG: Color = Color::Magenta;
const HINTS_BG: Color = Color::Yellow;
/// Hint labels: black on bright yellow.
const LABEL_STYLE: Style = Style {
    fg: vt100::Color::Idx(0),
    bg: vt100::Color::Idx(11),
    bold: true,
    italic: false,
    underline: false,
    inverse: false,
};
/// Matched text: bright yellow.
const MATCH_STYLE: Style = Style {
    fg: vt100::Color::Idx(11),
    bg: vt100::Color::Default,
    bold: true,
    italic: false,
    underline: false,
    inverse: false,
};
/// Everything else fades into the background.
const DIM_STYLE: Style = Style {
    fg: vt100::Color::Idx(8),
    bg: vt100::Color::Default,
    bold: false,
    italic: false,
    underline: false,
    inverse: false,
};

pub fn draw(app: &App, out: &mut impl Write) -> io::Result<()> {
    queue!(out, BeginSynchronizedUpdate, Hide)?;

    let area = app.pane_area();
    let window = app.active_window();
    let layout = window.layout(area);

    let mut cursor = None;
    for (id, rect) in &layout.panes {
        let Some(pane) = window.panes.get(id) else {
            continue;
        };
        let active = *id == window.active;
        if let Some(hints) = &pane.hints {
            draw_hints(out, hints, *rect)?;
            continue;
        }
        if let Some(copy) = &pane.copy {
            draw_copy(out, copy, *rect)?;
            let row = copy.cursor.line.checked_sub(copy.view_top());
            if let Some(row) = row.filter(|r| *r < rect.h as usize)
                && active
                && copy.cursor.col < rect.w
            {
                cursor = Some((rect.x + copy.cursor.col, rect.y + row as u16));
            }
            continue;
        }
        let parser = pane.parser.lock().unwrap();
        let screen = parser.screen();
        draw_screen(out, screen, *rect)?;
        if active && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            if row < rect.h && col < rect.w {
                cursor = Some((rect.x + col, rect.y + row));
            }
        }
    }

    let active_rect = layout.rect_of(window.active);
    draw_borders(out, &layout.borders, area, active_rect)?;
    draw_status(out, app)?;

    let shape = match app.mode() {
        None => SetCursorStyle::DefaultUserShape,
        Some(_) => SetCursorStyle::SteadyBlock,
    };
    queue!(out, shape)?;
    if let Some((x, y)) = cursor {
        queue!(out, MoveTo(x, y), Show)?;
    }
    queue!(out, EndSynchronizedUpdate)?;
    out.flush()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Style {
    pub fn of(cell: &vt100::Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn apply(&self, out: &mut impl Write) -> io::Result<()> {
        queue!(
            out,
            SetAttribute(Attribute::Reset),
            SetForegroundColor(color(self.fg)),
            SetBackgroundColor(color(self.bg)),
        )?;
        if self.bold {
            queue!(out, SetAttribute(Attribute::Bold))?;
        }
        if self.italic {
            queue!(out, SetAttribute(Attribute::Italic))?;
        }
        if self.underline {
            queue!(out, SetAttribute(Attribute::Underlined))?;
        }
        if self.inverse {
            queue!(out, SetAttribute(Attribute::Reverse))?;
        }
        Ok(())
    }
}

fn color(c: vt100::Color) -> Color {
    match c {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::AnsiValue(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

/// What to draw in one terminal cell.
struct CellView<'a> {
    text: &'a str,
    style: Style,
    wide: bool,
    continuation: bool,
}

fn draw_screen(out: &mut impl Write, screen: &vt100::Screen, rect: Rect) -> io::Result<()> {
    draw_cells(out, rect, |row, col| {
        screen.cell(row, col).map(|cell| CellView {
            text: cell.contents(),
            style: Style::of(cell),
            wide: cell.is_wide(),
            continuation: cell.is_wide_continuation(),
        })
    })
}

fn draw_copy(out: &mut impl Write, copy: &CopyMode, rect: Rect) -> io::Result<()> {
    let top = copy.view_top();
    draw_cells(out, rect, |row, col| {
        let line = top + row as usize;
        let cell = copy.line(line)?.cells.get(col as usize)?;
        let mut style = cell.style;
        if copy.is_selected(line, col) {
            style.inverse = !style.inverse;
        }
        Some(CellView {
            text: &cell.text,
            style,
            wide: cell.wide,
            continuation: cell.continuation,
        })
    })
}

fn draw_hints(out: &mut impl Write, hints: &HintMode, rect: Rect) -> io::Result<()> {
    draw_cells(out, rect, |row, col| {
        let cell = hints.lines.get(row as usize).and_then(|l: &Line| l.cells.get(col as usize))?;
        let overlay = hints.overlay(row, col);
        // A label is one column wide, so it can split a wide character; blank
        // out the half that is left over.
        let after_label =
            col > 0 && matches!(hints.overlay(row, col - 1), Overlay::Label(_)) && cell.continuation;
        let (text, style) = match overlay {
            Overlay::Label(c) => (c, LABEL_STYLE),
            _ if after_label => (" ", DIM_STYLE),
            Overlay::Match => (cell.text.as_str(), MATCH_STYLE),
            Overlay::Plain => (cell.text.as_str(), DIM_STYLE),
        };
        let replaced = matches!(overlay, Overlay::Label(_)) || after_label;
        Some(CellView {
            text,
            style,
            wide: cell.wide && !replaced,
            continuation: cell.continuation && !replaced,
        })
    })
}

fn draw_cells<'a>(
    out: &mut impl Write,
    rect: Rect,
    cell_at: impl Fn(u16, u16) -> Option<CellView<'a>>,
) -> io::Result<()> {
    queue!(out, SetAttribute(Attribute::Reset))?;
    let mut current: Option<Style> = None;
    for row in 0..rect.h {
        queue!(out, MoveTo(rect.x, rect.y + row))?;
        let mut col = 0;
        while col < rect.w {
            let Some(cell) = cell_at(row, col) else {
                if current.take().is_some() {
                    queue!(out, SetAttribute(Attribute::Reset))?;
                }
                queue!(out, Print(' '))?;
                col += 1;
                continue;
            };
            if cell.continuation {
                col += 1;
                continue;
            }
            if current != Some(cell.style) {
                cell.style.apply(out)?;
                current = Some(cell.style);
            }
            // A wide character in the last column would spill over the border.
            if cell.text.is_empty() || (cell.wide && col + 1 >= rect.w) {
                queue!(out, Print(' '))?;
            } else {
                queue!(out, Print(cell.text))?;
            }
            col += if cell.wide { 2 } else { 1 };
        }
    }
    queue!(out, SetAttribute(Attribute::Reset))
}

fn draw_borders(
    out: &mut impl Write,
    borders: &[Border],
    area: Rect,
    active: Option<Rect>,
) -> io::Result<()> {
    // Rasterise the borders first so junctions can pick the right glyph.
    let (w, h) = (area.w as usize, area.h as usize);
    let mut grid = vec![false; w * h];
    for border in borders {
        match *border {
            Border::Vertical { x, y, len } => {
                for dy in 0..len {
                    grid[(y + dy) as usize * w + x as usize] = true;
                }
            }
            Border::Horizontal { x, y, len } => {
                for dx in 0..len {
                    grid[y as usize * w + (x + dx) as usize] = true;
                }
            }
        }
    }
    let at = |x: isize, y: isize| {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && grid[y as usize * w + x as usize]
    };

    for y in 0..h {
        for x in 0..w {
            if !grid[y * w + x] {
                continue;
            }
            let (xi, yi) = (x as isize, y as isize);
            let glyph = match (at(xi, yi - 1), at(xi, yi + 1), at(xi - 1, yi), at(xi + 1, yi)) {
                (true, true, true, true) => '┼',
                (true, true, true, false) => '┤',
                (true, true, false, true) => '├',
                (true, false, true, true) => '┴',
                (false, true, true, true) => '┬',
                (true, false, true, false) => '┘',
                (true, false, false, true) => '└',
                (false, true, true, false) => '┐',
                (false, true, false, true) => '┌',
                (_, _, false, false) => '│',
                _ => '─',
            };
            let (x, y) = (x as u16, y as u16);
            let touches_active = active.is_some_and(|r| {
                x + 1 >= r.x && x <= r.right() && y + 1 >= r.y && y <= r.bottom()
            });
            let fg = if touches_active { ACTIVE_BORDER } else { INACTIVE_BORDER };
            queue!(out, MoveTo(area.x + x, area.y + y), SetForegroundColor(fg), Print(glyph))?;
        }
    }
    queue!(out, SetAttribute(Attribute::Reset))
}

struct Segment {
    text: String,
    bold: bool,
    fg: Color,
    bg: Color,
}

impl Segment {
    fn new(text: impl Into<String>, bold: bool) -> Self {
        Self {
            text: text.into(),
            bold,
            fg: STATUS_FG,
            bg: STATUS_BG,
        }
    }
}

fn draw_status(out: &mut impl Write, app: &App) -> io::Result<()> {
    let (cols, rows) = app.size;
    if rows == 0 {
        return Ok(());
    }

    let mode = match app.mode() {
        _ if app.hinting() => Segment {
            bg: HINTS_BG,
            ..Segment::new(" HINTS  ", true)
        },
        None => Segment::new(" INSERT ", true),
        Some(Mode::Normal) => Segment {
            bg: NORMAL_BG,
            fg: Color::White,
            ..Segment::new(" NORMAL ", true)
        },
        Some(Mode::Visual | Mode::VisualLine) => Segment {
            bg: VISUAL_BG,
            fg: Color::White,
            ..Segment::new(if app.mode() == Some(Mode::Visual) { " VISUAL " } else { " V-LINE " }, true)
        },
    };
    let mut segments = vec![mode, Segment::new(" [barbelo] ", false)];
    for (i, window) in app.windows.iter().enumerate() {
        let active = i == app.active;
        let flag = if active { "*" } else { " " };
        segments.push(Segment::new(format!("{}:{}{flag} ", i + 1, window.name()), active));
    }

    let right = match (&app.message, app.prefix_pending, app.copy_position()) {
        (Some(msg), _, _) => format!(" {msg} "),
        (None, true, _) => " PREFIX ".to_string(),
        (None, false, Some((line, total))) => format!(" {line}/{total} "),
        (None, false, None) => String::new(),
    };
    let width = cols as usize;
    let left_len: usize = segments.iter().map(|s| s.text.chars().count()).sum();
    let gap = width.saturating_sub(left_len + right.chars().count());
    segments.push(Segment::new(" ".repeat(gap), false));
    segments.push(Segment::new(right, true));

    queue!(out, MoveTo(0, rows - 1), SetAttribute(Attribute::Reset))?;
    let mut remaining = width;
    for segment in segments {
        let text: String = segment.text.chars().take(remaining).collect();
        remaining -= text.chars().count();
        let attr = if segment.bold { Attribute::Bold } else { Attribute::NormalIntensity };
        queue!(
            out,
            SetForegroundColor(segment.fg),
            SetBackgroundColor(segment.bg),
            SetAttribute(attr),
            Print(text)
        )?;
    }
    queue!(out, SetAttribute(Attribute::Reset))
}
