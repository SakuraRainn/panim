//! The local store: personas, accounts and timeline items.
//!
//! `Store` is the interface. `check_new_account` holds the rules for accounts, so that all stores obey the same
//! rules. `MemoryStore` keeps the data in memory, for tests and for the first versions of the app.

use std::collections::BTreeMap;
use std::fmt;

use crate::account::{Account, NewAccount, Platform};
use crate::ids::{AccountId, PersonaAddress, PersonaId};
use crate::persona::{NewPersona, Persona, Route};
use crate::timeline::{ItemKey, NewItem, TimelineFilter, TimelineItem};

/// An error from the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    UnknownPersona(PersonaId),
    UnknownAccount(AccountId),
    UnknownItem(ItemKey),
    /// A different persona has this address. Two personas with one key are linked, so this is not permitted.
    AddressInUse(PersonaAddress),
    /// The account has no joined access and no browser view.
    NoAccess,
    /// The platform cannot join accounts to the shared timeline.
    CannotJoin(Platform),
    /// The platform cannot show accounts in a browser view.
    CannotShowInBrowser(Platform),
    /// Each account of the platform must have a browser view.
    BrowserViewNecessary(Platform),
    /// The persona has a browser-view account on the platform. One browser profile holds one login for each site.
    BrowserLoginInUse {
        persona: PersonaId,
        platform: Platform,
        account: AccountId,
    },
    /// The account cannot get timeline items, because it is not joined.
    NotJoined(AccountId),
    /// The store could not read or write its data. A failed change does not change the data in the store.
    Storage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::UnknownPersona(id) => write!(f, "the store has no {id}"),
            Error::UnknownAccount(id) => write!(f, "the store has no {id}"),
            Error::UnknownItem(key) => write!(f, "the store has no item {} of {}", key.source_id, key.account),
            Error::AddressInUse(address) => write!(f, "a different persona has the address {address}"),
            Error::NoAccess => write!(f, "the account has no joined access and no browser view"),
            Error::CannotJoin(platform) => write!(f, "{platform} cannot join accounts to the timeline"),
            Error::CannotShowInBrowser(platform) => write!(f, "{platform} cannot show accounts in a browser view"),
            Error::BrowserViewNecessary(platform) => write!(f, "each {platform} account must have a browser view"),
            Error::BrowserLoginInUse {
                persona,
                platform,
                account,
            } => {
                write!(f, "{persona} has a {platform} browser view in {account}")
            }
            Error::NotJoined(id) => write!(f, "{id} is not joined"),
            Error::Storage(reason) => write!(f, "the store could not read or write its data: {reason}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// The interface of the local store.
pub trait Store {
    /// Adds a persona. In step 1, the app adds one persona on the first start.
    fn add_persona(&mut self, persona: NewPersona) -> Result<Persona>;
    fn persona(&self, id: PersonaId) -> Option<Persona>;
    /// All the personas, in the sequence that they were added.
    fn personas(&self) -> Vec<Persona>;
    fn set_route(&mut self, persona: PersonaId, route: Route) -> Result<()>;

    /// Adds an account. The account must obey the rules of `check_new_account`.
    fn add_account(&mut self, account: NewAccount) -> Result<Account>;
    fn account(&self, id: AccountId) -> Option<Account>;
    /// The accounts of one persona, or of all personas. They are in the sequence that they were added.
    fn accounts(&self, persona: Option<PersonaId>) -> Vec<Account>;
    /// Removes an account and all its timeline items. The caller removes its secrets from the secret store.
    fn remove_account(&mut self, id: AccountId) -> Result<Account>;
    /// Sets the handle and the stable identifier of an account on its platform. Returns the changed account.
    ///
    /// The person gives these values for a browser-view account, because PANIM does not read the page.
    fn set_identity(&mut self, id: AccountId, handle: String, remote_id: Option<String>) -> Result<Account>;

    /// Keeps the items from the connector of a joined account. Returns the number of new items.
    ///
    /// If the store has an item with the same source identifier, the new data replaces the old data.
    /// If the new item has a later time than the old item, the item becomes unread again.
    /// If the times are equal, the read state of the old item does not change.
    fn put_items(&mut self, account: AccountId, items: Vec<NewItem>) -> Result<usize>;
    /// Removes these items of an account. Returns the number of items that the store removed.
    ///
    /// A source identifier that the store does not have is not an error.
    fn remove_items(&mut self, account: AccountId, source_ids: &[String]) -> Result<usize>;
    /// The items that agree with the filter, the most recent first.
    fn timeline(&self, filter: &TimelineFilter) -> Vec<TimelineItem>;
    fn set_read(&mut self, key: &ItemKey, read: bool) -> Result<()>;
    /// Sets the read state of some items of one account in one step. Returns the number of items that changed.
    ///
    /// A source identifier that the store does not have is not an error.
    fn set_read_many(&mut self, account: AccountId, changes: &[(String, bool)]) -> Result<usize>;
    fn unread_count(&self, account: AccountId) -> usize;
    /// Does a set of changes to the items of one account in one step. Returns true if the items changed.
    ///
    /// The store does the changes in this sequence: `put` (as `put_items`), `read` (as `set_read_many`) and
    /// `remove` (as `remove_items`). An item is changed only if its data after the step is different from its data
    /// before the step. A sealed store writes the items file of the account one time, and only if the items changed.
    fn apply_items(&mut self, account: AccountId, changes: ItemChanges) -> Result<bool>;
}

/// A set of changes to the items of one account, for `Store::apply_items`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemChanges {
    /// The items to add or to replace.
    pub put: Vec<NewItem>,
    /// The new read state of some items, as source identifiers.
    pub read: Vec<(String, bool)>,
    /// The source identifiers of the items to remove.
    pub remove: Vec<String>,
}

