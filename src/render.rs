use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};

use crate::app::App;
use crate::config::Config;
use crate::copy::{CopyMode, Line, Mode};
use crate::hints::{HintMode, Overlay};
use crate::layout::{Border, Rect};

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
            draw_hints(out, hints, *rect, &app.config)?;
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
    draw_borders(out, &layout.borders, area, active_rect, &app.config)?;
    let prompt_cursor = draw_status(out, app)?;
    if prompt_cursor.is_some() {
        cursor = prompt_cursor;
    }

    let shape = match app.mode() {
        _ if app.rename.is_some() => SetCursorStyle::DefaultUserShape,
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

fn draw_hints(out: &mut impl Write, hints: &HintMode, rect: Rect, config: &Config) -> io::Result<()> {
    let colors = &config.hints;
    let label_style = Style {
        fg: colors.label_fg.cell(),
        bg: colors.label_bg.cell(),
        bold: true,
        ..Style::default()
    };
    let match_style = Style {
        fg: colors.match_fg.cell(),
        bold: true,
        ..Style::default()
    };
    let dim_style = Style {
        fg: colors.dim_fg.cell(),
        ..Style::default()
    };
    draw_cells(out, rect, |row, col| {
        let cell = hints.lines.get(row as usize).and_then(|l: &Line| l.cells.get(col as usize))?;
        let overlay = hints.overlay(row, col);
        // A label is one column wide, so it can split a wide character; blank
        // out the half that is left over.
        let after_label =
            col > 0 && matches!(hints.overlay(row, col - 1), Overlay::Label(_)) && cell.continuation;
        let (text, style) = match overlay {
            Overlay::Label(c) => (c, label_style),
            _ if after_label => (" ", dim_style),
            Overlay::Match => (cell.text.as_str(), match_style),
            Overlay::Plain => (cell.text.as_str(), dim_style),
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
    config: &Config,
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
            let colors = &config.colors;
            let fg = if touches_active { colors.active_border } else { colors.inactive_border };
            queue!(out, MoveTo(area.x + x, area.y + y), SetForegroundColor(fg.0), Print(glyph))?;
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

/// Draws the status bar. Returns where the cursor goes while a window name
/// is being typed.
fn draw_status(out: &mut impl Write, app: &App) -> io::Result<Option<(u16, u16)>> {
    let (cols, rows) = app.size;
    if rows == 0 {
        return Ok(None);
    }
    let colors = &app.config.colors;
    let status = &app.config.status;

    let (label, fg, bg) = match app.mode() {
        _ if app.hinting() => (Some(" HINTS  "), colors.hints_fg, colors.hints_bg),
        None => {
            let (fg, bg) = colors.insert();
            (status.show_insert.then_some(" INSERT "), fg, bg)
        }
        Some(Mode::Normal) => (Some(" NORMAL "), colors.normal_fg, colors.normal_bg),
        Some(Mode::Visual) => (Some(" VISUAL "), colors.visual_fg, colors.visual_bg),
        Some(Mode::VisualLine) => (Some(" V-LINE "), colors.visual_fg, colors.visual_bg),
    };
    let (bar_fg, bar_bg) = if status.color_by_mode {
        (fg, bg)
    } else {
        (colors.status_fg, colors.status_bg)
    };
    let segment = |text: String, bold: bool| Segment {
        text,
        bold,
        fg: bar_fg.0,
        bg: bar_bg.0,
    };

    let mut segments = Vec::new();
    if let Some(label) = label {
        segments.push(Segment {
            fg: fg.0,
            bg: bg.0,
            ..segment(label.into(), true)
        });
    }
    let mut cursor = None;
    if let Some(text) = &app.rename {
        segments.push(segment(format!(" rename window: {text}"), false));
        let len: usize = segments.iter().map(|s| s.text.chars().count()).sum();
        cursor = Some((len.min(cols.saturating_sub(1) as usize) as u16, rows - 1));
    } else {
        let title = if status.greek { " [βαρβηλώ] " } else { " [barbelo] " };
        segments.push(segment(title.into(), false));
        for (i, window) in app.windows.iter().enumerate() {
            let active = i == app.active;
            let flag = if active { "*" } else { " " };
            let name = window.name(&app.config.windows.name);
            segments.push(segment(format!("{}:{name}{flag} ", i + 1), active));
        }
    }

    let right = match (&app.message, app.prefix_pending, app.copy_position()) {
        _ if app.rename.is_some() => String::new(),
        (Some(msg), _, _) => format!(" {msg} "),
        (None, true, _) => " PREFIX ".to_string(),
        (None, false, Some((line, total))) => format!(" {line}/{total} "),
        (None, false, None) => String::new(),
    };
    let width = cols as usize;
    let left_len: usize = segments.iter().map(|s| s.text.chars().count()).sum();
    let gap = width.saturating_sub(left_len + right.chars().count());
    segments.push(segment(" ".repeat(gap), false));
    segments.push(segment(right, true));

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
    queue!(out, SetAttribute(Attribute::Reset))?;
    Ok(cursor)
}
