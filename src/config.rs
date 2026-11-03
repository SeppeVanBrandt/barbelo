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
    pub screensaver: ScreensaverConfig,
    pub beta: BetaConfig,
}

/// Features still being tried out, all off by default.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BetaConfig {
    /// Function keys for the command menu, browse mode, picking windows and hint mode.
    pub function_keys: bool,
    /// Show the command menu when the menu key is pressed. Without it the
    /// key is a prefix like any other; the menu key and `H` still show it.
    pub show_menu: bool,
    pub menu_key: FunctionKey,
    #[serde(alias = "normal_key")]
    pub browse_key: FunctionKey,
    /// Pick a window by its letter.
    pub windows_key: FunctionKey,
    pub hints_key: FunctionKey,
    /// Keys kept free for later jobs: they do nothing, and don't reach the
    /// pane either.
    pub spare_keys: Vec<FunctionKey>,
}

impl Default for BetaConfig {
    fn default() -> Self {
        Self {
            function_keys: false,
            show_menu: true,
            menu_key: FunctionKey(13),
            browse_key: FunctionKey(14),
            windows_key: FunctionKey(15),
            hints_key: FunctionKey(16),
            spare_keys: Vec::new(),
        }
    }
}

/// What a function key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionAction {
    Menu,
    Browse,
    Windows,
    Hints,
    Spare,
}

impl BetaConfig {
    /// What F`n` does, if it is one of barbelo's keys.
    pub fn action(&self, n: u8) -> Option<FunctionAction> {
        let keys = [
            (self.menu_key, FunctionAction::Menu),
            (self.browse_key, FunctionAction::Browse),
            (self.windows_key, FunctionAction::Windows),
            (self.hints_key, FunctionAction::Hints),
        ];
        let spare = self.spare_keys.iter().map(|&k| (k, FunctionAction::Spare));
        keys.into_iter().chain(spare).find(|(k, _)| k.0 == n).map(|(_, action)| action)
    }
}

/// One of F13 to F24, written like "F14".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct FunctionKey(pub u8);

impl TryFrom<String> for FunctionKey {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        s.strip_prefix(['F', 'f'])
            .and_then(|n| n.parse().ok())
            .filter(|n| (13..=24).contains(n))
            .map(Self)
            .ok_or_else(|| format!("invalid function key {s:?}: expected F13 to F24"))
    }
}

impl std::fmt::Display for FunctionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "F{}", self.0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusConfig {
    /// Show INSERT instead of the name in insert mode.
    pub show_insert: bool,
    /// Show the [barbelo] title, and the mode names that take its place.
    pub show_title: bool,
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
    /// With boxed borders, write the status bar into the panes' bottom
    /// edge instead of a line of its own.
    pub in_border: bool,
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
            show_title: true,
            color_by_mode: false,
            greek: false,
            clock: false,
            clock_format: "%H:%M".into(),
            window_format: "{number}:{name}{flags}".into(),
            active_window_format: None,
            in_border: true,
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
    #[serde(alias = "normal_fg")]
    pub browse_fg: ConfigColor,
    #[serde(alias = "normal_bg")]
    pub browse_bg: ConfigColor,
    #[serde(alias = "visual_fg")]
    pub select_fg: ConfigColor,
    #[serde(alias = "visual_bg")]
    pub select_bg: ConfigColor,
    pub hints_fg: ConfigColor,
    pub hints_bg: ConfigColor,
    /// A prefix key waiting for its command, without the menu showing.
    pub prefix_fg: ConfigColor,
    pub prefix_bg: ConfigColor,
    pub resize_fg: ConfigColor,
    pub resize_bg: ConfigColor,
    pub active_border: ConfigColor,
    pub inactive_border: ConfigColor,
    /// Window entries in the bar; the bar's colors unless set.
    /// Letters shown for picking a window.
    pub window_letter: ConfigColor,
    /// The status bar while picking a window; its usual colors unless set.
    pub window_pick_fg: Option<ConfigColor>,
    pub window_pick_bg: Option<ConfigColor>,
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
            browse_fg: ConfigColor(Color::White),
            browse_bg: ConfigColor(Color::Blue),
            select_fg: ConfigColor(Color::White),
            select_bg: ConfigColor(Color::Magenta),
            hints_fg: ConfigColor(Color::Black),
            hints_bg: ConfigColor(Color::Yellow),
            prefix_fg: ConfigColor(Color::Black),
            prefix_bg: ConfigColor(Color::AnsiValue(208)),
            resize_fg: ConfigColor(Color::Black),
            resize_bg: ConfigColor(Color::Cyan),
            active_border: ConfigColor(Color::Green),
            inactive_border: ConfigColor(Color::DarkGrey),
            window_letter: ConfigColor(Color::Red),
            window_pick_fg: None,
            window_pick_bg: None,
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
    /// Keys for picking a window, first window first.
    pub letters: Letters,
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
            letters: Letters("asdfjkl;qweruiop".chars().collect()),
        }
    }
}

