//! The state of the app, without Tauri: the first start, the gate, the open vault, the accounts and the settings.
//!
//! The Tauri commands in `main.rs` call these functions. Thus, the tests can examine the app without a window.
//!
//! This module has no network code. For the GitHub connector, it keeps the state of the device login and of the
//! sync. The module `github` does the requests and gives the results to the session.
//!
//! The proof screens (milestone 6) use `proofs`, `make_proof`, `set_identity`, `set_proof_location` and
//! `check_pasted_proof`. A proof is a public signature and does not show the key, so these functions do not ask for
//! the passcode again.
//!
//! For the Telegram connector, the child module `telegram` keeps the folders, the keys, the app values and the state
//! of the accounts. The module `crate::telegram` speaks to TDLib and gives the results to the session.
//!
//! The backup (milestone 10) writes the vault into one archive (`backup`). The restore (`restore`) is possible only on
//! the first start, when the folder has no vault.

mod telegram;

#[cfg(test)]
pub use telegram::VALUES_MISSING;
pub use telegram::{LOGIN_KEY, TelegramAppView, TelegramStart};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use panim_core::key::checksum_address;
use panim_core::proof::{self, Date, ProofError};
use panim_core::{
    Account, AccountId, BrowserAccess, Domain, ItemKey, JoinedAccess, KeptProof, NewAccount, NewItem, NewPersona,
    Persona, PersonaAddress, PersonaKey, Platform, RecoveryWords, Route, SecretBytes, SecretRef, SecretStore, Store,
    TimelineFilter, Timestamp,
};
use panim_github::{DeviceLogin, Fetched, User, Watched};
use panim_vault::{
    BackupInfo, KdfParams, ProfileArea, ScryptParams, SealedSecrets, SealedStore, Vault, VaultError, keyfile,
};
use serde::{Deserialize, Serialize};

use crate::unread::{Unread, UnreadTable};

/// The place in the vault for the state of the app.
const APP_STATE: &str = "app/state";
/// The name of the one persona of step 1. The user interface does not show it.
const FIRST_PERSONA: &str = "Me";

/// The settings that the gate needs before the vault is open. They are not secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// `en` or `zh`.
    pub language: String,
    /// The app locks after this number of minutes without input. 0 means never.
    pub auto_lock_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            auto_lock_minutes: 15,
        }
    }
}

/// The state of the app in the vault.
#[derive(Default, Serialize, Deserialize)]
struct AppState {
    /// True when the person checked the 12 recovery words. The first start is complete only then.
    #[serde(default)]
    words_checked: bool,
    /// True when the person saved a key file. A key file is an extra copy of the persona key.
    key_file_saved: bool,
    /// The secret of the key of each persona.
    persona_keys: BTreeMap<u64, SecretRef>,
    /// The secret of the 12 recovery words of each persona.
    #[serde(default)]
    persona_words: BTreeMap<u64, SecretRef>,
}

impl AppState {
    /// The first start is complete when the person has a copy of the persona key outside the app.
    fn backed_up(&self) -> bool {
        self.words_checked || self.key_file_saved
    }
}

/// The settings of the key derivations. The tests use weak settings, so that they are fast.
#[derive(Clone, Copy)]
pub struct Strength {
    pub kdf: KdfParams,
    pub scrypt: ScryptParams,
}

impl Strength {
    pub const STANDARD: Strength = Strength {
        kdf: KdfParams::STANDARD,
        scrypt: ScryptParams::STANDARD,
    };
    #[cfg(test)]
    pub const TEST: Strength = Strength {
        kdf: KdfParams::TEST,
        scrypt: ScryptParams::TEST,
    };
}

/// An error that the user interface shows. `code` selects the text in the text table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub tries_left: Option<u32>,
    pub detail: String,
}

impl AppError {
    pub(crate) fn new(code: &'static str, detail: impl ToString) -> Self {
        Self {
            code,
            tries_left: None,
            detail: detail.to_string(),
        }
    }
}

impl From<VaultError> for AppError {
    fn from(error: VaultError) -> Self {
        let detail = error.to_string();
        let mut out = AppError::new(
            match error {
                VaultError::WrongPasscode { .. } => "wrong_passcode",
                VaultError::GateLocked => "gate_locked",
                VaultError::PasscodeTooShort => "passcode_too_short",
                VaultError::WrongRecoveryKey => "wrong_recovery_key",
                VaultError::BadKeyFile(_) => "bad_key_file",
                VaultError::WrongKeyFilePassword => "wrong_key_file_password",
                VaultError::InUse => "in_use",
                VaultError::Exists => "exists",
                VaultError::NotFound => "not_found",
                VaultError::Damaged(_) => "damaged",
                VaultError::BadBackup(_) => "bad_backup",
                VaultError::NoRandom | VaultError::Io(_) => "io",
            },
            detail,
        );
        if let VaultError::WrongPasscode { tries_left } = error {
            out.tries_left = Some(tries_left);
        }
        out
    }
}

impl From<panim_core::Error> for AppError {
    fn from(error: panim_core::Error) -> Self {
        let code = match error {
            panim_core::Error::BrowserLoginInUse { .. } => "browser_login_in_use",
            panim_core::Error::Storage(_) => "storage",
            _ => "store",
        };
        AppError::new(code, error)
    }
}

impl From<panim_telegram::Error> for AppError {
    fn from(error: panim_telegram::Error) -> Self {
        // The detail of a flood wait or of a resend that is too early is the number of seconds. The user interface
        // shows it.
        let (code, detail) = match &error {
            panim_telegram::Error::Td(td) => match (td.code, td.seconds) {
                ("telegram_flood", None) => ("telegram_flood_unknown", String::new()),
                (code, Some(seconds)) => (code, seconds.to_string()),
                (code, None) => (code, error.to_string()),
            },
            other => (other.code(), other.to_string()),
        };
        AppError::new(code, detail)
    }
}

impl From<panim_github::Error> for AppError {
    fn from(error: panim_github::Error) -> Self {
        AppError::new(github_code(&error), error)
    }
}

/// The error code for an error of the GitHub connector. The user interface has a text for each code.
pub(crate) fn github_code(error: &panim_github::Error) -> &'static str {
    use panim_github::Error;
    match error {
        Error::Network(_) => "network",
        Error::BadToken => "github_bad_token",
        Error::RateLimit { .. } => "github_rate_limit",
        Error::Forbidden => "github_no_access",
        Error::NotFound => "github_not_found",
        Error::Login(_) => "github_login_failed",
        Error::Status(_) | Error::BadData(_) | Error::BadInput(_) => "github_error",
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// The error code for an error of the proof form. The user interface has a text for each code.
pub(crate) fn proof_code(error: &ProofError) -> &'static str {
    match error {
        ProofError::NoBlock => "proof_no_block",
        ProofError::ShortBlock => "proof_short_block",
        ProofError::BadByte => "proof_bad_byte",
        ProofError::BadLine(_) => "proof_bad_line",
        ProofError::UnknownPlatform(_) => "proof_unknown_platform",
        ProofError::BadId => "proof_bad_id",
        ProofError::BadHandle => "proof_bad_handle",
        ProofError::BadDate => "proof_bad_date",
        ProofError::NoAccount => "proof_no_account",
        ProofError::Signature(_) => "proof_bad_signature",
        ProofError::IdMissing => "proof_id_missing",
        ProofError::WrongKey | ProofError::Key(_) => "proof_key",
    }
}

/// The error for an error of the proof form. For some errors, `detail` is a value that the text shows.
fn proof_error(error: ProofError) -> AppError {
    let code = proof_code(&error);
    let detail = match &error {
        ProofError::BadLine(n) => n.to_string(),
        ProofError::UnknownPlatform(name) => name.chars().take(40).collect(),
        other => other.to_string(),
    };
    AppError::new(code, detail)
}

/// A pasted text has no more than this number of bytes. A block has fewer than 600 bytes.
pub const MAX_PASTE: usize = 16 * 1024;

/// A location of a proof has no more than this number of characters.
pub const MAX_LOCATION: usize = 2000;

/// The proof state of one account, for the proof screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProofView {
    pub account: u64,
    /// True if the platform has a numeric id (`base/panim-proof-v1.md` §5), but PANIM does not know the id.
    pub id_needed: bool,
    /// True if the person can give the handle and the id. This is true for an account without a connector.
    pub identity_editable: bool,
    /// The kept block, if PANIM made one for the account.
    pub block: Option<String>,
    /// The date of the kept block, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// The URL where the person posted the kept block, if the person gave it.
    pub location: Option<String>,
}

/// The result of the check of a pasted block. PANIM checked the form and the signature only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PastedView {
    /// The address of the persona, `0x` and 40 lowercase hexadecimal digits.
    pub persona: String,
    /// The platform of the statement, for example `x`.
    pub platform: &'static str,
    pub id: Option<String>,
    pub handle: Option<String>,
    /// `proof` or `revoke`.
    pub domain: &'static str,
    pub date: String,
    /// True if the persona of the block is the persona of the person.
    pub own: bool,
}

/// What the check of the person's own proof in a gist needs.
#[derive(Debug)]
pub struct OwnProofTarget {
    pub account: AccountId,
    /// The numeric id of the GitHub account.
    pub remote_id: String,
    pub persona: PersonaAddress,
    pub route: Route,
}

/// The sync waits this number of seconds between two rounds, if GitHub does not ask for more.
pub const SYNC_SECONDS: u64 = 60;

/// The background sync sends a request for notifications without `If-Modified-Since` at this interval.
///
/// A thread that the person reads on github.com possibly does not change `Last-Modified`. Then GitHub answers 304,
/// and PANIM does not see the read. The full request finds it.
pub const FULL_SYNC_SECONDS: u64 = 300;

/// The value of a local read while its request to GitHub is in progress.
const READ_IN_FLIGHT: u64 = u64::MAX;

/// What the user interface shows for a device login.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LoginView {
    /// The code that the person enters on the page of GitHub.
    pub user_code: String,
    pub verification_uri: String,
    /// The number of seconds between two polls.
    pub interval: u64,
    /// The number of seconds until the code expires.
    pub expires_in: u64,
}

/// A device login that waits for the person. It is in memory only. The device code never goes to the user interface.
struct PendingLogin {
    device_code: SecretBytes,
    interval: u64,
    expires: Instant,
    /// Do not poll before this time.
    next_poll: Instant,
}

