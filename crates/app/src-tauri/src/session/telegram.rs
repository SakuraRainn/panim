//! The part of the session for the Telegram connector: the folders, the keys and the state of the accounts.
//!
//! This module has no network code and does not speak to TDLib. The module `crate::telegram` does that work.
//!
//! - Each Telegram account has a TDLib folder with a random name. While the vault is open, the folder is in the
//!   profile area, which is in memory. The vault keeps the folder as one sealed archive, as it keeps the browser
//!   profile. The archive does not keep `files`.
//! - The secret of the account has the database key (32 random bytes) and then the name of the folder. Thus, the
//!   stored structs of the vault do not change, and a vault from before this milestone opens as before.
//! - The app values (`api_id`, `api_hash`) of a login are PANIM's values from the build, or the person's own values
//!   (milestone 10). The person selects them in the settings. They are in the sealed place `app/telegram`.
//! - The secret of an account also records the values of its login. A secret with no record is from PANIM's values,
//!   as all the accounts from before milestone 10. For the person's own values, the secret keeps a copy of them. Thus,
//!   a change of the setting changes only new logins, and a client always starts with the values of its login.
//! - A chat gives one timeline item. Its read state is the state of Telegram.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use panim_core::{
    Account, AccountId, ItemChanges, JoinedAccess, NewAccount, Platform, Route, SecretBytes, SecretStore, Store,
    TimelineFilter,
};
use panim_telegram::{AppConfig, AuthState, CHAT_PREFIX, ChatItem, Connection, KEY_BYTES, Me, source_id};
use panim_vault::Vault;
use serde::{Deserialize, Serialize};

use super::{AppError, AppResult, Open, Session, SyncView};

/// The sealed place of the Telegram app values of the person (milestone 10).
const APP_PLACE: &str = "app/telegram";
/// The version of the form of `app/telegram`. A new form gets a new number.
const APP_VERSION: u32 = 1;

/// The start of the record of the values in the secret of an account. A line feed comes before it.
const RECORD_OWN: &str = "panim-telegram-app/1 own ";

/// The error code of an account whose app values are not available. PANIM does not start its client.
pub const VALUES_MISSING: &str = "telegram_values_missing";

/// The form of `app/telegram`: JSON with a version number.
#[derive(Serialize, Deserialize)]
struct AppFile {
    version: u32,
    /// True if new logins use the own values of the person.
    use_own: bool,
    own: Option<OwnValues>,
}

#[derive(Serialize, Deserialize)]
struct OwnValues {
    api_id: i32,
    api_hash: String,
}

/// The setting of the Telegram app values, in memory while the vault is open.
#[derive(Clone, Default)]
pub(super) struct AppSetting {
    use_own: bool,
    own: Option<AppConfig>,
}

/// Reads the setting from the vault. A vault from before milestone 10 has no `app/telegram`: then new logins use
/// PANIM's values.
pub(super) fn load_app_setting(vault: &Vault) -> AppResult<AppSetting> {
    let Some(bytes) = vault.read(APP_PLACE)? else {
        return Ok(AppSetting::default());
    };
    let damaged = || AppError::new("damaged", APP_PLACE);
    let file: AppFile = serde_json::from_slice(&bytes).map_err(|_| damaged())?;
    if file.version != APP_VERSION {
        return Err(AppError::new(
            "damaged",
            format!("{APP_PLACE}: the version {} is not known", file.version),
        ));
    }
    let own = match file.own {
        Some(own) => Some(AppConfig::new(own.api_id, &own.api_hash).map_err(|_| damaged())?),
        None => None,
    };
    Ok(AppSetting {
        use_own: file.use_own && own.is_some(),
        own,
    })
}

fn save_app_setting(vault: &Vault, setting: &AppSetting) -> AppResult<()> {
    let file = AppFile {
        version: APP_VERSION,
        use_own: setting.use_own,
        own: setting.own.as_ref().map(|own| OwnValues {
            api_id: own.api_id(),
            api_hash: own.api_hash().to_string(),
        }),
    };
    let bytes = serde_json::to_vec(&file).expect("plain data");
    Ok(vault.write(APP_PLACE, &bytes)?)
}

/// The values of a login, as the secret of the account records them.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Values {
    /// PANIM's values from the build. A secret with no record has these values.
    Builtin,
    /// The own values of the person: a copy of the values of the login.
    Own(AppConfig),
    /// The record has a form that this version does not know.
    Unknown,
}

