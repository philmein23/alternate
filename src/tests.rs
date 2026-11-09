use rune::{Env, Key, KeyEvent, Mods, Runtime};

use crate::editor::Mode;
use crate::model::Doc;
use crate::{State, fonts, view};

pub fn runtime(state: State) -> Runtime<State> {
    let mut rt = Runtime::with_text(state, view, (1200.0, 800.0), Env::new(true, 2.0), fonts::text_system());
    rt.frame(0.0);
    rt.frame(1.0 / 60.0);
    rt
}

pub fn click<S: 'static>(rt: &mut Runtime<S>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is on screen"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
}

pub fn press<S: 'static>(rt: &mut Runtime<S>, key: Key) {
    rt.key(KeyEvent { key, mods: Mods::default() });
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
}

#[test]
fn typing_reaches_the_document() {
    let mut rt = runtime(State::with_doc(Doc::default()));
    click(&mut rt, "doc");
    press(&mut rt, Key::Text("a".to_string()));
    assert_eq!(rt.state.edit.text, "a");
    assert_eq!(rt.state.doc.text, "a");
    assert!(rt.state.file.dirty);
}

#[test]
fn clicking_the_count_shows_the_file_controls() {
    let mut rt = runtime(State::with_doc(Doc::new("words")));
    assert!(rt.find("save").is_none());
    click(&mut rt, "count");
    assert_eq!(rt.state.mode, Mode::Active);
    assert!(rt.find("save").is_some());
    click(&mut rt, "count");
    assert!(rt.find("save").is_none());
}

#[test]
fn embedded_families_resolve() {
    let text = fonts::text_system();
    drop(text);
    assert!(fonts::FONTS.iter().all(|f| f.len() > 10_000), "every font is real data");
}

#[test]
fn the_wheel_scrolls_the_whole_page() {
    let body = "A line of writing that goes on for a while.\n\n".repeat(120);
    let mut rt = runtime(State::with_doc(Doc::new(body)));
    let (_, before) = rt.find("doc").unwrap();
    rt.wheel(600.0, 400.0, -600.0);
    for _ in 0..120 {
        rt.frame(1.0 / 60.0);
    }
    let (_, after) = rt.find("doc").unwrap();
    assert!(after < before - 300.0, "the document moved up: {before} to {after}");
}