/// What a poll of the device login must do next.
pub enum LoginStep {
    /// The interval is not complete. Do not send a request.
    Wait { interval: u64 },
    /// Send a poll with this device code on this route.
    Poll { device_code: SecretBytes, route: Route },
}

/// The sync state of one GitHub account. It is in memory only.
#[derive(Clone, Default)]
struct SyncEntry {
    /// The `Last-Modified` value of the last notifications.
    last_modified: Option<String>,
    /// The `ETag` value of the last events.
    etag: Option<String>,
    /// The value of `X-Poll-Interval`, in seconds.
    poll_interval: u64,
    /// The time of the last sync without an error, in milliseconds after 1970-01-01 00:00:00 UTC.
    last_success: Option<u64>,
    error: Option<&'static str>,
    /// True after GitHub refused the token.
    needs_login: bool,
    /// The rate limit ends at this time, in seconds after 1970-01-01 00:00:00 UTC.
    wait_until: Option<u64>,
    /// The watched repositories, `owner/repo` in lowercase. `None` until the first good list.
    watched: Option<BTreeSet<String>>,
    /// The `ETag` value of the last list of watched repositories.
    watched_etag: Option<String>,
    /// The time of the last full list of notifications, in seconds after 1970-01-01 00:00:00 UTC.
    last_full: Option<u64>,
    /// True if the page limit cut the last full list of notifications. Then the unread count is too low.
    cut: bool,
}

/// The state of the GitHub connector. It is in memory only, and a lock removes it.
#[derive(Default)]
struct GitHubState {
    login: Option<PendingLogin>,
    accounts: BTreeMap<u64, SyncEntry>,
    /// This number changes at each lock. A result of a request from before the lock has an old number.
    generation: u64,
    /// A counter. It increases when a sync starts and when a request for a local read ends.
    clock: u64,
    /// The local reads of notification threads. The value is the `clock` when the request to GitHub ended, or
    /// `READ_IN_FLIGHT`. A sync that started before that time does not make the item unread again.
    local_reads: BTreeMap<ItemKey, u64>,
}

/// What one sync of one account needs. The sync does its requests without the lock of the session.
pub struct SyncJob {
    generation: u64,
    pub account: AccountId,
    pub login: String,
    pub token: SecretBytes,
    pub route: Route,
    /// The `Last-Modified` value to send. It is `None` for a full request.
    pub last_modified: Option<String>,
    pub etag: Option<String>,
    pub watched_etag: Option<String>,
    /// The `clock` when the sync started.
    started: u64,
}

/// The results of the three requests of one sync.
pub struct SyncResult {
    pub watched: Result<Watched, panim_github::Error>,
    pub notifications: Result<Fetched, panim_github::Error>,
    pub events: Result<Fetched, panim_github::Error>,
}

/// The state of a joined account, for the list of accounts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SyncView {
    /// The number of unread timeline items of the account. For GitHub, this is the number of unread notification
    /// threads. Events do not count.
    pub unread: usize,
    /// True if the page limit cut the list of notifications. Then the real number is larger than `unread`.
    pub unread_more: bool,
    /// The time of the last sync without an error, in milliseconds after 1970-01-01 00:00:00 UTC.
    pub last_success: Option<u64>,
    /// The error code of the last sync, if it failed.
    pub error: Option<&'static str>,
    /// True if GitHub refused the token, or if Telegram ended the session. The person must remove the account and
    /// add it again.
    pub needs_login: bool,
}

/// A timeline item, as the user interface gets it. It does not contain the raw record or the link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ItemView {
    pub key: ItemKey,
    pub author: String,
    pub time: Timestamp,
    /// The text of the item. For a known GitHub event, this is the English phrase. The user interface shows `event`.
    pub text: String,
    /// The kind and the repository of a known GitHub event. The user interface shows the phrase in its language.
    pub event: Option<EventView>,
    /// False if the item has no read state, for example a GitHub event. Then `read` is true, and the item never
    /// counts as unread.
    pub has_read_state: bool,
    pub read: bool,
    /// True if the item has a web page. The command `open_link` opens it.
    pub has_link: bool,
    /// True if the person can write an answer to the item.
    pub can_answer: bool,
    /// The data of a Telegram chat: the kind of the last message and the unread count. The user interface shows
    /// the label of the kind in its language.
    pub chat: Option<panim_telegram::ItemSummary>,
}

/// A received event of GitHub, for the user interface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EventView {
    /// The kind of the event, for example `starred`. The user interface has a phrase for each kind.
    pub kind: &'static str,
    /// The full name of the repository, `owner/repo`.
    pub repo: String,
}

/// What a request for one timeline item needs.
pub struct ItemTarget {
    pub token: SecretBytes,
    pub route: Route,
    pub raw: Vec<u8>,
}

/// What the gate shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// There is no vault. The first start begins.
    FirstRun,
    /// The vault is closed. The passcode opens it.
    Locked,
    /// Too many wrong tries. Only the key file opens the vault.
    GateLocked,
    /// The vault is open, but the person did not check the 12 recovery words yet.
    Backup,
    /// The vault is open.
    Open,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub phase: Phase,
    pub tries_left: u32,
    pub settings: Settings,
    pub persona: Option<PersonaView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PersonaView {
    /// The address in the EIP-55 form, for people.
    pub address: String,
    pub route: Route,
}

struct Open {
    vault: Arc<Vault>,
    store: SealedStore,
    secrets: SealedSecrets,
    state: AppState,
    persona: Persona,
    /// The folder of the open browser profile of the persona, in memory.
    profile: Option<PathBuf>,
    /// The names of the open TDLib folders, in memory.
    telegram: BTreeSet<String>,
    /// The setting of the Telegram app values (`app/telegram`).
    telegram_app: telegram::AppSetting,
}

impl Open {
    fn save_state(&self) -> AppResult<()> {
        let bytes = serde_json::to_vec(&self.state).expect("plain data");
        Ok(self.vault.write(APP_STATE, &bytes)?)
    }
}

/// The session of the app.
pub struct Session {
    data: PathBuf,
    area: ProfileArea,
    strength: Strength,
    settings: Settings,
    open: Option<Open>,
    /// The unread state of the account windows. It is in memory only.
    unread: UnreadTable,
    /// The state of the GitHub connector. It is in memory only.
    github: GitHubState,
    /// The state of the Telegram connector. It is in memory only.
    telegram: telegram::State,
}

impl Session {
    /// `data` is the vault folder. `area` is the folder in memory for the open browser profiles.
    pub fn new(data: PathBuf, area: ProfileArea, strength: Strength) -> Self {
        let settings = fs::read_to_string(data.join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            data,
            area,
            strength,
            settings,
            open: None,
            unread: UnreadTable::default(),
            github: GitHubState::default(),
            telegram: telegram::State::default(),
        }
    }

    pub fn status(&self) -> AppResult<Status> {
        let phase = match &self.open {
            Some(open) if !open.state.backed_up() => Phase::Backup,
            Some(_) => Phase::Open,
            None if !Vault::exists(&self.data) => Phase::FirstRun,
            None if Vault::tries_left(&self.data)? == 0 => Phase::GateLocked,
            None => Phase::Locked,
        };
        let tries_left = if Vault::exists(&self.data) {
            Vault::tries_left(&self.data)?
        } else {
            0
        };
        let persona = self.open.as_ref().map(|open| PersonaView {
            address: checksum_address(open.persona.address),
            route: open.persona.route.clone(),
        });
        Ok(Status {
            phase,
            tries_left,
            settings: self.settings.clone(),
            persona,
        })
    }

    pub fn set_settings(&mut self, settings: Settings) -> AppResult<()> {
        if !["en", "zh"].contains(&settings.language.as_str()) {
            return Err(AppError::new("store", "the language is not en or zh"));
        }
        fs::create_dir_all(&self.data).map_err(|e| AppError::new("io", e))?;
        let text = serde_json::to_string_pretty(&settings).expect("plain data");
        fs::write(self.data.join("settings.json"), text).map_err(|e| AppError::new("io", e))?;
        self.settings = settings;
        Ok(())
    }

    /// The first start: makes the 12 recovery words, the persona key, the vault and the one persona.
    pub fn create(&mut self, passcode: &str) -> AppResult<()> {
        let words = RecoveryWords::generate().map_err(|e| AppError::new("io", e))?;
        let key = words.persona_key().map_err(|e| AppError::new("io", e))?;
        let vault = Arc::new(Vault::create(&self.data, passcode, &key, self.strength.kdf)?);
        let mut store = SealedStore::open(vault.clone())?;
        let mut secrets = SealedSecrets::open(vault.clone())?;
        let key_ref = secrets.put(key.to_secret())?;
        let words_ref = secrets.put(words.to_secret())?;
        let persona = store.add_persona(NewPersona {
            name: FIRST_PERSONA.into(),
            address: key.address(),
            route: Route::Direct,
        })?;
        let mut state = AppState::default();
        state.persona_keys.insert(persona.id.0, key_ref);
        state.persona_words.insert(persona.id.0, words_ref);
        let open = Open {
            vault,
            store,
            secrets,
            state,
            persona,
            profile: None,
            telegram: BTreeSet::new(),
            telegram_app: telegram::AppSetting::default(),
        };
        open.save_state()?;
        self.open = Some(open);
        self.open_profile()
    }

    /// Opens the vault with the passcode.
    pub fn unlock(&mut self, passcode: &str) -> AppResult<()> {
        let vault = Arc::new(Vault::open(&self.data, passcode, self.strength.kdf)?);
        self.load(vault)
    }

    /// Opens the vault with the key file, and sets a new passcode.
    pub fn recover(&mut self, key_file: &Path, key_file_password: &str, new_passcode: &str) -> AppResult<()> {
        let text = fs::read_to_string(key_file).map_err(|e| AppError::new("bad_key_file", e))?;
        let key = keyfile::import(&text, key_file_password)?;
        let vault = Arc::new(Vault::recover(&self.data, &key, new_passcode, self.strength.kdf)?);
        self.load(vault)
    }

