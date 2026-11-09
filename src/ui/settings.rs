//! AI settings: which route to use, and the token or key if it needs one.

use rune::{El, TextEdit, col, el, row, text, text_input};

use crate::State;
use crate::ai::secrets;
use crate::ai::settings::{Configured, Route};
use crate::ai::claude_cli;
use crate::editor::recovery_dir;
use crate::ui::theme::{self, THEME};

pub struct SettingsPanel {
    pub secret: TextEdit,
    pub status: Option<String>,
}

impl State {
    pub fn toggle_settings(&mut self) {
        self.settings_panel = match self.settings_panel {
            Some(_) => None,
            None => Some(SettingsPanel { secret: TextEdit::new("").secure(), status: None }),
        };
    }

    pub fn choose_route(&mut self, route: Route) {
        self.ai.route = route;
        self.apply_settings();
        if let Some(panel) = &mut self.settings_panel {
            panel.secret = TextEdit::new("").secure();
            panel.status = None;
        }
    }

    fn apply_settings(&mut self) {
        self.ai.claude_path = claude_cli::find(self.ai.claude_path.as_deref());
        if let Some(dir) = recovery_dir() {
            self.ai.save(&dir);
        }
        self.provider = Configured::shared(self.ai.clone());
    }

    /// Hands the typed token or key to the Keychain and forgets it.
    pub fn save_secret(&mut self) {
        let Some(target) = self.ai.route.secret() else { return };
        let Some(panel) = &mut self.settings_panel else { return };
        let value = secrets::without_whitespace(&panel.secret.text);
        if value.is_empty() {
            return;
        }
        panel.secret = TextEdit::new("").secure();
        panel.status = Some(if secrets::save(target, &value) { "Saved to your Keychain." } else { "Couldn't save to your Keychain." }.to_string());
    }

    pub fn forget_secret(&mut self) {
        let Some(target) = self.ai.route.secret() else { return };
        secrets::clear(target);
        if let Some(panel) = &mut self.settings_panel {
            panel.status = Some("Removed from your Keychain.".to_string());
        }
    }
}

fn option(s: &State, label: &str, key: &'static str, route: Route) -> El {
    let color = if s.ai.route == route { THEME.accent.color() } else { THEME.faint.color() };
    text(label.to_string()).font(theme::small()).color(color).key(key).on_press_tx(move |s: &mut State, _| s.choose_route(route))
}

fn note(s: &State) -> String {
    match s.ai.route {
        Route::CliLogin if s.ai.claude_path.is_none() => "Couldn't find the claude command on this Mac.".to_string(),
        Route::CliLogin => "Uses the Claude Code sign-in already on this Mac.".to_string(),
        Route::SubscriptionToken => "Run claude setup-token in Terminal, then paste the token here.".to_string(),
        Route::ApiKey => "Paste an Anthropic API key. Usage is billed to that key.".to_string(),
    }
}

pub fn view(s: &State) -> Option<El> {
    let panel = s.settings_panel.as_ref()?;
    let routes = row().gap(16.0).child(option(s, "Claude Code sign-in", "route-cli", Route::CliLogin)).child(option(s, "Subscription token", "route-token", Route::SubscriptionToken)).child(option(s, "API key", "route-key", Route::ApiKey));
    let mut card = col().w(460.0).fill(THEME.panel.color()).radius(10.0).padding(24.0).gap(16.0).key("settings").on_press(|_: &mut State| {});
    card = card.child(text("AI settings".to_string()).font(theme::hand()).color(THEME.accent.color())).child(routes).child(text(note(s)).font(theme::small()).color(THEME.faint.color()).wrap());
    if s.ai.route.secret().is_some() {
        let field = text_input(&panel.secret, theme::small())
            .on_edit(rune::edit_access(|s: &mut State| match s.settings_panel.as_mut() {
                Some(p) => &mut p.secret,
                None => &mut s.spare_edit,
            }))
            .on_submit(|s: &mut State, _| s.save_secret())
            .placeholder("Paste here", THEME.faint.color())
            .color(THEME.ink.color())
            .key("settings-secret")
            .w_full();
        card = card.child(field).child(text("Save".to_string()).font(theme::small()).color(THEME.accent.color()).key("settings-save").on_press_tx(|s: &mut State, _| s.save_secret()));
        card = card.child(text("Remove saved".to_string()).font(theme::small()).color(THEME.faint.color()).key("settings-forget").on_press_tx(|s: &mut State, _| s.forget_secret()));
    }
    if let Some(status) = &panel.status {
        card = card.child(text(status.clone()).font(theme::small()).color(THEME.marker.color()).key("settings-status"));
    }
    card = card.child(text("Close".to_string()).font(theme::small()).color(THEME.faint.color()).key("settings-close").on_press_tx(|s: &mut State, _| s.toggle_settings()));
    Some(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(THEME.page.alpha(0.7)).items_center().justify_center().child(card).key("settings-scrim").on_press_tx(|s: &mut State, _| s.toggle_settings()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Doc;

    #[test]
    fn the_sign_in_route_never_stores_what_is_typed() {
        let mut s = State::with_doc(Doc::new("x"));
        s.ai.route = Route::CliLogin;
        s.toggle_settings();
        s.settings_panel.as_mut().unwrap().secret.insert("abc");
        s.save_secret();
        assert_eq!(s.settings_panel.as_ref().unwrap().secret.text, "abc");
        assert!(s.settings_panel.as_ref().unwrap().status.is_none());
    }

    #[test]
    fn changing_route_clears_anything_typed() {
        let mut s = State::with_doc(Doc::new("x"));
        s.toggle_settings();
        s.settings_panel.as_mut().unwrap().secret.insert("abc");
        s.choose_route(Route::ApiKey);
        assert_eq!(s.settings_panel.as_ref().unwrap().secret.text, "");
    }

    #[test]
    fn the_secret_field_masks_what_is_typed() {
        let mut s = State::with_doc(Doc::new("x"));
        s.toggle_settings();
        assert!(s.settings_panel.as_ref().unwrap().secret.is_secure());
    }
}
