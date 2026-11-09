//! Alternate. `cargo run` opens the app, `cargo run -- --snap out.png --scene active` renders headless.

mod editor;
#[allow(dead_code)] // Grows into use as the features arrive.
mod file;
mod fonts;
#[allow(dead_code)]
mod model;
#[allow(dead_code)]
mod rebase;
#[allow(dead_code)]
mod segment;
mod ui;

#[cfg(test)]
mod tests;

use rune::{AppSpec, Chrome, Cx, El, ScrollSpec, TextEdit, Tx, col, row, text_input};

use editor::{FileState, Mode};
use model::Doc;
use ui::theme::{self, THEME};

const APP_NAME: &str = "Alternate";

const SAMPLE: &str = "The ferry left before the lantern was lit, and nobody on the harbor wall seemed to mind.\n\nBy noon the fog had gone from the channel and the far shore showed its row of pale houses, each one a little crooked, each one with a boat pulled up beside it as if it had only stopped for a moment.\n\nThe kettle ticked on the stove. Outside, someone was dragging a chain along the quay, stopping, and dragging it again.\n";

pub struct State {
    pub edit: TextEdit,
    pub doc: Doc,
    pub file: FileState,
    pub mode: Mode,
    pub marks_hidden: bool,
    pub scene: Scene,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Scene {
    #[default]
    App,
    Palette,
}

impl State {
    fn new() -> State {
        let mut s = editor::recovery_dir().and_then(|d| State::from_recovery(&d)).unwrap_or_else(|| State::with_doc(Doc::default()));
        s.edit.focused = true;
        s
    }
}

fn sample(mode: Mode) -> State {
    let mut s = State::with_doc(Doc::new(SAMPLE));
    s.mode = mode;
    s
}

fn scene_doc(s: &mut State, mode: Mode) {
    *s = sample(mode);
}

fn add_sample_target(doc: &mut Doc, quote: &str, level: model::Level, original: &str, others: &[&str]) {
    let start = doc.text.find(quote).expect("the sample holds it");
    let id = doc.next_id();
    let mine = |t: &str| model::Alt { text: t.to_string(), source: model::Source::Mine };
    let alts = std::iter::once(mine(quote)).chain(others.iter().map(|t| mine(t))).collect();
    doc.targets.push(model::Target { id, range: start..start + quote.len(), level, original: original.to_string(), alts, active: 0 });
}

fn marked_sample() -> State {
    let mut s = sample(Mode::Active);
    add_sample_target(&mut s.doc, "lantern", model::Level::Word, "lantern", &["lamp"]);
    add_sample_target(&mut s.doc, "harbor", model::Level::Word, "quay", &["quay"]);
    add_sample_target(&mut s.doc, "row of pale houses, each one a little crooked, each one with a boat", model::Level::Sentence, "row of pale houses, each one a little crooked, each one with a boat", &["line of houses"]);
    add_sample_target(&mut s.doc, "The kettle ticked on the stove. Outside, someone was dragging a chain along the quay, stopping, and dragging it again.", model::Level::Paragraph, "x", &["The kettle ticked."]);
    s
}

/// Marks after typing near the top, proving they follow their text.
fn edited_scene(s: &mut State) {
    *s = marked_sample();
    let edit = "Early that morning, with some reluctance and a thermos of tea, ".to_string();
    s.edit.set_selection(0, 0);
    s.edit.insert(&edit);
    s.sync_field();
}

fn page(s: &State, cx: &mut Cx<State>) -> El {
    let style = theme::body();
    let field = text_input(&s.edit, style)
        .on_edit(|s: &mut State| &mut s.edit)
        .on_input_change(|s: &mut State, _| s.sync_field())
        .on_input_key(editor::handle_key)
        .selection_color(THEME.accent.alpha(0.3))
        .placeholder("Start writing.", THEME.faint.color())
        .key("doc")
        .w_full()
        .color(THEME.ink.color());
    // A bare field in a wrapper that grows with it: the page scrolls, not the field.
    let mut grown = col().w_full().min_w(0.0).min_h(style.line_px()).child(field.animate_edit_layout());
    if let Some(marks) = ui::marks::overlay(s, cx) {
        grown = grown.child(marks);
    }
    col().w_full().max_w(theme::COLUMN + theme::GUTTER * 2.0).px(theme::GUTTER).pt(72.0).pb(120.0).child(grown)
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    if s.scene == Scene::Palette {
        return ui::palette_scene::view(s);
    }
    let scroller = col().w_full().h_full().key("scroller").scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(48.0).items_center().child(page(s, cx));
    let top = row().px(96.0).pt(18.0).child(ui::chrome::count(s)).pin(Some(0.0), Some(0.0), None, None);
    let mut bottom = row().gap(24.0).w_full().px(20.0).pb(16.0).items_center();
    if let Some(files) = ui::chrome::file_controls(s) {
        bottom = bottom.child(files);
    }
    if let Some(notice) = ui::chrome::notice(s) {
        bottom = bottom.child(notice);
    }
    let centre = ui::chrome::marks_toggle(s).map(|t| row().w_full().pt(18.0).justify_center().child(t).pin(Some(0.0), Some(0.0), Some(0.0), None));
    let mut root = col().w_full().h_full().fill(THEME.page.color()).child(scroller).child(top);
    if let Some(centre) = centre {
        root = root.child(centre);
    }
    root.child(bottom.pin(Some(0.0), None, Some(0.0), Some(0.0)))
}

fn on_close(s: &mut State, _tx: &mut Tx) {
    if let Some(dir) = editor::recovery_dir() {
        s.write_recovery(&dir);
    }
}

fn main() {
    rune_mac::launch();
    rune::prewarm_gpu();
    let spec = AppSpec::<State> {
        min_size: Some((640.0, 480.0)),
        scenes: &[("plain", |s| scene_doc(s, Mode::Plain)), ("active", |s| scene_doc(s, Mode::Active)), ("marks", |s| *s = marked_sample()), ("marks-off", |s| { *s = marked_sample(); s.marks_hidden = true; }), ("marks-edited", edited_scene), ("palette", |s| s.scene = Scene::Palette)],
        fonts: fonts::FONTS,
        families: fonts::FAMILIES,
        on_key: Some(editor::handle_key),
        ..AppSpec::new(APP_NAME, rune_mac::saved_size(APP_NAME).unwrap_or((1200.0, 800.0)), State::new, view)
    };
    rune::App::new(spec).chrome(Chrome { full_size_content: true, traffic_lights: Some((18.0, 16.0)), ..Chrome::DEFAULT }).on_close(on_close).run();
}
