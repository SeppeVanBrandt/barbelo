//! User settings, read from `$XDG_CONFIG_HOME/barbelo/config.toml`
//! (normally `~/.config/barbelo/config.toml`). Every setting is optional.

use std::io::ErrorKind;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::Color;
use regex::Regex;
use serde::Deserialize;

use crate::layout::{BorderChars, BorderStyle};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Key that starts a command sequence.
    pub prefix: Prefix,
    pub status: StatusConfig,
    pub colors: Colors,
    pub hints: HintsConfig,
    pub windows: WindowsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusConfig {
    /// Show INSERT instead of the name in insert mode.
    pub show_insert: bool,
    /// Paint the whole bar in the current mode's colors.
    pub color_by_mode: bool,
    /// Spell the name as βαρβηλώ.
    pub greek: bool,
    /// Show the time at the right end of the bar.
    pub clock: bool,
    /// `strftime` format of the clock.
    pub clock_format: String,
    /// A window's entry in the bar; see `Window::status_entry`.
    pub window_format: String,
    /// The active window's entry; `window_format` unless set.
    pub active_window_format: Option<String>,
}

impl StatusConfig {
    pub fn active_window_format(&self) -> &str {
        self.active_window_format.as_deref().unwrap_or(&self.window_format)
    }
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            show_insert: false,
            color_by_mode: false,
            greek: false,
            clock: false,
            clock_format: "%H:%M".into(),
            window_format: "{number}:{name}{flags} ".into(),
            active_window_format: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub status_fg: ConfigColor,
    pub status_bg: ConfigColor,
    /// Insert mode colors; the status colors unless set.
    pub insert_fg: Option<ConfigColor>,
    pub insert_bg: Option<ConfigColor>,
    pub normal_fg: ConfigColor,
    pub normal_bg: ConfigColor,
    pub visual_fg: ConfigColor,
    pub visual_bg: ConfigColor,
    pub hints_fg: ConfigColor,
    pub hints_bg: ConfigColor,
    pub active_border: ConfigColor,
    pub inactive_border: ConfigColor,
    /// Window entries in the bar; the bar's colors unless set.
    pub window_fg: Option<ConfigColor>,
    pub window_bg: Option<ConfigColor>,
    pub active_window_fg: Option<ConfigColor>,
    pub active_window_bg: Option<ConfigColor>,
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            status_fg: ConfigColor(Color::Black),
            status_bg: ConfigColor(Color::Green),
            insert_fg: None,
            insert_bg: None,
            normal_fg: ConfigColor(Color::White),
            normal_bg: ConfigColor(Color::Blue),
            visual_fg: ConfigColor(Color::White),
            visual_bg: ConfigColor(Color::Magenta),
            hints_fg: ConfigColor(Color::Black),
            hints_bg: ConfigColor(Color::Yellow),
            active_border: ConfigColor(Color::Green),
            inactive_border: ConfigColor(Color::DarkGrey),
            window_fg: None,
            window_bg: None,
            active_window_fg: None,
            active_window_bg: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HintsConfig {
    /// Letters used for hint labels, easiest to reach first.
    pub alphabet: Alphabet,
    pub label_fg: ConfigColor,
    pub label_bg: ConfigColor,
    /// Color of the matched text next to a label.
    pub match_fg: ConfigColor,
    /// Color of everything that isn't a match.
    pub dim_fg: ConfigColor,
}

impl Default for HintsConfig {
    fn default() -> Self {
        Self {
            alphabet: Alphabet(b"asdfqwerzxcvjklmiuopghtybn".to_vec()),
            label_fg: ConfigColor(Color::Black),
            label_bg: ConfigColor(Color::Yellow),
            match_fg: ConfigColor(Color::Yellow),
            dim_fg: ConfigColor(Color::DarkGrey),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowsConfig {
    /// How windows are named, with `{process}`, `{dir}` and `{path}` filled
    /// in from the active pane.
    pub name: String,
    /// `shared` lines between panes, or a `boxed` frame around each pane.
    pub borders: BorderStyle,
    pub border_chars: BorderChars,
    /// Characters for the active pane's border; `border_chars` unless set.
    pub active_border_chars: Option<BorderChars>,
}

impl WindowsConfig {
    pub fn border_chars(&self, active: bool) -> BorderChars {
        match self.active_border_chars {
            Some(chars) if active => chars,
            _ => self.border_chars,
        }
    }
}

impl Default for WindowsConfig {
    fn default() -> Self {
        Self {
            name: "{process}".into(),
            borders: BorderStyle::Shared,
            border_chars: BorderChars::Light,
            active_border_chars: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Prefix(pub KeyEvent);

impl Default for Prefix {
    fn default() -> Self {
        Self(KeyEvent::new(KeyCode::Char('`'), KeyModifiers::NONE))
    }
}

impl TryFrom<String> for Prefix {
    type Error = String;

    /// Accepts a single character, optionally with a `C-` (Ctrl) or `M-`
    /// (Alt) modifier.
    fn try_from(s: String) -> Result<Self, String> {
        let (modifiers, key) = if let Some(k) = s.strip_prefix("C-") {
            (KeyModifiers::CONTROL, k)
        } else if let Some(k) = s.strip_prefix("M-") {
            (KeyModifiers::ALT, k)
        } else {
            (KeyModifiers::NONE, s.as_str())
        };
        let mut chars = key.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(Self(KeyEvent::new(KeyCode::Char(c), modifiers))),
            _ => Err(format!("invalid prefix {s:?}: expected a key like \"`\" or \"C-a\"")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Alphabet(pub Vec<u8>);

impl TryFrom<String> for Alphabet {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let mut letters = Vec::new();
        for c in s.chars() {
            // Uppercase is taken: typing a label in capitals also pastes.
            if !c.is_ascii_lowercase() {
                return Err(format!("hint alphabet may only contain a-z, found {c:?}"));
            }
            if letters.contains(&(c as u8)) {
                return Err(format!("hint alphabet contains {c:?} twice"));
            }
            letters.push(c as u8);
        }
        if letters.len() < 2 {
            return Err("hint alphabet needs at least two letters".into());
        }
        Ok(Self(letters))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "ColorValue")]
pub struct ConfigColor(pub Color);

#[derive(Deserialize)]
#[serde(untagged)]
enum ColorValue {
    Index(u8),
    Name(String),
}

impl TryFrom<ColorValue> for ConfigColor {
    type Error = String;

    /// Accepts a 256-color index, `#rrggbb`, `default`, or one of the 16
    /// named colors.
    fn try_from(value: ColorValue) -> Result<Self, String> {
        let name = match value {
            ColorValue::Index(i) => return Ok(Self(Color::AnsiValue(i))),
            ColorValue::Name(name) => name,
        };
        if let Some(hex) = name.strip_prefix('#')
            && hex.len() == 6
            && let Ok(rgb) = u32::from_str_radix(hex, 16)
        {
            let [_, r, g, b] = rgb.to_be_bytes();
            return Ok(Self(Color::Rgb { r, g, b }));
        }
        let color = match name.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "default" => Color::Reset,
            "black" => Color::Black,
            "dark-red" => Color::DarkRed,
            "dark-green" => Color::DarkGreen,
            "dark-yellow" => Color::DarkYellow,
            "dark-blue" => Color::DarkBlue,
            "dark-magenta" => Color::DarkMagenta,
            "dark-cyan" => Color::DarkCyan,
            "grey" | "gray" => Color::Grey,
            "dark-grey" | "dark-gray" => Color::DarkGrey,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "white" => Color::White,
            _ => return Err(format!("unknown color {name:?}")),
        };
        Ok(Self(color))
    }
}

impl Colors {
    pub fn insert(&self) -> (ConfigColor, ConfigColor) {
        (
            self.insert_fg.unwrap_or(self.status_fg),
            self.insert_bg.unwrap_or(self.status_bg),
        )
    }
}

impl ConfigColor {
    /// The same color as a cell color, for styling pane contents.
    pub fn cell(self) -> vt100::Color {
        let idx = match self.0 {
            Color::Reset => return vt100::Color::Default,
            Color::Rgb { r, g, b } => return vt100::Color::Rgb(r, g, b),
            Color::AnsiValue(i) => i,
            Color::Black => 0,
            Color::DarkRed => 1,
            Color::DarkGreen => 2,
            Color::DarkYellow => 3,
            Color::DarkBlue => 4,
            Color::DarkMagenta => 5,
            Color::DarkCyan => 6,
            Color::Grey => 7,
            Color::DarkGrey => 8,
            Color::Red => 9,
            Color::Green => 10,
            Color::Yellow => 11,
            Color::Blue => 12,
            Color::Magenta => 13,
            Color::Cyan => 14,
            Color::White => 15,
        };
        vt100::Color::Idx(idx)
    }
}

pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("barbelo").join("config.toml"))
}

/// Reads the config file, falling back to the defaults if there is none.
pub fn load() -> Result<Config> {
    let Some(path) = path() else {
        return Ok(Config::default());
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(e).with_context(|| format!("failed to read {}", path.display())),
    };
    match parse(&text) {
        Ok(config) => Ok(config),
        Err(e) => bail!("invalid config {}:\n{e}", path.display()),
    }
}

fn parse(text: &str) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    config.check_formats()?;
    Ok(config)
}

impl Config {
    /// Rejects placeholders the formats don't know, which are most likely
    /// typos.
    fn check_formats(&self) -> Result<(), String> {
        const PANE: &[&str] = &["process", "dir", "path"];
        const ENTRY: &[&str] = &["process", "dir", "path", "number", "name", "flags"];
        let status = &self.status;
        let formats = [
            ("windows.name", Some(&self.windows.name), PANE),
            ("status.window_format", Some(&status.window_format), ENTRY),
            ("status.active_window_format", status.active_window_format.as_ref(), ENTRY),
        ];
        let placeholder = Regex::new(r"\{(\w*)\}").expect("valid regex");
        for (setting, format, known) in formats {
            let Some(format) = format else { continue };
            for caps in placeholder.captures_iter(format) {
                if !known.contains(&&caps[1]) {
                    let known: Vec<String> = known.iter().map(|k| format!("{{{k}}}")).collect();
                    return Err(format!(
                        "unknown {} in {setting}; use {}",
                        &caps[0],
                        known.join(", ")
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn empty_config_is_default() {
        let config = parse("").unwrap();
        assert_eq!(config.prefix, Prefix::default());
        assert!(!config.status.show_insert);
        assert_eq!(config.colors.normal_bg, ConfigColor(Color::Blue));
        assert_eq!(config.windows.name, "{process}");
    }

    #[test]
    fn overrides_keep_other_defaults() {
        let config = parse(
            r##"
            prefix = "C-a"
            [status]
            show_insert = true
            [colors]
            status_bg = "#1e1e2e"
            normal_bg = 208
            visual_fg = "dark_grey"
            [hints]
            alphabet = "jkl"
            [windows]
            borders = "boxed"
            active_border_chars = "heavy"
            "##,
        )
        .unwrap();
        assert_eq!(config.prefix.0, KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(config.status.show_insert);
        assert!(!config.status.color_by_mode);
        assert_eq!(config.colors.status_bg.0, Color::Rgb { r: 0x1e, g: 0x1e, b: 0x2e });
        assert_eq!(config.colors.normal_bg.0, Color::AnsiValue(208));
        assert_eq!(config.colors.visual_fg.0, Color::DarkGrey);
        assert_eq!(config.colors.status_fg.0, Color::Black);
        assert_eq!(config.colors.insert().1, config.colors.status_bg);
        assert_eq!(config.hints.alphabet.0, b"jkl");
        assert_eq!(config.windows.borders, BorderStyle::Boxed);
        assert_eq!(config.windows.border_chars(true), BorderChars::Heavy);
        assert_eq!(config.windows.border_chars(false), BorderChars::Light);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse("prefix = \"ab\"").is_err());
        assert!(parse("[colors]\nstatus_bg = \"chartreuse\"").is_err());
        assert!(parse("[hints]\nalphabet = \"aa\"").is_err());
        assert!(parse("[hints]\nalphabet = \"aB\"").is_err());
        assert!(parse("[status]\nshow_inserts = false").is_err());
        assert!(parse("[windows]\nborders = \"double\"").is_err());
        assert!(parse("[windows]\nname = \"{number}\"").is_err());
        assert!(parse("[status]\nwindow_format = \"{nmber}:{name}\"").is_err());
        assert!(parse("[status]\nactive_window_format = \"{number}:{dir}:{process}\"").is_ok());
    }
}
