use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};

use crate::app::App;
use crate::layout::{Border, Rect};

const ACTIVE_BORDER: Color = Color::Green;
const INACTIVE_BORDER: Color = Color::DarkGrey;
const STATUS_FG: Color = Color::Black;
const STATUS_BG: Color = Color::Green;

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
        let parser = pane.parser.lock().unwrap();
        let screen = parser.screen();
        draw_screen(out, screen, *rect)?;
        if *id == window.active && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            if row < rect.h && col < rect.w {
                cursor = Some((rect.x + col, rect.y + row));
            }
        }
    }

    let active_rect = layout.rect_of(window.active);
    draw_borders(out, &layout.borders, area, active_rect)?;
    draw_status(out, app)?;

    if let Some((x, y)) = cursor {
        queue!(out, MoveTo(x, y), Show)?;
    }
    queue!(out, EndSynchronizedUpdate)?;
    out.flush()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Style {
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Style {
    fn of(cell: &vt100::Cell) -> Self {
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

fn draw_screen(out: &mut impl Write, screen: &vt100::Screen, rect: Rect) -> io::Result<()> {
    queue!(out, SetAttribute(Attribute::Reset))?;
    let mut current: Option<Style> = None;
    for row in 0..rect.h {
        queue!(out, MoveTo(rect.x, rect.y + row))?;
        let mut col = 0;
        while col < rect.w {
            let Some(cell) = screen.cell(row, col) else {
                if current.take().is_some() {
                    queue!(out, SetAttribute(Attribute::Reset))?;
                }
                queue!(out, Print(' '))?;
                col += 1;
                continue;
            };
            if cell.is_wide_continuation() {
                col += 1;
                continue;
            }
            let style = Style::of(cell);
            if current != Some(style) {
                style.apply(out)?;
                current = Some(style);
            }
            // A wide character in the last column would spill over the border.
            let contents = cell.contents();
            if contents.is_empty() || (cell.is_wide() && col + 1 >= rect.w) {
                queue!(out, Print(' '))?;
            } else {
                queue!(out, Print(contents))?;
            }
            col += if cell.is_wide() { 2 } else { 1 };
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

fn draw_status(out: &mut impl Write, app: &App) -> io::Result<()> {
    let (cols, rows) = app.size;
    if rows == 0 {
        return Ok(());
    }
    // Segments of (text, bold), laid out left to right.
    let mut segments = vec![("[barbelo] ".to_string(), false)];
    for (i, window) in app.windows.iter().enumerate() {
        let active = i == app.active;
        let flag = if active { "*" } else { " " };
        segments.push((format!("{i}:{}{flag} ", window.name()), active));
    }
    let right = match (&app.message, app.prefix_pending) {
        (Some(msg), _) => format!(" {msg} "),
        (None, true) => " PREFIX ".to_string(),
        (None, false) => String::new(),
    };
    let width = cols as usize;
    let left_len: usize = segments.iter().map(|(s, _)| s.chars().count()).sum();
    let gap = width.saturating_sub(left_len + right.chars().count());
    segments.push((" ".repeat(gap), false));
    segments.push((right, true));

    queue!(
        out,
        MoveTo(0, rows - 1),
        SetAttribute(Attribute::Reset),
        SetForegroundColor(STATUS_FG),
        SetBackgroundColor(STATUS_BG),
    )?;
    let mut remaining = width;
    for (text, bold) in segments {
        let text: String = text.chars().take(remaining).collect();
        remaining -= text.chars().count();
        let attr = if bold { Attribute::Bold } else { Attribute::NormalIntensity };
        queue!(out, SetAttribute(attr), Print(text))?;
    }
    queue!(out, SetAttribute(Attribute::Reset))
}
