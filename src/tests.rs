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

fn forty_target_doc() -> State {
    use crate::model::{Alt, Level, Source, Target};
    let body = "Some words to mark along this line of writing here. ".repeat(40);
    let mut doc = Doc::new(body.clone());
    for (i, (start, _)) in body.match_indices("words").enumerate() {
        let id = doc.next_id();
        doc.targets.push(Target { id, range: start..start + 5, level: if i % 2 == 0 { Level::Word } else { Level::Sentence }, original: "words".into(), alts: vec![Alt { text: "words".into(), source: Source::Mine }], active: 0 });
    }
    let mut s = State::with_doc(doc);
    s.mode = Mode::Active;
    s
}

#[test]
fn forty_marks_stay_cheap_while_typing() {
    let mut rt = runtime(forty_target_doc());
    assert_eq!(rt.state.doc.targets.len(), 40);
    click(&mut rt, "doc");
    let start = std::time::Instant::now();
    for _ in 0..60 {
        press(&mut rt, Key::Text("x".to_string()));
    }
    let per_key = start.elapsed().as_secs_f64() * 1000.0 / 60.0;
    eprintln!("forty marks: {per_key:.2} ms per keystroke");
    assert!(per_key < 16.0, "{per_key:.2} ms per keystroke");
    assert_eq!(rt.state.doc.targets.len(), 40);
}

#[test]
fn the_dots_button_hides_the_marks_and_keeps_the_alternatives() {
    let mut rt = runtime(crate::marked_sample());
    assert!(rt.find("marks").is_some());
    click(&mut rt, "toggle-marks");
    assert!(rt.find("marks").is_none());
    assert_eq!(rt.state.doc.targets.len(), 4);
}