/// Distinct characters to press, such as the letters for picking a window.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Letters(pub Vec<char>);

impl TryFrom<String> for Letters {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let mut letters = Vec::new();
        for c in s.chars() {
            if c.is_whitespace() || c.is_control() {
                return Err(format!("window letters can't contain {c:?}"));
            }
            if letters.contains(&c) {
                return Err(format!("window letters contain {c:?} twice"));
            }
            letters.push(c);
        }
        if letters.is_empty() {
            return Err("window letters can't be empty".into());
        }
        Ok(Self(letters))
    }
}

impl Config {
    /// Whether the status bar is drawn into the panes' bottom border.
    pub fn status_in_border(&self) -> bool {
        self.status.in_border && self.windows.borders == BorderStyle::Boxed
    }
}

/// The screensaver runs pipes.sh full screen. Its options are passed on
/// only when set, so pipes.sh's own defaults apply otherwise.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreensaverConfig {
    pub command: String,
    /// Seconds without input before it starts; 0 starts it only by hand.
    pub timeout: u64,
    /// Number of pipes (`-p`).
    pub pipes: Option<u32>,
    /// Pipe types (`-t`): an index into pipes.sh's sets, or `c` followed by
    /// 16 characters.
    pub types: Vec<PipesValue>,
    /// Pipe colors (`-c`): color indices, or `#` and a hex value.
    pub colors: Vec<PipesValue>,
    /// Frames per second, 20-100 (`-f`).
    pub fps: Option<u32>,
    /// A pipe goes straight with odds 1 in `straight`, 5-15 (`-s`).
    pub straight: Option<u32>,
    /// Clear the screen after this many characters, 0 for never (`-r`).
    pub reset: Option<u32>,
    /// Random starting positions and directions (`-R`).
    pub random_start: bool,
    /// `false` turns off bold (`-B`).
    pub bold: bool,
    /// `false` turns off color (`-C`).
    pub color: bool,
    /// Pipes keep their color and type when they cross an edge (`-K`).
    pub keep_style: bool,
    /// More arguments, passed on as they are.
    pub args: Vec<String>,
}

impl Default for ScreensaverConfig {
    fn default() -> Self {
        Self {
            command: "pipes.sh".into(),
            timeout: 0,
            pipes: None,
            types: Vec::new(),
            colors: Vec::new(),
            fps: None,
            straight: None,
            reset: None,
            random_start: false,
            bold: true,
            color: true,
            keep_style: false,
            args: Vec::new(),
        }
    }
}

impl ScreensaverConfig {
    /// Command-line arguments for pipes.sh.
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let mut opt = |flag: &str, value: String| {
            args.push(flag.to_string());
            args.push(value);
        };
        if let Some(n) = self.pipes {
            opt("-p", n.to_string());
        }
        for t in &self.types {
            opt("-t", t.to_string());
        }
        for c in &self.colors {
            opt("-c", c.to_string());
        }
        if let Some(n) = self.fps {
            opt("-f", n.to_string());
        }
        if let Some(n) = self.straight {
            opt("-s", n.to_string());
        }
        if let Some(n) = self.reset {
            opt("-r", n.to_string());
        }
        let flags = [
            (self.random_start, "-R"),
            (!self.bold, "-B"),
            (!self.color, "-C"),
            (self.keep_style, "-K"),
        ];
        for (set, flag) in flags {
            if set {
                args.push(flag.into());
            }
        }
        args.extend(self.args.iter().cloned());
        args
    }

    /// Catches values pipes.sh would refuse, since it would just quit.
    fn check(&self) -> Result<(), String> {
        let ranges = [
            ("pipes", self.pipes, 1..=u32::MAX),
            ("fps", self.fps, 20..=100),
            ("straight", self.straight, 5..=15),
        ];
        for (name, value, range) in ranges {
            if let Some(v) = value
                && !range.contains(&v)
            {
                let (lo, hi) = (range.start(), range.end());
                return Err(match *hi {
                    u32::MAX => format!("screensaver.{name} must be at least {lo}"),
                    _ => format!("screensaver.{name} must be between {lo} and {hi}"),
                });
            }
        }
        Ok(())
    }
}

