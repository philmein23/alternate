//! The document in the window: its field, its file, and whether the extra features are showing.

use std::fs;
use std::path::{Path, PathBuf};

use rune::{FileDialogOptions, Key, KeyEvent, TextEdit, Tx};

use crate::file::{self, Problem};
use crate::model::Doc;
use crate::swap;
use crate::undo;
use crate::{Scene, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Plain,
    Active,
}

#[derive(Clone, Debug, Default)]
pub struct FileState {
    pub path: Option<PathBuf>,
    /// The file as last read or written, to tell whether the document has changed.
    pub saved: String,
    /// A comment that couldn't be read, written back on save.
    pub preserved: Option<String>,
    pub dirty: bool,
    pub notice: Option<String>,
}

pub fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

pub fn count_label(text: &str) -> String {
    let (w, c) = (words(text), text.chars().count());
    format!("{w} {} · {c} {}", if w == 1 { "word" } else { "words" }, if c == 1 { "character" } else { "characters" })
}

fn notice_for(problems: &[Problem]) -> Option<String> {
    if problems.contains(&Problem::UnreadableExtras) {
        return Some("Couldn't read this file's alternatives. Your text is here, and saving keeps them.".to_string());
    }
    match problems.len() {
        0 => None,
        1 => Some("One saved alternative no longer matches the text, so it was dropped.".to_string()),
        n => Some(format!("{n} saved alternatives no longer match the text, so they were dropped.")),
    }
}

fn file_options(title: &str, file_name: Option<String>) -> FileDialogOptions {
    FileDialogOptions { title: title.to_string(), file_name, ..Default::default() }.filter("Markdown", &["md", "markdown", "txt"])
}

impl State {
    pub fn with_doc(doc: Doc) -> State {
        let edit = TextEdit::new(doc.text.clone());
        let overflow_edit = TextEdit::new(doc.overflow.clone());
        let saved = file::serialize(&doc, None);
        State { edit, doc, file: FileState { saved, ..FileState::default() }, mode: Mode::Plain, marks_hidden: false, scene: Scene::App, hover: None, hit: Default::default(), fade: None, fade_now: 0.0, panel: None, spare_edit: TextEdit::new(""), history: Default::default(), sound: Default::default(), bursts: Vec::new(), menu: None, overflow_open: false, overflow_edit, kit: crate::ui::theme::kit(), ai: Default::default(), provider: crate::ai::settings::Configured::shared(Default::default()), settings_panel: None, pending: None, dance_now: 0.0, demo_asking: false }
    }

    /// Takes in the field after a change.
    pub fn sync_field(&mut self) {
        if self.doc.sync_text(&self.edit.text, Some(self.edit.focus)).is_some() {
            self.history.clear_redo();
            self.touch();
        }
    }

    /// Recomputes whether the document differs from its file.
    pub fn touch(&mut self) {
        self.file.dirty = self.serialized() != self.file.saved;
    }

    pub fn serialized(&self) -> String {
        file::serialize(&self.doc, self.file.preserved.as_deref())
    }

    pub fn toggle_mode(&mut self) {
        self.mode = if self.mode == Mode::Plain { Mode::Active } else { Mode::Plain };
    }

    pub fn load_file(&mut self, path: &Path, contents: &str) {
        let loaded = file::parse(contents);
        self.edit = TextEdit::new(loaded.doc.text.clone());
        self.edit.focused = true;
        self.overflow_edit = TextEdit::new(loaded.doc.overflow.clone());
        self.history = Default::default();
        self.doc = loaded.doc;
        self.file = FileState {
            path: Some(path.to_path_buf()),
            saved: contents.to_string(),
            preserved: loaded.preserved,
            dirty: false,
            notice: notice_for(&loaded.problems),
        };
    }

    pub fn open_path(&mut self, path: &Path) {
        match fs::read_to_string(path) {
            Ok(contents) => self.load_file(path, &contents),
            Err(e) => self.file.notice = Some(format!("Couldn't open that file: {e}")),
        }
    }

    pub fn save_to(&mut self, path: &Path) {
        let contents = self.serialized();
        match file::save_atomic(path, &contents) {
            Ok(()) => {
                self.file.path = Some(path.to_path_buf());
                self.file.saved = contents;
                self.file.dirty = false;
                self.file.notice = None;
            }
            Err(e) => self.file.notice = Some(format!("Couldn't save: {e}. Your text is still here.")),
        }
    }

