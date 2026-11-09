use rune::{Env, Key, KeyEvent, Mods, Runtime};

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
    let mut rt = runtime(State { edit: rune::TextEdit::new("") });
    click(&mut rt, "doc");
    press(&mut rt, Key::Text("a".to_string()));
    assert_eq!(rt.state.edit.text, "a");
}

#[test]
fn embedded_families_resolve() {
    let text = fonts::text_system();
    drop(text);
    assert!(fonts::FONTS.iter().all(|f| f.len() > 10_000), "every font is real data");
}