impl Values {
    /// The values for the client, or `None` if they are not available.
    fn resolve(&self, builtin: Option<&AppConfig>) -> Option<AppConfig> {
        match self {
            Values::Builtin => builtin.cloned(),
            Values::Own(app) => Some(app.clone()),
            Values::Unknown => None,
        }
    }
}

/// What the settings page shows about the Telegram app values. It never contains the `api_hash`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TelegramAppView {
    /// True if the build has PANIM's values.
    pub builtin: bool,
    /// True if the person selected their own values for new logins.
    pub use_own: bool,
    /// The `api_id` of the own values, if the person gave them. It is not secret.
    pub own_api_id: Option<i32>,
    /// The values of the next login: `builtin`, `own`, or `None` if PANIM has no values.
    pub next_login: Option<&'static str>,
}

/// The runtime key of the client of a login. An account id is never 0.
pub const LOGIN_KEY: u64 = 0;

/// The archive of a TDLib folder does not keep these entries. TDLib downloads the files again.
const NOT_SEALED: &[&str] = &["files"];

/// What a TDLib client needs to start.
pub struct TelegramStart {
    /// The key of the client in the runtime: the account id, or `LOGIN_KEY`.
    pub key: u64,
    /// The open folder of the account, in memory.
    pub dir: PathBuf,
    pub database_key: SecretBytes,
    pub route: Route,
    /// The language of the app.
    pub language: String,
    /// The app values of the client: the values of its login.
    pub app: AppConfig,
}

/// The clients that can start after the vault opens.
pub struct Opened {
    pub starts: Vec<TelegramStart>,
    /// True if an account got an error mark, for example because its app values are not available.
    pub marked: bool,
}

/// A login that is in work. It is in memory only.
struct Login {
    folder: String,
    database_key: SecretBytes,
    /// The values of the login. The secret of the new account records them.
    values: Values,
}

/// The state of one Telegram account. It is in memory only.
#[derive(Clone, Default)]
pub(super) struct Entry {
    connection: Option<Connection>,
    /// The code of the error that stops the client, for example a wrong database key.
    error: Option<&'static str>,
    /// True if Telegram ended the session, or if the folder has no login.
    needs_login: bool,
    /// True if the chat list of the client is not complete. Then the count can be too low.
    cut: bool,
    /// True if Telegram ended the session of this client ("Logging out"). Then no session stays on Telegram.
    logged_out: bool,
}

/// The state of the Telegram connector. A lock removes it.
#[derive(Default)]
pub(super) struct State {
    login: Option<Login>,
    pub(super) accounts: BTreeMap<u64, Entry>,
}

impl State {
    /// The state of an account for the list of accounts. `unread` is the number of unread chat items.
    pub(super) fn view(&self, account: u64, unread: usize) -> SyncView {
        let entry = self.accounts.get(&account).cloned().unwrap_or_default();
        // A client that tries to connect has no error code of its own. The mark shows that it is not connected.
        let offline = matches!(
            entry.connection,
            Some(Connection::WaitingForNetwork | Connection::ConnectingToProxy | Connection::Connecting)
        );
        SyncView {
            unread,
            unread_more: entry.cut,
            last_success: None,
            error: entry.error.or(offline.then_some("telegram_offline")),
            needs_login: entry.needs_login,
        }
    }
}

/// The secret of a Telegram account: the database key, the folder name, and the record of the values.
///
/// The form is the key (32 bytes), the folder name, and, for the own values only, a line feed and the record
/// `panim-telegram-app/1 own <api_id> <api_hash>`. A secret from before milestone 10 has no record. A login with
/// PANIM's values also writes no record, so that an earlier version of PANIM can still read the secret.
fn make_secret(key: &SecretBytes, folder: &str, values: &Values) -> SecretBytes {
    let mut secret = key.expose().to_vec();
    secret.extend_from_slice(folder.as_bytes());
    if let Values::Own(app) = values {
        secret.extend_from_slice(format!("\n{RECORD_OWN}{} {}", app.api_id(), app.api_hash()).as_bytes());
    }
    SecretBytes::new(secret)
}

