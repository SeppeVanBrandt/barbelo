use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, SetCursorStyle, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate};

use crate::app::App;
use crate::clock;
use crate::config::{Config, ConfigColor};
use crate::copy::{CopyMode, Line, Mode};
use crate::hints::{HintMode, Overlay};
use crate::layout::{Border, BorderChars, Layout, Rect};
use crate::menu;
use crate::pane::PaneId;
use crate::resize::{self, ResizeMode};

pub fn draw(app: &App, out: &mut impl Write) -> io::Result<()> {
    queue!(out, BeginSynchronizedUpdate, Hide)?;

    if let Some((_, saver)) = &app.screensaver {
        let (cols, rows) = app.size;
        let full = Rect {
            x: 0,
            y: 0,
            w: cols,
            h: rows,
        };
        draw_screen(out, saver.parser.lock().unwrap().screen(), full)?;
        queue!(out, EndSynchronizedUpdate)?;
        return out.flush();
    }

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
    // With the status bar in the border, the active frame takes the mode's
    // color.
    let active_color = match app.config.status_in_border() {
        true => mode_color(app),
        false => app.config.colors.active_border,
    };
    draw_frames(out, &layout.frames, window.active, active_color, &app.config)?;
    let prompt_cursor = draw_status(out, app)?;
    if prompt_cursor.is_some() {
        cursor = prompt_cursor;
    }
    if let Some(mode) = &app.resize {
        draw_resize(out, mode, &layout, app)?;
        cursor = None;
    }
    if app.menu {
        draw_menu(out, app)?;
        cursor = None;
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
            // Index into `Glyphs::lines`.
            let glyph = match (at(xi, yi - 1), at(xi, yi + 1), at(xi - 1, yi), at(xi + 1, yi)) {
                (true, true, true, true) => 10,
                (true, true, true, false) => 7,
                (true, true, false, true) => 6,
                (true, false, true, true) => 9,
                (false, true, true, true) => 8,
                (true, false, true, false) => 5,
                (true, false, false, true) => 4,
                (false, true, true, false) => 3,
                (false, true, false, true) => 2,
                (_, _, false, false) => 1,
                _ => 0,
            };
            let (x, y) = (x as u16, y as u16);
            let touches_active = active.is_some_and(|r| {
                x + 1 >= r.x && x <= r.right() && y + 1 >= r.y && y <= r.bottom()
            });
            let colors = &config.colors;
            let color = if touches_active { colors.active_border } else { colors.inactive_border };
            let glyphs = border_style(config, touches_active, color, out)?;
            queue!(out, MoveTo(area.x + x, area.y + y), Print(glyphs.lines[glyph]))?;
        }
    }
    queue!(out, SetAttribute(Attribute::Reset))
}

/// The characters a border can be drawn with.
struct Glyphs {
    /// `─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼`, for lines shared between panes.
    lines: [char; 11],
    /// Top, bottom, left and right edge, then `┌ ┐ └ ┘`, for a box around
    /// one pane.
    frame: [char; 8],
}

impl Glyphs {
    fn of(chars: BorderChars) -> Self {
        let (lines, frame) = match chars {
            BorderChars::Light => ("─│┌┐└┘├┤┬┴┼", "──││┌┐└┘"),
            BorderChars::Heavy => ("━┃┏┓┗┛┣┫┳┻╋", "━━┃┃┏┓┗┛"),
            BorderChars::Double => ("═║╔╗╚╝╠╣╦╩╬", "══║║╔╗╚╝"),
            BorderChars::Rounded => ("─│╭╮╰╯├┤┬┴┼", "──││╭╮╰╯"),
            // Half blocks hug the outside of a box; shared lines are full.
            BorderChars::Block => ("███████████", "▀▄▌▐▛▜▙▟"),
            BorderChars::Solid => ("           ", "        "),
        };
        Self {
            lines: char_array(lines),
            frame: char_array(frame),
        }
    }
}

