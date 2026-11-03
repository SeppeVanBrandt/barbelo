//! The command menu (beta): the prefix key's commands, shown in a pop-up.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::BetaConfig;

/// A group of commands, as (keys, what they do). The keys are the ones
/// `App::command` handles.
pub struct Section {
    pub title: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
}

pub const SECTIONS: &[Section] = &[
    Section {
        title: "Windows",
        entries: &[
            ("c", "new window"),
            ("n p", "next / previous"),
            ("1-9 0", "go to window"),
            ("w", "pick by letter"),
            ("< >", "move left / right"),
            (",", "rename"),
            ("&", "close"),
        ],
    },
    Section {
        title: "Panes",
        entries: &[
            ("% -", "split beside / below"),
            ("h j k l", "focus"),
            ("o ;", "next / last pane"),
            ("z", "zoom"),
            ("r", "resize"),
            ("x", "close"),
        ],
    },
    Section {
        title: "Other",
        entries: &[
            ("[", "browse mode"),
            ("f", "hint mode"),
            ("]", "paste"),
            ("s", "screensaver"),
            ("H", "this help"),
            ("q", "quit"),
        ],
    },
];

/// Shown below the commands.
pub fn footer(beta: &BetaConfig) -> String {
    format!(
        "{} browse   {} windows   {} hints   Esc close",
        beta.browse_key, beta.windows_key, beta.hints_key
    )
}

/// The number of a function key from F13 to F24. Many terminals send these
/// as Shift+F1 to Shift+F12, so those count too.
pub fn function_key(key: KeyEvent) -> Option<u8> {
    match key.code {
        KeyCode::F(n @ 13..=24) => Some(n),
        KeyCode::F(n @ 1..=12) if key.modifiers.contains(KeyModifiers::SHIFT) => Some(n + 12),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_f1_to_f12_are_f13_to_f24() {
        let key = |n, mods| KeyEvent::new(KeyCode::F(n), mods);
        assert_eq!(function_key(key(13, KeyModifiers::NONE)), Some(13));
        assert_eq!(function_key(key(2, KeyModifiers::SHIFT)), Some(14));
        assert_eq!(function_key(key(2, KeyModifiers::NONE)), None);
        assert_eq!(function_key(key(5, KeyModifiers::SHIFT)), Some(17));
        assert_eq!(function_key(key(25, KeyModifiers::NONE)), None);
    }
}