/// A pipes.sh value that is either a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PipesValue {
    Number(u32),
    Text(String),
}

impl std::fmt::Display for PipesValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipesValue::Number(n) => write!(f, "{n}"),
            PipesValue::Text(s) => f.write_str(s),
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
    config.screensaver.check()?;
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
        assert_eq!(config.colors.browse_bg, ConfigColor(Color::Blue));
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
            browse_bg = 208
            visual_fg = "dark_grey"  # the old name for select_fg still works
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
        assert_eq!(config.colors.browse_bg.0, Color::AnsiValue(208));
        assert_eq!(config.colors.select_fg.0, Color::DarkGrey);
        assert_eq!(config.colors.status_fg.0, Color::Black);
        assert_eq!(config.colors.insert().1, config.colors.status_bg);
        assert_eq!(config.hints.alphabet.0, b"jkl");
        assert_eq!(config.windows.borders, BorderStyle::Boxed);
        assert_eq!(config.windows.border_chars(true), BorderChars::Heavy);
        assert_eq!(config.windows.border_chars(false), BorderChars::Light);
    }

    #[test]
    fn screensaver_args() {
        let config = parse("").unwrap();
        assert!(config.screensaver.args().is_empty());
        let config = parse(
            r##"
            [screensaver]
            pipes = 3
            types = [0, 5, "c┃┏┓┛━┓  ┗┃┛┗ ┏━"]
            colors = [1, "#ff8800"]
            fps = 60
            bold = false
            random_start = true
            args = ["-K"]
            "##,
        )
        .unwrap();
        let args = config.screensaver.args();
        assert_eq!(
            args,
            [
                "-p", "3", "-t", "0", "-t", "5", "-t", "c┃┏┓┛━┓  ┗┃┛┗ ┏━", "-c", "1", "-c",
                "#ff8800", "-f", "60", "-R", "-B", "-K"
            ]
        );
        assert!(parse("[screensaver]\nfps = 10").is_err());
        assert!(parse("[screensaver]\npipes = 0").is_err());
    }

    #[test]
    fn function_keys() {
        let config = parse("").unwrap();
        assert_eq!(config.beta.action(13), Some(FunctionAction::Menu));
        assert_eq!(config.beta.action(15), Some(FunctionAction::Windows));
        assert_eq!(config.beta.action(16), Some(FunctionAction::Hints));
        assert_eq!(config.beta.action(17), None);
        let config = parse(
            "[beta]\nmenu_key = \"F14\"\nbrowse_key = \"F15\"\nwindows_key = \"F16\"\nhints_key = \"F17\"\nspare_keys = [\"F18\"]",
        )
        .unwrap();
        assert_eq!(config.beta.action(13), None);
        assert_eq!(config.beta.action(14), Some(FunctionAction::Menu));
        assert_eq!(config.beta.action(16), Some(FunctionAction::Windows));
        assert_eq!(config.beta.action(17), Some(FunctionAction::Hints));
        assert_eq!(config.beta.action(18), Some(FunctionAction::Spare));
        assert!(parse("[windows]\nletters = \"aa\"").is_err());
        assert_eq!(parse("").unwrap().windows.letters.0[7], ';');
        assert!(parse("[beta]\nmenu_key = \"F12\"").is_err());
        assert!(parse("[beta]\nmenu_key = \"14\"").is_err());
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