fn char_array<const N: usize>(s: &str) -> [char; N] {
    let chars: Vec<char> = s.chars().collect();
    chars.try_into().expect("glyph count")
}

/// Sets the border color and returns the characters for the active or an
/// inactive border.
fn border_style(
    config: &Config,
    active: bool,
    color: ConfigColor,
    out: &mut impl Write,
) -> io::Result<Glyphs> {
    let chars = config.windows.border_chars(active);
    // Solid borders are blank cells painted in the border color.
    let bg = if chars == BorderChars::Solid { color.0 } else { Color::Reset };
    queue!(out, SetForegroundColor(color.0), SetBackgroundColor(bg))?;
    Ok(Glyphs::of(chars))
}

/// Marks every node; the selected one in the resize color, with the keys
/// that move it next to it.
fn draw_resize(out: &mut impl Write, mode: &ResizeMode, layout: &Layout, app: &App) -> io::Result<()> {
    let colors = &app.config.colors;
    let nodes = resize::layout_nodes(layout);
    let selected = mode.selected(&nodes);
    let (cols, rows) = app.size;
    queue!(out, SetAttribute(Attribute::Reset), SetAttribute(Attribute::Bold))?;
    for (i, node) in nodes.iter().enumerate() {
        let active = Some(i) == selected;
        let fg = if active { colors.resize_bg.0 } else { Color::Reset };
        queue!(out, MoveTo(node.x, node.y), SetForegroundColor(fg), Print('●'))?;
        if !active {
            continue;
        }
        let mut keys = Vec::new();
        if !node.vertical.is_empty() {
            keys.extend([(-1, 0, 'h'), (1, 0, 'l')]);
        }
        if !node.horizontal.is_empty() {
            keys.extend([(0, -1, 'k'), (0, 1, 'j')]);
        }
        queue!(out, SetForegroundColor(colors.resize_fg.0), SetBackgroundColor(colors.resize_bg.0))?;
        for (dx, dy, key) in keys {
            let (x, y) = (i32::from(node.x) + dx, i32::from(node.y) + dy);
            if (0..i32::from(cols)).contains(&x) && (0..i32::from(rows)).contains(&y) {
                queue!(out, MoveTo(x as u16, y as u16), Print(key))?;
            }
        }
        queue!(out, SetBackgroundColor(Color::Reset))?;
    }
    queue!(out, SetAttribute(Attribute::Reset))
}