/// The database key, the folder name and the values in the secret of a Telegram account.
fn read_secret(secret: &SecretBytes) -> Option<(SecretBytes, String, Values)> {
    let bytes = secret.expose();
    if bytes.len() <= KEY_BYTES {
        return None;
    }
    let (key, rest) = bytes.split_at(KEY_BYTES);
    let rest = std::str::from_utf8(rest).ok()?;
    let (folder, values) = match rest.split_once('\n') {
        None => (rest, Values::Builtin),
        Some((folder, record)) => {
            let own = record.strip_prefix(RECORD_OWN).and_then(|values| {
                let (id, hash) = values.split_once(' ')?;
                AppConfig::parse(id, hash).ok()
            });
            (folder, own.map_or(Values::Unknown, Values::Own))
        }
    };
    Some((SecretBytes::new(key.to_vec()), folder.to_string(), values))
}

fn account_secret(open: &Open, account: &Account) -> Option<(SecretBytes, String, Values)> {
    let joined = account.joined.as_ref()?;
    read_secret(&open.secrets.get(joined.credentials)?)
}

/// The values of the next login, from the setting: the own values if the person selected them, or if the build has
/// no values. `None` if PANIM has no values ("Telegram is not configured").
fn next_login(setting: &AppSetting, builtin: Option<&AppConfig>) -> Option<(Values, AppConfig)> {
    match (&setting.own, builtin) {
        (Some(own), _) if setting.use_own => Some((Values::Own(own.clone()), own.clone())),
        (_, Some(builtin)) => Some((Values::Builtin, builtin.clone())),
        (Some(own), None) => Some((Values::Own(own.clone()), own.clone())),
        (None, None) => None,
    }
}

fn chat_items(open: &Open, account: AccountId) -> Vec<panim_core::TimelineItem> {
    let filter = TimelineFilter {
        accounts: Some(vec![account]),
        ..Default::default()
    };
    let items = open.store.timeline(&filter).into_iter();
    items
        .filter(|item| item.key.source_id.starts_with(CHAT_PREFIX))
        .collect()
}

impl Session {
    /// Opens the TDLib folder of each Telegram account into memory, and gives what each client needs.
    ///
    /// `builtin` is PANIM's values from the build. Each client gets the values of its login. If those values are not
    /// available, the account gets the mark `telegram_values_missing`, and its folder stays sealed. PANIM never starts
    /// a client with different values.
    ///
    /// Call it after the vault opens, and after `telegram_close` when the route changed.
    pub fn telegram_open_all(&mut self, builtin: Option<&AppConfig>) -> AppResult<Opened> {
        let language = self.settings.language.clone();
        let Session {
            area, open, telegram, ..
        } = self;
        let open = open
            .as_mut()
            .ok_or_else(|| AppError::new("locked", "the vault is locked"))?;
        let mut starts = Vec::new();
        let mut marked = false;
        for account in open.store.accounts(Some(open.persona.id)) {
            if account.platform != Platform::Telegram || account.joined.is_none() {
                continue;
            }
            let Some((database_key, folder, values)) = account_secret(open, &account) else {
                telegram.accounts.entry(account.id.0).or_default().error = Some("telegram_database");
                marked = true;
                continue;
            };
            let Some(app) = values.resolve(builtin) else {
                telegram.accounts.entry(account.id.0).or_default().error = Some(VALUES_MISSING);
                marked = true;
                continue;
            };
            let dir = if open.telegram.contains(&folder) {
                area.path(&open.vault, &folder)
            } else {
                let dir = area.open(&open.vault, &folder)?;
                open.telegram.insert(folder);
                dir
            };
            telegram.accounts.entry(account.id.0).or_default();
            starts.push(TelegramStart {
                key: account.id.0,
                dir,
                database_key,
                route: open.persona.route.clone(),
                language: language.clone(),
                app,
            });
        }
        Ok(Opened { starts, marked })
    }

    /// The setting of the Telegram app values, for the settings page. It does not contain the `api_hash`.
    pub fn telegram_app(&self, builtin: Option<&AppConfig>) -> AppResult<TelegramAppView> {
        let open = self.open_ref()?;
        let setting = &open.telegram_app;
        Ok(TelegramAppView {
            builtin: builtin.is_some(),
            use_own: setting.use_own,
            own_api_id: setting.own.as_ref().map(AppConfig::api_id),
            next_login: next_login(setting, builtin).map(|(values, _)| match values {
                Values::Own(_) => "own",
                _ => "builtin",
            }),
        })
    }

