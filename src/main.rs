//! Alternate. `cargo run` opens the app, `cargo run -- --snap out.png --scene active` renders headless.

mod ai;
mod article;
mod editor;
mod file;
mod fonts;
mod ghost;
mod lab;
mod model;
mod overflow;
mod post;
mod rebase;
mod segment;
mod shortcuts;
mod sidebar;
mod sound;
mod swap;
mod ui;
mod undo;
mod vim;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use rune::{AppSpec, Chrome, Cx, El, ScrollSpec, TextEdit, Tx, col, row, text, text_input};

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
    pub hover: Option<swap::Hover>,
    /// Each target's line boxes in the field's own points, from the last view.
    pub hit: RefCell<Vec<(model::TargetId, Vec<[f32; 4]>)>>,
    pub fade: Option<swap::Fade>,
    pub fade_now: f64,
    pub panel: Option<sidebar::Panel>,
    /// Answers a stale row binding after the panel closes.
    pub spare_edit: TextEdit,
    pub history: undo::History,
    pub sound: sound::Sound,
    pub bursts: Vec<ui::fx::Burst>,
    pub menu: Option<ui::menu::MenuContext>,
    pub overflow_open: bool,
    pub overflow_edit: TextEdit,
    pub kit: rune_kit::Kit,
    pub ai: ai::settings::AiSettings,
    pub provider: std::sync::Arc<dyn ai::Provider>,
    pub settings_panel: Option<ui::settings::SettingsPanel>,
    pub pending: Option<ai::alternatives::Pending>,
    pub dance_now: f64,
    /// Renders the waiting row without a request, for the snapshot scene.
    pub demo_asking: bool,
    pub lab: lab::Lab,
    pub review: Option<lab::trim::Review>,
    pub help_open: bool,
    pub vim: vim::Vim,
    /// How dimmed the page is, from 0 to 1.
    pub dim: f32,
    dim_at: Option<f64>,
    /// The system's Reduce Motion setting, as of the last view.
    pub still: std::cell::Cell<bool>,
    pub lab_hit: RefCell<Vec<Vec<[f32; 4]>>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Scene {
    #[default]
    App,
    Palette,
    Icon,
}

