//! Every keyboard command in one table: the handlers and the help both read from it.

use rune::{Key, KeyEvent, Tx};

use crate::State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Alternatives,
    AiAlternatives,
    Ghost,
    Stash,
    Save,
    SaveAs,
    Open,
    Undo,
    Redo,
}

pub struct Shortcut {
    pub action: Action,
    key: char,
    ctrl: bool,
    shift: bool,
    cmd: bool,
    pub keys: &'static str,
    pub does: &'static str,
}

const fn shortcut(action: Action, key: char, ctrl: bool, shift: bool, cmd: bool, keys: &'static str, does: &'static str) -> Shortcut {
    Shortcut { action, key, ctrl, shift, cmd, keys, does }
}

pub const SHORTCUTS: &[Shortcut] = &[
    shortcut(Action::Alternatives, 'a', true, true, false, "\u{2303}\u{21e7}A", "Alternatives"),
    shortcut(Action::AiAlternatives, 'g', true, true, false, "\u{2303}\u{21e7}G", "AI alternatives"),
    shortcut(Action::Ghost, '/', true, false, false, "\u{2303}/", "Ghost or revive"),
    shortcut(Action::Stash, 'x', true, true, false, "\u{2303}\u{21e7}X", "Stash in Overflow"),
    shortcut(Action::Save, 's', false, false, true, "\u{2318}S", "Save"),
    shortcut(Action::SaveAs, 's', false, true, true, "\u{2318}\u{21e7}S", "Save as"),
    shortcut(Action::Open, 'o', false, false, true, "\u{2318}O", "Open"),
    shortcut(Action::Undo, 'z', false, false, true, "\u{2318}Z", "Undo"),
    shortcut(Action::Redo, 'z', false, true, true, "\u{2318}\u{21e7}Z", "Redo"),
];

/// Keys that depend on where the pointer or focus is, so they're handled where they apply.
pub const IN_PLACE: &[(&str, &str)] = &[("\u{2190} \u{2192}", "Swap, with the pointer on a marked word"), ("\u{232b}", "Delete that alternative"), ("\u{2303}\u{23ce}", "Use Overflow text, from Overflow"), ("??", "Ask Claude, in the empty row")];

/// The label for a command, for the menu.
pub fn keys(action: Action) -> &'static str {
    SHORTCUTS.iter().find(|s| s.action == action).map_or("", |s| s.keys)
}

pub fn find(ev: &KeyEvent) -> Option<Action> {
    // A real window sends ⌘ chords as `Key::Command`, and Control chords as text.
    let k = match &ev.key {
        Key::Command(c) => *c,
        Key::Text(t) => {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => c,
                _ => return None,
            }
        }
        _ => return None,
    };
    SHORTCUTS.iter().find(|s| k.eq_ignore_ascii_case(&s.key) && s.ctrl == ev.mods.ctrl && s.shift == ev.mods.shift && s.cmd == ev.mods.cmd && !ev.mods.alt).map(|s| s.action)
}

/// Runs a command. False means it didn't apply, and the key should carry on to the field.
pub fn run(s: &mut State, action: Action, tx: &mut Tx) -> bool {
    match action {
        Action::Alternatives => return s.open_panel(),
        Action::AiAlternatives => return s.open_panel() && s.ask_ai(),
        Action::Ghost => return s.caret_range().map(|r| s.toggle_ghost(r)).is_some(),
        Action::Stash => return s.caret_range().map(|r| s.stash(r)).is_some(),
        Action::Save => s.save(tx),
        Action::SaveAs => s.save_as(tx),
        Action::Open => s.open(tx),
        Action::Undo | Action::Redo if s.panel.is_some() => return false,
        Action::Undo => s.undo(),
        Action::Redo => s.redo(),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    /// Shaped as a real window sends it: `Key::Command` under ⌘, text otherwise.
    fn ev(key: char, mods: Mods) -> KeyEvent {
        KeyEvent { key: if mods.cmd { Key::Command(key.to_ascii_lowercase()) } else { Key::Text(key.to_string()) }, mods }
    }

    #[test]
    fn each_chord_finds_only_its_own_command() {
        for s in SHORTCUTS {
            let mods = Mods { ctrl: s.ctrl, shift: s.shift, cmd: s.cmd, ..Mods::default() };
            assert_eq!(find(&ev(s.key, mods)), Some(s.action), "{}", s.keys);
        }
    }

    #[test]
    fn no_two_commands_share_a_chord() {
        let mut seen = std::collections::HashSet::new();
        for s in SHORTCUTS {
            assert!(seen.insert((s.key, s.ctrl, s.shift, s.cmd)), "{}", s.keys);
        }
    }

    #[test]
    fn plain_typing_is_no_command() {
        assert_eq!(find(&ev('a', Mods::default())), None);
        assert_eq!(find(&ev('/', Mods::default())), None);
    }

    #[test]
    fn a_capital_from_shift_still_matches() {
        assert_eq!(find(&ev('Z', Mods { cmd: true, shift: true, ..Mods::default() })), Some(Action::Redo));
    }
}
