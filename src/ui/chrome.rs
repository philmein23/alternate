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
    let vim_label = if s.vim.on { "Vim on" } else { "Vim off" };
    (s.mode == Mode::Active).then(|| row().gap(16.0).child(control("Save", "save", |s, tx| s.save(tx))).child(control("Open", "open", |s, tx| s.open(tx))).child(control(vim_label, "toggle-vim", |s, _| s.vim.toggle())).child(control("Post to X", "post-x", |s, tx| s.post_to_x(tx))).child(control(sound, "toggle-sound", |s, _| s.sound.toggle())).child(control("AI", "open-settings", |s, _| s.toggle_settings())))
}

/// Hides the marks so the page can be read clean. The alternatives stay.
pub fn marks_toggle(s: &State) -> Option<El> {
    let label = if s.marks_hidden { "· · · show marks" } else { "· · ·" };
    (s.mode == Mode::Active).then(|| control(label, "toggle-marks", |s, _| s.marks_hidden = !s.marks_hidden))
}

pub fn overflow_toggle(s: &State) -> Option<El> {
    let color = if s.overflow_open { THEME.accent.color() } else { THEME.faint.color() };
    (s.mode == Mode::Active).then(|| text("Overflow".to_string()).font(theme::small()).color(color).key("toggle-overflow").on_press_tx(|s: &mut State, _| s.toggle_overflow()))
}

/// Which Vim mode the page is in. Shown even with the other controls hidden.
pub fn vim_mode(s: &State) -> Option<El> {
    use crate::vim::Mode as M;
    if !s.vim.on {
        return None;
    }
    let (label, color) = match s.vim.mode {
        M::Insert => ("INSERT", THEME.faint.color()),
        M::Normal => ("NORMAL", THEME.accent.color()),
        M::Visual => ("VISUAL", THEME.accent.color()),
    };
    Some(text(label.to_string()).font(theme::small()).color(color).key("vim-mode"))
}

pub fn notice(s: &State) -> Option<El> {
    s.file.notice.as_ref().map(|n| text(n.clone()).font(theme::small()).color(THEME.marker.color()).key("notice"))
}
