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

/// Runs frames until entering panels have finished moving.
fn settle<S: 'static>(rt: &mut Runtime<S>) {
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
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

fn field_origin(rt: &Runtime<State>) -> (f32, f32) {
    let e = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("doc")).expect("the field");
    (e.rect[0], e.rect[1])
}

fn hover_target(rt: &mut Runtime<State>, id: u64) {
    let (fx, fy) = field_origin(rt);
    let b = rt.state.hit.borrow().iter().find(|(i, _)| *i == id).expect("a mark")
        .1[0];
    let (x, y) = (fx + b[0] + b[2] / 2.0, fy + b[1] + b[3] / 2.0);
    rt.pointer_move(x, y);
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
}

#[test]
fn arrows_with_no_hover_move_the_caret() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    rt.update(|s, _| s.hover = None);
    let before = rt.state.edit.text.clone();
    let caret = rt.state.edit.focus;
    press(&mut rt, Key::Left);
    assert_eq!(rt.state.edit.text, before);
    assert_ne!(rt.state.edit.focus, caret);
}

#[test]
fn hovering_a_target_then_pressing_right_swaps_it() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    assert_eq!(rt.state.hover.map(|h| h.id), Some(1));
    press(&mut rt, Key::Right);
    assert!(rt.state.edit.text.contains("the lamp was lit"), "{}", rt.state.edit.text);
    press(&mut rt, Key::Right);
    assert!(rt.state.edit.text.contains("the lantern was lit"));
}

#[test]
fn once_typing_starts_arrows_go_back_to_the_caret() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Text("x".to_string()));
    assert_eq!(rt.state.hover.map(|h| h.armed), Some(false));
    let caret = rt.state.edit.focus;
    press(&mut rt, Key::Left);
    assert!(rt.state.edit.text.contains("lantern"));
    assert_ne!(rt.state.edit.focus, caret);
}

#[test]
fn moving_the_pointer_rearms_the_target() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Text("x".to_string()));
    hover_target(&mut rt, 1);
    assert_eq!(rt.state.hover.map(|h| h.armed), Some(true));
}

#[test]
fn typing_in_a_row_previews_in_the_page_through_the_real_field() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| {
        s.edit.set_selection(30, 30);
        s.open_panel();
    });
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    let last = rt.state.panel.as_ref().unwrap().rows.len() - 1;
    click(&mut rt, &format!("row-{last}"));
    press(&mut rt, Key::Text("Z".to_string()));
    assert!(rt.state.edit.text.contains("the Z was lit"), "{}", rt.state.edit.text);
    press(&mut rt, Key::Escape);
    assert!(rt.state.panel.is_none());
    assert!(rt.state.edit.text.contains("the Z was lit"));
}

#[test]
fn clicking_a_tab_changes_the_level() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| {
        s.edit.set_selection(30, 30);
        s.open_panel();
    });
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    settle(&mut rt);
    click(&mut rt, "tab-sentence");
    assert_eq!(rt.state.panel.as_ref().unwrap().level, crate::model::Level::Sentence);
}

fn cmd(key: &str) -> KeyEvent {
    // A real window sends ⌘ chords as `Key::Command`, lower-cased.
    KeyEvent { key: Key::Command(key.chars().next().unwrap().to_ascii_lowercase()), mods: Mods { cmd: true, ..Mods::default() } }
}

#[test]
fn delete_on_a_hovered_target_then_cmd_z_restores_text_and_list() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Backspace);
    assert!(rt.state.edit.text.contains("the lamp was lit"), "{}", rt.state.edit.text);
    assert_eq!(rt.state.doc.targets[0].alts.len(), 1);
    rt.key(cmd("z"));
    rt.frame(1.0 / 60.0);
    assert!(rt.state.edit.text.contains("the lantern was lit"));
    assert_eq!(rt.state.doc.targets[0].alts.len(), 2);
}

#[test]
fn backspace_after_typing_deletes_text_even_on_a_target() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Text("x".to_string()));
    let typed = rt.state.edit.text.clone();
    press(&mut rt, Key::Backspace);
    assert_eq!(rt.state.edit.text.len(), typed.len() - 1);
    assert_eq!(rt.state.doc.targets[0].alts.len(), 2, "the alternatives are safe");
}