impl ItemChanges {
    /// True if the set has no change.
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.read.is_empty() && self.remove.is_empty()
    }
}

/// Makes sure that a new account obeys the rules of `docs/DESIGN.md` §4 and §5.
///
/// `persona_accounts` are the accounts that the persona has now.
pub fn check_new_account(account: &NewAccount, persona_accounts: &[Account]) -> Result<()> {
    let platform = account.platform;
    if account.joined.is_none() && account.browser.is_none() {
        return Err(Error::NoAccess);
    }
    if account.joined.is_some() && !platform.can_join() {
        return Err(Error::CannotJoin(platform));
    }
    if account.browser.is_some() && !platform.can_show_in_browser() {
        return Err(Error::CannotShowInBrowser(platform));
    }
    if account.browser.is_none() && platform.needs_browser() {
        return Err(Error::BrowserViewNecessary(platform));
    }
    if account.browser.is_some() {
        let taken = persona_accounts
            .iter()
            .find(|other| other.platform == platform && other.browser.is_some());
        if let Some(other) = taken {
            return Err(Error::BrowserLoginInUse {
                persona: account.persona,
                platform,
                account: other.id,
            });
        }
    }
    Ok(())
}

/// A store in memory. The data does not stay after the program stops.
///
/// A sealed store saves a `MemoryStore` in parts: `meta` once, and `items_of` for each account.
/// `from_parts` makes the store again from these parts.
#[derive(Default)]
pub struct MemoryStore {
    next_persona: u64,
    next_account: u64,
    personas: BTreeMap<PersonaId, Persona>,
    accounts: BTreeMap<AccountId, Account>,
    items: BTreeMap<ItemKey, TimelineItem>,
}

/// The personas, the accounts and the next identifiers of a store, without the timeline items.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StoreMeta {
    pub next_persona: u64,
    pub next_account: u64,
    pub personas: Vec<Persona>,
    pub accounts: Vec<Account>,
}

impl MemoryStore {
    pub fn meta(&self) -> StoreMeta {
        StoreMeta {
            next_persona: self.next_persona,
            next_account: self.next_account,
            personas: self.personas.values().cloned().collect(),
            accounts: self.accounts.values().cloned().collect(),
        }
    }

    /// The timeline items of one account.
    pub fn items_of(&self, account: AccountId) -> Vec<TimelineItem> {
        self.items
            .values()
            .filter(|item| item.key.account == account)
            .cloned()
            .collect()
    }

    /// Puts back the personas, the accounts and the next identifiers from `meta`. The timeline items do not change.
    ///
    /// A sealed store uses this to put back a copy after a failed write.
    pub fn restore_meta(&mut self, meta: StoreMeta) {
        self.next_persona = meta.next_persona;
        self.next_account = meta.next_account;
        self.personas = meta.personas.into_iter().map(|persona| (persona.id, persona)).collect();
        self.accounts = meta.accounts.into_iter().map(|account| (account.id, account)).collect();
    }

