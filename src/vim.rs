//! Optional Vim motions on the page. Off by default, and always starts in Insert so typing works.

use std::fs;
use std::path::{Path, PathBuf};

use rune::{Key, KeyEvent, Mods, Tx};

use crate::State;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Insert,
    Normal,
    Visual,
}

#[derive(Debug, Default)]
pub struct Vim {
    pub on: bool,
    pub mode: Mode,
    count: u32,
    /// The first `g` of `gg`.
    pending_g: bool,
    dir: Option<PathBuf>,
}

const ON_FILE: &str = "vim-on";

impl Vim {
    pub fn load(dir: &Path) -> Vim {
        Vim { on: dir.join(ON_FILE).exists(), dir: Some(dir.to_path_buf()), ..Vim::default() }
    }

    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.mode = Mode::Insert;
        self.reset();
        if let Some(path) = self.dir.as_ref().map(|d| d.join(ON_FILE)) {
            let _ = if self.on { fs::write(path, b"") } else { fs::remove_file(path) };
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        self.pending_g = false;
    }

    fn take_count(&mut self) -> usize {
        let n = self.count.max(1) as usize;
        self.reset();
        n
    }
}

fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// The start of the next word.
pub fn next_word(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[at..].char_indices().peekable();
    if let Some(&(_, first)) = chars.peek() {
        let start = class(first, big);
        while start != 0 && chars.peek().is_some_and(|&(_, c)| class(c, big) == start) {
            chars.next();
        }
    }
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    chars.peek().map_or(text.len(), |&(i, _)| at + i)
}

/// The start of this word, or of the one before it.
pub fn prev_word(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    let Some(&(mut start, first)) = chars.peek() else { return 0 };
    let run = class(first, big);
    while let Some(&(i, c)) = chars.peek() {
        if class(c, big) != run {
            break;
        }
        start = i;
        chars.next();
    }
    start
}

/// Just after the end of this word, or the next one. The caret sits between letters, so this is
/// where the word visibly ends.
pub fn word_end(text: &str, at: usize, big: bool) -> usize {
    let mut chars = text[at..].char_indices().peekable();
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == 0) {
        chars.next();
    }
    let Some(&(_, first)) = chars.peek() else { return text.len() };
    let run = class(first, big);
    while chars.peek().is_some_and(|&(_, c)| class(c, big) == run) {
        chars.next();
    }
    chars.peek().map_or(text.len(), |&(i, _)| at + i)
}

/// The next blank line, or the end.
pub fn next_paragraph(text: &str, at: usize) -> usize {
    let from = at + text[at..].len() - text[at..].trim_start_matches('\n').len();
    text[from..].find("\n\n").map_or(text.len(), |i| from + i + 1)
}

