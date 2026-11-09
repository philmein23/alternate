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

/// Tokens and keys never contain whitespace, but a long one wrapped in Terminal can pick some up
/// when it's copied, and the claude command then refuses it. Saved copies are mended on load too.
pub fn without_whitespace(value: &str) -> String {
    value.split_whitespace().collect()
}

/// Blocks on the Keychain, so call it off the main thread.
pub fn load(secret: Secret) -> Option<String> {
    let bytes = get_generic_password(SERVICE, secret.account()).ok()?;
    String::from_utf8(bytes).ok().map(|s| without_whitespace(&s))
}

pub fn save(secret: Secret, value: &str) -> bool {
    set_generic_password(SERVICE, secret.account(), value.as_bytes()).is_ok()
}

pub fn clear(secret: Secret) {
    let _ = delete_generic_password(SERVICE, secret.account());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapped_paste_becomes_one_clean_token() {
        assert_eq!(without_whitespace("  sk-ant-oat01-abc\ndef\r\n  ghi jkl\n"), "sk-ant-oat01-abcdefghijkl");
    }
}