/// A line of the command menu.
#[derive(Clone, Copy)]
enum MenuLine {
    Title(&'static str),
    Entry(&'static str, &'static str),
    Blank,
}

/// The menu's sections in `n` columns, each added to the shortest so far.
fn menu_columns(n: usize) -> Vec<Vec<MenuLine>> {
    let mut columns: Vec<Vec<MenuLine>> = (0..n).map(|_| Vec::new()).collect();
    for s in menu::SECTIONS {
        let column = columns.iter_mut().min_by_key(|c| c.len()).expect("n > 0");
        if !column.is_empty() {
            column.push(MenuLine::Blank);
        }
        column.push(MenuLine::Title(s.title));
        column.extend(s.entries.iter().map(|&(keys, what)| MenuLine::Entry(keys, what)));
    }
    columns
}

/// Width of a column's keys, and of the whole column.
fn menu_column_width(column: &[MenuLine]) -> (usize, usize) {
    let keys = column
        .iter()
        .map(|line| match line {
            MenuLine::Entry(keys, _) => keys.chars().count(),
            _ => 0,
        })
        .max()
        .unwrap_or(0);
    let width = column
        .iter()
        .map(|line| match line {
            MenuLine::Title(title) => title.chars().count(),
            MenuLine::Entry(_, what) => keys + 2 + what.chars().count(),
            MenuLine::Blank => 0,
        })
        .max()
        .unwrap_or(0);
    (keys, width)
}

/// Draws the command menu in the middle of the screen, in as many columns
/// as fit.
fn draw_menu(out: &mut impl Write, app: &App) -> io::Result<()> {
    const SPACING: usize = 3;
    let (cols, rows) = app.size;
    let fits = |columns: &[Vec<MenuLine>]| {
        let widths: usize = columns.iter().map(|c| menu_column_width(c).1).sum();
        widths + SPACING * (columns.len() - 1) + 4 <= cols as usize
    };
    let columns = (1..=menu::SECTIONS.len())
        .rev()
        .map(menu_columns)
        .find(|c| fits(c))
        .unwrap_or_else(|| menu_columns(1));
    let widths: Vec<(usize, usize)> = columns.iter().map(|c| menu_column_width(c)).collect();
    let content = widths.iter().map(|w| w.1).sum::<usize>() + SPACING * (widths.len() - 1);
    let height = columns.iter().map(Vec::len).max().unwrap_or(0);

    // The box: a border, a cell of padding, the columns, a blank line and
    // the footer. Cut off if the screen is too small, leaving the status
    // bar in view.
    let footer = menu::footer(&app.config.beta);
    let inner_w = content.max(footer.chars().count());
    let rows = rows.saturating_sub(1);
    let w = (inner_w + 4).min(cols as usize) as u16;
    let h = (height + 4).min(rows as usize) as u16;
    if w < 4 || h < 3 {
        return Ok(());
    }
    let frame = Rect {
        x: (cols - w) / 2,
        y: (rows - h) / 2,
        w,
        h,
    };
    let colors = &app.config.colors;
    let accent = colors.active_border.0;

    queue!(out, SetAttribute(Attribute::Reset), SetForegroundColor(accent))?;
    let line: String = "─".repeat(w as usize - 2);
    let blank = " ".repeat(w as usize - 2);
    queue!(out, MoveTo(frame.x, frame.y), Print(format!("┌{line}┐")))?;
    for y in frame.y + 1..frame.bottom() - 1 {
        queue!(out, MoveTo(frame.x, y), Print(format!("│{blank}│")))?;
    }
    queue!(out, MoveTo(frame.x, frame.bottom() - 1), Print(format!("└{line}┘")))?;
    let title = " barbelo ";
    if title.len() + 4 <= w as usize {
        queue!(out, MoveTo(frame.x + 2, frame.y), SetAttribute(Attribute::Bold), Print(title))?;
    }

    let dim = colors.inactive_border.0;
    let mut x = frame.x + 2;
    for (column, &(key_width, width)) in columns.iter().zip(&widths) {
        for (row, line) in column.iter().enumerate() {
            let y = frame.y + 1 + row as u16;
            match line {
                MenuLine::Title(title) => put_inside(out, frame, (x, y), title, accent, true)?,
                MenuLine::Entry(keys, what) => {
                    put_inside(out, frame, (x, y), keys, accent, true)?;
                    let x = x + key_width as u16 + 2;
                    put_inside(out, frame, (x, y), what, Color::Reset, false)?;
                }
                MenuLine::Blank => {}
            }
        }
        x += (width + SPACING) as u16;
    }
    let footer_at = (frame.x + 2, frame.bottom() - 2);
    put_inside(out, frame, footer_at, &footer, dim, false)?;
    queue!(out, SetAttribute(Attribute::Reset))
}

/// Prints `text` at `(x, y)`, cut off at the inside of `frame`.
fn put_inside(
    out: &mut impl Write,
    frame: Rect,
    (x, y): (u16, u16),
    text: &str,
    fg: Color,
    bold: bool,
) -> io::Result<()> {
    let right = frame.right() - 2;
    if y >= frame.bottom() - 1 || x >= right {
        return Ok(());
    }
    let text: String = text.chars().take((right - x) as usize).collect();
    let attr = if bold { Attribute::Bold } else { Attribute::NormalIntensity };
    queue!(out, SetForegroundColor(fg), SetAttribute(attr), MoveTo(x, y), Print(text))
}

/// Draws a box around each pane, highlighting the active one.
fn draw_frames(
    out: &mut impl Write,
    frames: &[(PaneId, Rect)],
    active: PaneId,
    active_color: ConfigColor,
    config: &Config,
) -> io::Result<()> {
    for &(id, r) in frames {
        if r.w < 2 || r.h < 2 {
            continue;
        }
        let color = if id == active { active_color } else { config.colors.inactive_border };
        let [top, bottom, left, right, tl, tr, bl, br] = border_style(config, id == active, color, out)?.frame;
        let inner = r.w as usize - 2;
        let top: String = std::iter::repeat_n(top, inner).collect();
        let bottom: String = std::iter::repeat_n(bottom, inner).collect();
        queue!(out, MoveTo(r.x, r.y), Print(format!("{tl}{top}{tr}")))?;
        for y in r.y + 1..r.bottom() - 1 {
            queue!(out, MoveTo(r.x, y), Print(left), MoveTo(r.right() - 1, y), Print(right))?;
        }
        queue!(out, MoveTo(r.x, r.bottom() - 1), Print(format!("{bl}{bottom}{br}")))?;
    }
    queue!(out, SetAttribute(Attribute::Reset))
}

struct Segment {
    text: String,
    bold: bool,
    fg: Color,
    bg: Color,
}

/// The current mode's color: the mode's name is written in it when the
/// status bar is in the border, and the active pane's frame takes it.
fn mode_color(app: &App) -> ConfigColor {
    let colors = &app.config.colors;
    match app.mode() {
        _ if app.picking_window => colors.window_letter,
        _ if app.resize.is_some() => colors.resize_bg,
        _ if app.hinting() => colors.hints_bg,
        None => colors.active_border,
        Some(Mode::Normal) => colors.normal_bg,
        Some(Mode::Visual | Mode::VisualLine) => colors.visual_bg,
    }
}

/// Draws the status bar, either as a line of its own or written into the
/// panes' bottom border. Returns where the cursor goes while a window name
/// is being typed.
fn draw_status(out: &mut impl Write, app: &App) -> io::Result<Option<(u16, u16)>> {
    let (cols, rows) = app.size;
    if rows == 0 {
        return Ok(None);
    }
    let colors = &app.config.colors;
    let status = &app.config.status;
    let in_border = app.config.status_in_border();
    let plain = ConfigColor(Color::Reset);

    // The mode's name takes the place of the title, except in insert mode.
    let title = if status.greek { "[βαρβηλώ]" } else { "[barbelo]" };
    let (label, fg, bg) = match app.mode() {
        _ if app.picking_window => (Some("WINDOW"), ConfigColor(Color::Black), colors.window_letter),
        _ if app.resize.is_some() => (Some("RESIZE"), colors.resize_fg, colors.resize_bg),
        _ if app.hinting() => (Some("HINTS"), colors.hints_fg, colors.hints_bg),
        None => {
            let (fg, bg) = colors.insert();
            (status.show_insert.then_some("INSERT"), fg, bg)
        }
        Some(Mode::Normal) => (Some("NORMAL"), colors.normal_fg, colors.normal_bg),
        Some(Mode::Visual) => (Some("VISUAL"), colors.visual_fg, colors.visual_bg),
        Some(Mode::VisualLine) => (Some("V-LINE"), colors.visual_fg, colors.visual_bg),
    };
    // Wide enough for the title and every mode name, so nothing after it
    // shifts when the mode changes.
    let slot_width = title.chars().count().max("INSERT".len());
    let slot = format!(" {:^slot_width$} ", label.unwrap_or(title));
    // In the border, text sits on the terminal's own background, and the
    // mode's name is written in the mode's color.
    let (slot_fg, slot_bg) = if in_border { (mode_color(app), plain) } else { (fg, bg) };
    let (mut bar_fg, mut bar_bg) = if in_border {
        (plain, plain)
    } else if status.color_by_mode {
        (fg, bg)
    } else {
        (colors.status_fg, colors.status_bg)
    };
    // Picking a window can give the bar colors of its own; in the border,
    // a background then fills the whole line.
    let mut filled = !in_border;
    if app.picking_window {
        bar_fg = colors.window_pick_fg.unwrap_or(bar_fg);
        if let Some(bg) = colors.window_pick_bg {
            bar_bg = bg;
            filled = true;
        }
    }
    let segment = |text: String, bold: bool| Segment {
        text,
        bold,
        fg: bar_fg.0,
        bg: bar_bg.0,
    };

    let mut left = vec![match status.show_title {
        true => Segment {
            fg: slot_fg.0,
            bg: slot_bg.0,
            ..segment(slot, label.is_some() || in_border)
        },
        // Just the space the title would have started with.
        false => segment(" ".into(), false),
    }];
    if let Some(text) = &app.rename {
        left.push(segment(format!(" rename window: {text}"), false));
    } else {
        for (i, window) in app.windows.iter().enumerate() {
            let active = i == app.active;
            let (format, fg, bg) = match active {
                true => (status.active_window_format(), colors.active_window_fg, colors.active_window_bg),
                false => (status.window_format.as_str(), colors.window_fg, colors.window_bg),
            };
            let (fg, bg) = (fg.unwrap_or(bar_fg).0, bg.unwrap_or(bar_bg).0);
            let entry = |format: &str| Segment {
                fg,
                bg,
                ..segment(window.status_entry(format, &app.config.windows.name, i + 1), active)
            };
            // While picking a window, its letter takes the number's place,
            // or goes in front if the format has no number.
            let letter = app.picking_window.then(|| app.config.windows.letters.0.get(i)).flatten();
            let Some(letter) = letter else {
                left.push(entry(format));
                continue;
            };
            let letter = || Segment {
                text: letter.to_string(),
                bold: true,
                fg: colors.window_letter.0,
                bg,
            };
            if !format.contains("{number}") {
                left.push(letter());
            }
            for (k, part) in format.split("{number}").enumerate() {
                if k > 0 {
                    left.push(letter());
                }
                left.push(entry(part));
            }
        }
    }

    let message = match (&app.message, app.prefix_pending, app.copy_position()) {
        _ if app.rename.is_some() => String::new(),
        (Some(msg), _, _) => format!(" {msg} "),
        (None, true, _) => " PREFIX ".to_string(),
        (None, false, Some((line, total))) => format!(" {line}/{total} "),
        (None, false, None) => String::new(),
    };
    let clock = match status.clock {
        true => format!(" {} ", clock::now(&status.clock_format)),
        false => String::new(),
    };
    let right = [segment(message, true), segment(clock, false)];

    // In the border, keep the corners and a piece of the edge at both ends.
    let (start, end) = if in_border { (2, cols.saturating_sub(2)) } else { (0, cols) };
    let width = end.saturating_sub(start) as usize;
    let len = |segments: &[Segment]| segments.iter().map(|s| s.text.chars().count()).sum::<usize>();
    let right_len = len(&right).min(width);
    let left_room = width - right_len;
    let left_len = len(&left).min(left_room);
    if filled {
        // A bar of its own is filled from end to end; in the border, the
        // border shows through between the two sides, unless picking a
        // window gives it a background.
        left.push(segment(" ".repeat(left_room - left_len), false));
    }

    let y = rows - 1;
    print_segments(out, start, y, &left, left_room)?;
    print_segments(out, end - right_len as u16, y, &right, right_len)?;
    queue!(out, SetAttribute(Attribute::Reset))?;
    let cursor = app.rename.as_ref().map(|_| {
        let x = (start as usize + left_len).min(cols.saturating_sub(1) as usize);
        (x as u16, y)
    });
    Ok(cursor)
}

/// Prints `segments` from (`x`, `y`), cut off after `width` columns.
fn print_segments(out: &mut impl Write, x: u16, y: u16, segments: &[Segment], width: usize) -> io::Result<()> {
    queue!(out, MoveTo(x, y), SetAttribute(Attribute::Reset))?;
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
    Ok(())
}
