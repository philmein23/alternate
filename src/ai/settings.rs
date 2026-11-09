//! Which route to use. Nothing secret is kept here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::secrets::{self, Secret};
use super::{AiError, AiEvent, AiRequest, Provider, anthropic, claude_cli};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// The login Claude Code already has. No token is stored.
    #[default]
    CliLogin,
    SubscriptionToken,
    ApiKey,
}

impl Route {
    /// The Keychain item this route needs, if any.
    pub fn secret(self) -> Option<Secret> {
        match self {
            Route::CliLogin => None,
            Route::SubscriptionToken => Some(Secret::SubscriptionToken),
            Route::ApiKey => Some(Secret::ApiKey),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub route: Route,
    pub claude_path: Option<PathBuf>,
}

impl AiSettings {
    pub fn parse(json: &str) -> AiSettings {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn load(dir: &Path) -> AiSettings {
        std::fs::read_to_string(dir.join("ai.json")).map(|j| AiSettings::parse(&j)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join("ai.json"), json);
        }
    }
}

/// Runs a request by whichever route is chosen. Secrets are read here, on the request's thread.
pub struct Configured(pub AiSettings);

impl Configured {
    pub fn shared(settings: AiSettings) -> Arc<dyn Provider> {
        Arc::new(Configured(settings))
    }
}

impl Provider for Configured {
    fn run(&self, req: &AiRequest, sink: &mut dyn FnMut(AiEvent), cancel: &AtomicBool) {
        let secret = self.0.route.secret().map(secrets::load);
        if matches!(secret, Some(None)) {
            return sink(AiEvent::Failed(AiError::NoSecret));
        }
        let secret = secret.flatten();
        if self.0.route == Route::ApiKey {
            return anthropic::run(secret.as_deref().unwrap_or(""), req, sink, cancel);
        }
        let Some(claude) = claude_cli::find(self.0.claude_path.as_deref()) else {
            return sink(AiEvent::Failed(AiError::MissingCli));
        };
        claude_cli::run(&claude, secret.as_deref(), req, sink, cancel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_hold_no_secret() {
        let s = AiSettings { route: Route::SubscriptionToken, claude_path: Some("/x/claude".into()) };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(AiSettings::parse(&json), s);
        assert!(!json.to_lowercase().contains("sk-"));
    }

    #[test]
    fn unreadable_settings_fall_back_to_the_default() {
        assert_eq!(AiSettings::parse("{oops"), AiSettings::default());
    }

    #[test]
    fn only_the_stored_routes_need_a_secret() {
        assert_eq!(Route::CliLogin.secret(), None);
        assert_eq!(Route::ApiKey.secret(), Some(Secret::ApiKey));
    }
}
