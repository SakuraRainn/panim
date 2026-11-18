//! Timeline items: the updates that joined accounts send to the shared timeline.

use crate::ids::{AccountId, PersonaId};

/// A time in milliseconds after 1970-01-01 00:00:00 UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct Timestamp(pub i64);

/// The key of a timeline item. An account cannot have two items with the same source identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct ItemKey {
    pub account: AccountId,
    /// The identifier of the update on the platform.
    pub source_id: String,
}

/// An update in the shared timeline.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimelineItem {
    pub key: ItemKey,
    /// The persona of the account. The store sets it from the account.
    pub persona: PersonaId,
    pub author: String,
    pub time: Timestamp,
    pub text: String,
    /// The page of the update on the platform, if it has one.
    pub link: Option<String>,
    pub read: bool,
    /// The record as the platform sent it. PANIM keeps it, so a later version can read more fields.
    pub raw: Vec<u8>,
}

/// An update from a connector, before the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewItem {
    pub source_id: String,
    pub author: String,
    pub time: Timestamp,
    pub text: String,
    pub link: Option<String>,
    pub raw: Vec<u8>,
}

/// Which items the timeline shows.
///
/// All the conditions must be true for an item. An empty filter shows all the items, the most recent first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimelineFilter {
    /// Show only the items of this persona.
    pub persona: Option<PersonaId>,
    /// Show only the items of these accounts.
    pub accounts: Option<Vec<AccountId>>,
    /// Show only the unread items.
    pub unread_only: bool,
    /// Show only the items before this time. Use it to get the next page.
    pub before: Option<Timestamp>,
    /// Show no more than this number of items.
    pub limit: Option<usize>,
}

impl TimelineFilter {
    /// True if the item agrees with the filter. The limit is not part of this check.
    pub fn accepts(&self, item: &TimelineItem) -> bool {
        self.persona.is_none_or(|persona| item.persona == persona)
            && self
                .accounts
                .as_ref()
                .is_none_or(|accounts| accounts.contains(&item.key.account))
            && (!self.unread_only || !item.read)
            && self.before.is_none_or(|before| item.time < before)
    }
}
