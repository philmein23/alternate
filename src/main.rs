//! Alternate. `cargo run` opens the app, `cargo run -- --snap out.png --scene empty` renders headless.

mod fonts;
#[allow(dead_code)] // Used from phase 7 on.
mod model;
#[allow(dead_code)]
mod rebase;
mod ui;

#[cfg(test)]
mod tests;

use rune::{AppSpec, Chrome, Color, Cx, El, Family, TextEdit, TextStyle, col, oklch, text, text_input};

const APP_NAME: &str = "Alternate";

pub struct State {
    pub edit: TextEdit,
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
        State { edit: TextEdit::new("Start writing."), scene: Scene::App }
    }
}

pub fn view(s: &State, _cx: &mut Cx<State>) -> El {
    if s.scene == Scene::Palette {
        return ui::palette_scene::view(s);
    }
    let ink: Color = oklch(0.9, 0.03, 70.0);
    let title = text("Alternate".to_string()).font(TextStyle::new(24.0, 400.0).family(Family::Named(fonts::HAND))).color(oklch(0.7, 0.12, 285.0));
    let body = text_input(&s.edit, TextStyle::new(16.0, 400.0).lh(26.0).family(Family::Mono)).on_edit(|s: &mut State| &mut s.edit).key("doc").w_full().color(ink);
    col().w_full().h_full().fill(oklch(0.16, 0.03, 270.0)).padding(48.0).gap(16.0).child(title).child(body)
}

fn main() {
    rune_mac::launch();
    rune::prewarm_gpu();
    let spec = AppSpec::<State> {
        min_size: Some((640.0, 480.0)),
        scenes: &[("empty", |_| {}), ("palette", |s| s.scene = Scene::Palette)],
        fonts: fonts::FONTS,
        families: fonts::FAMILIES,
        ..AppSpec::new(APP_NAME, rune_mac::saved_size(APP_NAME).unwrap_or((1200.0, 800.0)), State::new, view)
    };
    rune::App::new(spec).chrome(Chrome { full_size_content: true, traffic_lights: Some((18.0, 16.0)), ..Chrome::DEFAULT }).run();
}