    /// Opens the vault with the 12 recovery words, and sets a new passcode.
    pub fn recover_with_words(&mut self, words: &str, new_passcode: &str) -> AppResult<()> {
        let words =
            RecoveryWords::parse(words).ok_or_else(|| AppError::new("bad_words", "the words are not correct"))?;
        let key = words.persona_key().map_err(|e| AppError::new("bad_words", e))?;
        let vault = Arc::new(Vault::recover(&self.data, &key, new_passcode, self.strength.kdf)?);
        self.load(vault)
    }

    /// The 12 recovery words of the persona. Before the words are checked, the first start shows them without a
    /// passcode. After that, the passcode is necessary, and a wrong passcode counts as a wrong try.
    pub fn recovery_words(&self, passcode: Option<&str>) -> AppResult<Vec<String>> {
        let open = self.open_ref()?;
        if open.state.backed_up() {
            let passcode = passcode.ok_or_else(|| AppError::new("wrong_passcode", "the passcode is necessary"))?;
            open.vault.verify_passcode(passcode)?;
        }
        Ok(persona_words(open)?.words())
    }

    /// Marks the words as checked if each answer is the word at its place (from 1 to 12).
    pub fn check_words(&mut self, answers: &[(usize, String)]) -> AppResult<()> {
        let open = self.open_mut()?;
        let words = persona_words(open)?.words();
        let correct = !answers.is_empty()
            && answers.iter().all(|(place, word)| {
                (1..=words.len()).contains(place) && words[place - 1] == word.trim().to_lowercase()
            });
        if !correct {
            return Err(AppError::new("words_differ", "a word is not correct"));
        }
        open.state.words_checked = true;
        open.save_state()
    }