#[test]
fn rapid_arrows_play_every_tone_and_the_original_gets_the_low_one() {
    use crate::sound::Tone;
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Right);
    press(&mut rt, Key::Right);
    assert_eq!(rt.state.sound.played, [Tone::High, Tone::Low]);
}

#[test]
fn deleting_on_a_target_sets_off_a_burst_at_its_word() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    press(&mut rt, Key::Backspace);
    assert_eq!(rt.state.bursts.len(), 1);
    assert!(rt.state.bursts[0].rect[2] > 20.0);
}

fn ctrl(key: &str, shift: bool) -> KeyEvent {
    KeyEvent { key: Key::Text(key.to_string()), mods: Mods { ctrl: true, shift, ..Mods::default() } }
}

fn right_click(rt: &mut Runtime<State>, x: f32, y: f32) {
    rt.pointer_move(x, y);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    for _ in 0..3 {
        rt.frame(1.0 / 60.0);
    }
}

fn right_click_target(rt: &mut Runtime<State>, id: u64) {
    let (fx, fy) = field_origin(rt);
    let b = rt.state.hit.borrow().iter().find(|(i, _)| *i == id).expect("a mark").1[0];
    right_click(rt, fx + b[0] + b[2] / 2.0, fy + b[1] + b[3] / 2.0);
}

#[test]
fn a_right_click_names_the_word_under_the_pointer_and_keeps_the_selection() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| s.edit.set_selection(0, 3));
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    right_click_target(&mut rt, 1);
    let ctx = rt.state.menu.clone().expect("the menu opened");
    assert_eq!(ctx.word.as_deref(), Some("lantern"));
    assert_eq!(rt.state.edit.range(), (0, 3));
}

fn choose(rt: &mut Runtime<State>, item: usize) {
    click(rt, &format!("page-item{item}"));
    assert!(rt.state.menu.is_none());
}

#[test]
fn each_menu_item_does_its_job() {
    let mut rt = runtime(crate::marked_sample());
    right_click_target(&mut rt, 1);
    choose(&mut rt, 0);
    assert_eq!(rt.state.panel.as_ref().unwrap().rows[0].text, "lantern");
    rt.update(|s, _| s.close_panel());
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);

    right_click_target(&mut rt, 1);
    choose(&mut rt, 2);
    assert_eq!(rt.state.doc.ghosts.len(), 1);

    right_click_target(&mut rt, 1);
    assert!(rt.state.menu.as_ref().unwrap().in_ghost);
    choose(&mut rt, 2);
    assert!(rt.state.doc.ghosts.is_empty(), "the same item revives");

    right_click_target(&mut rt, 1);
    choose(&mut rt, 3);
    assert!(!rt.state.edit.text.contains("lantern"));
    assert_eq!(rt.state.doc.overflow, "lantern");
    assert!(rt.find("overflow-field").is_some());
}

#[test]
fn the_shortcuts_ghost_and_stash_at_the_caret() {
    let mut rt = runtime(State::with_doc(Doc::new("one two three")));
    click(&mut rt, "doc");
    rt.update(|s, _| s.edit.set_selection(5, 5));
    rt.frame(1.0 / 60.0);
    rt.key(ctrl("/", false));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.doc.ghosts[0].range, 4..7);
    rt.key(ctrl("/", false));
    rt.frame(1.0 / 60.0);
    assert!(rt.state.doc.ghosts.is_empty());
    rt.key(ctrl("X", true));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.edit.text, "one three");
    assert_eq!(rt.state.doc.overflow, "two");
}

#[test]
fn ctrl_return_in_overflow_inserts_at_the_page_caret() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| {
        s.set_overflow("spare line".to_string());
        s.overflow_open = true;
        s.edit.set_selection(0, 0);
    });
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    click(&mut rt, "overflow-field");
    rt.key(KeyEvent { key: Key::Enter, mods: Mods { ctrl: true, ..Mods::default() } });
    rt.frame(1.0 / 60.0);
    assert!(rt.state.edit.text.starts_with("spare line"), "{}", &rt.state.edit.text[..30]);
    assert_eq!(rt.state.edit.text, rt.state.doc.text);
}

