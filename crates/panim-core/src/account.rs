//! Accounts, their platforms and their two types of access.
//!
//! A joined account sends its updates to the shared timeline through a connector.
//! A browser-view account opens the web app of the service in the browser profile of its persona.
//! An account can have one type of access or the two types. Refer to `docs/DESIGN.md` §4 and §5.

use std::fmt;

use crate::ids::{AccountId, PersonaId};
use crate::secret::SecretRef;

/// A platform that PANIM supports in step 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Platform {
    GitHub,
    Telegram,
    /// Email through IMAP/SMTP. For Proton Mail, the IMAP/SMTP server is Proton Mail Bridge.
    Email,
    X,
    Discord,
}

impl Platform {
    /// All the platforms, in the recommended order of work.
    pub const ALL: [Platform; 5] = [
        Platform::GitHub,
        Platform::Telegram,
        Platform::Email,
        Platform::X,
        Platform::Discord,
    ];

    /// True if a connector can join an account of this platform to the shared timeline.
    pub fn can_join(self) -> bool {
        !matches!(self, Platform::Discord)
    }

    /// True if an account of this platform can open in a browser view.
    pub fn can_show_in_browser(self) -> bool {
        matches!(self, Platform::Email | Platform::X | Platform::Discord)
    }

    /// True if each account of this platform must have a browser view.
    pub fn needs_browser(self) -> bool {
        matches!(self, Platform::X | Platform::Discord)
    }

    /// The usual first page of the browser view, if the platform has one fixed page.
    ///
    /// Email has no fixed page, because the page depends on the service.
    pub fn default_start_url(self) -> Option<&'static str> {
        match self {
            Platform::X => Some("https://x.com"),
            Platform::Discord => Some("https://discord.com/app"),
            _ => None,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Platform::GitHub => "GitHub",
            Platform::Telegram => "Telegram",
            Platform::Email => "Email",
            Platform::X => "X",
            Platform::Discord => "Discord",
        })
    }
}

/// An account of a person on a platform. Each account belongs to one persona.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub persona: PersonaId,
    pub platform: Platform,
    /// The name that PANIM shows for the account.
    pub display_name: String,
    /// The handle or the address on the platform, for example `@name` or `name@proton.me`.
    pub handle: String,
    /// The stable identifier on the platform, if the platform has one. A handle can change, but this identifier does not.
    pub remote_id: Option<String>,
    /// The access through a connector, if the account is joined.
    pub joined: Option<JoinedAccess>,
    /// The access through a browser view, if the account has one.
    pub browser: Option<BrowserAccess>,
}

/// The access of a joined account.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JoinedAccess {
    /// The sealed credentials of the connector: a token, a password or an API key.
    pub credentials: SecretRef,
}

/// The access of a browser-view account.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BrowserAccess {
    /// The page that the browser view opens first.
    pub start_url: String,
}

/// The data that is necessary to add an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewAccount {
    pub persona: PersonaId,
    pub platform: Platform,
    pub display_name: String,
    pub handle: String,
    pub remote_id: Option<String>,
    pub joined: Option<JoinedAccess>,
    pub browser: Option<BrowserAccess>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_has_a_possible_access() {
        for platform in Platform::ALL {
            assert!(platform.can_join() || platform.can_show_in_browser(), "{platform}");
            if platform.needs_browser() {
                assert!(platform.can_show_in_browser(), "{platform}");
            }
        }
    }

    #[test]
    fn platforms_match_the_design() {
        assert!(Platform::GitHub.can_join() && !Platform::GitHub.can_show_in_browser());
        assert!(Platform::Telegram.can_join() && !Platform::Telegram.can_show_in_browser());
        assert!(
            Platform::Email.can_join() && Platform::Email.can_show_in_browser() && !Platform::Email.needs_browser()
        );
        assert!(Platform::X.can_join() && Platform::X.needs_browser());
        assert!(!Platform::Discord.can_join() && Platform::Discord.needs_browser());
    }
}