    fn load(&mut self, vault: Arc<Vault>) -> AppResult<()> {
        let store = SealedStore::open(vault.clone())?;
        let secrets = SealedSecrets::open(vault.clone())?;
        let state: AppState = match vault.read(APP_STATE)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| AppError::new("damaged", e))?,
            None => AppState::default(),
        };
        let persona = store
            .personas()
            .into_iter()
            .next()
            .ok_or_else(|| AppError::new("damaged", "the vault has no persona"))?;
        let telegram_app = telegram::load_app_setting(&vault)?;
        self.open = Some(Open {
            vault,
            store,
            secrets,
            state,
            persona,
            profile: None,
            telegram: BTreeSet::new(),
            telegram_app,
        });
        self.open_profile()
    }

    /// Writes the key file of the persona key to `path`, and marks the first start as complete.
    pub fn export_key_file(&mut self, password: &str, path: &Path) -> AppResult<()> {
        if password.chars().count() < panim_vault::MIN_PASSCODE_CHARS {
            return Err(AppError::new(
                "passcode_too_short",
                "the key file password is too short",
            ));
        }
        let strength = self.strength;
        let open = self.open_mut()?;
        let key = persona_key(open)?;
        let text = keyfile::export(&key, password, strength.scrypt)?;
        if path.exists() {
            return Err(AppError::new("file_exists", path.display()));
        }
        fs::write(path, text).map_err(|e| AppError::new("io", e))?;
        open.state.key_file_saved = true;
        open.save_state()
    }

    /// Seals the browser profile and the TDLib folders, and closes the vault. Close the browser windows and the
    /// Telegram clients before this.
    pub fn lock(&mut self) -> AppResult<()> {
        let telegram = self.telegram_close();
        let result = self.seal_profile(true).and(telegram);
        self.open = None;
        self.unread.clear();
        // The device code, the cursors and the sync state do not stay after a lock.
        self.github = GitHubState {
            generation: self.github.generation + 1,
            ..Default::default()
        };
        // The state of the Telegram accounts does not stay after a lock.
        self.telegram = telegram::State::default();
        result
    }

    /// Writes the vault into one archive at `target`: the header and the sealed files.
    ///
    /// First, the function seals the TDLib folders and removes them from memory, as a lock does, and seals the
    /// browser profile. Close the Telegram clients and the browser windows before this. After this, start the
    /// Telegram clients again (`telegram_open_all`). The session stays locked while the archive is made, so no sealed
    /// file changes.
    pub fn backup(&mut self, target: &Path) -> AppResult<BackupInfo> {
        self.open_ref()?;
        let telegram = self.telegram_close();
        self.seal_profile(false).and(telegram)?;
        let open = self.open_ref()?;
        Ok(panim_vault::write_backup(&open.vault, target)?)
    }

    /// Restores a vault from an archive of `backup`. This is possible only when the folder has no vault. Then the
    /// gate opens the vault with its passcode, or with its recovery words or key file.
    pub fn restore(&mut self, archive: &Path) -> AppResult<BackupInfo> {
        if self.open.is_some() || Vault::exists(&self.data) {
            return Err(AppError::new("exists", "the folder has a vault already"));
        }
        Ok(panim_vault::restore_backup(archive, &self.data)?)
    }

    pub fn change_passcode(&mut self, current: &str, new: &str) -> AppResult<()> {
        let kdf = self.strength.kdf;
        let open = self.open_ref()?;
        open.vault.verify_passcode(current)?;
        Ok(open.vault.change_passcode(new, kdf)?)
    }

    pub fn set_route(&mut self, route: Route) -> AppResult<()> {
        if let Route::Proxy(proxy) = &route
            && (proxy.host.is_empty() || proxy.port == 0)
        {
            return Err(AppError::new("bad_proxy", "the proxy needs a host and a port"));
        }
        let open = self.open_mut()?;
        open.store.set_route(open.persona.id, route.clone())?;
        open.persona.route = route;
        Ok(())
    }

    pub fn accounts(&self) -> AppResult<Vec<Account>> {
        let open = self.open_ref()?;
        Ok(open.store.accounts(Some(open.persona.id)))
    }

    /// Adds a browser-view account. Joined accounts come with the connectors.
    pub fn add_browser_account(&mut self, platform: Platform, display_name: &str, handle: &str) -> AppResult<Account> {
        let start_url = match platform {
            Platform::Email => "https://mail.proton.me",
            other => other
                .default_start_url()
                .ok_or_else(|| AppError::new("store", format!("{other} has no browser view")))?,
        };
        let open = self.open_mut()?;
        let display_name = if display_name.trim().is_empty() {
            platform.to_string()
        } else {
            display_name.trim().to_string()
        };
        Ok(open.store.add_account(NewAccount {
            persona: open.persona.id,
            platform,
            display_name,
            handle: handle.trim().to_string(),
            remote_id: None,
            joined: None,
            browser: Some(BrowserAccess {
                start_url: start_url.into(),
            }),
        })?)
    }

    /// Removes an account, its items and its secret. For a Telegram account, it also deletes the TDLib folder.
    /// End the Telegram client of the account before this.
    pub fn remove_account(&mut self, id: AccountId) -> AppResult<()> {
        // Delete the TDLib folder first. If that fails, the account stays and the person can try again.
        self.telegram_discard(id)?;
        let open = self.open_mut()?;
        let account = open.store.remove_account(id)?;
        if let Some(joined) = account.joined {
            open.secrets.remove(joined.credentials)?;
        }
        self.unread.remove(id.0);
        self.github.accounts.remove(&id.0);
        self.telegram.accounts.remove(&id.0);
        Ok(())
    }

    /// The proof state of each account of the persona.
    pub fn proofs(&self) -> AppResult<Vec<ProofView>> {
        let open = self.open_ref()?;
        let accounts = open.store.accounts(Some(open.persona.id));
        Ok(accounts.iter().map(|account| proof_view(open, account)).collect())
    }

    /// Makes a proof for an account of the persona and keeps it. `now` is in seconds after 1970-01-01 00:00:00 UTC.
    ///
    /// The statement has the date of `now` in UTC. The signature is deterministic, so a second proof on the same
    /// date gives the same block. If the account has a different kept block, the function refuses with the error
    /// `proof_replace` (its detail is the old date). Then the person agrees, and the caller sets `replace`.
    ///
    /// The function does not ask for the passcode. A proof is a public signature, and it does not show the key.
    pub fn make_proof(&mut self, id: AccountId, now: u64, replace: bool) -> AppResult<ProofView> {
        let open = self.open_mut()?;
        let account = own_account(open, id)?;
        let date = i64::try_from(now)
            .ok()
            .and_then(Date::from_unix_seconds)
            .ok_or_else(|| AppError::new("io", "the clock of the computer is not correct"))?;
        let statement = proof::proof_statement(open.persona.address, &account, date).map_err(proof_error)?;
        let block = {
            // The key stays in memory only for the signature.
            let key = persona_key(open)?;
            statement.sign(&key).map_err(proof_error)?.text()
        };
        if let Some(kept) = open.store.kept_proof(id) {
            if kept.block == block {
                return Ok(proof_view(open, &account));
            }
            if !replace {
                let old = kept_date(kept).unwrap_or_default();
                return Err(AppError::new("proof_replace", old));
            }
        }
        open.store.keep_proof(id, KeptProof { block, location: None })?;
        Ok(proof_view(open, &account))
    }

    /// Sets the handle and the numeric id of an account without a connector, for example X or Discord.
    ///
    /// PANIM does not read the page, so the person gives these values. An empty id removes the id. Email has no id.
    /// If the handle or the id in the form of §5 changes, the kept block no longer names the account. Then the
    /// function removes the kept proof.
    pub fn set_identity(&mut self, id: AccountId, handle: &str, remote_id: &str) -> AppResult<Account> {
        let open = self.open_mut()?;
        let account = own_account(open, id)?;
        if account.joined.is_some() {
            return Err(AppError::new(
                "proof_identity_fixed",
                "the connector gives the handle and the id",
            ));
        }
        let platform = account.platform;
        let new_handle = proof::account_handle(platform, handle).map_err(proof_error)?;
        let remote_id = match remote_id.trim() {
            "" => None,
            _ if !proof::has_id(platform) => None,
            text => proof::account_id(platform, Some(text)).map_err(proof_error)?,
        };
        let handle = handle.trim().to_string();
        if handle == account.handle && remote_id == account.remote_id {
            return Ok(account);
        }
        // The kept block names the handle and the id in the form of §5. It stays if that form does not change.
        let same_names = proof::account_handle(platform, &account.handle).ok() == Some(new_handle)
            && (!proof::has_id(platform) || remote_id == account.remote_id);
        let changed = open.store.set_identity(id, handle, remote_id)?;
        if !same_names {
            open.store.forget_proof(id)?;
        }
        Ok(changed)
    }

    /// Sets the URL where the person posted the kept block of an account. An empty text removes the URL.
    pub fn set_proof_location(&mut self, id: AccountId, location: &str) -> AppResult<ProofView> {
        let open = self.open_mut()?;
        let account = own_account(open, id)?;
        let mut kept = open
            .store
            .kept_proof(id)
            .cloned()
            .ok_or_else(|| AppError::new("proof_none", "the account has no kept proof"))?;
        let location = location.trim();
        kept.location = if location.is_empty() {
            None
        } else {
            Some(check_location(location)?)
        };
        open.store.keep_proof(id, kept)?;
        Ok(proof_view(open, &account))
    }

    /// What the check of the person's own proof in a gist needs. The account must be a GitHub account with a kept
    /// proof.
    pub fn own_proof_target(&self, id: AccountId) -> AppResult<OwnProofTarget> {
        let open = self.open_ref()?;
        let account = own_account(open, id)?;
        if account.platform != Platform::GitHub {
            return Err(AppError::new("proof_not_github", "only a GitHub proof has a check"));
        }
        if open.store.kept_proof(id).is_none() {
            return Err(AppError::new("proof_none", "the account has no kept proof"));
        }
        let remote_id = account.remote_id.ok_or_else(|| proof_error(ProofError::IdMissing))?;
        Ok(OwnProofTarget {
            account: id,
            remote_id,
            persona: open.persona.address,
            route: open.persona.route.clone(),
        })
    }

    /// Keeps the location of the person's own proof after a check that passed.
    pub fn own_proof_passed(&mut self, id: AccountId, location: &str) -> AppResult<ProofView> {
        self.set_proof_location(id, location)
    }

    /// Finds the block in a pasted text, and checks its form and its signature (`base/panim-proof-v1.md` §7, steps
    /// 2 to 6). It uses no network.
    ///
    /// The result does not show that the named account published the block. The person must compare the account and
    /// the platform of the post (steps 7 and 8).
    pub fn check_pasted_proof(&self, text: &str) -> AppResult<PastedView> {
        let open = self.open_ref()?;
        if text.len() > MAX_PASTE {
            return Err(AppError::new("paste_too_long", MAX_PASTE));
        }
        let block = proof::read_block(text).map_err(proof_error)?;
        block.check_signature().map_err(proof_error)?;
        let statement = block.statement;
        Ok(PastedView {
            persona: statement.persona.to_string(),
            platform: proof::platform_name(statement.platform),
            id: statement.id,
            handle: statement.handle,
            domain: match statement.domain {
                Domain::Proof => "proof",
                Domain::Revoke => "revoke",
            },
            date: statement.date.to_string(),
            own: statement.persona == open.persona.address,
        })
    }

    /// The timeline items that agree with the filter, in the form for the user interface.
    ///
    /// With `unread_only`, the items without a read state (the GitHub events) are not in the result. The session
    /// removes them before it applies the limit. Thus, many events do not push the unread notifications out of
    /// the result.
    pub fn timeline(&self, filter: &TimelineFilter) -> AppResult<Vec<ItemView>> {
        let open = self.open_ref()?;
        let github: Vec<AccountId> = open
            .store
            .accounts(None)
            .into_iter()
            .filter(|account| account.platform == Platform::GitHub)
            .map(|account| account.id)
            .collect();
        // The store gets the filter without the limit. The session applies the limit after it removes items.
        let all = TimelineFilter {
            limit: None,
            ..filter.clone()
        };
        let items = open.store.timeline(&all).into_iter().filter_map(|item| {
            let is_github = github.contains(&item.key.account);
            // Only a Telegram account has items with the prefix of a chat and this form of the raw record.
            let chat = (!is_github && item.key.source_id.starts_with(panim_telegram::CHAT_PREFIX))
                .then(|| panim_telegram::item_summary(&item.raw))
                .flatten();
            let has_read_state = !is_github || panim_github::has_read_state(&item.key.source_id);
            if filter.unread_only && !has_read_state {
                return None;
            }
            Some(ItemView {
                has_read_state,
                can_answer: (is_github && panim_github::answer_target(&item.raw, &item.key.source_id).is_some())
                    || chat.as_ref().is_some_and(|chat| chat.can_write),
                chat,
                event: is_github
                    .then(|| panim_github::event_summary(&item.raw, &item.key.source_id))
                    .flatten()
                    .map(|(kind, repo)| EventView { kind, repo }),
                has_link: item.link.is_some(),
                key: item.key,
                author: item.author,
                time: item.time,
                text: item.text,
                read: item.read || !has_read_state,
            })
        });
        Ok(items.take(filter.limit.unwrap_or(usize::MAX)).collect())
    }

    /// Sets the read state of a timeline item in the local store.
    pub fn set_read(&mut self, key: &ItemKey, read: bool) -> AppResult<()> {
        Ok(self.open_mut()?.store.set_read(key, read)?)
    }

    /// What a request for one timeline item of a joined account needs: the token, the route and the stored record.
    pub fn item_target(&self, key: &ItemKey) -> AppResult<ItemTarget> {
        let open = self.open_ref()?;
        let account = open
            .store
            .account(key.account)
            .ok_or_else(|| AppError::new("store", format!("no {}", key.account)))?;
        let joined = account
            .joined
            .ok_or_else(|| AppError::new("store", "the account is not joined"))?;
        let item = find_item(open, key)?;
        let token = open
            .secrets
            .get(joined.credentials)
            .ok_or_else(|| AppError::new("github_bad_token", "the account has no token"))?;
        Ok(ItemTarget {
            token,
            route: open.persona.route.clone(),
            raw: item.raw,
        })
    }

    /// The stored link of a timeline item.
    pub fn item_link(&self, key: &ItemKey) -> AppResult<Option<String>> {
        Ok(find_item(self.open_ref()?, key)?.link)
    }

    /// True if the vault is open and the account is a Telegram account.
    pub fn is_telegram(&self, id: AccountId) -> bool {
        let account = self.open.as_ref().and_then(|open| open.store.account(id));
        account.is_some_and(|account| account.platform == Platform::Telegram)
    }

    /// True if the vault is open and the account is a GitHub account.
    pub fn is_github(&self, id: AccountId) -> bool {
        let account = self.open.as_ref().and_then(|open| open.store.account(id));
        account.is_some_and(|account| account.platform == Platform::GitHub)
    }

    /// Puts items in the store directly. Only the tests use it.
    #[cfg(test)]
    pub fn put_test_items(&mut self, account: AccountId, items: Vec<NewItem>) {
        self.open
            .as_mut()
            .expect("the vault is open")
            .store
            .put_items(account, items)
            .unwrap();
    }

    /// The number of secrets in the vault. Only the tests use it.
    #[cfg(test)]
    pub fn secret_count(&self) -> usize {
        let open = self.open.as_ref().expect("the vault is open");
        (1..=50).filter(|n| open.secrets.get(SecretRef(*n)).is_some()).count()
    }

    /// The route of the persona. A request of a connector must use it.
    pub fn route(&self) -> AppResult<Route> {
        Ok(self.open_ref()?.persona.route.clone())
    }

    /// What a browser window of the persona needs: the profile folder and the route.
    pub fn profile_target(&self) -> AppResult<(PathBuf, Route)> {
        let open = self.open_ref()?;
        let profile = open
            .profile
            .clone()
            .ok_or_else(|| AppError::new("io", "the browser profile is not open"))?;
        Ok((profile, open.persona.route.clone()))
    }

    /// Keeps a new device login in memory. A login that was in work stops.
    pub fn github_login_begin(&mut self, login: DeviceLogin, now: Instant) -> AppResult<LoginView> {
        self.open_ref()?;
        let view = LoginView {
            user_code: login.user_code,
            verification_uri: login.verification_uri,
            interval: login.interval,
            expires_in: login.expires_in,
        };
        self.github.login = Some(PendingLogin {
            device_code: login.device_code,
            interval: login.interval,
            expires: now + Duration::from_secs(login.expires_in),
            next_poll: now + Duration::from_secs(login.interval),
        });
        Ok(view)
    }

    /// Tells what a poll of the device login must do. The function removes an expired login.
    pub fn github_login_step(&mut self, now: Instant) -> AppResult<LoginStep> {
        let route = self.route()?;
        let no_login = || AppError::new("github_login_none", "no login is in work");
        let login = self.github.login.as_mut().ok_or_else(no_login)?;
        if now >= login.expires {
            self.github.login = None;
            return Err(AppError::new("github_login_expired", "the code expired"));
        }
        if now < login.next_poll {
            return Ok(LoginStep::Wait {
                interval: login.interval,
            });
        }
        login.next_poll = now + Duration::from_secs(login.interval);
        Ok(LoginStep::Poll {
            device_code: login.device_code.clone(),
            route,
        })
    }

    /// The interval of the device login, in seconds. `slow_down` sets a new interval first.
    ///
    /// The result is `None` if the login with this device code is not in work.
    pub fn github_login_interval(&mut self, device_code: &SecretBytes, slow_down: Option<Option<u64>>) -> Option<u64> {
        let login = self.github.login.as_mut().filter(|l| l.device_code == *device_code)?;
        if let Some(new) = slow_down {
            // GitHub adds five seconds. Use its number if it gave one, but never a smaller interval.
            let step = panim_github::login::SLOW_DOWN_STEP;
            login.interval = new.unwrap_or(0).max(login.interval + step);
        }
        Some(login.interval)
    }

    /// Removes the device login with this device code, or each device login if there is no code.
    pub fn github_login_drop(&mut self, device_code: Option<&SecretBytes>) {
        if device_code.is_none_or(|code| self.github.login.as_ref().is_some_and(|l| l.device_code == *code)) {
            self.github.login = None;
        }
    }

    /// Completes the device login: seals the token and adds the account.
    ///
    /// The login with this device code must be in work. If the person cancelled it or locked the app, the token is
    /// not kept.
    pub fn github_login_finish(
        &mut self,
        device_code: &SecretBytes,
        token: SecretBytes,
        user: &User,
    ) -> AppResult<Account> {
        let in_work = self
            .github
            .login
            .as_ref()
            .is_some_and(|l| l.device_code == *device_code);
        if !in_work {
            return Err(AppError::new("github_login_none", "the login was cancelled"));
        }
        self.github.login = None;
        let open = self.open_mut()?;
        let remote_id = user.id.to_string();
        let exists =
            open.store.accounts(None).iter().any(|account| {
                account.platform == Platform::GitHub && account.remote_id.as_deref() == Some(&remote_id)
            });
        if exists {
            return Err(AppError::new("account_exists", "this GitHub account is in PANIM"));
        }
        let credentials = open.secrets.put(token)?;
        let added = open.store.add_account(NewAccount {
            persona: open.persona.id,
            platform: Platform::GitHub,
            display_name: user.login.clone(),
            handle: user.login.clone(),
            remote_id: Some(remote_id),
            joined: Some(JoinedAccess { credentials }),
            browser: None,
        });
        match added {
            Ok(account) => Ok(account),
            Err(error) => {
                // The account does not exist, so the token must not stay.
                let _ = open.secrets.remove(credentials);
                Err(error.into())
            }
        }
    }

    /// The sync jobs for the joined GitHub accounts. A locked session gives no jobs.
    ///
    /// The sync does not try an account while its rate limit continues. Without `force`, it also does not try an
    /// account that needs a new login. `now` is in seconds after 1970-01-01 00:00:00 UTC.
    ///
    /// The request for notifications has no `If-Modified-Since` with `force`, and after `FULL_SYNC_SECONDS`.
    pub fn sync_jobs(&mut self, force: bool, now: u64) -> Vec<SyncJob> {
        let Some(open) = &self.open else { return Vec::new() };
        self.github.clock += 1;
        let mut jobs = Vec::new();
        for account in open.store.accounts(Some(open.persona.id)) {
            let Some(joined) = account.joined.filter(|_| account.platform == Platform::GitHub) else {
                continue;
            };
            let entry = self.github.accounts.get(&account.id.0).cloned().unwrap_or_default();
            let limited = entry.wait_until.is_some_and(|until| now < until);
            if limited || (entry.needs_login && !force) {
                continue;
            }
            let Some(token) = open.secrets.get(joined.credentials) else {
                continue;
            };
            let full = force || entry.last_full.is_none_or(|last| now >= last + FULL_SYNC_SECONDS);
            jobs.push(SyncJob {
                generation: self.github.generation,
                account: account.id,
                login: account.handle,
                token,
                route: open.persona.route.clone(),
                last_modified: if full { None } else { entry.last_modified },
                etag: entry.etag,
                watched_etag: entry.watched_etag,
                started: self.github.clock,
            });
        }
        jobs
    }

    /// Records that a request to mark a thread as read on GitHub starts.
    pub fn local_read_start(&mut self, key: &ItemKey) {
        self.github.local_reads.insert(key.clone(), READ_IN_FLIGHT);
    }

    /// Records that the request to mark a thread as read ended, with or without success.
    ///
    /// A sync that starts after this time sees the state of GitHub. If the request failed, that sync makes the
    /// item unread again, because the thread is unread on GitHub.
    pub fn local_read_end(&mut self, key: &ItemKey) {
        self.github.clock += 1;
        if let Some(value) = self.github.local_reads.get_mut(key) {
            *value = self.github.clock;
        }
    }

    /// Keeps the results of one sync. Returns true if the timeline or the state of the account changed.
    ///
    /// The session ignores a result from before a lock, and a result for an account that does not exist.
    /// `now` is in milliseconds after 1970-01-01 00:00:00 UTC.
    pub fn sync_done(&mut self, job: &SyncJob, result: SyncResult, now: u64) -> bool {
        if job.generation != self.github.generation {
            return false;
        }
        let Some(open) = self.open.as_mut() else { return false };
        if open.store.account(job.account).is_none() {
            return false;
        }
        let mut entry = self.github.accounts.get(&job.account.0).cloned().unwrap_or_default();
        let before = (entry.error, entry.needs_login, entry.last_success.is_some(), entry.cut);
        let mut changed = false;
        let mut failure: Option<panim_github::Error> = None;
        let mut poll_interval = 0;
        let store_failure = |error: panim_core::Error| panim_github::Error::BadData(error.to_string());

        // 1. The watched repositories. If the list is not available, the sync uses the last list.
        let mut good_list = false;
        let mut list_changed = false;
        match result.watched {
            Ok(watched) => match watched.repos {
                Some(repos) => {
                    // The first list filters the events of this sync, so it is not a change.
                    list_changed = entry.watched.as_ref().is_some_and(|old| *old != repos);
                    entry.watched = Some(repos);
                    entry.watched_etag = watched.validator;
                    good_list = true;
                }
                None if entry.watched.is_some() => {
                    entry.watched_etag = watched.validator;
                    good_list = true;
                }
                None => {
                    // A 304 without a list in memory. The next sync asks for the full list.
                    entry.watched_etag = None;
                    note(
                        &mut failure,
                        panim_github::Error::BadData("no list of watched repositories".into()),
                    );
                }
            },
            Err(error) => note(&mut failure, error),
        }

        // 2. The notifications. A complete list also gives the read state of GitHub.
        match result.notifications {
            Ok(fetched) => {
                poll_interval = poll_interval.max(fetched.poll_interval.unwrap_or(0));
                let mut kept = true;
                if let Some(items) = fetched.items {
                    let unread: BTreeSet<String> = items.iter().map(|item| item.source_id.clone()).collect();
                    if differs(&open.store, job.account, &items) {
                        match open.store.put_items(job.account, items) {
                            Ok(_) => changed = true,
                            Err(error) => {
                                kept = false;
                                note(&mut failure, store_failure(error));
                            }
                        }
                    }
                    if kept {
                        entry.last_full = Some(now / 1000);
                        entry.cut = !fetched.complete;
                    }
                    if kept && fetched.complete {
                        let stored = items_of(&open.store, job.account);
                        let reads = &self.github.local_reads;
                        let protected = |source_id: &str| {
                            let key = ItemKey {
                                account: job.account,
                                source_id: source_id.to_string(),
                            };
                            reads.get(&key).is_some_and(|ended| *ended >= job.started)
                        };
                        let changes = read_changes(&stored, &unread, protected);
                        match open.store.set_read_many(job.account, &changes) {
                            Ok(count) => changed |= count > 0,
                            Err(error) => {
                                kept = false;
                                note(&mut failure, store_failure(error));
                            }
                        }
                    }
                }
                if kept {
                    entry.last_modified = fetched.validator;
                } else {
                    // Do not keep the validator. The next sync gets the items again.
                    entry.last_modified = None;
                }
                // The reads that ended before this sync started are now known to GitHub.
                let account = job.account;
                self.github
                    .local_reads
                    .retain(|key, ended| key.account != account || *ended >= job.started);
            }
            Err(error) => note(&mut failure, error),
        }

        // 3. The received events. Only the events of the watched repositories stay.
        match result.events {
            Ok(fetched) => {
                poll_interval = poll_interval.max(fetched.poll_interval.unwrap_or(0));
                match (fetched.items, &entry.watched) {
                    (Some(items), Some(watched)) => {
                        let items: Vec<NewItem> = items
                            .into_iter()
                            .filter(|item| panim_github::is_watched_event(&item.raw, &item.source_id, watched))
                            .collect();
                        let mut kept = true;
                        if differs(&open.store, job.account, &items) {
                            match open.store.put_items(job.account, items) {
                                Ok(_) => changed = true,
                                Err(error) => {
                                    kept = false;
                                    note(&mut failure, store_failure(error));
                                }
                            }
                        }
                        entry.etag = if kept { fetched.validator } else { None };
                    }
                    // Without a list, the sync adds no event. The next sync gets the events again.
                    (Some(_), None) => entry.etag = None,
                    (None, _) => entry.etag = fetched.validator,
                }
            }
            Err(error) => note(&mut failure, error),
        }
        if list_changed {
            // A new list can show events that came before. The next sync gets the full page of events.
            entry.etag = None;
        }

        // 4. With a good list, remove the stored events of the repositories that are not watched.
        if good_list && let Some(watched) = &entry.watched {
            let stale: Vec<String> = open
                .store
                .timeline(&TimelineFilter {
                    accounts: Some(vec![job.account]),
                    ..Default::default()
                })
                .into_iter()
                .filter(|item| item.key.source_id.starts_with(panim_github::sync::EVENT_PREFIX))
                .filter(|item| !panim_github::is_watched_event(&item.raw, &item.key.source_id, watched))
                .map(|item| item.key.source_id)
                .collect();
            match open.store.remove_items(job.account, &stale) {
                Ok(count) => changed |= count > 0,
                Err(error) => note(&mut failure, store_failure(error)),
            }
        }

        entry.poll_interval = poll_interval;
        entry.wait_until = None;
        match &failure {
            None => {
                entry.last_success = Some(now);
                entry.error = None;
                entry.needs_login = false;
            }
            Some(error) => {
                entry.error = Some(github_code(error));
                if *error == panim_github::Error::BadToken {
                    entry.needs_login = true;
                }
                if let panim_github::Error::RateLimit { reset, retry_after } = error {
                    let now = now / 1000;
                    let reset = reset.and_then(|reset| u64::try_from(reset).ok());
                    let until = reset.or(retry_after.map(|seconds| now + seconds));
                    entry.wait_until = Some(until.unwrap_or(now + SYNC_SECONDS));
                }
            }
        }
        let after = (entry.error, entry.needs_login, entry.last_success.is_some(), entry.cut);
        self.github.accounts.insert(job.account.0, entry);
        changed || before != after
    }

    /// Records that GitHub refused the token of an account. Returns true if the state changed.
    pub fn mark_needs_login(&mut self, id: AccountId) -> bool {
        if !self.has_account(id) {
            return false;
        }
        let entry = self.github.accounts.entry(id.0).or_default();
        let changed = !entry.needs_login;
        entry.needs_login = true;
        entry.error = Some("github_bad_token");
        changed
    }

    /// The state of each joined account: the unread count and the result of the last sync. The key is the account ID.
    pub fn sync_state(&self) -> AppResult<BTreeMap<u64, SyncView>> {
        let open = self.open_ref()?;
        let accounts = open.store.accounts(Some(open.persona.id));
        let views = accounts.into_iter().filter(|a| a.joined.is_some()).map(|account| {
            if account.platform == Platform::Telegram {
                // The count of Telegram is the number of shown chats that are unread, not the number of messages.
                let unread = open.store.unread_count(account.id);
                return (account.id.0, self.telegram.view(account.id.0, unread));
            }
            let entry = self.github.accounts.get(&account.id.0).cloned().unwrap_or_default();
            let unread = if account.platform == Platform::GitHub {
                // The count of GitHub is the count of the inbox. Only notification threads have a read state.
                let unread_items = TimelineFilter {
                    accounts: Some(vec![account.id]),
                    unread_only: true,
                    ..Default::default()
                };
                let items = open.store.timeline(&unread_items);
                items
                    .iter()
                    .filter(|item| panim_github::has_read_state(&item.key.source_id))
                    .count()
            } else {
                open.store.unread_count(account.id)
            };
            let view = SyncView {
                unread,
                unread_more: entry.cut,
                last_success: entry.last_success,
                error: entry.error,
                needs_login: entry.needs_login,
            };
            (account.id.0, view)
        });
        Ok(views.collect())
    }

    /// The number of seconds between two rounds of the sync.
    pub fn sync_interval(&self) -> u64 {
        let asked = self.github.accounts.values().map(|entry| entry.poll_interval).max();
        asked.unwrap_or(0).max(SYNC_SECONDS)
    }

    /// What a browser window of the account needs: the first page, the profile folder and the route.
    pub fn browser_target(&self, id: AccountId) -> AppResult<(Account, PathBuf, Route)> {
        let open = self.open_ref()?;
        let account = open
            .store
            .account(id)
            .ok_or_else(|| AppError::new("store", format!("no {id}")))?;
        let profile = open
            .profile
            .clone()
            .ok_or_else(|| AppError::new("io", "the browser profile is not open"))?;
        Ok((account, profile, open.persona.route.clone()))
    }

    /// The home page of a joined account without a browser view, and the title of its profile window.
    ///
    /// A click on a joined GitHub account opens `https://github.com/` in the profile of the persona. An account
    /// without a known home page gives the error `no_window`.
    pub fn home_page(&self, id: AccountId) -> AppResult<(url::Url, String)> {
        let open = self.open_ref()?;
        let account = open
            .store
            .account(id)
            .ok_or_else(|| AppError::new("store", format!("no {id}")))?;
        let page = match account.platform {
            Platform::GitHub if account.browser.is_none() => "https://github.com/",
            _ => return Err(AppError::new("no_window", "the account has no home page")),
        };
        let url = url::Url::parse(page).map_err(|e| AppError::new("store", e))?;
        Ok((url, format!("{} · {}", account.display_name, account.handle)))
    }

    /// Sets the unread count of an account from the title of its page. Returns true if the state changed.
    ///
    /// The session keeps only the count. It ignores the title if the vault is locked or the account does not exist.
    pub fn set_unread_title(&mut self, id: AccountId, title: &str) -> bool {
        if !self.has_account(id) {
            return false;
        }
        self.unread.set_title(id.0, title)
    }

    /// Records a notification of the page of an account. Returns true if the state changed.
    pub fn record_notice(&mut self, id: AccountId, now: Instant) -> bool {
        if !self.has_account(id) {
            return false;
        }
        self.unread.record_notice(id.0, now)
    }

    /// Clears the notice flag of an account. Returns true if the state changed.
    pub fn clear_notice(&mut self, id: AccountId) -> bool {
        self.unread.clear_notice(id.0)
    }

    /// Removes the unread state of an account, for example when its window closes. Returns true if there was a state.
    pub fn drop_unread(&mut self, id: AccountId) -> bool {
        self.unread.remove(id.0)
    }

    /// The unread state of each account with an open window. The key is the account ID.
    pub fn unread(&self) -> BTreeMap<u64, Unread> {
        self.unread.list()
    }

    fn has_account(&self, id: AccountId) -> bool {
        self.open.as_ref().is_some_and(|open| open.store.account(id).is_some())
    }

    /// Seals the open browser profile into the vault. The app calls this every few minutes.
    pub fn seal_profile(&mut self, close: bool) -> AppResult<()> {
        let Some(open) = self.open.as_mut() else { return Ok(()) };
        if open.profile.is_none() {
            return Ok(());
        }
        let folder = open.persona.profile_folder();
        if close {
            open.profile = None;
            self.area.close(&open.vault, &folder)?;
        } else {
            self.area.seal(&open.vault, &folder)?;
        }
        Ok(())
    }

    fn open_profile(&mut self) -> AppResult<()> {
        let open = self.open.as_mut().expect("the vault is open");
        let path = self.area.open(&open.vault, &open.persona.profile_folder())?;
        open.profile = Some(path);
        Ok(())
    }

    fn open_ref(&self) -> AppResult<&Open> {
        self.open
            .as_ref()
            .ok_or_else(|| AppError::new("locked", "the vault is locked"))
    }

    fn open_mut(&mut self) -> AppResult<&mut Open> {
        self.open
            .as_mut()
            .ok_or_else(|| AppError::new("locked", "the vault is locked"))
    }
}