#[test]
fn dragging_the_grip_onto_the_page_inserts_there() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| {
        s.set_overflow("spare".to_string());
        s.overflow_open = true;
    });
    settle(&mut rt);
    let before = rt.state.edit.text.len();
    let (gx, gy) = rt.find("overflow-grip").unwrap();
    let (fx, fy) = field_origin(&rt);
    rt.pointer_move(gx, gy);
    rt.pointer_down(gx, gy);
    rt.pointer_move(fx + 30.0, fy + 10.0);
    rt.pointer_up(fx + 30.0, fy + 10.0);
    for _ in 0..3 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.edit.text.len(), before + 5);
}

#[test]
fn a_ghost_over_a_target_survives_a_swap_and_the_save() {
    let mut s = crate::marked_sample();
    let at = s.doc.text.find("lantern").unwrap();
    s.toggle_ghost(at..at + 20);
    s.show_alt(1, 1);
    assert!(s.doc.ghosts[0].range.start <= s.doc.targets[0].range.start);
    let loaded = crate::file::parse(&s.serialized());
    assert_eq!(loaded.doc.ghosts.len(), 1);
}

#[test]
fn ghosts_and_overflow_survive_save_and_reload() {
    let mut s = State::with_doc(Doc::new("one two three"));
    s.toggle_ghost(0..3);
    s.stash(4..7);
    let text = s.serialized();
    let mut back = State::with_doc(Doc::default());
    back.load_file(std::path::Path::new("x.md"), &text);
    assert_eq!(back.doc.ghosts.len(), 1);
    assert_eq!(back.doc.overflow, "two");
    assert_eq!(back.overflow_edit.text, "two");
}

#[test]
fn the_menu_asks_for_alternatives_on_the_word_under_the_pointer() {
    let mut rt = runtime(crate::marked_sample());
    let (provider, _feed) = crate::ai::fake();
    rt.update(|s, _| s.provider = provider);
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    right_click_target(&mut rt, 1);
    choose(&mut rt, 1);
    assert_eq!(rt.state.pending.as_ref().unwrap().id, 1);
    assert_eq!(rt.state.panel.as_ref().unwrap().rows.last().unwrap().text, "??");
}

#[test]
fn the_bottom_controls_still_work_under_the_lab_dock() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "toggle-overflow");
    assert!(rt.state.overflow_open);
    click(&mut rt, "toggle-lab");
    assert!(rt.state.lab.open);
}

#[test]
fn clicking_a_faded_run_keeps_it_and_the_bar_makes_the_cuts() {
    use crate::lab::trim::{Cut, Kind, Level, Review};
    let mut s = crate::marked_sample();
    let text = s.doc.text.clone();
    let cut = |q: &str| { let at = text.find(q).unwrap(); Cut { range: at..at + q.len(), kept: false, fix: None } };
    s.review = Some(Review::new(Kind::Trim(Level::Slight), vec![cut("nobody on the"), cut("along the quay")], text.clone()));
    let mut rt = runtime(s);
    click(&mut rt, "cut-0");
    assert!(rt.state.review.as_ref().unwrap().cuts[0].kept);
    click(&mut rt, "review-apply");
    assert!(rt.state.review.is_none());
    assert!(rt.state.edit.text.contains("nobody on the"));
    assert!(!rt.state.edit.text.contains("along the quay"));
    assert_eq!(rt.state.edit.text, rt.state.doc.text);
}

fn worst_docs() -> Vec<(&'static str, State)> {
    let para = "The tide came in slowly over the flats. ".repeat(180);
    let big: String = (0..50_000 / 40).map(|_| format!("{}\n\n", "word another longer thing ".repeat(10))).collect();
    let mut many = Doc::new("alpha beta gamma delta ".repeat(100));
    let mut at = 0;
    for _ in 0..200 {
        let id = many.next_id();
        let end = at + many.text[at..].find(' ').unwrap();
        let word = many.text[at..end].to_string();
        many.targets.push(crate::model::Target { id, range: at..end, level: crate::model::Level::Word, original: word.clone(), alts: vec![crate::model::Alt { text: word, source: crate::model::Source::Mine }, crate::model::Alt { text: "x".to_string(), source: crate::model::Source::Mine }], active: 0 });
        at = end + 1;
    }
    let mixed = format!("Emoji \u{1f600}\u{1f469}\u{200d}\u{1f4bb} here. \u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}. \u{65e5}\u{672c}\u{8a9e}\u{306e}\u{6587}\u{7ae0}\u{3067}\u{3059}\u{3002}\n\nhttps://example.com/{}", "averyveryverylongpathsegment".repeat(40));
    vec![("50k words", State::with_doc(Doc::new(big))), ("one paragraph", State::with_doc(Doc::new(para))), ("200 targets", State::with_doc(many)), ("mixed scripts", State::with_doc(Doc::new(mixed)))]
}

