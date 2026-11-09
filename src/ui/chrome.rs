//! The quiet controls around the page.

use rune::{El, row, text};

use crate::State;
use crate::editor::{self, Mode};
use crate::ui::theme::{self, THEME};

fn control(label: &str, key: &'static str, press: fn(&mut State, &mut rune::Tx)) -> El {
    text(label.to_string()).font(theme::small()).color(THEME.faint.color()).key(key).on_press_tx(move |s: &mut State, tx| press(s, tx))
}

/// The word count. Pressing it turns the extra features on and off.
pub fn count(s: &State) -> El {
    let mark = if s.file.dirty { " · edited" } else { "" };
    text(format!("{}{mark}", editor::count_label(&s.edit.text))).font(theme::small()).color(THEME.faint.color()).key("count").on_press_tx(|s: &mut State, _| s.toggle_mode())
}

pub fn file_controls(s: &State) -> Option<El> {
    let sound = if s.sound.off { "Sound off" } else { "Sound on" };
    (s.mode == Mode::Active).then(|| row().gap(16.0).child(control("Save", "save", |s, tx| s.save(tx))).child(control("Open", "open", |s, tx| s.open(tx))).child(control(sound, "toggle-sound", |s, _| s.sound.toggle())))
}

/// Hides the marks so the page can be read clean. The alternatives stay.
pub fn marks_toggle(s: &State) -> Option<El> {
    let label = if s.marks_hidden { "· · · show marks" } else { "· · ·" };
    (s.mode == Mode::Active).then(|| control(label, "toggle-marks", |s, _| s.marks_hidden = !s.marks_hidden))
}

pub fn notice(s: &State) -> Option<El> {
    s.file.notice.as_ref().map(|n| text(n.clone()).font(theme::small()).color(THEME.marker.color()).key("notice"))
}