/// Keeps the first error of a sync. A refused token replaces a different error, so that the account gets the mark.
fn note(failure: &mut Option<panim_github::Error>, error: panim_github::Error) {
    let replace = failure.is_none() || (error == panim_github::Error::BadToken);
    if replace {
        *failure = Some(error);
    }
}

/// The source identifiers and the read state of all the stored items of an account.
fn items_of(store: &SealedStore, account: AccountId) -> Vec<(String, bool)> {
    let filter = TimelineFilter {
        accounts: Some(vec![account]),
        ..Default::default()
    };
    store
        .timeline(&filter)
        .into_iter()
        .map(|item| (item.key.source_id, item.read))
        .collect()
}

/// The changes of the read state after a complete list of the unread threads of GitHub.
///
/// - A stored thread that is in the list is unread on GitHub, so it becomes unread.
/// - A stored thread that is not in the list is read on GitHub, so it becomes read.
/// - A `protected` thread does not become unread. The person read it in PANIM, and GitHub possibly did not get the
///   read before the list.
/// - An item without a read state, for example an event, does not change.
///
/// Use this function only for a complete list. If the page limit cut the list, a missing thread can be unread.
pub(crate) fn read_changes(
    stored: &[(String, bool)],
    unread_on_github: &BTreeSet<String>,
    protected: impl Fn(&str) -> bool,
) -> Vec<(String, bool)> {
    stored
        .iter()
        .filter(|(source_id, _)| panim_github::has_read_state(source_id))
        .filter_map(|(source_id, read)| {
            let read_on_github = !unread_on_github.contains(source_id);
            if read_on_github == *read || (!read_on_github && protected(source_id)) {
                return None;
            }
            Some((source_id.clone(), read_on_github))
        })
        .collect()
}