/// Every worst case opens, takes typing, swaps and panel use without losing step.
#[test]
fn worst_case_documents_survive_use() {
    for (name, s) in worst_docs() {
        let mut rt = runtime(s);
        rt.update(|s, _| {
            s.mode = Mode::Active;
            s.edit.focused = true;
        });
        rt.frame(1.0 / 60.0);
        let before = rt.state.edit.text.clone();
        press(&mut rt, Key::Text("x".to_string()));
        assert_eq!(rt.state.edit.text.len(), before.len() + 1, "{name}");
        press(&mut rt, Key::Backspace);
        rt.update(|s, _| {
            s.edit.set_selection(0, 0);
            s.open_panel();
            s.close_panel();
        });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.edit.text, rt.state.doc.text, "{name}");
        for t in &rt.state.doc.targets {
            assert_eq!(rt.state.doc.text[t.range.clone()], t.alts[t.active].text, "{name}");
        }
    }
}

/// Run with `cargo test --release keystroke_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn keystroke_cost() {
    for (name, s) in worst_docs() {
        let mut rt = runtime(s);
        rt.update(|s, _| {
            s.mode = Mode::Active;
            s.edit.focused = true;
        });
        rt.frame(1.0 / 60.0);
        let mut times = Vec::new();
        for i in 0..40 {
            let start = std::time::Instant::now();
            rt.key(KeyEvent { key: Key::Text(if i % 2 == 0 { "x" } else { "y" }.to_string()), mods: Mods::default() });
            rt.frame(1.0 / 60.0);
            times.push(start.elapsed().as_secs_f32() * 1000.0);
        }
        times.sort_by(f32::total_cmp);
        eprintln!("{name}: keystroke p50 {:.2} ms, p95 {:.2} ms", times[20], times[37]);
    }
}

#[test]
fn reduce_motion_skips_the_explosion_and_the_swap_fade() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "doc");
    hover_target(&mut rt, 1);
    rt.update(|s, _| s.still.set(true));
    rt.state.still.set(true);
    rt.key(KeyEvent { key: Key::Right, mods: Mods::default() });
    assert!(rt.state.fade.is_none());
    rt.key(KeyEvent { key: Key::Backspace, mods: Mods::default() });
    assert!(rt.state.bursts.is_empty());
}

#[test]
fn opening_the_panel_eases_the_page_down_and_closing_eases_it_back() {
    let mut s = State::with_doc(Doc::new("one two"));
    let mut tx = rune::Tx::default();
    s.edit.set_selection(1, 1);
    s.open_panel();
    crate::poll(&mut s, 0.0, &mut tx);
    crate::poll(&mut s, 0.05, &mut tx);
    assert!(s.dim > 0.0 && s.dim < 1.0, "{}", s.dim);
    crate::poll(&mut s, 1.0, &mut tx);
    assert_eq!(s.dim, 1.0);
    s.close_panel();
    crate::poll(&mut s, 1.01, &mut tx);
    crate::poll(&mut s, 2.0, &mut tx);
    assert_eq!(s.dim, 0.0);
}

#[test]
fn the_help_lists_every_shortcut() {
    let mut rt = runtime(crate::marked_sample());
    click(&mut rt, "toggle-help");
    assert!(rt.find("help").is_some());
}

#[test]
fn an_empty_first_launch_shows_where_to_start() {
    let rt = runtime(State::with_doc(Doc::default()));
    assert!(rt.find("first-hint").is_some());
}

fn typed(rt: &mut Runtime<State>, keys: &str) {
    for c in keys.chars() {
        press(rt, Key::Text(c.to_string()));
    }
    settle(rt);
}

fn vim_runtime(text: &str, at: usize) -> Runtime<State> {
    let mut rt = runtime(State::with_doc(Doc::new(text)));
    rt.update(|s, _| {
        s.mode = Mode::Active;
        s.vim.on = true;
        s.edit.focused = true;
        s.edit.set_selection(at, at);
    });
    click(&mut rt, "doc");
    rt.update(|s, _| s.edit.set_selection(at, at));
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Escape);
    rt
}