    /// Replaces all the timeline items of one account with `items`. Items of a different account are not kept.
    ///
    /// A sealed store uses this to put back a copy after a failed write.
    pub fn replace_items(&mut self, account: AccountId, items: Vec<TimelineItem>) {
        self.items.retain(|key, _| key.account != account);
        self.items.extend(
            items
                .into_iter()
                .filter(|item| item.key.account == account)
                .map(|item| (item.key.clone(), item)),
        );
    }

    /// Makes a store from its parts. Items of an account that is not in `meta` are not kept.
    pub fn from_parts(meta: StoreMeta, items: impl IntoIterator<Item = TimelineItem>) -> Self {
        let accounts: BTreeMap<_, _> = meta.accounts.into_iter().map(|account| (account.id, account)).collect();
        let items = items
            .into_iter()
            .filter(|item| accounts.contains_key(&item.key.account))
            .map(|item| (item.key.clone(), item))
            .collect();
        Self {
            next_persona: meta.next_persona,
            next_account: meta.next_account,
            personas: meta.personas.into_iter().map(|persona| (persona.id, persona)).collect(),
            accounts,
            items,
        }
    }
}

impl Store for MemoryStore {
    fn add_persona(&mut self, persona: NewPersona) -> Result<Persona> {
        if self.personas.values().any(|other| other.address == persona.address) {
            return Err(Error::AddressInUse(persona.address));
        }
        self.next_persona += 1;
        let persona = Persona {
            id: PersonaId(self.next_persona),
            name: persona.name,
            address: persona.address,
            route: persona.route,
        };
        self.personas.insert(persona.id, persona.clone());
        Ok(persona)
    }

    fn persona(&self, id: PersonaId) -> Option<Persona> {
        self.personas.get(&id).cloned()
    }

    fn personas(&self) -> Vec<Persona> {
        self.personas.values().cloned().collect()
    }

    fn set_route(&mut self, persona: PersonaId, route: Route) -> Result<()> {
        let persona = self.personas.get_mut(&persona).ok_or(Error::UnknownPersona(persona))?;
        persona.route = route;
        Ok(())
    }

    fn add_account(&mut self, account: NewAccount) -> Result<Account> {
        if !self.personas.contains_key(&account.persona) {
            return Err(Error::UnknownPersona(account.persona));
        }
        check_new_account(&account, &self.accounts(Some(account.persona)))?;
        self.next_account += 1;
        let account = Account {
            id: AccountId(self.next_account),
            persona: account.persona,
            platform: account.platform,
            display_name: account.display_name,
            handle: account.handle,
            remote_id: account.remote_id,
            joined: account.joined,
            browser: account.browser,
        };
        self.accounts.insert(account.id, account.clone());
        Ok(account)
    }

    fn account(&self, id: AccountId) -> Option<Account> {
        self.accounts.get(&id).cloned()
    }

    fn accounts(&self, persona: Option<PersonaId>) -> Vec<Account> {
        self.accounts
            .values()
            .filter(|account| persona.is_none_or(|persona| account.persona == persona))
            .cloned()
            .collect()
    }

    fn remove_account(&mut self, id: AccountId) -> Result<Account> {
        let account = self.accounts.remove(&id).ok_or(Error::UnknownAccount(id))?;
        self.items.retain(|key, _| key.account != id);
        Ok(account)
    }

    fn set_identity(&mut self, id: AccountId, handle: String, remote_id: Option<String>) -> Result<Account> {
        let account = self.accounts.get_mut(&id).ok_or(Error::UnknownAccount(id))?;
        account.handle = handle;
        account.remote_id = remote_id;
        Ok(account.clone())
    }

    fn put_items(&mut self, account: AccountId, items: Vec<NewItem>) -> Result<usize> {
        let owner = self.accounts.get(&account).ok_or(Error::UnknownAccount(account))?;
        if owner.joined.is_none() {
            return Err(Error::NotJoined(account));
        }
        let persona = owner.persona;
        let mut added = 0;
        for item in items {
            let key = ItemKey {
                account,
                source_id: item.source_id,
            };
            let read = match self.items.get(&key) {
                // A later time shows that the platform changed the update, so the person must see it again.
                Some(old) => old.read && item.time <= old.time,
                None => {
                    added += 1;
                    false
                }
            };
            let item = TimelineItem {
                key: key.clone(),
                persona,
                author: item.author,
                time: item.time,
                text: item.text,
                link: item.link,
                read,
                raw: item.raw,
            };
            self.items.insert(key, item);
        }
        Ok(added)
    }