fn find_item(open: &Open, key: &ItemKey) -> AppResult<panim_core::TimelineItem> {
    let filter = TimelineFilter {
        accounts: Some(vec![key.account]),
        ..Default::default()
    };
    open.store
        .timeline(&filter)
        .into_iter()
        .find(|item| item.key == *key)
        .ok_or_else(|| AppError::new("store", "the timeline has no such item"))
}

/// True if the store does not have each of these items with the same data.
fn differs(store: &SealedStore, account: AccountId, items: &[NewItem]) -> bool {
    let filter = TimelineFilter {
        accounts: Some(vec![account]),
        ..Default::default()
    };
    let old: BTreeMap<String, _> = store
        .timeline(&filter)
        .into_iter()
        .map(|item| (item.key.source_id.clone(), item))
        .collect();
    items.iter().any(|new| {
        old.get(&new.source_id).is_none_or(|old| {
            old.time != new.time
                || old.text != new.text
                || old.author != new.author
                || old.link != new.link
                || old.raw != new.raw
        })
    })
}

/// An account of the persona of the session.
fn own_account(open: &Open, id: AccountId) -> AppResult<Account> {
    open.store
        .account(id)
        .filter(|account| account.persona == open.persona.id)
        .ok_or_else(|| AppError::new("store", format!("no {id}")))
}

/// The date of a kept block, `YYYY-MM-DD`.
fn kept_date(kept: &KeptProof) -> Option<String> {
    proof::read_block(&kept.block)
        .ok()
        .map(|block| block.statement.date.to_string())
}

fn proof_view(open: &Open, account: &Account) -> ProofView {
    let kept = open.store.kept_proof(account.id);
    ProofView {
        account: account.id.0,
        id_needed: proof::account_id(account.platform, account.remote_id.as_deref()).is_err(),
        identity_editable: account.joined.is_none(),
        block: kept.map(|kept| kept.block.clone()),
        date: kept.and_then(kept_date),
        location: kept.and_then(|kept| kept.location.clone()),
    }
}

/// Makes sure that a location is an `https` URL, and gives its usual form.
fn check_location(text: &str) -> AppResult<String> {
    let bad = || AppError::new("proof_bad_location", "the location is not an https URL");
    if text.chars().count() > MAX_LOCATION {
        return Err(bad());
    }
    let url = url::Url::parse(text).map_err(|_| bad())?;
    let plain =
        url.scheme() == "https" && url.host_str().is_some() && url.username().is_empty() && url.password().is_none();
    if !plain {
        return Err(bad());
    }
    Ok(url.to_string())
}

fn persona_words(open: &Open) -> AppResult<RecoveryWords> {
    let reference = open
        .state
        .persona_words
        .get(&open.persona.id.0)
        .ok_or_else(|| AppError::new("no_words", "this persona has no recovery words"))?;
    let secret = open
        .secrets
        .get(*reference)
        .ok_or_else(|| AppError::new("damaged", "the recovery words are missing"))?;
    RecoveryWords::from_secret(&secret).ok_or_else(|| AppError::new("damaged", "the recovery words are damaged"))
}