    /// Sets the values of new logins. `own` is the text of the two fields, or `None` to keep the stored own values.
    ///
    /// The change does not change an existing account: each account keeps the values of its login.
    pub fn set_telegram_app(&mut self, use_own: bool, own: Option<(&str, &str)>) -> AppResult<()> {
        let open = self.open_mut()?;
        let mut setting = open.telegram_app.clone();
        if let Some((api_id, api_hash)) = own {
            let app = AppConfig::parse(api_id, api_hash)
                .map_err(|error| AppError::new("telegram_values_invalid", error.to_string()))?;
            setting.own = Some(app);
        }
        if use_own && setting.own.is_none() {
            return Err(AppError::new(
                "telegram_values_none",
                "give your own api_id and api_hash",
            ));
        }
        setting.use_own = use_own;
        save_app_setting(&open.vault, &setting)?;
        open.telegram_app = setting;
        Ok(())
    }

    /// Removes the own values of the person. New logins then use PANIM's values. Existing accounts keep their copy.
    pub fn forget_telegram_app(&mut self) -> AppResult<()> {
        let open = self.open_mut()?;
        let setting = AppSetting::default();
        save_app_setting(&open.vault, &setting)?;
        open.telegram_app = setting;
        Ok(())
    }

    /// The values that the next login uses, or `None` if PANIM has no values.
    pub fn telegram_next_login(&self, builtin: Option<&AppConfig>) -> Option<&'static str> {
        let open = self.open.as_ref()?;
        next_login(&open.telegram_app, builtin).map(|(values, _)| match values {
            Values::Own(_) => "own",
            _ => "builtin",
        })
    }

    /// Seals each open TDLib folder into the vault, if a file of the folder changed after its last seal. The folders
    /// stay open. The app calls this every few minutes.
    pub fn telegram_seal(&mut self) -> AppResult<()> {
        let Session { area, open, .. } = self;
        let Some(open) = open.as_ref() else { return Ok(()) };
        #[cfg(debug_assertions)]
        if !open.telegram.is_empty() {
            super::test_slow();
        }
        let mut result = Ok(());
        for folder in &open.telegram {
            // Try each folder. Keep the first error.
            let sealed = area.seal_changed_except(&open.vault, folder, NOT_SEALED);
            result = result.and(sealed.map(|_| ()).map_err(AppError::from));
        }
        result
    }

    /// Seals each open TDLib folder and removes it from memory. A login that is in work is deleted.
    ///
    /// Close the TDLib clients before this, so that TDLib does not write while the archive is made.
    pub fn telegram_close(&mut self) -> AppResult<()> {
        self.telegram_login_drop();
        let Session { area, open, .. } = self;
        let Some(open) = open.as_mut() else { return Ok(()) };
        let mut result = Ok(());
        for folder in std::mem::take(&mut open.telegram) {
            let closed = area.close_except(&open.vault, &folder, NOT_SEALED);
            result = result.and(closed.map_err(AppError::from));
        }
        result
    }

    /// Starts a login: makes a new folder and a new database key. A login that was in work is deleted.
    ///
    /// The login uses the values of the setting (`next_login`). Without values, the function refuses with
    /// `telegram_not_configured`.
    pub fn telegram_login_begin(&mut self, builtin: Option<&AppConfig>) -> AppResult<TelegramStart> {
        self.telegram_login_drop();
        let language = self.settings.language.clone();
        let Session {
            area, open, telegram, ..
        } = self;
        let open = open
            .as_mut()
            .ok_or_else(|| AppError::new("locked", "the vault is locked"))?;
        let (values, app) = next_login(&open.telegram_app, builtin).ok_or(panim_telegram::Error::NotConfigured)?;
        let folder = panim_telegram::new_folder_name()?;
        let database_key = panim_telegram::new_database_key()?;
        let dir = area.open(&open.vault, &folder)?;
        telegram.login = Some(Login {
            folder,
            database_key: database_key.clone(),
            values,
        });
        Ok(TelegramStart {
            key: LOGIN_KEY,
            dir,
            database_key,
            route: open.persona.route.clone(),
            language,
            app,
        })
    }

    /// True if a login is in work.
    pub fn telegram_login_active(&self) -> bool {
        self.telegram.login.is_some()
    }

    /// Deletes the folder of the login that is in work. End its TDLib client before this.
    pub fn telegram_login_drop(&mut self) {
        let Some(login) = self.telegram.login.take() else {
            return;
        };
        if let Some(open) = self.open.as_ref() {
            // The folder is in memory and has no login. An error leaves an empty folder until the next lock.
            let _ = self.area.discard(&open.vault, &login.folder);
        }
    }

    /// Completes the login: seals the database key, adds the account and seals the folder.
    ///
    /// If PANIM has this Telegram account already, the function refuses with `account_exists`, and the login stays
    /// in work. Then the caller ends the new session on Telegram and drops the login.
    pub fn telegram_login_finish(&mut self, me: &Me) -> AppResult<Account> {
        let Session {
            area, open, telegram, ..
        } = self;
        let open = open
            .as_mut()
            .ok_or_else(|| AppError::new("locked", "the vault is locked"))?;
        let login = telegram
            .login
            .as_ref()
            .ok_or_else(|| AppError::new("telegram_login_none", "no login is in work"))?;
        let remote_id = me.id.to_string();
        let exists =
            open.store.accounts(None).iter().any(|account| {
                account.platform == Platform::Telegram && account.remote_id.as_deref() == Some(&remote_id)
            });
        if exists {
            return Err(AppError::new("account_exists", "this Telegram account is in PANIM"));
        }
        // Make the archive before the account exists. If the write fails, the vault has no account without a folder.
        area.seal_except(&open.vault, &login.folder, NOT_SEALED)?;
        let secret = make_secret(&login.database_key, &login.folder, &login.values);
        let credentials = open.secrets.put(secret)?;
        let handle = me.username.clone().unwrap_or_default();
        let display_name = match (me.name.is_empty(), handle.is_empty()) {
            (false, _) => me.name.clone(),
            (true, false) => handle.clone(),
            (true, true) => Platform::Telegram.to_string(),
        };
        let added = open.store.add_account(NewAccount {
            persona: open.persona.id,
            platform: Platform::Telegram,
            display_name,
            handle,
            remote_id: Some(remote_id),
            joined: Some(JoinedAccess { credentials }),
            browser: None,
        });
        let account = match added {
            Ok(account) => account,
            Err(error) => {
                // The account does not exist, so the key must not stay.
                let _ = open.secrets.remove(credentials);
                return Err(error.into());
            }
        };
        let login = telegram.login.take().expect("checked");
        open.telegram.insert(login.folder);
        telegram.accounts.insert(account.id.0, Entry::default());
        Ok(account)
    }

    /// Deletes the TDLib folder of a Telegram account: the folder in memory and the archive in the vault.
    ///
    /// For an account of a different platform, the function does nothing.
    pub(super) fn telegram_discard(&mut self, id: AccountId) -> AppResult<()> {
        let Session { area, open, .. } = self;
        let Some(open) = open.as_mut() else { return Ok(()) };
        let Some(account) = open.store.account(id).filter(|a| a.platform == Platform::Telegram) else {
            return Ok(());
        };
        if let Some((_, folder, _)) = account_secret(open, &account) {
            area.discard(&open.vault, &folder)?;
            open.telegram.remove(&folder);
        }
        Ok(())
    }

    /// Keeps the changes of the chat items of an account. Returns true if the timeline changed.
    ///
    /// The unread count comes from the items, so it can change only when the timeline changed.
    ///
    /// The read state of each item is the state that the connector gives. It does not come from the time of the item.
    pub fn telegram_items(&mut self, account: AccountId, upsert: &[ChatItem], remove: &[i64]) -> bool {
        if !self.is_telegram(account) {
            return false;
        }
        let Some(open) = self.open.as_mut() else { return false };
        let changes = ItemChanges {
            put: upsert.iter().map(ChatItem::to_new_item).collect(),
            read: upsert.iter().map(|item| (source_id(item.chat_id), item.read)).collect(),
            remove: remove.iter().map(|chat| source_id(*chat)).collect(),
        };
        // One step writes the items file one time, and only if the items changed. A failed write changes nothing.
        open.store.apply_items(account, changes).unwrap_or(false)
    }

    /// Records that the client has its chat list. With a complete list, the function removes the stored items of
    /// the chats that give no item now. Returns true if the timeline or the state of the account changed.
    pub fn telegram_loaded(&mut self, account: AccountId, complete: bool, shown: &[i64]) -> bool {
        if !self.is_telegram(account) {
            return false;
        }
        let Some(open) = self.open.as_mut() else { return false };
        let entry = self.telegram.accounts.entry(account.0).or_default();
        let mut changed = entry.cut == complete;
        entry.cut = !complete;
        if complete {
            let keep: BTreeSet<String> = shown.iter().map(|chat| source_id(*chat)).collect();
            let stale: Vec<String> = chat_items(open, account)
                .into_iter()
                .map(|item| item.key.source_id)
                .filter(|id| !keep.contains(id))
                .collect();
            changed |= open.store.remove_items(account, &stale).is_ok_and(|count| count > 0);
        }
        changed
    }

    /// Records the authorization state of the client of an account. Returns true if the state of the account changed.
    ///
    /// - "Logging out" tells that Telegram ended the session, for example from a different device.
    /// - A state that asks for a phone number tells that the folder has no login.
    pub fn telegram_auth(&mut self, account: AccountId, state: &AuthState) -> bool {
        if !self.is_telegram(account) {
            return false;
        }
        let entry = self.telegram.accounts.entry(account.0).or_default();
        let before = (entry.needs_login, entry.error);
        match state {
            AuthState::Ready => {
                entry.needs_login = false;
                entry.error = None;
            }
            AuthState::LoggingOut => {
                entry.needs_login = true;
                entry.logged_out = true;
            }
            AuthState::WaitPhone
            | AuthState::WaitCode { .. }
            | AuthState::WaitPassword { .. }
            | AuthState::Unsupported { .. } => entry.needs_login = true,
            AuthState::Starting | AuthState::Closing | AuthState::Closed => {}
        }
        before != (entry.needs_login, entry.error)
    }

    /// True if Telegram ended the session of the account while the vault was open.
    pub fn telegram_logged_out(&self, account: AccountId) -> bool {
        self.telegram
            .accounts
            .get(&account.0)
            .is_some_and(|entry| entry.logged_out)
    }

    /// Records the connection state of the client of an account. Returns true if the mark of the account changed.
    pub fn telegram_connection(&mut self, account: AccountId, connection: Connection) -> bool {
        if !self.is_telegram(account) {
            return false;
        }
        let before = self.telegram.view(account.0, 0);
        self.telegram.accounts.entry(account.0).or_default().connection = Some(connection);
        before != self.telegram.view(account.0, 0)
    }

    /// Records an error that stops the client of an account. Returns true if the state of the account changed.
    pub fn telegram_error(&mut self, account: AccountId, code: &'static str) -> bool {
        if !self.is_telegram(account) {
            return false;
        }
        let entry = self.telegram.accounts.entry(account.0).or_default();
        let changed = entry.error != Some(code);
        entry.error = Some(code);
        changed
    }

    /// Keeps the username of an account after its client told it. Returns true if the account changed.
    ///
    /// A person can change the username in a Telegram app. The user id does not change. If the handle in the form of
    /// the proof changes, the kept block no longer names the account, and the function removes the kept proof.
    pub fn telegram_me(&mut self, account: AccountId, me: &Me) -> bool {
        let Some(open) = self.open.as_mut() else { return false };
        let Some(stored) = open.store.account(account).filter(|a| a.platform == Platform::Telegram) else {
            return false;
        };
        let handle = me.username.clone().unwrap_or_default();
        if stored.remote_id.as_deref() != Some(&me.id.to_string()) || stored.handle == handle {
            return false;
        }
        if open.store.set_identity(account, handle, stored.remote_id).is_err() {
            return false;
        }
        let _ = open.store.forget_proof(account);
        true
    }

    /// The number of open TDLib folders. Only the tests use it.
    #[cfg(test)]
    pub fn telegram_open_folders(&self) -> usize {
        self.open.as_ref().map_or(0, |open| open.telegram.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_records_the_values_of_the_login() {
        let key = SecretBytes::new((0u8..32).collect());
        let folder = "telegram-0123456789abcdef";
        let own = AppConfig::new(4242, "0123456789abcdef0123456789abcdef").unwrap();

        // PANIM's values write no record. This is the form of the secret before milestone 10.
        let builtin = make_secret(&key, folder, &Values::Builtin);
        let mut old = key.expose().to_vec();
        old.extend_from_slice(folder.as_bytes());
        assert_eq!(builtin.expose(), old.as_slice());
        let (k, f, v) = read_secret(&builtin).unwrap();
        assert_eq!((k.expose(), f.as_str(), v), (key.expose(), folder, Values::Builtin));

        // The own values: a copy in the secret.
        let secret = make_secret(&key, folder, &Values::Own(own.clone()));
        let (k, f, v) = read_secret(&secret).unwrap();
        assert_eq!(
            (k.expose(), f.as_str(), v),
            (key.expose(), folder, Values::Own(own.clone()))
        );

        // A record that this version does not know.
        let mut future = old.clone();
        future.extend_from_slice(b"\npanim-telegram-app/2 something");
        let (_, f, v) = read_secret(&SecretBytes::new(future)).unwrap();
        assert_eq!((f.as_str(), v), (folder, Values::Unknown));
        assert!(read_secret(&SecretBytes::new(vec![1; 32])).is_none());

        // The values that a client gets.
        assert_eq!(Values::Builtin.resolve(None), None);
        assert_eq!(Values::Builtin.resolve(Some(&own)), Some(own.clone()));
        assert_eq!(Values::Own(own.clone()).resolve(None), Some(own.clone()));
        assert_eq!(Values::Unknown.resolve(Some(&own)), None);
    }

    /// The periodic seal of a TDLib folder does not occur when no file changed. The close always seals.
    #[test]
    fn the_periodic_seal_of_a_tdlib_folder_skips_an_unchanged_folder() {
        let (data, memory) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut session = Session::new(
            data.path().join("panim"),
            panim_vault::ProfileArea::at(memory.path().to_path_buf()),
            super::super::Strength::TEST,
        );
        session.create("passcode1").unwrap();
        let folder = "telegram-0123456789abcdef";
        let dir = {
            let Session { area, open, .. } = &mut session;
            let open = open.as_mut().unwrap();
            let dir = area.open(&open.vault, folder).unwrap();
            open.telegram.insert(folder.into());
            dir
        };
        std::fs::create_dir_all(dir.join("db")).unwrap();
        std::fs::write(dir.join("db/td.binlog"), b"a login").unwrap();
        crate::session::tests::age(&dir);
        // The sealed files have a random nonce, so each write changes the bytes on the disk.
        let disk = || {
            let mut files = std::collections::BTreeMap::new();
            for entry in std::fs::read_dir(data.path().join("panim/data")).unwrap() {
                let entry = entry.unwrap();
                files.insert(entry.file_name(), std::fs::read(entry.path()).unwrap());
            }
            files
        };

        session.telegram_seal().unwrap();
        let first = disk();
        session.telegram_seal().unwrap();
        assert_eq!(disk(), first, "no change, no write");
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("files/photo.jpg"), b"picture").unwrap();
        session.telegram_seal().unwrap();
        assert_eq!(disk(), first, "the folder `files` is not sealed");

        std::fs::write(dir.join("db/td.binlog"), b"a login and more").unwrap();
        session.telegram_seal().unwrap();
        let second = disk();
        assert_ne!(second, first);
        session.telegram_close().unwrap();
        assert_ne!(disk(), second, "the close always seals");
    }

    /// A vault from milestone 7: a Telegram account whose secret is the key and the folder name, and no
    /// `app/telegram`. The test writes the account as the code of milestone 7 wrote it.
    #[test]
    fn a_vault_from_before_milestone_10_keeps_panims_values() {
        let (data, memory) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let new_session = || {
            Session::new(
                data.path().join("panim"),
                panim_vault::ProfileArea::at(memory.path().to_path_buf()),
                super::super::Strength::TEST,
            )
        };
        let mut session = new_session();
        session.create("passcode1").unwrap();
        let folder = "telegram-0123456789abcdef";
        let mut old_secret = vec![9u8; KEY_BYTES];
        old_secret.extend_from_slice(folder.as_bytes());
        let account = {
            let Session { area, open, .. } = &mut session;
            let open = open.as_mut().unwrap();
            let dir = area.open(&open.vault, folder).unwrap();
            std::fs::create_dir_all(dir.join("db")).unwrap();
            std::fs::write(dir.join("db/td.binlog"), b"an old login").unwrap();
            area.close_except(&open.vault, folder, NOT_SEALED).unwrap();
            let credentials = open.secrets.put(SecretBytes::new(old_secret.clone())).unwrap();
            open.store
                .add_account(NewAccount {
                    persona: open.persona.id,
                    platform: Platform::Telegram,
                    display_name: "Old".into(),
                    handle: "old".into(),
                    remote_id: Some("1".into()),
                    joined: Some(JoinedAccess { credentials }),
                    browser: None,
                })
                .unwrap()
        };
        session.lock().unwrap();

        let mut session = new_session();
        session.unlock("passcode1").unwrap();
        let secret_of = |session: &Session| {
            let open = session.open.as_ref().unwrap();
            open.secrets
                .get(account.joined.unwrap().credentials)
                .unwrap()
                .expose()
                .to_vec()
        };
        let stored = |session: &Session| session.open.as_ref().unwrap().vault.read(APP_PLACE).unwrap().is_some();
        assert!(!stored(&session), "an old vault has no app/telegram");
        let builtin = AppConfig::for_test();
        let view = session.telegram_app(Some(&builtin)).unwrap();
        assert_eq!(
            (view.use_own, view.own_api_id, view.next_login),
            (false, None, Some("builtin"))
        );

        // The account starts with PANIM's values, from its old folder.
        let opened = session.telegram_open_all(Some(&builtin)).unwrap();
        assert!(!opened.marked);
        assert_eq!(opened.starts.len(), 1);
        assert_eq!(opened.starts[0].app.api_id(), 12345);
        assert_eq!(
            std::fs::read(opened.starts[0].dir.join("db/td.binlog")).unwrap(),
            b"an old login"
        );
        session.telegram_close().unwrap();
        // Without PANIM's values, the account does not start, and its folder stays sealed.
        let opened = session.telegram_open_all(None).unwrap();
        assert!(opened.marked && opened.starts.is_empty());
        assert_eq!(session.telegram.view(account.id.0, 0).error, Some(VALUES_MISSING));

        // The own values of the person go into the new place. The secret of the old account does not change.
        session
            .set_telegram_app(true, Some(("4242", "0123456789abcdef0123456789abcdef")))
            .unwrap();
        assert!(stored(&session));
        assert_eq!(secret_of(&session), old_secret);
        let opened = session.telegram_open_all(Some(&builtin)).unwrap();
        assert_eq!(
            opened.starts[0].app.api_id(),
            12345,
            "the account keeps the values of its login"
        );
        session.telegram_close().unwrap();
        session.lock().unwrap();

        // The new place has a version number, and a version that this code does not know stops the open.
        let mut session = new_session();
        session.unlock("passcode1").unwrap();
        assert_eq!(session.telegram_app(Some(&builtin)).unwrap().own_api_id, Some(4242));
        let vault = session.open.as_ref().unwrap().vault.clone();
        let text = String::from_utf8(vault.read(APP_PLACE).unwrap().unwrap().to_vec()).unwrap();
        assert!(text.starts_with("{\"version\":1,"), "{text}");
        vault
            .write(APP_PLACE, text.replace("\"version\":1", "\"version\":2").as_bytes())
            .unwrap();
        drop(vault);
        session.lock().unwrap();
        assert_eq!(new_session().unlock("passcode1").unwrap_err().code, "damaged");
    }

    #[test]
    fn the_next_login_follows_the_setting() {
        let builtin = AppConfig::for_test();
        let own = AppConfig::new(4242, "0123456789abcdef0123456789abcdef").unwrap();
        let none = AppSetting::default();
        let kept = AppSetting {
            use_own: false,
            own: Some(own.clone()),
        };
        let selected = AppSetting {
            use_own: true,
            own: Some(own.clone()),
        };
        let pick = |setting: &AppSetting, builtin: Option<&AppConfig>| {
            next_login(setting, builtin).map(|(v, a)| (v, a.api_id()))
        };
        assert_eq!(pick(&none, None), None);
        assert_eq!(pick(&none, Some(&builtin)), Some((Values::Builtin, 12345)));
        assert_eq!(pick(&kept, Some(&builtin)), Some((Values::Builtin, 12345)));
        // A build without values uses the own values, also if the person did not select them.
        assert_eq!(pick(&kept, None), Some((Values::Own(own.clone()), 4242)));
        assert_eq!(pick(&selected, Some(&builtin)), Some((Values::Own(own.clone()), 4242)));
    }
}