    pub fn save(&mut self, tx: &mut Tx) {
        match self.file.path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(tx),
        }
    }

    pub fn save_as(&mut self, tx: &mut Tx) {
        tx.save_file(file_options("Save", Some("Untitled.md".to_string())), |s: &mut State, result, _| match result {
            Ok(Some(path)) => s.save_to(&path),
            Ok(None) => {}
            Err(e) => s.file.notice = Some(format!("Couldn't save: {e}. Your text is still here.")),
        });
    }

    pub fn open(&mut self, tx: &mut Tx) {
        tx.open_file(file_options("Open", None), |s: &mut State, result, _| match result {
            Ok(Some(path)) => s.open_path(&path),
            Ok(None) => {}
            Err(e) => s.file.notice = Some(format!("Couldn't open that file: {e}")),
        });
    }

    /// Keeps unsaved work when the window closes, since Rune's close hook can't ask first. The next
    /// launch brings it back.
    pub fn write_recovery(&self, dir: &Path) {
        let (text, path) = (dir.join("recovery.md"), dir.join("recovery.path"));
        if !self.file.dirty {
            let _ = fs::remove_file(text);
            let _ = fs::remove_file(path);
            return;
        }
        if fs::create_dir_all(dir).is_err() || file::save_atomic(&text, &self.serialized()).is_err() {
            return;
        }
        match &self.file.path {
            Some(original) => {
                let _ = fs::write(path, original.to_string_lossy().as_bytes());
            }
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// The unsaved work from last time, if any.
    pub fn from_recovery(dir: &Path) -> Option<State> {
        let contents = fs::read_to_string(dir.join("recovery.md")).ok()?;
        let original = fs::read_to_string(dir.join("recovery.path")).ok().map(PathBuf::from);
        let mut s = State::with_doc(Doc::default());
        let on_disk = original.as_deref().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
        s.load_file(original.as_deref().unwrap_or(Path::new("")), &contents);
        s.file.path = original;
        s.file.saved = on_disk;
        s.file.dirty = true;
        s.file.notice = Some("Restored your unsaved changes from last time.".to_string());
        Some(s)
    }
}

pub fn recovery_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Alternate"))
}

/// Keys that act on the whole document. Runs before the field and when nothing is focused.
pub fn handle_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if swap::handle_key(s, ev, tx) || undo::handle_key(s, ev, tx) {
        return true;
    }
    let Key::Text(k) = &ev.key else { return false };
    if ev.mods.ctrl && ev.mods.shift && k.eq_ignore_ascii_case("a") {
        return s.open_panel();
    }
    if ev.mods.ctrl && ev.mods.shift && k.eq_ignore_ascii_case("g") {
        return s.open_panel() && s.ask_ai();
    }
    if ev.mods.ctrl && k == "/" {
        return s.caret_range().map(|r| s.toggle_ghost(r)).is_some();
    }
    if ev.mods.ctrl && ev.mods.shift && k.eq_ignore_ascii_case("x") {
        return s.caret_range().map(|r| s.stash(r)).is_some();
    }
    if !ev.mods.cmd {
        return false;
    }
    match (k.as_str(), ev.mods.shift) {
        ("s", false) => s.save(tx),
        ("s", true) | ("S", true) => s.save_as(tx),
        ("o", false) => s.open(tx),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("alternate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(count_label(""), "0 words · 0 characters");
        assert_eq!(count_label("one"), "1 word · 3 characters");
        assert_eq!(count_label("two wörds\n"), "2 words · 10 characters");
    }

    #[test]
    fn opening_then_saving_round_trips_the_file() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("a.md");
        let mut first = Doc::new("A body.\n");
        first.overflow = "stash".to_string();
        let contents = file::serialize(&first, None);
        fs::write(&path, &contents).unwrap();
        let mut s = State::with_doc(Doc::default());
        s.open_path(&path);
        assert!(!s.file.dirty);
        assert_eq!(s.doc.overflow, "stash");
        s.save_to(&path);
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn typing_makes_it_dirty_and_undoing_the_text_clears_that() {
        let mut s = State::with_doc(Doc::new("abc"));
        s.edit.text = "abcd".to_string();
        s.edit.focus = 4;
        s.sync_field();
        assert!(s.file.dirty);
        s.edit.text = "abc".to_string();
        s.edit.focus = 3;
        s.sync_field();
        assert!(!s.file.dirty);
    }

    #[test]
    fn a_missing_file_says_so_and_keeps_the_work() {
        let mut s = State::with_doc(Doc::new("keep me"));
        s.open_path(Path::new("/nonexistent/alternate.md"));
        assert_eq!(s.doc.text, "keep me");
        assert!(s.file.notice.as_deref().unwrap().starts_with("Couldn't open"));
    }

    #[test]
    fn a_failed_save_stays_dirty() {
        let mut s = State::with_doc(Doc::new("keep me"));
        s.edit.text = "keep me!".to_string();
        s.edit.focus = 8;
        s.sync_field();
        s.save_to(Path::new("/nonexistent/dir/alternate.md"));
        assert!(s.file.dirty);
        assert!(s.file.notice.as_deref().unwrap().contains("still here"));
    }

    #[test]
    fn unsaved_work_comes_back_after_closing() {
        let dir = temp_dir("recovery");
        let mut s = State::with_doc(Doc::new("draft"));
        s.edit.text = "draft two".to_string();
        s.edit.focus = 9;
        s.sync_field();
        s.write_recovery(&dir);
        let back = State::from_recovery(&dir).unwrap();
        assert_eq!(back.edit.text, "draft two");
        assert!(back.file.dirty);
        assert_eq!(back.file.path, None);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closing_with_nothing_unsaved_clears_the_recovery() {
        let dir = temp_dir("clean");
        fs::write(dir.join("recovery.md"), "old").unwrap();
        State::with_doc(Doc::new("saved")).write_recovery(&dir);
        assert!(State::from_recovery(&dir).is_none());
        fs::remove_dir_all(dir).unwrap();
    }
}
