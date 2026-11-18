//! The Telegram connector of PANIM. Refer to `docs/ARCHITECTURE.md` §4b.
//!
//! This crate has the code that speaks to TDLib. `panim-core` has no network code.
//!
//! - `tdjson` is the thin layer over the JSON interface of TDLib (`libtdjson.so`).
//! - `td` has the `Td` interface, the real library behind it, and a fake for the tests.
//! - `start` has the requests of the start: the log, the proxy and the parameters. It also has the `api_id` and the
//!   `api_hash` from the build, and the check of the values that the person gives.
//! - `guard` is the second protection for a proxy route.
//! - `login` has the login states and the errors.
//! - `chats` keeps the chats of an account and makes the timeline items.
//! - `messages` makes the simple text form of a message.
//! - `client` is the state of one TDLib client. It has no network code and no clock.
//! - `runtime` has the one receive loop for all the clients.
//!
//! All the logic uses the `Td` interface. Thus, the tests use a fake TDLib, no network and no Telegram account.

pub mod chats;
pub mod client;
pub mod guard;
pub mod login;
pub mod messages;
pub mod runtime;
pub mod start;
pub mod td;
mod tdjson;

use std::fmt;

pub use chats::{CHAT_PREFIX, ChatItem, ItemSummary, chat_of, item_summary, source_id};
pub use client::{Client, ClientConfig, Connection, Event, Me, PanelView};
pub use login::{AuthState, CODE_ROUTES, TdError, clean_phone};
pub use messages::{KINDS, MESSAGE_TEXT_CHARS, MessageView};
pub use runtime::{End, Runtime, Sink};
pub use start::{API_HASH_DIGITS, AppConfig, KEY_BYTES, is_configured, new_database_key, new_folder_name};
pub use td::{RealTd, Td};

/// An error of the connector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// PANIM has no `api_id` and `api_hash`: the build has none, and the person gave none.
    NotConfigured,
    /// The proxy of the route has a host name. TDLib gets only an IP address.
    ProxyHostName,
    /// The system gave no random bytes.
    NoRandom,
    /// A value from the app is not correct. The text does not contain the value.
    BadInput(&'static str),
    /// The account has no open client, for example after a lock.
    NoClient,
    /// TDLib did not answer in the time limit.
    Timeout,
    /// TDLib or Telegram refused the request.
    Td(TdError),
}

impl Error {
    /// The code of the error. The user interface has a text for each code.
    pub fn code(&self) -> &'static str {
        match self {
            Error::NotConfigured => "telegram_not_configured",
            Error::ProxyHostName => "telegram_proxy_name",
            Error::NoRandom => "io",
            Error::BadInput(_) => "telegram_bad_input",
            Error::NoClient => "telegram_no_client",
            Error::Timeout => "telegram_timeout",
            Error::Td(error) => error.code,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotConfigured => f.write_str("Telegram is not configured: PANIM has no app values"),
            Error::ProxyHostName => f.write_str("the proxy must have an IP address for Telegram"),
            Error::NoRandom => f.write_str("the system gave no random bytes"),
            Error::BadInput(reason) => f.write_str(reason),
            Error::NoClient => f.write_str("the account has no open Telegram client"),
            Error::Timeout => f.write_str("TDLib did not answer in time"),
            Error::Td(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}