/// The blank line before this paragraph, or the start.
pub fn prev_paragraph(text: &str, at: usize) -> usize {
    let end = text[..at].trim_end_matches('\n').len();
    text[..end].rfind("\n\n").map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

fn key(key: Key, mods: Mods) -> KeyEvent {
    KeyEvent { key, mods }
}

impl State {
    fn vim_mods(&self) -> Mods {
        Mods { shift: self.vim.mode == Mode::Visual, ..Mods::default() }
    }

    /// Goes to `target` with one real arrow key at the end, so the page scrolls to the caret.
    fn vim_jump(&mut self, target: usize, tx: &mut Tx) {
        let text = &self.edit.text;
        let (near, arrow) = if target > 0 {
            (text[..target].char_indices().next_back().map_or(0, |(i, _)| i), Key::Right)
        } else if let Some(c) = text.chars().next() {
            (c.len_utf8(), Key::Left)
        } else {
            return;
        };
        let anchor = if self.vim.mode == Mode::Visual { self.edit.anchor } else { near };
        self.edit.set_selection(anchor, near);
        tx.send_key(key(arrow, self.vim_mods()));
    }

    fn vim_repeat(&mut self, ev: KeyEvent, n: usize, tx: &mut Tx) {
        for _ in 0..n.min(self.edit.text.len().max(1)) {
            tx.send_key(ev.clone());
        }
    }

    fn vim_insert(&mut self) {
        self.vim.mode = Mode::Insert;
        self.vim.reset();
    }

    fn vim_open_line(&mut self, below: bool) {
        let at = self.edit.focus;
        let (pos, caret) = if below {
            let end = line_end(&self.edit.text, at);
            (end, end + 1)
        } else {
            let start = line_start(&self.edit.text, at);
            (start, start)
        };
        self.edit.set_selection(pos, pos);
        self.edit.insert("\n");
        self.edit.set_selection(caret, caret);
        self.sync_field();
        self.vim_insert();
    }

    fn vim_word(&mut self, motion: fn(&str, usize, bool) -> usize, big: bool, tx: &mut Tx) {
        let n = self.vim.take_count();
        let mut at = self.edit.focus;
        for _ in 0..n {
            at = motion(&self.edit.text, at, big);
        }
        self.vim_jump(at, tx);
    }

    fn vim_paragraph(&mut self, motion: fn(&str, usize) -> usize, tx: &mut Tx) {
        let n = self.vim.take_count();
        let mut at = self.edit.focus;
        for _ in 0..n {
            at = motion(&self.edit.text, at);
        }
        self.vim_jump(at, tx);
    }

    fn vim_char(&mut self, c: char, tx: &mut Tx) {
        if c.is_ascii_digit() && (c != '0' || self.vim.count > 0) {
            self.vim.count = self.vim.count.saturating_mul(10).saturating_add(c as u32 - '0' as u32).min(99_999);
            self.vim.pending_g = false;
            return;
        }
        if c == 'g' && !self.vim.pending_g {
            self.vim.pending_g = true;
            return;
        }
        let was_g = std::mem::take(&mut self.vim.pending_g);
        if was_g && c != 'g' {
            self.vim.reset();
            return;
        }
        let plain = self.vim_mods();
        let edge = Mods { cmd: true, ..plain };
        let at = self.edit.focus;
        match c {
            'h' if !self.edit.text[..at].ends_with('\n') => {
                let n = self.vim.take_count();
                self.vim_repeat(key(Key::Left, plain), n, tx);
            }
            'l' if !self.edit.text[at..].starts_with('\n') => {
                let n = self.vim.take_count();
                self.vim_repeat(key(Key::Right, plain), n, tx);
            }
            'j' => {
                let n = self.vim.take_count();
                self.vim_repeat(key(Key::Down, plain), n, tx);
            }
            'k' => {
                let n = self.vim.take_count();
                self.vim_repeat(key(Key::Up, plain), n, tx);
            }
            'w' => self.vim_word(next_word, false, tx),
            'W' => self.vim_word(next_word, true, tx),
            'b' => self.vim_word(prev_word, false, tx),
            'B' => self.vim_word(prev_word, true, tx),
            'e' => self.vim_word(word_end, false, tx),
            'E' => self.vim_word(word_end, true, tx),
            '}' => self.vim_paragraph(next_paragraph, tx),
            '{' => self.vim_paragraph(prev_paragraph, tx),
            '0' | '^' => tx.send_key(key(Key::Left, edge)),
            '$' => tx.send_key(key(Key::Right, edge)),
            'g' => {
                self.vim.reset();
                self.vim_jump(0, tx);
            }
            'G' => {
                self.vim.reset();
                let end = self.edit.text.len();
                self.vim_jump(end, tx);
            }
            'v' => {
                self.vim.mode = if self.vim.mode == Mode::Visual { Mode::Normal } else { Mode::Visual };
                if self.vim.mode == Mode::Normal {
                    self.edit.set_selection(at, at);
                }
                self.vim.reset();
            }
            'i' if self.vim.mode == Mode::Normal => self.vim_insert(),
            'a' if self.vim.mode == Mode::Normal => {
                if !self.edit.text[at..].starts_with('\n') {
                    tx.send_key(key(Key::Right, Mods::default()));
                }
                self.vim_insert();
            }
            'I' if self.vim.mode == Mode::Normal => {
                tx.send_key(key(Key::Left, edge));
                self.vim_insert();
            }
            'A' if self.vim.mode == Mode::Normal => {
                tx.send_key(key(Key::Right, edge));
                self.vim_insert();
            }
            'o' if self.vim.mode == Mode::Normal => self.vim_open_line(true),
            'O' if self.vim.mode == Mode::Normal => self.vim_open_line(false),
            _ => self.vim.reset(),
        }
    }
}

/// True when Vim took the key. Chords always go on, so every shortcut still works in Normal mode.
pub fn handle_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if !s.vim.on || ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    if s.vim.mode == Mode::Insert {
        if ev.key == Key::Escape {
            s.vim.mode = Mode::Normal;
            s.vim.reset();
            return true;
        }
        return false;
    }
    match &ev.key {
        Key::Escape => {
            if s.vim.mode == Mode::Visual {
                let at = s.edit.focus;
                s.edit.set_selection(at, at);
            }
            s.vim.mode = Mode::Normal;
            s.vim.reset();
            true
        }
        Key::Text(t) => {
            if let Some(h) = &mut s.hover {
                h.armed = false;
            }
            let mut chars = t.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                s.vim_char(c, tx);
            }
            true
        }
        Key::Enter | Key::Backspace | Key::Delete | Key::Tab => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "one two, three\n\nnext para here\nsecond\n\nlast";

    #[test]
    fn w_goes_to_word_starts_and_stops_on_punctuation() {
        assert_eq!(next_word(T, 0, false), 4);
        assert_eq!(next_word(T, 4, false), 7);
        assert_eq!(next_word(T, 7, false), 9);
        assert_eq!(next_word(T, 4, true), 9);
        assert_eq!(next_word("end", 1, false), 3);
    }

    #[test]
    fn b_goes_back_to_word_starts() {
        assert_eq!(prev_word(T, 9, false), 7);
        assert_eq!(prev_word(T, 7, false), 4);
        assert_eq!(prev_word(T, 9, true), 4);
        assert_eq!(prev_word(T, 6, false), 4);
        assert_eq!(prev_word(T, 0, false), 0);
    }

    #[test]
    fn e_lands_just_after_the_word_and_moves_on_when_repeated() {
        assert_eq!(word_end(T, 0, false), 3);
        assert_eq!(word_end(T, 3, false), 7);
        assert_eq!(word_end("word  ", 4, false), 6);
    }

    #[test]
    fn paragraphs_stop_on_blank_lines() {
        let first = next_paragraph(T, 0);
        assert_eq!(first, T.find("\n\n").unwrap() + 1);
        let second = next_paragraph(T, first);
        assert_eq!(second, T.rfind("\n\n").unwrap() + 1);
        assert_eq!(next_paragraph(T, second), T.len());
        assert_eq!(prev_paragraph(T, second), first);
        assert_eq!(prev_paragraph(T, first), 0);
    }

    #[test]
    fn motions_never_land_inside_a_character() {
        let text = "\u{1f600} caf\u{e9} \u{65e5}\u{672c}\n\n\u{5e9}\u{5dc}";
        for at in text.char_indices().map(|(i, _)| i) {
            for big in [false, true] {
                for got in [next_word(text, at, big), prev_word(text, at, big), word_end(text, at, big)] {
                    assert!(text.is_char_boundary(got), "{got}");
                }
            }
            assert!(text.is_char_boundary(next_paragraph(text, at)));
            assert!(text.is_char_boundary(prev_paragraph(text, at)));
        }
    }

    #[test]
    fn the_setting_is_remembered_but_each_launch_starts_in_insert() {
        let dir = std::env::temp_dir().join(format!("alternate-vim-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut v = Vim::load(&dir);
        assert!(!v.on);
        v.toggle();
        v.mode = Mode::Normal;
        let back = Vim::load(&dir);
        assert!(back.on);
        assert_eq!(back.mode, Mode::Insert);
        v.toggle();
        assert!(!Vim::load(&dir).on);
        fs::remove_dir_all(&dir).unwrap();
    }
}