fn persona_key(open: &Open) -> AppResult<PersonaKey> {
    let reference = open
        .state
        .persona_keys
        .get(&open.persona.id.0)
        .ok_or_else(|| AppError::new("damaged", "the persona has no key"))?;
    let secret = open
        .secrets
        .get(*reference)
        .ok_or_else(|| AppError::new("damaged", "the persona key is missing"))?;
    PersonaKey::from_secret(&secret).map_err(|e| AppError::new("damaged", e))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    struct Fixture {
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        data: PathBuf,
        memory: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let data = tempfile::tempdir().unwrap();
            let memory = tempfile::tempdir().unwrap();
            Self {
                data: data.path().join("panim"),
                memory: memory.path().to_path_buf(),
                _dirs: (data, memory),
            }
        }

        fn session(&self) -> Session {
            Session::new(self.data.clone(), ProfileArea::at(self.memory.clone()), Strength::TEST)
        }
    }

    #[test]
    fn first_start_needs_the_words() {
        let f = Fixture::new();
        let mut session = f.session();
        assert_eq!(session.status().unwrap().phase, Phase::FirstRun);
        assert_eq!(session.create("short").unwrap_err().code, "passcode_too_short");
        session.create("passcode1").unwrap();
        assert_eq!(session.status().unwrap().phase, Phase::Backup);
        let words = session.recovery_words(None).unwrap();
        assert_eq!(words.len(), 12);

        // The first start is not complete after a restart without the check of the words.
        session.lock().unwrap();
        let mut session = f.session();
        assert_eq!(session.status().unwrap().phase, Phase::Locked);
        session.unlock("passcode1").unwrap();
        assert_eq!(session.status().unwrap().phase, Phase::Backup);
        assert_eq!(session.recovery_words(None).unwrap(), words);

        let wrong = vec![(3, "zzz".to_string())];
        assert_eq!(session.check_words(&wrong).unwrap_err().code, "words_differ");
        assert_eq!(
            session.check_words(&[(13, words[0].clone())]).unwrap_err().code,
            "words_differ"
        );
        let answers = vec![
            (2, words[1].to_uppercase()),
            (7, format!(" {} ", words[6])),
            (12, words[11].clone()),
        ];
        session.check_words(&answers).unwrap();
        assert_eq!(session.status().unwrap().phase, Phase::Open);

        // After the first start, the words need the passcode.
        assert_eq!(session.recovery_words(None).unwrap_err().code, "wrong_passcode");
        assert_eq!(session.recovery_words(Some("passcode1")).unwrap(), words);

        // The key file is an extra copy.
        let key_file = f.memory.join("key.json");
        assert_eq!(
            session.export_key_file("short", &key_file).unwrap_err().code,
            "passcode_too_short"
        );
        session.export_key_file("key file password", &key_file).unwrap();
        assert_eq!(
            session
                .export_key_file("key file password", &key_file)
                .unwrap_err()
                .code,
            "file_exists"
        );
    }

    #[test]
    fn recovery_with_the_words() {
        let f = Fixture::new();
        let mut session = f.session();
        session.create("passcode1").unwrap();
        let words = session.recovery_words(None).unwrap();
        session.check_words(&[(1, words[0].clone())]).unwrap();
        let address = session.status().unwrap().persona.unwrap().address;
        session.lock().unwrap();

        assert_eq!(
            session
                .recover_with_words("abandon abandon", "passcode2")
                .unwrap_err()
                .code,
            "bad_words"
        );
        let other = RecoveryWords::generate().unwrap().words().join(" ");
        assert_eq!(
            session.recover_with_words(&other, "passcode2").unwrap_err().code,
            "wrong_recovery_key"
        );
        session.recover_with_words(&words.join("  "), "passcode2").unwrap();
        assert_eq!(session.status().unwrap().persona.unwrap().address, address);
        session.lock().unwrap();
        session.unlock("passcode2").unwrap();
    }

    #[test]
    fn gate_and_recovery_with_the_key_file() {
        let f = Fixture::new();
        let mut session = f.session();
        session.create("passcode1").unwrap();
        let key_file = f.memory.join("key.json");
        session.export_key_file("key file password", &key_file).unwrap();
        let address = session.status().unwrap().persona.unwrap().address;
        session.lock().unwrap();

        for left in (1..5).rev() {
            let error = session.unlock("not-the-one").unwrap_err();
            assert_eq!((error.code, error.tries_left), ("wrong_passcode", Some(left)));
        }
        assert_eq!(session.unlock("not-the-one").unwrap_err().code, "gate_locked");
        assert_eq!(session.status().unwrap().phase, Phase::GateLocked);

        assert_eq!(
            session
                .recover(&key_file, "wrong password", "passcode2")
                .unwrap_err()
                .code,
            "wrong_key_file_password"
        );
        session.recover(&key_file, "key file password", "passcode2").unwrap();
        let status = session.status().unwrap();
        assert_eq!((status.phase, status.persona.unwrap().address), (Phase::Open, address));
        session.lock().unwrap();
        session.unlock("passcode2").unwrap();
    }

    #[test]
    fn browser_accounts_and_the_profile() {
        let f = Fixture::new();
        let mut session = f.session();
        session.create("passcode1").unwrap();
        let x = session.add_browser_account(Platform::X, "", "me").unwrap();
        assert_eq!(x.display_name, "X");
        assert_eq!(
            session.add_browser_account(Platform::X, "", "me2").unwrap_err().code,
            "browser_login_in_use"
        );
        assert_eq!(
            session
                .add_browser_account(Platform::GitHub, "", "me")
                .unwrap_err()
                .code,
            "store"
        );
        let mail = session
            .add_browser_account(Platform::Email, "Proton", "me@proton.me")
            .unwrap();
        assert_eq!(mail.browser.as_ref().unwrap().start_url, "https://mail.proton.me");

        // The profile is in memory while the vault is open, and it stays after a lock.
        let (_, profile, route) = session.browser_target(x.id).unwrap();
        assert_eq!(route, Route::Direct);
        fs::write(profile.join("cookies"), "login").unwrap();
        session.lock().unwrap();
        assert!(!profile.exists());
        session.unlock("passcode1").unwrap();
        let (_, profile, _) = session.browser_target(x.id).unwrap();
        assert_eq!(fs::read_to_string(profile.join("cookies")).unwrap(), "login");

        session.remove_account(x.id).unwrap();
        assert_eq!(session.accounts().unwrap(), vec![mail]);
    }

    #[test]
    fn route_and_passcode_and_settings() {
        let f = Fixture::new();
        let mut session = f.session();
        session
            .set_settings(Settings {
                language: "zh".into(),
                auto_lock_minutes: 5,
            })
            .unwrap();
        session.create("passcode1").unwrap();
        let proxy = Route::Proxy(panim_core::ProxyEndpoint {
            kind: panim_core::ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 9050,
        });
        session.set_route(proxy.clone()).unwrap();
        assert_eq!(
            session.change_passcode("wrong-passcode", "passcode2").unwrap_err().code,
            "wrong_passcode"
        );
        session.change_passcode("passcode1", "passcode2").unwrap();
        session.lock().unwrap();

        let mut session = f.session();
        assert_eq!(session.status().unwrap().settings.language, "zh");
        session.unlock("passcode2").unwrap();
        assert_eq!(session.status().unwrap().persona.unwrap().route, proxy);
        assert!(
            session
                .set_settings(Settings {
                    language: "fr".into(),
                    auto_lock_minutes: 5
                })
                .is_err()
        );
    }

    #[test]
    fn unread_state_of_the_accounts() {
        let f = Fixture::new();
        let mut session = f.session();
        assert!(
            !session.set_unread_title(AccountId(1), "(3) Home / X"),
            "the vault is locked"
        );
        session.create("passcode1").unwrap();
        let x = session.add_browser_account(Platform::X, "", "me").unwrap();
        let discord = session.add_browser_account(Platform::Discord, "", "me").unwrap();
        assert!(
            !session.set_unread_title(AccountId(999), "(3) Home / X"),
            "no such account"
        );

        assert!(session.set_unread_title(x.id, "(3) Home / X"));
        assert!(!session.set_unread_title(x.id, "(3) Explore / X"));
        assert!(session.set_unread_title(discord.id, "• Discord | #general"));
        let now = Instant::now();
        assert!(session.record_notice(x.id, now));
        assert!(!session.record_notice(x.id, now));
        let state = session.unread();
        assert_eq!(state.len(), 2);
        assert_eq!(
            state[&x.id.0],
            Unread {
                count: 3,
                some: false,
                notice: true
            }
        );
        assert!(state[&discord.id.0].some);

        // The person looks at the window.
        assert!(session.clear_notice(x.id));
        assert!(!session.unread()[&x.id.0].notice);

        // The window closes.
        assert!(session.drop_unread(discord.id));
        assert!(!session.drop_unread(discord.id));
        assert_eq!(session.unread().keys().copied().collect::<Vec<_>>(), vec![x.id.0]);

        // The removal of an account removes its state.
        session.set_unread_title(discord.id, "(1) Discord");
        session.remove_account(discord.id).unwrap();
        assert!(!session.unread().contains_key(&discord.id.0));

        // The lock removes all states, and the unlock does not bring them back.
        session.lock().unwrap();
        assert!(session.unread().is_empty());
        session.unlock("passcode1").unwrap();
        assert!(session.unread().is_empty());
        assert!(!session.record_notice(discord.id, now), "the account was removed");
    }

    #[test]
    fn locked_session_refuses_work() {
        let f = Fixture::new();
        let session = f.session();
        assert_eq!(session.accounts().unwrap_err().code, "locked");
    }

    /// 2026-11-12 00:00:00 UTC, in seconds.
    const NOV_12: u64 = 1_794_441_600;
    const DAY: u64 = 86_400;

    // The worked example of base/panim-proof-v1.md §9. The key is 32 bytes of 0x46, not the key of the session.
    pub(crate) const EXAMPLE_PROOF: &str = "panim/1-proof