    fn remove_items(&mut self, account: AccountId, source_ids: &[String]) -> Result<usize> {
        if !self.accounts.contains_key(&account) {
            return Err(Error::UnknownAccount(account));
        }
        let before = self.items.len();
        for source_id in source_ids {
            self.items.remove(&ItemKey {
                account,
                source_id: source_id.clone(),
            });
        }
        Ok(before - self.items.len())
    }

    fn timeline(&self, filter: &TimelineFilter) -> Vec<TimelineItem> {
        let mut items: Vec<_> = self
            .items
            .values()
            .filter(|item| filter.accepts(item))
            .cloned()
            .collect();
        items.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.key.cmp(&b.key)));
        if let Some(limit) = filter.limit {
            items.truncate(limit);
        }
        items
    }

    fn set_read(&mut self, key: &ItemKey, read: bool) -> Result<()> {
        let item = self.items.get_mut(key).ok_or_else(|| Error::UnknownItem(key.clone()))?;
        item.read = read;
        Ok(())
    }

    fn set_read_many(&mut self, account: AccountId, changes: &[(String, bool)]) -> Result<usize> {
        if !self.accounts.contains_key(&account) {
            return Err(Error::UnknownAccount(account));
        }
        let mut changed = 0;
        for (source_id, read) in changes {
            let key = ItemKey {
                account,
                source_id: source_id.clone(),
            };
            if let Some(item) = self.items.get_mut(&key)
                && item.read != *read
            {
                item.read = *read;
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn unread_count(&self, account: AccountId) -> usize {
        self.items
            .values()
            .filter(|item| item.key.account == account && !item.read)
            .count()
    }

    fn apply_items(&mut self, account: AccountId, changes: ItemChanges) -> Result<bool> {
        let owner = self.accounts.get(&account).ok_or(Error::UnknownAccount(account))?;
        if !changes.put.is_empty() && owner.joined.is_none() {
            return Err(Error::NotJoined(account));
        }
        // Keep a copy of each item that the changes can touch. After the changes, compare each item with its copy.
        let ids = changes.put.iter().map(|item| &item.source_id);
        let ids = ids.chain(changes.read.iter().map(|(id, _)| id)).chain(&changes.remove);
        let mut before: BTreeMap<ItemKey, Option<TimelineItem>> = BTreeMap::new();
        for source_id in ids {
            let key = ItemKey {
                account,
                source_id: source_id.clone(),
            };
            let old = self.items.get(&key).cloned();
            before.entry(key).or_insert(old);
        }
        // The checks above make sure that these steps cannot fail.
        if !changes.put.is_empty() {
            self.put_items(account, changes.put)?;
        }
        self.set_read_many(account, &changes.read)?;
        self.remove_items(account, &changes.remove)?;
        Ok(before
            .into_iter()
            .any(|(key, old)| self.items.get(&key) != old.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{BrowserAccess, JoinedAccess};
    use crate::secret::SecretRef;
    use crate::timeline::Timestamp;

    fn store_with_persona() -> (MemoryStore, PersonaId) {
        let mut store = MemoryStore::default();
        let persona = store
            .add_persona(NewPersona {
                name: "Me".into(),
                address: PersonaAddress([1; 20]),
                route: Route::Direct,
            })
            .unwrap();
        (store, persona.id)
    }

    fn new_account(persona: PersonaId, platform: Platform, joined: bool, browser: bool) -> NewAccount {
        NewAccount {
            persona,
            platform,
            display_name: format!("{platform} account"),
            handle: "@me".into(),
            remote_id: None,
            joined: joined.then_some(JoinedAccess {
                credentials: SecretRef(1),
            }),
            browser: browser.then(|| BrowserAccess {
                start_url: platform.default_start_url().unwrap_or("https://mail.proton.me").into(),
            }),
        }
    }

    fn item(source_id: &str, time: i64) -> NewItem {
        NewItem {
            source_id: source_id.into(),
            author: "someone".into(),
            time: Timestamp(time),
            text: format!("update {source_id}"),
            link: None,
            raw: b"{}".to_vec(),
        }
    }

    #[test]
    fn two_personas_cannot_have_one_address() {
        let (mut store, _) = store_with_persona();
        let second = NewPersona {
            name: "Other".into(),
            address: PersonaAddress([1; 20]),
            route: Route::Direct,
        };
        assert_eq!(
            store.add_persona(second),
            Err(Error::AddressInUse(PersonaAddress([1; 20])))
        );
    }

    #[test]
    fn accounts_obey_the_platform_rules() {
        let (mut store, me) = store_with_persona();
        let refused = [
            (new_account(me, Platform::GitHub, false, false), Error::NoAccess),
            (
                new_account(me, Platform::Discord, true, true),
                Error::CannotJoin(Platform::Discord),
            ),
            (
                new_account(me, Platform::GitHub, true, true),
                Error::CannotShowInBrowser(Platform::GitHub),
            ),
            (
                new_account(me, Platform::X, true, false),
                Error::BrowserViewNecessary(Platform::X),
            ),
        ];
        for (account, error) in refused {
            assert_eq!(store.add_account(account), Err(error));
        }
        for (platform, joined, browser) in [
            (Platform::GitHub, true, false),
            (Platform::Telegram, true, false),
            (Platform::Email, true, false),
            (Platform::Email, false, true),
            (Platform::X, true, true),
            (Platform::Discord, false, true),
        ] {
            store.add_account(new_account(me, platform, joined, browser)).unwrap();
        }
    }

    #[test]
    fn one_browser_login_for_each_site_in_a_persona() {
        let (mut store, me) = store_with_persona();
        let first = store.add_account(new_account(me, Platform::X, false, true)).unwrap();
        assert_eq!(
            store.add_account(new_account(me, Platform::X, true, true)),
            Err(Error::BrowserLoginInUse {
                persona: me,
                platform: Platform::X,
                account: first.id
            })
        );
        // Joined accounts without a browser view have no limit.
        store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        // A different persona has its own browser profile.
        let other = store
            .add_persona(NewPersona {
                name: "Other".into(),
                address: PersonaAddress([2; 20]),
                route: Route::Direct,
            })
            .unwrap();
        store
            .add_account(new_account(other.id, Platform::X, false, true))
            .unwrap();
    }

    #[test]
    fn account_needs_a_known_persona() {
        let mut store = MemoryStore::default();
        let account = new_account(PersonaId(9), Platform::GitHub, true, false);
        assert_eq!(store.add_account(account), Err(Error::UnknownPersona(PersonaId(9))));
    }

    #[test]
    fn items_keep_their_read_state_when_they_come_again() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        assert_eq!(store.put_items(github.id, vec![item("a", 10), item("b", 20)]), Ok(2));
        let key = ItemKey {
            account: github.id,
            source_id: "a".into(),
        };
        store.set_read(&key, true).unwrap();
        assert_eq!(store.unread_count(github.id), 1);

        let mut changed = item("a", 10);
        changed.text = "edited".into();
        assert_eq!(store.put_items(github.id, vec![changed, item("c", 30)]), Ok(1));
        let items = store.timeline(&TimelineFilter::default());
        let a = items.iter().find(|i| i.key == key).unwrap();
        assert_eq!((a.text.as_str(), a.read), ("edited", true));
        assert_eq!(store.unread_count(github.id), 2);
    }

    #[test]
    fn items_with_a_later_time_become_unread_again() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        store.put_items(github.id, vec![item("a", 10), item("b", 20)]).unwrap();
        for id in ["a", "b"] {
            let key = ItemKey {
                account: github.id,
                source_id: id.into(),
            };
            store.set_read(&key, true).unwrap();
        }
        assert_eq!(store.unread_count(github.id), 0);

        // The platform changed `a`. An old copy of `b` comes again.
        assert_eq!(store.put_items(github.id, vec![item("a", 11), item("b", 15)]), Ok(0));
        let items = store.timeline(&TimelineFilter::default());
        let state: Vec<_> = items
            .iter()
            .map(|i| (i.key.source_id.as_str(), i.time.0, i.read))
            .collect();
        assert_eq!(state, vec![("b", 15, true), ("a", 11, false)]);

        // An unread item stays unread.
        assert_eq!(store.put_items(github.id, vec![item("a", 11)]), Ok(0));
        assert_eq!(store.unread_count(github.id), 1);
    }

    #[test]
    fn remove_some_items() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        let other = store
            .add_account(NewAccount {
                handle: "@other".into(),
                ..new_account(me, Platform::GitHub, true, false)
            })
            .unwrap();
        store
            .put_items(github.id, vec![item("a", 1), item("b", 2), item("c", 3)])
            .unwrap();
        store.put_items(other.id, vec![item("a", 1)]).unwrap();
        let ids = ["a".to_string(), "c".to_string(), "missing".to_string()];
        assert_eq!(store.remove_items(github.id, &ids), Ok(2));
        let left: Vec<_> = store
            .timeline(&TimelineFilter::default())
            .into_iter()
            .map(|i| (i.key.account, i.key.source_id))
            .collect();
        assert_eq!(left, vec![(github.id, "b".to_string()), (other.id, "a".to_string())]);
        assert_eq!(store.remove_items(github.id, &[]), Ok(0));

        let changes = [("b".to_string(), true), ("missing".to_string(), true)];
        assert_eq!(store.set_read_many(github.id, &changes), Ok(1));
        assert_eq!(
            store.set_read_many(github.id, &changes),
            Ok(0),
            "no change the second time"
        );
        assert_eq!(store.unread_count(github.id), 0);
        assert_eq!(store.set_read_many(github.id, &[("b".to_string(), false)]), Ok(1));
        assert_eq!(store.unread_count(github.id), 1);
        assert_eq!(store.unread_count(other.id), 1);
        assert_eq!(
            store.remove_items(AccountId(99), &ids),
            Err(Error::UnknownAccount(AccountId(99)))
        );
    }

    #[test]
    fn browser_view_accounts_get_no_items() {
        let (mut store, me) = store_with_persona();
        let discord = store
            .add_account(new_account(me, Platform::Discord, false, true))
            .unwrap();
        assert_eq!(
            store.put_items(discord.id, vec![item("a", 1)]),
            Err(Error::NotJoined(discord.id))
        );
    }

    #[test]
    fn timeline_order_and_filters() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        let mail = store
            .add_account(new_account(me, Platform::Email, true, false))
            .unwrap();
        store
            .put_items(github.id, vec![item("g1", 10), item("g2", 30)])
            .unwrap();
        store.put_items(mail.id, vec![item("m1", 20)]).unwrap();
        store
            .set_read(
                &ItemKey {
                    account: github.id,
                    source_id: "g2".into(),
                },
                true,
            )
            .unwrap();

        let ids = |filter: TimelineFilter| -> Vec<String> {
            store
                .timeline(&filter)
                .into_iter()
                .map(|item| item.key.source_id)
                .collect()
        };
        assert_eq!(ids(TimelineFilter::default()), ["g2", "m1", "g1"]);
        assert_eq!(
            ids(TimelineFilter {
                accounts: Some(vec![mail.id]),
                ..Default::default()
            }),
            ["m1"]
        );
        assert_eq!(
            ids(TimelineFilter {
                unread_only: true,
                ..Default::default()
            }),
            ["m1", "g1"]
        );
        assert_eq!(
            ids(TimelineFilter {
                before: Some(Timestamp(30)),
                limit: Some(1),
                ..Default::default()
            }),
            ["m1"]
        );
        assert_eq!(
            ids(TimelineFilter {
                persona: Some(PersonaId(99)),
                ..Default::default()
            }),
            Vec::<String>::new()
        );
    }

    #[test]
    fn removing_an_account_removes_its_items() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        store.put_items(github.id, vec![item("a", 1)]).unwrap();
        store.remove_account(github.id).unwrap();
        assert!(store.timeline(&TimelineFilter::default()).is_empty());
        assert_eq!(store.remove_account(github.id), Err(Error::UnknownAccount(github.id)));
    }

    #[test]
    fn store_goes_back_together_from_its_parts() {
        let (mut store, me) = store_with_persona();
        let github = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap();
        let mail = store
            .add_account(new_account(me, Platform::Email, true, false))
            .unwrap();
        store.put_items(github.id, vec![item("a", 1), item("b", 2)]).unwrap();
        store.put_items(mail.id, vec![item("m", 3)]).unwrap();
        store
            .set_read(
                &ItemKey {
                    account: github.id,
                    source_id: "a".into(),
                },
                true,
            )
            .unwrap();

        let items = [store.items_of(github.id), store.items_of(mail.id)].concat();
        let mut again = MemoryStore::from_parts(store.meta(), items);
        assert_eq!(again.meta(), store.meta());
        assert_eq!(
            again.timeline(&TimelineFilter::default()),
            store.timeline(&TimelineFilter::default())
        );
        // The next identifiers continue from the old store.
        let next = again
            .add_account(new_account(me, Platform::Telegram, true, false))
            .unwrap();
        assert_eq!(next.id, AccountId(3));
    }

    #[test]
    fn restore_puts_back_the_meta_and_the_items_of_one_account() {
        let (mut store, persona) = store_with_persona();
        let first = store
            .add_account(new_account(persona, Platform::GitHub, true, false))
            .unwrap();
        let second = store
            .add_account(new_account(persona, Platform::GitHub, true, false))
            .unwrap();
        store.put_items(first.id, vec![item("a", 1)]).unwrap();
        store.put_items(second.id, vec![item("b", 2)]).unwrap();
        let (meta, items) = (store.meta(), store.items_of(first.id));
        let timeline = store.timeline(&TimelineFilter::default());

        store.remove_account(first.id).unwrap();
        store
            .add_account(new_account(persona, Platform::Telegram, true, false))
            .unwrap();
        store.restore_meta(meta.clone());
        store.replace_items(first.id, items);
        assert_eq!(store.meta(), meta);
        assert_eq!(store.timeline(&TimelineFilter::default()), timeline);

        // The items of a different account do not go into the account.
        store.replace_items(first.id, store.items_of(second.id));
        assert!(store.items_of(first.id).is_empty());
        assert_eq!(store.items_of(second.id).len(), 1);
    }

    #[test]
    fn identity_of_an_account_changes() {
        let (mut store, me) = store_with_persona();
        let x = store.add_account(new_account(me, Platform::X, false, true)).unwrap();
        let changed = store.set_identity(x.id, "name".into(), Some("12345".into())).unwrap();
        assert_eq!(
            (changed.handle.as_str(), changed.remote_id.as_deref()),
            ("name", Some("12345"))
        );
        assert_eq!(store.account(x.id), Some(changed));
        assert_eq!(
            store.set_identity(AccountId(9), String::new(), None),
            Err(Error::UnknownAccount(AccountId(9)))
        );
    }

    #[test]
    fn route_changes_for_the_persona() {
        use crate::persona::{ProxyEndpoint, ProxyKind};
        let (mut store, me) = store_with_persona();
        let tor = Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 9050,
        });
        store.set_route(me, tor.clone()).unwrap();
        assert_eq!(store.persona(me).unwrap().route, tor);
        assert_eq!(
            store.set_route(PersonaId(9), Route::Direct),
            Err(Error::UnknownPersona(PersonaId(9)))
        );
    }

    #[test]
    fn apply_items_tells_if_the_items_changed() {
        let (mut store, me) = store_with_persona();
        let account = store
            .add_account(new_account(me, Platform::GitHub, true, false))
            .unwrap()
            .id;
        store.put_items(account, vec![item("a", 1), item("b", 2)]).unwrap();
        let changes = |put: Vec<NewItem>, read: &[(&str, bool)], remove: &[&str]| ItemChanges {
            put,
            read: read.iter().map(|(id, read)| (id.to_string(), *read)).collect(),
            remove: remove.iter().map(|id| id.to_string()).collect(),
        };

        // The same data, a read state that is already set, and an unknown item are not changes.
        let same = changes(vec![item("a", 1)], &[("a", false)], &["unknown"]);
        assert_eq!(store.apply_items(account, same), Ok(false));
        assert_eq!(store.apply_items(account, ItemChanges::default()), Ok(false));

        // A put, a read and a removal in one step.
        let all = changes(vec![item("c", 3)], &[("a", true)], &["b"]);
        assert_eq!(store.apply_items(account, all), Ok(true));
        let ids: Vec<_> = store
            .items_of(account)
            .into_iter()
            .map(|item| (item.key.source_id, item.read))
            .collect();
        assert_eq!(ids, [("a".to_string(), true), ("c".to_string(), false)]);

        // A later time makes the item unread, and the read state of the same step makes it read again. The time
        // changed, so the items changed.
        let later = changes(vec![item("a", 5)], &[("a", true)], &[]);
        assert_eq!(store.apply_items(account, later), Ok(true));
        assert!(store.items_of(account)[0].read);

        assert_eq!(
            store.apply_items(AccountId(9), ItemChanges::default()),
            Err(Error::UnknownAccount(AccountId(9)))
        );
    }
}
