//! Tokens and keys live only in the Keychain.

use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

const SERVICE: &str = "ai.dbla.alternate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Secret {
    ApiKey,
    SubscriptionToken,
}

impl Secret {
    fn account(self) -> &'static str {
        match self {
            Secret::ApiKey => "anthropic-api-key",
            Secret::SubscriptionToken => "claude-subscription-token",
        }
    }
}

/// Blocks on the Keychain, so call it off the main thread.
pub fn load(secret: Secret) -> Option<String> {
    let bytes = get_generic_password(SERVICE, secret.account()).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn save(secret: Secret, value: &str) -> bool {
    set_generic_password(SERVICE, secret.account(), value.as_bytes()).is_ok()
}

pub fn clear(secret: Secret) {
    let _ = delete_generic_password(SERVICE, secret.account());
}