persona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f
platform github
account 583231 octocat
date 2026-11-12
sig 0xb566ce41a13d26ab1ed686f0f3032e296d6eb7953aa0ccef96065dbc33313ba771817564b371933a0cbaa3cda6cbe6a661e49eef56faddbf52033e3f5f389a2f1c";

    fn open_session(f: &Fixture) -> Session {
        let mut session = f.session();
        session.create("passcode1").unwrap();
        let words = session.recovery_words(None).unwrap();
        session.check_words(&[(1, words[0].clone())]).unwrap();
        session
    }

    fn own_address(session: &Session) -> String {
        session.status().unwrap().persona.unwrap().address.to_lowercase()
    }

    fn view(session: &Session, id: AccountId) -> ProofView {
        session
            .proofs()
            .unwrap()
            .into_iter()
            .find(|view| view.account == id.0)
            .unwrap()
    }

    #[test]
    fn proofs_for_browser_view_accounts() {
        let f = Fixture::new();
        let mut session = open_session(&f);
        let x = session.add_browser_account(Platform::X, "", "@me").unwrap();
        let state = view(&session, x.id);
        assert!(state.id_needed && state.identity_editable && state.block.is_none());

        // X has a numeric id. PANIM does not read the page, so the person gives it.
        assert_eq!(
            session.make_proof(x.id, NOV_12, false).unwrap_err().code,
            "proof_id_missing"
        );
        assert_eq!(
            session.set_identity(x.id, "@me", "0123").unwrap_err().code,
            "proof_bad_id"
        );
        assert_eq!(
            session.set_identity(x.id, "@", "123").unwrap_err().code,
            "proof_bad_handle"
        );
        let x = session.set_identity(x.id, " @me ", " 12345 ").unwrap();
        assert_eq!((x.handle.as_str(), x.remote_id.as_deref()), ("@me", Some("12345")));
        assert!(!view(&session, x.id).id_needed);

        // The block passes the check of the signature, and names the account.
        let made = session.make_proof(x.id, NOV_12 + 100, false).unwrap();
        let text = made.block.clone().unwrap();
        assert_eq!(made.date.as_deref(), Some("2026-11-12"));
        assert_eq!(made.location, None);
        let block = proof::parse_block(&text).unwrap();
        block.check_signature().unwrap();
        assert!(block.is_from(Platform::X, Some("12345"), Some("me")));
        assert_eq!(block.statement.persona.to_string(), own_address(&session));
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("\naccount 12345 me\n"));

        // The same date gives the same block. A later date asks for a confirmation.
        assert_eq!(session.make_proof(x.id, NOV_12 + DAY - 1, false).unwrap(), made);
        let error = session.make_proof(x.id, NOV_12 + DAY, false).unwrap_err();
        assert_eq!((error.code, error.detail.as_str()), ("proof_replace", "2026-11-12"));
        assert_eq!(
            view(&session, x.id).block,
            Some(text.clone()),
            "no change without the confirmation"
        );
        let later = session.make_proof(x.id, NOV_12 + DAY, true).unwrap();
        assert_eq!(later.date.as_deref(), Some("2026-11-13"));

        // The location of the post.
        assert_eq!(
            session
                .set_proof_location(x.id, "http://x.com/me/status/1")
                .unwrap_err()
                .code,
            "proof_bad_location"
        );
        assert_eq!(
            session
                .set_proof_location(x.id, "https://user:pw@x.com/")
                .unwrap_err()
                .code,
            "proof_bad_location"
        );
        let located = session.set_proof_location(x.id, " https://x.com/me/status/1 ").unwrap();
        assert_eq!(located.location.as_deref(), Some("https://x.com/me/status/1"));

        // The kept proof stays after a lock and a new open.
        session.lock().unwrap();
        let mut session = f.session();
        session.unlock("passcode1").unwrap();
        assert_eq!(view(&session, x.id), located);
        let x = session.accounts().unwrap().into_iter().find(|a| a.id == x.id).unwrap();
        assert_eq!(x.remote_id.as_deref(), Some("12345"));

        // The same identity keeps the proof. A new id removes it, because the block names the old id.
        session.set_identity(x.id, "@me", "12345").unwrap();
        assert!(view(&session, x.id).block.is_some());
        session.set_identity(x.id, "@me", "777").unwrap();
        assert_eq!(view(&session, x.id).block, None);
        assert_eq!(
            session.set_proof_location(x.id, "https://x.com/").unwrap_err().code,
            "proof_none"
        );
    }

    #[test]
    fn email_and_discord_proofs() {
        let f = Fixture::new();
        let mut session = open_session(&f);
        // A Proton Mail browser-view account is an email account: the id is `-`, and the address is the handle.
        let mail = session
            .add_browser_account(Platform::Email, "Proton", "Me@Proton.me")
            .unwrap();
        assert_eq!(mail.platform, Platform::Email);
        assert!(!view(&session, mail.id).id_needed);
        let made = session.make_proof(mail.id, NOV_12, false).unwrap();
        assert!(
            made.block
                .unwrap()
                .contains("\nplatform email\naccount - me@proton.me\n")
        );
        // Email has no id, so a given id is not kept. The handle in the form of §5 does not change, so the proof stays.
        let mail = session.set_identity(mail.id, "me@proton.me", "42").unwrap();
        assert_eq!(mail.remote_id, None);
        assert!(view(&session, mail.id).block.is_some());
        assert_eq!(
            session.set_identity(mail.id, "me", "").unwrap_err().code,
            "proof_bad_handle"
        );

        let discord = session.add_browser_account(Platform::Discord, "", "").unwrap();
        assert_eq!(
            session.make_proof(discord.id, NOV_12, false).unwrap_err().code,
            "proof_id_missing"
        );
        assert_eq!(
            session.set_identity(discord.id, "", "1").unwrap_err().code,
            "proof_bad_handle"
        );
        session
            .set_identity(discord.id, "Some.Name", "80351110224678912")
            .unwrap();
        let made = session.make_proof(discord.id, NOV_12, false).unwrap();
        assert!(made.block.unwrap().contains("\naccount 80351110224678912 some.name\n"));

        // The removal of an account removes its kept proof, also after a new open.
        session.remove_account(discord.id).unwrap();
        let again = session.add_browser_account(Platform::Discord, "", "x").unwrap();
        session.lock().unwrap();
        session.unlock("passcode1").unwrap();
        assert!(view(&session, mail.id).block.is_some());
        assert_eq!(view(&session, again.id).block, None);
        assert_eq!(session.make_proof(discord.id, NOV_12, false).unwrap_err().code, "store");
    }

    #[test]
    fn a_vault_without_proofs_opens_and_keeps_a_proof() {
        let f = Fixture::new();
        let mut session = open_session(&f);
        let x = session.add_browser_account(Platform::X, "", "me").unwrap();
        session.lock().unwrap();
        // The vault has no file of proofs, as a vault from before milestone 6.
        {
            let vault = Vault::open(&f.data, "passcode1", KdfParams::TEST).unwrap();
            assert!(vault.read("store/proofs").unwrap().is_none());
        }
        session.unlock("passcode1").unwrap();
        assert_eq!(view(&session, x.id).block, None);
        session.set_identity(x.id, "me", "5").unwrap();
        session.make_proof(x.id, NOV_12, false).unwrap();
        session.lock().unwrap();
        session.unlock("passcode1").unwrap();
        assert!(view(&session, x.id).block.is_some());
    }

    #[test]
    fn pasted_proof_check() {
        let f = Fixture::new();
        let mut session = open_session(&f);
        let code = |session: &Session, text: &str| session.check_pasted_proof(text).unwrap_err().code;

        let example = session.check_pasted_proof(EXAMPLE_PROOF).unwrap();
        assert_eq!(
            example,
            PastedView {
                persona: "0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f".into(),
                platform: "github",
                id: Some("583231".into()),
                handle: Some("octocat".into()),
                domain: "proof",
                date: "2026-11-12".into(),
                own: false,
            }
        );
        // Text around the block, and CR LF line ends.
        let post = format!("My proof:\r\n\r\n{}\r\n\r\nBye", EXAMPLE_PROOF.replace('\n', "\r\n"));
        assert_eq!(session.check_pasted_proof(&post).unwrap(), example);

        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace("date 2026-11-12", "date 2026-11-13")),
            "proof_bad_signature"
        );
        assert_eq!(code(&session, "no block here"), "proof_no_block");
        let unknown = session
            .check_pasted_proof(&EXAMPLE_PROOF.replace("platform github", "platform facebook"))
            .unwrap_err();
        assert_eq!(
            (unknown.code, unknown.detail.as_str()),
            ("proof_unknown_platform", "facebook")
        );
        let short: String = EXAMPLE_PROOF.lines().take(4).collect::<Vec<_>>().join("\n");
        assert_eq!(code(&session, &short), "proof_short_block");
        let line = session
            .check_pasted_proof(&EXAMPLE_PROOF.replace("persona 0x", "persona  0x"))
            .unwrap_err();
        assert_eq!((line.code, line.detail.as_str()), ("proof_bad_line", "2"));
        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace("2026-11-12\n", "2026-02-30\n")),
            "proof_bad_date"
        );
        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace("583231 octocat", "- -")),
            "proof_no_account"
        );
        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace("583231 ", "0583231 ")),
            "proof_bad_id"
        );
        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace(" octocat", " @octocat")),
            "proof_bad_handle"
        );
        assert_eq!(
            code(&session, &EXAMPLE_PROOF.replace("octocat", "octocät")),
            "proof_bad_byte"
        );
        let long = format!("{}{EXAMPLE_PROOF}", " ".repeat(MAX_PASTE));
        assert_eq!(code(&session, &long), "paste_too_long");

        // A block of the person's own persona.
        let x = session.add_browser_account(Platform::X, "", "me").unwrap();
        session.set_identity(x.id, "me", "9").unwrap();
        let own = session.make_proof(x.id, NOV_12, false).unwrap().block.unwrap();
        let checked = session.check_pasted_proof(&own).unwrap();
        assert!(checked.own);
        assert_eq!((checked.platform, checked.id.as_deref()), ("x", Some("9")));
    }

    #[test]
    fn locked_session_refuses_the_proof_commands() {
        let f = Fixture::new();
        let mut session = open_session(&f);
        let x = session.add_browser_account(Platform::X, "", "me").unwrap();
        session.lock().unwrap();
        assert_eq!(session.proofs().unwrap_err().code, "locked");
        assert_eq!(session.make_proof(x.id, NOV_12, false).unwrap_err().code, "locked");
        assert_eq!(session.set_identity(x.id, "me", "1").unwrap_err().code, "locked");
        assert_eq!(
            session.set_proof_location(x.id, "https://x.com/").unwrap_err().code,
            "locked"
        );
        assert_eq!(session.own_proof_target(x.id).unwrap_err().code, "locked");
        assert_eq!(session.check_pasted_proof(EXAMPLE_PROOF).unwrap_err().code, "locked");
    }
}