impl State {
    fn new() -> State {
        let mut s = editor::recovery_dir().and_then(|d| State::from_recovery(&d)).unwrap_or_else(|| State::with_doc(Doc::default()));
        if let Some(dir) = editor::recovery_dir() {
            s.ai = ai::settings::AiSettings::load(&dir);
            s.provider = ai::settings::Configured::shared(s.ai.clone());
        }
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

/// Moves the page's dimming towards where the panel wants it.
fn ease_dim(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let target = if s.panel.is_some() { 1.0 } else { 0.0 };
    if s.dim == target {
        s.dim_at = None;
        return false;
    }
    let step = (now - s.dim_at.unwrap_or(now)) as f32 / theme::SWAP_FADE;
    s.dim_at = Some(now);
    s.dim = if s.still.get() { target } else { (s.dim + step.copysign(target - s.dim)).clamp(s.dim.min(target), s.dim.max(target)) };
    tx.wake_after(std::time::Duration::from_millis(8));
    true
}

fn worst_scene(text: String) -> State {
    let mut s = State::with_doc(Doc::new(text));
    s.mode = Mode::Active;
    let end = s.doc.text.char_indices().nth(5).map_or(0, |(i, _)| i);
    s.doc.add_ghost(0..end);
    s
}

fn page(s: &State, cx: &mut Cx<State>) -> El {
    let style = theme::body();
    let field = text_input(&s.edit, style)
        .on_edit(|s: &mut State| &mut s.edit)
        .on_input_change(|s: &mut State, _| s.sync_field())
        .on_input_key(editor::handle_key)
        .on_submit(|s: &mut State, _| {
            s.edit.insert("\n");
            s.sync_field();
        })
        .on_pointer_move_local(|s: &mut State, p, tx| s.pointer_moved(p.x, p.y, tx))
        .on_secondary_press_local(|_: &mut State, p, tx| ui::menu::request(p.window(), tx))
        .on_hover_change(|s: &mut State, inside, _| {
            if !inside {
                s.pointer_left();
            }
        })
        .spans(ui::spans::spans(s))
        .text_highlights(ui::spans::highlights(s))
        .selection_color(THEME.accent.alpha(0.3))
        .placeholder("Start writing.", THEME.faint.color())
        .caret(if vim::block_visible(s) { THEME.accent.alpha(0.0) } else { THEME.accent.color() }, THEME.accent.alpha(0.3))
        .key("doc")
        .wrap_anywhere()
        .w_full()
        .color(THEME.ink.alpha(1.0 - (1.0 - THEME.dimmed) * s.dim));
    // A bare field in a wrapper that grows with it: the page scrolls, not the field.
    let mut grown = col().w_full().min_w(0.0).min_h(style.line_px()).child(field.animate_edit_layout());
    ui::lab::read_rects(s, cx);
    if let Some(marks) = ui::marks::overlay(s, cx) {
        grown = grown.child(marks);
    }
    for cut in ui::review::overlays(s, cx) {
        grown = grown.child(cut);
    }
    for slot in ui::fx::slots(s) {
        grown = grown.child(slot);
    }
    col().w_full().max_w(theme::COLUMN + theme::GUTTER * 2.0).px(theme::GUTTER).pt(72.0).pb(120.0).child(grown)
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    s.still.set(cx.is_reduced_motion());
    if s.scene == Scene::Palette {
        return ui::palette_scene::view(s);
    }
    if s.scene == Scene::Icon {
        return ui::palette_scene::icon();
    }
    let scroller = col().w_full().h_full().key("scroller").scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(84.0).items_center().child(page(s, cx));
    let top = row().px(96.0).pt(18.0).child(ui::chrome::count(s)).pin(Some(0.0), Some(0.0), None, None);
    let mut bottom = row().gap(24.0).w_full().px(20.0).pb(16.0).items_center();
    if let Some(files) = ui::chrome::file_controls(s) {
        bottom = bottom.child(files);
    }
    if let Some(notice) = ui::chrome::notice(s) {
        bottom = bottom.child(notice);
    }
    if let Some(mode) = ui::chrome::vim_mode(s) {
        bottom = bottom.child(mode);
    }
    bottom = bottom.child(row().grow(1.0));
    if let Some(toggle) = ui::chrome::overflow_toggle(s) {
        bottom = bottom.child(toggle);
    }
    let centre = ui::chrome::marks_toggle(s).map(|t| row().w_full().pt(18.0).justify_center().child(t).pin(Some(0.0), Some(0.0), Some(0.0), None));
    let mut main = col().grow(1.0).min_w(0.0).h_full().child(scroller).child(top);
    if s.doc.text.is_empty() && s.mode == Mode::Plain {
        let hint = text("Click the word count for alternatives, marks and the Lab.".to_string()).font(theme::small()).color(THEME.faint.color()).key("first-hint");
        main = main.child(row().w_full().justify_center().pb(20.0).child(hint).pin(Some(0.0), None, Some(0.0), Some(0.0)));
    }
    if let Some(centre) = centre {
        main = main.child(centre);
    }
    let main = main.child(bottom.pin(Some(0.0), None, Some(0.0), Some(0.0)));
    let main = match ui::lab::dock(s) {
        Some(dock) => main.child(dock),
        None => main,
    };
    let main = match ui::help::view(s) {
        Some(help) => main.child(help),
        None => main,
    };
    let root = row().w_full().h_full().fill(THEME.page.color());
    let root = match ui::sidebar::view(s) {
        Some(panel) => root.child(panel).child(main),
        None => root.child(main),
    };
    let root = match ui::overflow::view(s) {
        Some(panel) => root.child(panel),
        None => root,
    };
    let mut layer = Vec::new();
    ui::menu::mount(s, &mut layer);
    layer.extend(ui::settings::view(s));
    root.children(layer)
}

fn poll(s: &mut State, now: f64, tx: &mut Tx) -> bool {
    let fading = swap::poll(s, now, tx);
    let burning = ui::fx::poll(s, now, tx);
    let asking = ai::alternatives::poll(s, now, tx);
    let reading = lab::poll(s, tx);
    let trimming = lab::trim::poll(s, tx);
    let easing = ease_dim(s, now, tx);
    s.track_panel() || fading || burning || asking || reading || trimming || easing
}

fn on_close(s: &mut State, _tx: &mut Tx) {
    if let Some(dir) = editor::recovery_dir() {
        s.write_recovery(&dir);
    }
}

/// `--ai-check "text"` sends one request by the chosen route and prints what came back.
fn ai_check(text: &str) -> ! {
    let settings = editor::recovery_dir().map(|d| ai::settings::AiSettings::load(&d)).unwrap_or_default();
    match ai::check(ai::settings::Configured::shared(settings), text) {
        Ok(lines) => {
            lines.iter().for_each(|l| println!("{l}"));
            std::process::exit(0)
        }
        Err(e) => {
            eprintln!("{}", e.message());
            std::process::exit(1)
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--ai-check") {
        ai_check(args.get(i + 1).map_or("Say hello.", String::as_str));
    }
    rune_mac::launch();
    rune::prewarm_gpu();
    let spec = AppSpec::<State> {
        min_size: Some((640.0, 480.0)),
        scenes: &[("plain", |s| scene_doc(s, Mode::Plain)), ("active", |s| scene_doc(s, Mode::Active)), ("marks", |s| *s = marked_sample()), ("hover", |s| { *s = marked_sample(); s.hover = Some(swap::Hover { id: 1, armed: true }); }), ("swapped", |s| { *s = marked_sample(); s.step_alt(1, 1); s.fade = None; }), ("sidebar", |s| { *s = marked_sample(); s.edit.set_selection(30, 30); s.open_panel(); }), ("sidebar-sentence", |s| { *s = marked_sample(); s.edit.set_selection(30, 30); s.open_panel(); s.set_level(model::Level::Sentence); }), ("burst", |s| { *s = marked_sample(); s.bursts.push(ui::fx::Burst { rect: [190.0, 0.0, 70.0, 28.0], started: Some(-0.09) }); }), ("ghost", |s| { *s = marked_sample(); let at = s.doc.text.find("By noon").unwrap(); s.toggle_ghost(at..at + 30); }), ("overflow", |s| { *s = marked_sample(); s.set_overflow("A line about the ferry that didn't fit.\nThe harbor master's dog.".to_string()); s.overflow_open = true; }), ("menu", |s| { *s = marked_sample(); let at = s.doc.text.find("harbor").unwrap(); s.open_menu(Some(at), rune::PointerPress { x: 420.0, y: 170.0 }); }), ("settings", |s| { *s = marked_sample(); s.toggle_settings(); }), ("asking", |s| { *s = marked_sample(); s.edit.set_selection(47, 47); s.open_panel(); s.panel.as_mut().unwrap().rows.last_mut().unwrap().insert("??"); s.dance_now = 0.3; s.demo_asking = true; }), ("lab", |s| { *s = marked_sample(); s.lab.open = true; s.lab.guide = true; }), ("lab-marks", |s| { *s = marked_sample(); s.lab.open = true; for (quote, pass) in [("By noon the fog had gone from the channel", lab::Pass::Weakest), ("as if it had only stopped for a moment", lab::Pass::Long), ("seemed to", lab::Pass::Hedges)] { let at = s.doc.text.find(quote).unwrap(); s.doc.marks.push(model::LabMark { range: at..at + quote.len(), reason: "Example reason".to_string(), pass }); } s.lab.hover = Some(0); }), ("review", |s| { *s = marked_sample(); let text = s.doc.text.clone(); let cuts = ["before the lantern was lit, and ", "as if it had only stopped for a moment", "stopping, and "].iter().map(|q| { let at = text.find(q).unwrap(); lab::trim::Cut { range: at..at + q.len(), kept: false, fix: None } }).collect(); s.review = Some(lab::trim::Review::new(lab::trim::Kind::Trim(lab::trim::Level::More), cuts, text)); }), ("worst-para", |s| *s = worst_scene("The tide came in slowly over the flats. ".repeat(60))), ("worst-mixed", |s| *s = worst_scene(format!("Emoji \u{1f600}\u{1f469}\u{200d}\u{1f4bb} here. \u{5e9}\u{5dc}\u{5d5}\u{5dd} \u{5e2}\u{5d5}\u{5dc}\u{5dd}. \u{65e5}\u{672c}\u{8a9e}\u{306e}\u{6587}\u{7ae0}\u{3067}\u{3059}\u{3002}\n\nhttps://example.com/{}", "averyveryverylongpathsegment".repeat(40)))), ("help-vim", |s| { *s = marked_sample(); s.help_open = true; s.vim.on = true; s.vim.mode = vim::Mode::Normal; s.edit.set_selection(16, 16); }), ("help", |s| { *s = marked_sample(); s.help_open = true; }), ("first", |s| *s = State::with_doc(Doc::default())), ("icon", |s| s.scene = Scene::Icon), ("marks-off", |s| { *s = marked_sample(); s.marks_hidden = true; }), ("marks-edited", edited_scene), ("palette", |s| s.scene = Scene::Palette)],
        fonts: fonts::FONTS,
        families: fonts::FAMILIES,
        on_key: Some(editor::handle_key),
        poll: Some(poll),
        setup: Some(|s, _| {
            if let Some(dir) = editor::recovery_dir() {
                s.sound = sound::Sound::load(&dir);
                if !s.vim.on {
                    s.vim = vim::Vim::load(&dir);
                }
            }
        }),
        ..AppSpec::new(APP_NAME, rune_mac::saved_size(APP_NAME).unwrap_or((1200.0, 800.0)), State::new, view)
    };
    rune::App::new(spec).chrome(Chrome { full_size_content: true, traffic_lights: Some((18.0, 16.0)), ..Chrome::DEFAULT }).on_close(on_close).compositor(ui::fx::Gfx::default()).run();
}