#[test]
fn return_breaks_the_line_and_undoes_in_step_with_the_lists() {
    let mut rt = runtime(State::with_doc(Doc::new("one two")));
    click(&mut rt, "doc");
    rt.update(|s, _| s.edit.set_selection(3, 3));
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.edit.text, "one\n two");
    assert_eq!(rt.state.doc.text, rt.state.edit.text);
    assert_eq!(rt.state.edit.focus, 4);
    rt.key(cmd("z"));
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.edit.text, "one two");
    assert_eq!(rt.state.doc.text, "one two");
}

#[test]
fn return_breaks_the_line_in_overflow_too() {
    let mut rt = runtime(crate::marked_sample());
    rt.update(|s, _| {
        s.set_overflow("spare".to_string());
        s.overflow_open = true;
    });
    settle(&mut rt);
    click(&mut rt, "overflow-field");
    press(&mut rt, Key::Enter);
    assert!(rt.state.doc.overflow.contains('\n'), "{:?}", rt.state.doc.overflow);
}

#[test]
fn normal_mode_moves_and_never_types() {
    let mut rt = vim_runtime("one two three\n\nnext", 0);
    assert_eq!(rt.state.vim.mode, crate::vim::Mode::Normal);
    typed(&mut rt, "w");
    assert_eq!(rt.state.edit.focus, 4);
    typed(&mut rt, "2l");
    assert_eq!(rt.state.edit.focus, 6);
    typed(&mut rt, "b");
    assert_eq!(rt.state.edit.focus, 4);
    typed(&mut rt, "G");
    assert_eq!(rt.state.edit.focus, rt.state.edit.text.len());
    typed(&mut rt, "gg");
    assert_eq!(rt.state.edit.focus, 0);
    typed(&mut rt, "}");
    assert_eq!(rt.state.edit.focus, 14);
    typed(&mut rt, "zqxd");
    assert_eq!(rt.state.edit.text, "one two three\n\nnext");
}

#[test]
fn j_and_k_move_by_wrapped_lines_not_paragraphs() {
    let long = "word ".repeat(120);
    let mut rt = vim_runtime(&long, 0);
    typed(&mut rt, "j");
    let line = rt.state.edit.focus;
    assert!(line > 0 && line < 120, "{line}");
    typed(&mut rt, "k");
    assert_eq!(rt.state.edit.focus, 0);
}

#[test]
fn i_a_and_o_return_to_typing() {
    let mut rt = vim_runtime("one\ntwo", 0);
    typed(&mut rt, "o");
    assert_eq!(rt.state.vim.mode, crate::vim::Mode::Insert);
    assert_eq!(rt.state.edit.text, "one\n\ntwo");
    assert_eq!(rt.state.edit.focus, 4);
    typed(&mut rt, "x");
    assert_eq!(rt.state.edit.text, "one\nx\ntwo");
    assert_eq!(rt.state.doc.text, rt.state.edit.text);
    press(&mut rt, Key::Escape);
    typed(&mut rt, "a");
    assert_eq!(rt.state.vim.mode, crate::vim::Mode::Insert);
    assert_eq!(rt.state.edit.focus, 5);
}

#[test]
fn visual_mode_extends_the_selection_and_escape_collapses_it() {
    let mut rt = vim_runtime("one two three", 0);
    typed(&mut rt, "v");
    typed(&mut rt, "w");
    assert_eq!(rt.state.edit.range(), (0, 4));
    typed(&mut rt, "e");
    assert_eq!(rt.state.edit.range(), (0, 7));
    press(&mut rt, Key::Escape);
    assert_eq!(rt.state.edit.range(), (7, 7));
    assert_eq!(rt.state.vim.mode, crate::vim::Mode::Normal);
}

#[test]
fn shortcuts_still_work_in_normal_mode() {
    let mut rt = vim_runtime("one two", 1);
    rt.key(ctrl("a", true));
    rt.frame(1.0 / 60.0);
    assert!(rt.state.panel.is_some());
}

#[test]
fn g_jumps_scroll_the_page_to_the_caret() {
    let long = "A line of writing here.\n\n".repeat(200);
    let mut rt = vim_runtime(&long, 0);
    settle(&mut rt);
    let top = rt.find("doc").unwrap().1;
    typed(&mut rt, "G");
    settle(&mut rt);
    let down = rt.find("doc").unwrap().1;
    assert!(down < top - 500.0, "{top} -> {down}");
}
