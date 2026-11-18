//! The work of the Telegram connector for the app, without Tauri.
//!
//! Each function obeys the rule of the GitHub connector: it does not hold the lock of the session while it waits
//! for TDLib. It locks the session, copies what it needs, and unlocks. Then it speaks to the runtime.
//!
//! The functions get the runtime as an argument. The tests give them a runtime with a fake TDLib.
//!
//! These are the choices of this module (`docs/ARCHITECTURE.md` §4b):
//!
//! - A read in the timeline sends `viewMessages` with `force_read`. The chat panel uses `openChat` and `closeChat`,
//!   and also `viewMessages` for the messages that it shows.
//! - A Telegram item has no "mark as unread" in PANIM, as a GitHub thread has none.
//! - The removal of an account sends `logOut`. If Telegram does not confirm it in the time limit, PANIM deletes the
//!   local data and tells the person to end the session in the settings of Telegram.
//! - The functions get PANIM's app values from the build as `builtin`. Each client starts with the values of its
//!   login. A new login uses the values of the setting (`Session::telegram_app`).
//! - A backup closes the clients, as a lock does, so that the archive has the exact state of each TDLib folder.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use panim_core::{Account, AccountId, ItemKey};
use panim_telegram::client::Out;
use panim_telegram::{AppConfig, AuthState, ClientConfig, End, Event, MESSAGE_TEXT_CHARS, PanelView, Runtime, chat_of};
use serde::Serialize;
use serde_json::json;

use crate::session::{AppError, AppResult, LOGIN_KEY, Session, TelegramStart};
use panim_vault::BackupInfo;

/// The time limit for one answer of TDLib to a request of the person.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// The time limit for the state after a login step.
const STATE_TIMEOUT: Duration = Duration::from_secs(20);
/// The time limit for all the clients to close at a lock or an exit.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
/// The time limit for Telegram to confirm a logout.
const LOGOUT_TIMEOUT: Duration = Duration::from_secs(15);
/// The chat panel asks for this number of messages.
const PANEL_WANTED: usize = 50;
/// The chat panel sends no more than this number of history requests. TDLib can give fewer messages than the limit.
const PANEL_ROUNDS: usize = 5;

/// What the main window must know after an event of a client.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// The timeline or the state of an account changed.
    pub timeline: bool,
    /// The messages of the open chat panel changed.
    pub panel: bool,
}

/// The state of the login for the user interface.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct LoginView {
    /// The step that the person must do now.
    #[serde(flatten)]
    pub state: AuthState,
    /// The new account, when the login is complete.
    pub account: Option<Account>,
}

/// What the dialog "Add an account" shows for Telegram before a login starts.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct StatusView {
    /// False if PANIM has no `api_id` and `api_hash`: the build has none, and the person gave none. Then PANIM cannot
    /// add a Telegram account.
    pub configured: bool,
    /// The values of the next login: `builtin` (PANIM's values) or `own` (the values of the person).
    pub values: Option<&'static str>,
    /// True if the persona has a proxy route. The dialog then shows what PANIM does and does not make sure of.
    pub proxy: bool,
}

/// The result of the removal of an account.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Removed {
    /// True if the account is a Telegram account and Telegram did not confirm the logout. Then the session is
    /// possibly still in the list of devices of Telegram.
    pub session_left: bool,
}

fn locked<T>(session: &Mutex<Session>, f: impl FnOnce(&mut Session) -> AppResult<T>) -> AppResult<T> {
    let mut session = session.lock().map_err(|_| AppError::new("io", "poisoned"))?;
    f(&mut session)
}

fn config(start: TelegramStart) -> (u64, ClientConfig) {
    let config = ClientConfig {
        dir: start.dir,
        key: start.database_key,
        route: start.route,
        app: start.app,
        language: start.language,
    };
    (start.key, config)
}

/// `builtin` is PANIM's values from the build, or `None` if the build has none.
pub fn status(session: &Mutex<Session>, builtin: Option<&AppConfig>) -> AppResult<StatusView> {
    let (route, values) = locked(session, |s| Ok((s.route()?, s.telegram_next_login(builtin))))?;
    Ok(StatusView {
        configured: values.is_some(),
        values,
        proxy: matches!(route, panim_core::Route::Proxy(_)),
    })
}

/// Starts the client of each Telegram account, with the values of its login. Call it after the vault opens.
///
/// A client that cannot start gets an error mark in the list of accounts. An account whose values are not available
/// gets the mark `telegram_values_missing`, and no client. Returns true if the list changed.
pub fn start_all(session: &Mutex<Session>, runtime: &Arc<Runtime>, builtin: Option<&AppConfig>) -> bool {
    let Ok(opened) = locked(session, |s| s.telegram_open_all(builtin)) else {
        return false;
    };
    let mut changed = opened.marked;
    for start in opened.starts {
        let account = AccountId(start.key);
        let (key, config) = config(start);
        if let Err(error) = runtime.open(key, config) {
            changed |= locked(session, |s| Ok(s.telegram_error(account, error.code()))).unwrap_or(false);
        }
    }
    changed
}

/// Closes all the clients, for a lock, an exit or a change of the route. The folders stay open in memory.
///
/// Then the caller locks the session or calls `Session::telegram_close`. Those functions seal the folders.
pub fn stop_all(runtime: &Runtime) -> bool {
    runtime.end_all(CLOSE_TIMEOUT)
}

/// Starts all the clients again after the route of the persona changed.
///
/// The proxy request goes to TDLib only at the start of a client. Thus, a new route needs new clients.
pub fn restart_all(session: &Mutex<Session>, runtime: &Arc<Runtime>, builtin: Option<&AppConfig>) -> AppResult<()> {
    stop_all(runtime);
    locked(session, |s| s.telegram_close())?;
    start_all(session, runtime, builtin);
    Ok(())
}

/// Writes a backup of the vault to `target`.
///
/// A login in work stops. The clients close, as at a lock, so that TDLib does not write while PANIM seals the
/// folders. After the archive, the clients start again. Close the browser windows before this.
pub fn backup(
    session: &Mutex<Session>,
    runtime: &Arc<Runtime>,
    builtin: Option<&AppConfig>,
    target: &std::path::Path,
) -> AppResult<BackupInfo> {
    locked(session, |s| s.route())?;
    login_cancel(session, runtime)?;
    stop_all(runtime);
    let result = locked(session, |s| s.backup(target));
    start_all(session, runtime, builtin);
    result
}

/// Gives an event of a client to the session. `key` is the account id, or `LOGIN_KEY` for a login in work.
pub fn apply_event(session: &Mutex<Session>, key: u64, event: Event) -> Change {
    // The login functions read the state of the login from the runtime. The items of the new account come again
    // after the account exists.
    if key == LOGIN_KEY {
        return Change::default();
    }
    let account = AccountId(key);
    let Ok(mut session) = session.lock() else {
        return Change::default();
    };
    let timeline = match event {
        Event::Auth(state) => {
            if state == AuthState::Ready {
                // TDLib can change its binlog at the start. Seal the folders now, not only at the next interval. An
                // error is not fatal: the next seal tries again.
                let _ = session.telegram_seal();
            }
            session.telegram_auth(account, &state)
        }
        Event::Connection(connection) => session.telegram_connection(account, connection),
        Event::Me(me) => session.telegram_me(account, &me),
        Event::Items { upsert, remove } => session.telegram_items(account, &upsert, &remove),
        Event::Loaded { complete, shown } => session.telegram_loaded(account, complete, &shown),
        Event::Failed(error) => session.telegram_error(account, error.code),
        Event::Panel { .. } => {
            return Change {
                timeline: false,
                panel: true,
            };
        }
    };
    Change { timeline, panel: false }
}

/// Starts a login: a new folder, a new key and a new client. The result is the first step for the person.
///
/// The login uses the values of the setting. Without values (no `builtin` and no own values), the function refuses
/// with `telegram_not_configured`.
pub fn login_start(
    session: &Mutex<Session>,
    runtime: &Arc<Runtime>,
    builtin: Option<&AppConfig>,
) -> AppResult<LoginView> {
    // A login that was in work stops first.
    login_cancel(session, runtime)?;
    let start = locked(session, |s| s.telegram_login_begin(builtin))?;
    let (key, config) = config(start);
    if let Err(error) = runtime.open(key, config) {
        locked(session, |s| {
            s.telegram_login_drop();
            Ok(())
        })?;
        return Err(error.into());
    }
    login_view(session, runtime, |state| *state != AuthState::Starting)
}

/// Gives the phone number to Telegram. The number has the country code.
///
/// TDLib also accepts this request in the state "wait for the code" (`setAuthenticationPhoneNumber` in `td_api.tl`
/// 1.8.68), if no other login request is in progress. Thus, the person can go back and use a different number.
pub fn login_phone(session: &Mutex<Session>, runtime: &Arc<Runtime>, phone: &str) -> AppResult<LoginView> {
    let phone = panim_telegram::clean_phone(phone)
        .ok_or_else(|| AppError::new("telegram_phone_invalid", "the form of the number is not correct"))?;
    let request = json!({"@type": "setAuthenticationPhoneNumber", "phone_number": phone});
    login_step(session, runtime, request)
}

/// Gives the login code to Telegram. A code can be digits, a word or a phrase of words.
pub fn login_code(session: &Mutex<Session>, runtime: &Arc<Runtime>, code: &str) -> AppResult<LoginView> {
    let code = code.trim();
    let ok = (1..=64).contains(&code.chars().count()) && code.chars().all(|c| c.is_alphanumeric() || c == ' ');
    if !ok {
        return Err(AppError::new(
            "telegram_code_invalid",
            "the form of the code is not correct",
        ));
    }
    let request = json!({"@type": "checkAuthenticationCode", "code": code});
    login_step(session, runtime, request)
}

/// Gives the two-step password to Telegram.
pub fn login_password(session: &Mutex<Session>, runtime: &Arc<Runtime>, password: &str) -> AppResult<LoginView> {
    if password.is_empty() {
        return Err(AppError::new("telegram_password_invalid", "the password is empty"));
    }
    let request = json!({"@type": "checkAuthenticationPassword", "password": password});
    login_step(session, runtime, request)
}

/// Asks Telegram to send the login code again, on the next route.
///
/// The client refuses the request outside the state "wait for the code", without a next route, and before the
/// timeout of Telegram. The new state gives the new route, the next route and a new timeout.
pub fn login_resend(session: &Mutex<Session>, runtime: &Arc<Runtime>) -> AppResult<LoginView> {
    if !locked(session, |s| Ok(s.telegram_login_active()))? {
        return Err(AppError::new("telegram_login_none", "no login is in work"));
    }
    let request = runtime.with(LOGIN_KEY, |client| {
        (client.resend_request(Instant::now()), Out::default())
    })??;
    login_step(session, runtime, request)
}

/// Sends one login request, and waits for the next authorization state.
///
/// Each successful login request gives a new `updateAuthorizationState`. The state can be the same as before, for
/// example after a resend on the same route. Thus, the function waits for a new update, not for a different state.
fn login_step(session: &Mutex<Session>, runtime: &Arc<Runtime>, request: serde_json::Value) -> AppResult<LoginView> {
    let none = || AppError::new("telegram_login_none", "no login is in work");
    if !locked(session, |s| Ok(s.telegram_login_active()))? {
        return Err(none());
    }
    let serial = runtime.auth_serial(LOGIN_KEY).ok_or_else(none)?;
    runtime.call(LOGIN_KEY, request, CALL_TIMEOUT)?;
    let (state, new) = runtime
        .wait_auth_after(LOGIN_KEY, serial, STATE_TIMEOUT)
        .ok_or_else(none)?;
    if !new {
        return Err(panim_telegram::Error::Timeout.into());
    }
    state_view(session, runtime, state)
}

/// Waits for the first state of a new client, and gives the view of the login.
fn login_view(
    session: &Mutex<Session>,
    runtime: &Arc<Runtime>,
    moved: impl Fn(&AuthState) -> bool,
) -> AppResult<LoginView> {
    let state = runtime
        .wait_auth(LOGIN_KEY, moved, STATE_TIMEOUT)
        .ok_or_else(|| AppError::new("telegram_login_none", "no login is in work"))?;
    state_view(session, runtime, state)
}

/// The view of a login state. The state "ready" completes the login.
fn state_view(session: &Mutex<Session>, runtime: &Arc<Runtime>, state: AuthState) -> AppResult<LoginView> {
    match state {
        AuthState::Ready => login_finish(session, runtime),
        AuthState::Starting => Err(panim_telegram::Error::Timeout.into()),
        AuthState::LoggingOut | AuthState::Closing | AuthState::Closed => {
            login_cancel(session, runtime)?;
            Err(AppError::new("telegram_login_none", "the login stopped"))
        }
        state => Ok(LoginView { state, account: None }),
    }
}

/// Adds the account of a complete login, and gives its client the key of the account.
fn login_finish(session: &Mutex<Session>, runtime: &Arc<Runtime>) -> AppResult<LoginView> {
    // The client asks Telegram for the account immediately after the login. Wait for the answer.
    let deadline = std::time::Instant::now() + STATE_TIMEOUT;
    let me = loop {
        let me = runtime.with(LOGIN_KEY, |client| (client.me().cloned(), Out::default()))?;
        match me {
            Some(me) => break me,
            None if std::time::Instant::now() >= deadline => return Err(panim_telegram::Error::Timeout.into()),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let account = match locked(session, |s| s.telegram_login_finish(&me)) {
        Ok(account) => account,
        Err(error) => {
            // PANIM does not keep this login. End the new session on Telegram, so that it does not stay in the
            // list of devices.
            runtime.end(LOGIN_KEY, End::LogOut, LOGOUT_TIMEOUT);
            locked(session, |s| {
                s.telegram_login_drop();
                Ok(())
            })?;
            return Err(error);
        }
    };
    runtime.rekey(LOGIN_KEY, account.id.0);
    // The first items came before the account was in the store. Get them again.
    let events = runtime.with(account.id.0, |client| (client.replay(), Out::default()))?;
    for event in events {
        apply_event(session, account.id.0, event);
    }
    Ok(LoginView {
        state: AuthState::Ready,
        account: Some(account),
    })
}

/// Stops the login that is in work and deletes its folder. Without a login, the function does nothing.
pub fn login_cancel(session: &Mutex<Session>, runtime: &Arc<Runtime>) -> AppResult<()> {
    if !locked(session, |s| Ok(s.telegram_login_active()))? {
        return Ok(());
    }
    // A login that is not complete has no session on Telegram, so `close` is sufficient.
    runtime.end(LOGIN_KEY, End::Close, CLOSE_TIMEOUT);
    locked(session, |s| {
        s.telegram_login_drop();
        Ok(())
    })
}

fn chat(key: &ItemKey) -> AppResult<i64> {
    chat_of(&key.source_id).ok_or_else(|| AppError::new("telegram_no_chat", "the item is not a chat"))
}

/// Marks the chat of an item as read, in PANIM and on Telegram. `read` must be true.
///
/// Telegram can mark a chat as unread, but that mark is not the unread count. PANIM does not offer it, as for
/// GitHub. The item becomes read immediately. If Telegram refuses the read, the item becomes unread again.
pub fn set_read(runtime: &Arc<Runtime>, key: &ItemKey, read: bool) -> AppResult<()> {
    if !read {
        return Err(AppError::new(
            "telegram_no_unread",
            "PANIM cannot mark a chat as unread",
        ));
    }
    let chat = chat(key)?;
    runtime.with(key.account.0, |client| ((), client.mark_read(chat)))?;
    Ok(())
}

/// Sends a plain text message to the chat of an item, as the account of the item.
pub fn send(runtime: &Arc<Runtime>, key: &ItemKey, text: &str) -> AppResult<()> {
    let chat = chat(key)?;
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::new("answer_empty", "the answer has no text"));
    }
    // Telegram counts UTF-16 code units.
    if text.encode_utf16().count() > MESSAGE_TEXT_CHARS {
        return Err(AppError::new("telegram_too_long", MESSAGE_TEXT_CHARS));
    }
    let can_write = runtime.with(key.account.0, |client| (client.can_write(chat), Out::default()))?;
    if !can_write {
        return Err(AppError::new(
            "telegram_no_write",
            "the account cannot write in this chat",
        ));
    }
    runtime.call(
        key.account.0,
        panim_telegram::messages::send_text_request(chat, text),
        CALL_TIMEOUT,
    )?;
    Ok(())
}

/// Opens the chat panel for the chat of an item, gets its last messages and marks them as read.
pub fn chat_open(runtime: &Arc<Runtime>, key: &ItemKey) -> AppResult<PanelView> {
    let chat = chat(key)?;
    let account = key.account.0;
    let known = runtime.with(account, |client| {
        if !client.has_chat(chat) {
            return (false, Out::default());
        }
        let requests = client.panel_open(chat);
        (
            true,
            Out {
                requests,
                events: Vec::new(),
            },
        )
    })?;
    if !known {
        return Err(AppError::new("telegram_no_chat", "the client does not have this chat"));
    }
    let mut have = 0;
    for _ in 0..PANEL_ROUNDS {
        let request = runtime.with(account, |client| {
            (client.panel_history_request(PANEL_WANTED - have), Out::default())
        })?;
        // The person closed the panel or opened a different chat.
        let Some(request) = request.filter(|request| request["chat_id"] == chat) else {
            break;
        };
        let answer = runtime.call(account, request, CALL_TIMEOUT)?;
        let added = runtime.with(account, |client| (client.panel_history(chat, &answer), Out::default()))?;
        have += added;
        if added == 0 || have >= PANEL_WANTED {
            break;
        }
    }
    chat_view(runtime, key)
}

/// The messages of the open chat panel. The call also tells Telegram that the person sees them.
pub fn chat_view(runtime: &Arc<Runtime>, key: &ItemKey) -> AppResult<PanelView> {
    let chat = chat(key)?;
    runtime.with(key.account.0, |client| ((), client.panel_seen(chat)))?;
    runtime
        .with(key.account.0, |client| (client.panel_view(chat), Out::default()))?
        .ok_or_else(|| AppError::new("telegram_no_chat", "the chat panel is not open for this chat"))
}

/// Closes the chat panel of an account. It is not an error if the panel is not open or the client is closed.
pub fn chat_close(runtime: &Arc<Runtime>, account: AccountId) {
    let _ = runtime.with(account.0, |client| {
        let requests = client.panel_close();
        (
            (),
            Out {
                requests,
                events: Vec::new(),
            },
        )
    });
}

/// Removes an account. For a Telegram account, the function first ends the session on Telegram.
///
/// If Telegram does not confirm the logout, the function deletes the local data all the same.
pub fn remove_account(session: &Mutex<Session>, runtime: &Arc<Runtime>, id: AccountId) -> AppResult<Removed> {
    let is_telegram = locked(session, |s| Ok(s.is_telegram(id)))?;
    let mut session_left = false;
    if is_telegram {
        let open = runtime.keys().contains(&id.0);
        // Without a client, PANIM cannot tell Telegram. This occurs when the client did not start. If Telegram
        // ended the session itself, no session stays.
        let ended = open && runtime.end(id.0, End::LogOut, LOGOUT_TIMEOUT);
        let logged_out = locked(session, |s| Ok(s.telegram_logged_out(id)))?;
        session_left = !ended && !logged_out;
        // Remove a client that TDLib closed itself.
        runtime.forget(id.0);
    }
    locked(session, |s| s.remove_account(id))?;
    Ok(Removed { session_left })
}

/// Seals the open TDLib folders into the vault. The app calls it at the interval of the browser profile.
pub fn seal(session: &Mutex<Session>) -> AppResult<()> {
    locked(session, |s| s.telegram_seal())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex as StdMutex;

    use panim_core::{Platform, ProxyEndpoint, ProxyKind, Route, TimelineFilter};
    use panim_telegram::td::fake::FakeTd;
    use panim_vault::ProfileArea;
    use serde_json::Value;

    use super::*;
    use crate::session::Strength;

    struct Fixture {
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        data: PathBuf,
        memory: PathBuf,
        session: Arc<Mutex<Session>>,
        td: Arc<FakeTd>,
        runtime: Arc<Runtime>,
        app: AppConfig,
        changes: Arc<StdMutex<Vec<Change>>>,
    }

    fn auth(state: Value) -> Value {
        json!({"@type": "updateAuthorizationState", "authorization_state": state})
    }

    fn state(name: &str) -> Value {
        auth(json!({"@type": name}))
    }

    fn message(chat: i64, id: i64, text: &str, outgoing: bool) -> Value {
        json!({"@type": "message", "id": id, "chat_id": chat, "date": 1794800000 + id, "is_outgoing": outgoing,
            "sender_id": {"@type": "messageSenderUser", "user_id": 42},
            "content": {"@type": "messageText", "text": {"text": text}}})
    }

    fn new_chat(id: i64, title: &str, unread: u32) -> Value {
        json!({"@type": "updateNewChat", "chat": {"id": id, "title": title,
            "type": {"@type": "chatTypePrivate", "user_id": 42},
            "permissions": {"can_send_basic_messages": true},
            "last_message": message(id, 100, "hello", false),
            "positions": [{"list": {"@type": "chatListMain"}, "order": "9221294780000000000"}],
            "unread_count": unread, "is_marked_as_unread": false,
            "notification_settings": {"use_default_mute_for": true, "mute_for": 0}}})
    }

    /// A fake Telegram: one account with the phone number 15550001111, the code 24680 and the password "open".
    ///
    /// `logged_in` tells if the database of a client has a login. A new client of a login has none.
    fn telegram(td: &FakeTd, two_step: bool) {
        let mut ready: std::collections::BTreeSet<i32> = Default::default();
        // The number of each client, for the answer to a resend.
        let mut numbers: std::collections::BTreeMap<i32, String> = Default::default();
        td.respond(move |client, request| {
            let ok = || json!({"@type": "ok", "@answer": true});
            let error =
                |code: i64, message: &str| json!({"@type": "error", "code": code, "message": message, "@answer": true});
            match request["@type"].as_str().unwrap() {
                "addProxy" | "disableProxy" => vec![state("authorizationStateWaitTdlibParameters")],
                "setTdlibParameters" => {
                    // The folder of a client that had a login has a file. The fake reads it as TDLib reads its
                    // database.
                    let dir = request["database_directory"].as_str().unwrap();
                    let has_login = std::path::Path::new(dir).join("td.binlog").exists();
                    if has_login {
                        ready.insert(client);
                        vec![ok(), state("authorizationStateReady")]
                    } else {
                        std::fs::create_dir_all(dir).unwrap();
                        std::fs::create_dir_all(request["files_directory"].as_str().unwrap()).unwrap();
                        std::fs::write(std::path::Path::new(dir).join("marker"), dir).unwrap();
                        vec![ok(), state("authorizationStateWaitPhoneNumber")]
                    }
                }
                "setAuthenticationPhoneNumber" => match request["phone_number"].as_str().unwrap() {
                    "15550001111" => vec![
                        ok(),
                        auth(
                            json!({"@type": "authorizationStateWaitCode", "code_info": {"phone_number": "15550001111",
                            "type": {"@type": "authenticationCodeTypeTelegramMessage", "length": 5}}}),
                        ),
                    ],
                    // A code in the app, then an SMS. The resend is permitted at once, or after 60 seconds.
                    number @ ("15550004444" | "15550005555" | "15550006666") => {
                        numbers.insert(client, number.to_string());
                        let timeout = if number == "15550005555" { 60 } else { 0 };
                        vec![
                            ok(),
                            code_state(
                                "authenticationCodeTypeTelegramMessage",
                                Some("authenticationCodeTypeSms"),
                                timeout,
                            ),
                        ]
                    }
                    "15550002222" => vec![ok(), state("authorizationStateWaitEmailAddress")],
                    "15550003333" => vec![error(429, "Too Many Requests: retry after 120")],
                    _ => vec![error(400, "PHONE_NUMBER_INVALID")],
                },
                "resendAuthenticationCode" => match numbers.get(&client).map(String::as_str) {
                    Some("15550006666") => vec![error(429, "Too Many Requests: retry after 30")],
                    // The SMS is the last route.
                    Some(_) => vec![ok(), code_state("authenticationCodeTypeSms", None, 0)],
                    None => vec![error(400, "Authentication code can't be resend")],
                },
                "checkAuthenticationCode" if request["code"] != "24680" => vec![error(400, "PHONE_CODE_INVALID")],
                "checkAuthenticationCode" if two_step => vec![
                    ok(),
                    auth(json!({"@type": "authorizationStateWaitPassword", "password_hint": "a door"})),
                ],
                "checkAuthenticationPassword" if request["password"] != "open" => {
                    vec![error(400, "PASSWORD_HASH_INVALID")]
                }
                "checkAuthenticationCode" | "checkAuthenticationPassword" => {
                    ready.insert(client);
                    vec![ok(), state("authorizationStateReady")]
                }
                "getMe" => vec![
                    json!({"@type": "user", "id": 777001, "first_name": "Sakura", "last_name": "R",
                    "usernames": {"active_usernames": ["sakura_r"]}, "@answer": true}),
                ],
                "getScopeNotificationSettings" => {
                    vec![json!({"@type": "scopeNotificationSettings", "mute_for": 0, "@answer": true})]
                }
                "loadChats" => vec![new_chat(5, "Alice", 2), error(404, "Not Found")],
                "viewMessages" => vec![
                    ok(),
                    json!({"@type": "updateChatReadInbox", "chat_id": request["chat_id"],
                        "last_read_inbox_message_id": request["message_ids"][0], "unread_count": 0}),
                ],
                "getChatHistory" => {
                    let chat = request["chat_id"].as_i64().unwrap();
                    // TDLib gives fewer messages than the limit: two messages for each request.
                    let from = request["from_message_id"].as_i64().unwrap();
                    let newest = if from == 0 { 100 } else { from - 10 };
                    let messages: Vec<Value> = [newest, newest - 10]
                        .into_iter()
                        .filter(|id| *id > 50)
                        .map(|id| message(chat, id, &format!("message {id}"), id % 20 == 0))
                        .collect();
                    vec![json!({"@type": "messages", "messages": messages, "@answer": true})]
                }
                "sendMessage" => {
                    let chat = request["chat_id"].as_i64().unwrap();
                    let text = request["input_message_content"]["text"]["text"].as_str().unwrap();
                    let mut pending = message(chat, 1000, text, true);
                    pending["sending_state"] = json!({"@type": "messageSendingStatePending"});
                    let mut answer = pending.clone();
                    answer["@answer"] = true.into();
                    vec![
                        answer,
                        json!({"@type": "updateNewMessage", "message": pending}),
                        json!({"@type": "updateMessageSendSucceeded", "old_message_id": 1000,
                            "message": message(chat, 110, text, true)}),
                    ]
                }
                "close" => vec![state("authorizationStateClosing"), state("authorizationStateClosed")],
                "logOut" => {
                    ready.remove(&client);
                    vec![state("authorizationStateLoggingOut"), state("authorizationStateClosed")]
                }
                _ => vec![],
            }
        });
    }

    impl Fixture {
        fn new() -> Self {
            let data = tempfile::tempdir().unwrap();
            let memory = tempfile::tempdir().unwrap();
            let mut session = Session::new(
                data.path().join("panim"),
                ProfileArea::at(memory.path().to_path_buf()),
                Strength::TEST,
            );
            session.create("passcode1").unwrap();
            let session = Arc::new(Mutex::new(session));
            let td = Arc::new(FakeTd::new());
            telegram(&td, false);
            let changes: Arc<StdMutex<Vec<Change>>> = Arc::default();
            let (sink_session, sink_changes) = (session.clone(), changes.clone());
            let runtime = Runtime::new(
                td.clone(),
                Box::new(move |key, event| {
                    let change = apply_event(&sink_session, key, event);
                    sink_changes.lock().unwrap().push(change);
                }),
            );
            Self {
                data: data.path().join("panim"),
                memory: memory.path().to_path_buf(),
                _dirs: (data, memory),
                session,
                td,
                runtime,
                app: AppConfig::for_test(),
                changes,
            }
        }

        fn with<T>(&self, f: impl FnOnce(&mut Session) -> T) -> T {
            f(&mut self.session.lock().unwrap())
        }

        /// Does a full login and gives the new account.
        fn login(&self) -> Account {
            login_start(&self.session, &self.runtime, Some(&self.app)).unwrap();
            // The fake TDLib of a real login writes the database. This file is the login of the fake.
            self.write_binlog();
            login_phone(&self.session, &self.runtime, "+1 555 000 1111").unwrap();
            let view = login_code(&self.session, &self.runtime, "24680").unwrap();
            view.account.expect("the login is complete")
        }

        fn write_binlog(&self) {
            for folder in self.folders() {
                std::fs::write(folder.join("db/td.binlog"), b"the login of the fake").unwrap();
            }
        }

        /// The open TDLib folders in memory.
        fn folders(&self) -> Vec<PathBuf> {
            let mut found = Vec::new();
            for vault in std::fs::read_dir(&self.memory).unwrap() {
                for folder in std::fs::read_dir(vault.unwrap().path()).unwrap() {
                    let path = folder.unwrap().path();
                    if path.file_name().unwrap().to_string_lossy().starts_with("telegram-") {
                        found.push(path);
                    }
                }
            }
            found
        }

        fn items(&self) -> Vec<crate::session::ItemView> {
            self.with(|s| s.timeline(&TimelineFilter::default()).unwrap())
        }

        fn sent(&self) -> Vec<String> {
            self.td
                .log()
                .into_iter()
                .filter(|line| line.starts_with("send"))
                .collect()
        }

        fn wait(&self, what: impl Fn() -> bool) {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !what() {
                assert!(Instant::now() < deadline, "the condition did not become true");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn lock(&self) {
            stop_all(&self.runtime);
            self.with(|s| s.lock().unwrap());
        }

        fn unlock(&self) {
            self.with(|s| s.unlock("passcode1").unwrap());
            start_all(&self.session, &self.runtime, Some(&self.app));
        }
    }

    fn code<T: std::fmt::Debug>(result: AppResult<T>) -> &'static str {
        result.unwrap_err().code
    }

    fn key(account: &Account, chat: i64) -> ItemKey {
        ItemKey {
            account: account.id,
            source_id: format!("c/{chat}"),
        }
    }

    #[test]
    fn login_with_phone_and_code_adds_the_account() {
        let f = Fixture::new();
        let view = login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        assert_eq!((&view.state, &view.account), (&AuthState::WaitPhone, &None));
        f.write_binlog();

        assert_eq!(
            code(login_phone(&f.session, &f.runtime, "not a number")),
            "telegram_phone_invalid"
        );
        assert_eq!(
            code(login_phone(&f.session, &f.runtime, "+1 555 000 9999")),
            "telegram_phone_invalid"
        );
        let view = login_phone(&f.session, &f.runtime, "+1 (555) 000-1111").unwrap();
        assert_eq!(
            view.state,
            AuthState::WaitCode {
                via: "app",
                length: 5,
                detail: String::new(),
                next: None,
                timeout: 0
            }
        );
        let text = serde_json::to_string(&view).unwrap();
        assert_eq!(
            text,
            r#"{"state":"wait_code","via":"app","length":5,"detail":"","next":null,"timeout":0,"account":null}"#
        );
        // Telegram offers no other route for this number.
        assert_eq!(code(login_resend(&f.session, &f.runtime)), "telegram_resend_none");

        assert_eq!(
            code(login_code(&f.session, &f.runtime, "11111")),
            "telegram_code_invalid"
        );
        assert_eq!(code(login_code(&f.session, &f.runtime, "")), "telegram_code_invalid");
        let view = login_code(&f.session, &f.runtime, " 24680 ").unwrap();
        let account = view.account.unwrap();
        assert_eq!(view.state, AuthState::Ready);
        assert_eq!(
            (account.platform, account.handle.as_str(), account.display_name.as_str()),
            (Platform::Telegram, "sakura_r", "Sakura R")
        );
        assert_eq!(account.remote_id.as_deref(), Some("777001"));
        assert!(account.joined.is_some() && account.browser.is_none());

        // The first chats come before the account exists. The item is in the timeline all the same.
        f.wait(|| f.items().len() == 1);
        let item = &f.items()[0];
        assert_eq!(
            (item.key.source_id.as_str(), item.author.as_str(), item.text.as_str()),
            ("c/5", "Alice", "hello")
        );
        assert!(!item.read && item.has_read_state && item.can_answer && !item.has_link);
        assert_eq!(item.chat.as_ref().map(|chat| (chat.kind, chat.unread)), Some((None, 2)));
        // The count of the account is the number of unread chats.
        let state = f.with(|s| s.sync_state().unwrap());
        assert_eq!(state[&account.id.0].unread, 1);
        assert!(!state[&account.id.0].needs_login && state[&account.id.0].error.is_none());

        // The requests have the fixed order, and the phone number has digits only.
        let sent = f.sent();
        assert_eq!(
            &sent[..3],
            [
                "send 1 disableProxy",
                "send 1 setTdlibParameters",
                "send 1 setAuthenticationPhoneNumber"
            ]
        );
        // The login is complete, so a new step finds no login.
        assert_eq!(code(login_code(&f.session, &f.runtime, "24680")), "telegram_login_none");
        f.lock();
    }

    #[test]
    fn login_with_the_two_step_password() {
        let f = Fixture::new();
        telegram(&f.td, true);
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        f.write_binlog();
        login_phone(&f.session, &f.runtime, "15550001111").unwrap();
        let view = login_code(&f.session, &f.runtime, "24680").unwrap();
        assert_eq!(view.state, AuthState::WaitPassword { hint: "a door".into() });
        assert_eq!(
            code(login_password(&f.session, &f.runtime, "wrong")),
            "telegram_password_invalid"
        );
        assert_eq!(
            code(login_password(&f.session, &f.runtime, "")),
            "telegram_password_invalid"
        );
        let view = login_password(&f.session, &f.runtime, "open").unwrap();
        assert!(view.account.is_some());
        f.lock();
    }

    /// A state "wait for the code" of the fake. `next` is the type of the next route, or `None`.
    fn code_state(via: &str, next: Option<&str>, timeout: u32) -> Value {
        let next = next.map_or(Value::Null, |next| json!({"@type": next, "length": 5}));
        auth(
            json!({"@type": "authorizationStateWaitCode", "code_info": {"phone_number": "15550004444",
            "type": {"@type": via, "length": 5}, "next_type": next, "timeout": timeout}}),
        )
    }

    #[test]
    fn resend_gives_the_next_route() {
        let f = Fixture::new();
        // No login is in work.
        assert_eq!(code(login_resend(&f.session, &f.runtime)), "telegram_login_none");
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        // The state asks for the phone number, not for a code.
        assert_eq!(code(login_resend(&f.session, &f.runtime)), "telegram_resend_state");

        let view = login_phone(&f.session, &f.runtime, "+1 555 000 4444").unwrap();
        assert!(matches!(
            view.state,
            AuthState::WaitCode {
                via: "app",
                next: Some("sms"),
                timeout: 0,
                ..
            }
        ));
        let view = login_resend(&f.session, &f.runtime).unwrap();
        assert!(matches!(
            view.state,
            AuthState::WaitCode {
                via: "sms",
                next: None,
                timeout: 0,
                ..
            }
        ));
        let resend =
            f.td.take_sent()
                .into_iter()
                .find(|(_, r)| r["@type"] == "resendAuthenticationCode")
                .unwrap()
                .1;
        assert_eq!(resend["reason"]["@type"], "resendCodeReasonUserRequest");
        // The SMS was the last route.
        assert_eq!(code(login_resend(&f.session, &f.runtime)), "telegram_resend_none");
        assert_eq!(
            f.sent()
                .iter()
                .filter(|line| line.ends_with("resendAuthenticationCode"))
                .count(),
            1
        );

        // The code of the new route completes the login.
        f.write_binlog();
        assert!(login_code(&f.session, &f.runtime, "24680").unwrap().account.is_some());
        f.lock();
    }

    #[test]
    fn resend_waits_for_the_timeout_and_shows_a_flood_wait() {
        let f = Fixture::new();
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        login_phone(&f.session, &f.runtime, "15550005555").unwrap();
        let error = login_resend(&f.session, &f.runtime).unwrap_err();
        assert_eq!(error.code, "telegram_resend_early");
        let seconds: u64 = error.detail.parse().unwrap();
        assert!((59..=60).contains(&seconds), "{seconds}");
        // PANIM sent no request: TDLib would refuse it.
        assert!(!f.sent().iter().any(|line| line.ends_with("resendAuthenticationCode")));

        // A different number in the code state. TDLib permits `setAuthenticationPhoneNumber` there.
        let view = login_phone(&f.session, &f.runtime, "15550006666").unwrap();
        assert!(matches!(
            view.state,
            AuthState::WaitCode {
                next: Some("sms"),
                timeout: 0,
                ..
            }
        ));
        let error = login_resend(&f.session, &f.runtime).unwrap_err();
        assert_eq!((error.code, error.detail.as_str()), ("telegram_flood", "30"));
        login_cancel(&f.session, &f.runtime).unwrap();
    }

    #[test]
    fn the_same_state_again_is_a_new_step() {
        let f = Fixture::new();
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        let first = login_phone(&f.session, &f.runtime, "15550004444").unwrap();
        // The same number again gives the same state. The step does not wait until its time limit.
        let started = Instant::now();
        let again = login_phone(&f.session, &f.runtime, "15550004444").unwrap();
        assert_eq!(first, again);
        assert!(started.elapsed() < Duration::from_secs(5));
        login_cancel(&f.session, &f.runtime).unwrap();
    }

    #[test]
    fn login_errors_and_states_that_panim_cannot_serve() {
        let f = Fixture::new();
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        let error = login_phone(&f.session, &f.runtime, "15550003333").unwrap_err();
        assert_eq!((error.code, error.detail.as_str()), ("telegram_flood", "120"));
        let view = login_phone(&f.session, &f.runtime, "15550002222").unwrap();
        assert_eq!(
            view.state,
            AuthState::Unsupported {
                reason: "email_address"
            }
        );
        // The person cannot continue. A cancel deletes the folder of the login.
        assert_eq!(f.folders().len(), 1);
        login_cancel(&f.session, &f.runtime).unwrap();
        assert!(f.folders().is_empty());
        assert_eq!(
            code(login_phone(&f.session, &f.runtime, "15550001111")),
            "telegram_login_none"
        );
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());
        login_cancel(&f.session, &f.runtime).unwrap();
    }

    #[test]
    fn the_secrets_of_the_login_are_not_in_an_error_or_in_the_vault() {
        let f = Fixture::new();
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        f.write_binlog();
        let errors = [
            login_phone(&f.session, &f.runtime, "+1 555 000 9999").unwrap_err(),
            login_phone(&f.session, &f.runtime, "15550003333").unwrap_err(),
        ];
        login_phone(&f.session, &f.runtime, "15550001111").unwrap();
        let wrong_code = login_code(&f.session, &f.runtime, "13579").unwrap_err();
        for error in errors.iter().chain([&wrong_code]) {
            let text = format!("{error:?}");
            for secret in ["5550009999", "5550003333", "13579"] {
                assert!(!text.contains(secret), "{text}");
            }
        }
        let account = login_code(&f.session, &f.runtime, "24680").unwrap().account.unwrap();
        f.wait(|| f.items().len() == 1);
        f.lock();

        // The files of the vault do not show the phone number, the code, the user id or a message.
        let mut folders = vec![f.data.clone()];
        let mut files = 0;
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                    continue;
                }
                files += 1;
                let bytes = std::fs::read(&path).unwrap();
                for secret in [
                    "15550001111",
                    "24680",
                    "777001",
                    "the login of the fake",
                    "Alice",
                    "telegram-",
                ] {
                    let found = bytes.windows(secret.len()).any(|window| window == secret.as_bytes());
                    assert!(!found, "{secret} in {}", path.display());
                }
            }
        }
        assert!(files > 3);
        // The folder in memory is gone after the lock.
        assert!(
            !f.memory.read_dir().unwrap().any(|_| true),
            "the area is empty after a lock"
        );
        let _ = account;
    }

    #[test]
    fn a_second_login_of_the_same_account_is_refused_and_logged_out() {
        let f = Fixture::new();
        let first = f.login();
        login_start(&f.session, &f.runtime, Some(&f.app)).unwrap();
        login_phone(&f.session, &f.runtime, "15550001111").unwrap();
        assert_eq!(code(login_code(&f.session, &f.runtime, "24680")), "account_exists");
        assert_eq!(f.with(|s| s.accounts().unwrap()), vec![first.clone()]);
        // The new session ends on Telegram, and its folder is deleted. The first client continues.
        assert!(f.sent().contains(&"send 2 logOut".to_string()));
        assert_eq!(f.folders().len(), 1);
        assert_eq!(f.runtime.keys(), vec![first.id.0]);
        f.lock();
    }

    #[test]
    fn lock_and_unlock_keep_the_login() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        assert_eq!(f.with(|s| s.telegram_open_folders()), 1);
        f.lock();
        // Each client got `close` and reached the state "closed" before the folders were sealed.
        assert!(f.sent().contains(&"send 1 close".to_string()));
        assert!(f.runtime.keys().is_empty());
        assert!(f.folders().is_empty());

        f.unlock();
        // The new client reads the login from the folder. It asks for no phone number.
        f.wait(|| f.runtime.auth(account.id.0) == Some(AuthState::Ready));
        let sent = f.sent();
        let second: Vec<&String> = sent.iter().filter(|line| line.starts_with("send 2 ")).collect();
        assert_eq!(&second[..2], ["send 2 disableProxy", "send 2 setTdlibParameters"]);
        assert!(!second.iter().any(|line| line.contains("setAuthenticationPhoneNumber")));
        let folder = &f.folders()[0];
        assert_eq!(
            std::fs::read(folder.join("db/td.binlog")).unwrap(),
            b"the login of the fake"
        );
        assert!(!folder.join("files").exists(), "the archive does not keep the files");
        // The items stay in the store during the lock.
        assert_eq!(f.items().len(), 1);
        f.lock();
    }

    #[test]
    fn a_vault_without_telegram_opens_as_before() {
        let f = Fixture::new();
        let list = |dir: &PathBuf| {
            let mut names: Vec<String> = std::fs::read_dir(dir.join("data"))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        f.lock();
        let before = list(&f.data);
        f.unlock();
        assert!(!start_all(&f.session, &f.runtime, Some(&f.app)));
        assert!(f.runtime.keys().is_empty());
        assert!(
            f.td.take_sent().is_empty(),
            "no client starts without a Telegram account"
        );
        assert_eq!(f.with(|s| s.telegram_open_folders()), 0);
        seal(&f.session).unwrap();
        f.lock();
        // The vault has the same files as before. Telegram adds no file to a vault that does not use it.
        assert_eq!(list(&f.data), before);
        f.unlock();
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());
        f.lock();
    }

    #[test]
    fn read_state_follows_telegram_in_the_two_directions() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        let item = key(&account, 5);

        // A read in PANIM: the item is read immediately, and Telegram gets `viewMessages`.
        assert_eq!(code(set_read(&f.runtime, &item, false)), "telegram_no_unread");
        set_read(&f.runtime, &item, true).unwrap();
        assert!(f.items()[0].read);
        f.wait(|| f.sent().contains(&"send 1 viewMessages".to_string()));
        let view =
            f.td.take_sent()
                .into_iter()
                .find(|(_, r)| r["@type"] == "viewMessages")
                .unwrap()
                .1;
        assert_eq!(
            (&view["chat_id"], &view["message_ids"], &view["force_read"]),
            (&json!(5), &json!([100]), &json!(true))
        );
        f.wait(|| f.with(|s| s.sync_state().unwrap())[&account.id.0].unread == 0);

        // A new message comes: the item is unread again, with the count of Telegram.
        f.td.push(1, json!({"@type": "updateChatLastMessage", "chat_id": 5, "last_message": message(5, 101, "are you there?", false),
            "positions": [{"list": {"@type": "chatListMain"}, "order": "9221294780000000001"}]}));
        f.td.push(
            1,
            json!({"@type": "updateChatReadInbox", "chat_id": 5, "last_read_inbox_message_id": 100, "unread_count": 1}),
        );
        f.wait(|| !f.items()[0].read);
        let shown = &f.items()[0];
        assert_eq!(
            (shown.text.as_str(), shown.chat.as_ref().unwrap().unread),
            ("are you there?", 1)
        );

        // A read in a Telegram app: the item becomes read in PANIM.
        f.td.push(
            1,
            json!({"@type": "updateChatReadInbox", "chat_id": 5, "last_read_inbox_message_id": 101, "unread_count": 0}),
        );
        f.wait(|| f.items()[0].read);
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread, 0);
        assert!(f.changes.lock().unwrap().iter().any(|change| change.timeline));
        f.lock();
    }

    #[test]
    fn muted_and_archived_chats_leave_the_timeline() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        f.td.push(1, new_chat(6, "Bob", 0));
        f.wait(|| f.items().len() == 2);
        // Bob becomes muted. Alice goes to the archive.
        f.td.push(
            1,
            json!({"@type": "updateChatNotificationSettings", "chat_id": 6,
            "notification_settings": {"use_default_mute_for": false, "mute_for": 3600}}),
        );
        f.td.push(
            1,
            json!({"@type": "updateChatPosition", "chat_id": 5,
            "position": {"list": {"@type": "chatListMain"}, "order": "0"}}),
        );
        f.wait(|| f.items().is_empty());
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread, 0);
        f.lock();
    }

    #[test]
    fn old_items_go_away_after_the_complete_chat_list() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        // The store has an item of a chat from an earlier session. The chat is not in the list now.
        let old = panim_telegram::ChatItem {
            chat_id: 9,
            title: "Old".into(),
            time: 1,
            text: "bye".into(),
            kind: None,
            unread: 1,
            marked: false,
            read: false,
            outgoing: false,
            can_write: true,
        };
        assert!(f.with(|s| s.telegram_items(account.id, &[old], &[])));
        assert_eq!(f.items().len(), 2);
        // An incomplete list removes nothing.
        assert!(f.with(|s| s.telegram_loaded(account.id, false, &[5])));
        assert_eq!(f.items().len(), 2);
        assert!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread_more);
        assert!(f.with(|s| s.telegram_loaded(account.id, true, &[5])));
        assert_eq!(f.items().len(), 1);
        assert!(!f.with(|s| s.sync_state().unwrap())[&account.id.0].unread_more);
        f.lock();
    }

    #[test]
    fn the_chat_panel_reads_sends_and_shows_new_messages() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        let item = key(&account, 5);

        let view = chat_open(&f.runtime, &item).unwrap();
        assert_eq!((view.title.as_str(), view.can_write), ("Alice", true));
        // TDLib gave two messages for each request. The panel asked again until the history ended.
        let ids: Vec<i64> = view.messages.iter().map(|m| m.id).collect();
        assert_eq!(ids, [60, 70, 80, 90, 100]);
        assert!(view.messages[0].outgoing && !view.messages[1].outgoing);
        let history = f.sent().iter().filter(|line| line.ends_with("getChatHistory")).count();
        assert_eq!(history, 4);
        assert!(f.sent().contains(&"send 1 openChat".to_string()));
        // The person sees the messages, so the chat becomes read on Telegram and in the timeline.
        f.wait(|| f.items()[0].read);

        send(&f.runtime, &item, "  I am here  ").unwrap();
        let sent =
            f.td.take_sent()
                .into_iter()
                .find(|(_, r)| r["@type"] == "sendMessage")
                .unwrap()
                .1;
        assert_eq!(sent["input_message_content"]["text"]["text"], "I am here");
        f.wait(|| {
            chat_view(&f.runtime, &item)
                .unwrap()
                .messages
                .last()
                .is_some_and(|m| m.id == 110)
        });
        let view = chat_view(&f.runtime, &item).unwrap();
        let last = view.messages.last().unwrap();
        assert_eq!(
            (last.text.as_str(), last.outgoing, last.sending),
            ("I am here", true, None)
        );
        assert!(f.changes.lock().unwrap().iter().any(|change| change.panel));

        assert_eq!(code(send(&f.runtime, &item, "   ")), "answer_empty");
        assert_eq!(code(send(&f.runtime, &item, &"x".repeat(4097))), "telegram_too_long");
        assert_eq!(code(chat_open(&f.runtime, &key(&account, 404))), "telegram_no_chat");
        let not_chat = ItemKey {
            account: account.id,
            source_id: "n/5".into(),
        };
        assert_eq!(code(chat_open(&f.runtime, &not_chat)), "telegram_no_chat");

        chat_close(&f.runtime, account.id);
        f.wait(|| f.sent().contains(&"send 1 closeChat".to_string()));
        assert_eq!(code(chat_view(&f.runtime, &item)), "telegram_no_chat");
        f.lock();
    }

    #[test]
    fn a_chat_where_the_account_cannot_write() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        let mut channel = new_chat(-1007, "News", 1);
        channel["chat"]["type"] = json!({"@type": "chatTypeSupergroup", "supergroup_id": 7, "is_channel": true});
        f.td.push(1, json!({"@type": "updateSupergroup", "supergroup": {"id": 7, "status": {"@type": "chatMemberStatusMember"}}}));
        f.td.push(1, channel);
        f.wait(|| f.items().len() == 2);
        let news = f.items().into_iter().find(|item| item.author == "News").unwrap();
        assert!(!news.can_answer);
        assert_eq!(
            code(send(&f.runtime, &key(&account, -1007), "hello")),
            "telegram_no_write"
        );
        assert!(!f.sent().iter().any(|line| line.ends_with("sendMessage")));
        let view = chat_open(&f.runtime, &key(&account, -1007)).unwrap();
        assert!(!view.can_write);
        f.lock();
    }

    #[test]
    fn removal_logs_out_and_deletes_the_local_data() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        let secrets = f.with(|s| s.secret_count());
        let removed = remove_account(&f.session, &f.runtime, account.id).unwrap();
        assert!(!removed.session_left);
        assert!(f.sent().contains(&"send 1 logOut".to_string()));
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());
        assert!(f.items().is_empty());
        assert!(f.folders().is_empty());
        assert_eq!(f.with(|s| s.secret_count()), secrets - 1);
        assert_eq!(f.with(|s| s.telegram_open_folders()), 0);
        f.lock();
        f.unlock();
        assert!(f.runtime.keys().is_empty(), "no client starts for a removed account");
        f.lock();
    }

    #[test]
    fn removal_without_an_answer_of_telegram_deletes_locally_and_tells_the_person() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        f.lock();
        // After the unlock, the app has no Telegram values, so no client starts.
        f.with(|s| s.unlock("passcode1").unwrap());
        assert!(start_all(&f.session, &f.runtime, None));
        let state = f.with(|s| s.sync_state().unwrap());
        assert_eq!(state[&account.id.0].error, Some(crate::session::VALUES_MISSING));
        let removed = remove_account(&f.session, &f.runtime, account.id).unwrap();
        assert!(removed.session_left);
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());
        assert!(f.folders().is_empty());
        f.lock();
    }

    #[test]
    fn a_session_that_telegram_ended_gets_the_mark() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        f.td.push(1, state("authorizationStateLoggingOut"));
        f.td.push(1, state("authorizationStateClosed"));
        f.wait(|| f.with(|s| s.sync_state().unwrap())[&account.id.0].needs_login);
        f.wait(|| f.runtime.keys().is_empty());
        assert_eq!(
            code(set_read(&f.runtime, &key(&account, 5), true)),
            "telegram_no_client"
        );
        // The removal does not tell the person to end a session that Telegram ended.
        let removed = remove_account(&f.session, &f.runtime, account.id).unwrap();
        assert!(!removed.session_left);
        assert!(!f.sent().contains(&"send 1 logOut".to_string()));
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());
        f.lock();
    }

    /// The `api_id` in each `setTdlibParameters` that the fake got, with the number of the client.
    fn sent_api_ids(f: &Fixture) -> Vec<(i32, i64)> {
        f.td.take_sent()
            .into_iter()
            .filter(|(_, request)| request["@type"] == "setTdlibParameters")
            .map(|(client, request)| (client, request["api_id"].as_i64().unwrap()))
            .collect()
    }

    const OWN_HASH: &str = "fedcba9876543210fedcba9876543210";

    #[test]
    fn own_values_are_only_for_new_logins() {
        let f = Fixture::new();
        // An account with PANIM's values.
        let first = f.login();
        f.wait(|| f.items().len() == 1);
        assert_eq!(sent_api_ids(&f), vec![(1, 12345)]);
        remove_account(&f.session, &f.runtime, first.id).unwrap();

        // The person gives their own values. A bad form is refused, and the setting does not change.
        let set = |use_own: bool, own: Option<(&str, &str)>| f.with(|s| s.set_telegram_app(use_own, own));
        assert_eq!(set(true, None).unwrap_err().code, "telegram_values_none");
        assert_eq!(
            set(true, Some(("12a", OWN_HASH))).unwrap_err().code,
            "telegram_values_invalid"
        );
        assert_eq!(
            set(true, Some(("777", "abc"))).unwrap_err().code,
            "telegram_values_invalid"
        );
        let view = f.with(|s| s.telegram_app(Some(&f.app))).unwrap();
        assert_eq!(
            (view.use_own, view.own_api_id, view.next_login),
            (false, None, Some("builtin"))
        );
        set(true, Some((" 777 ", OWN_HASH))).unwrap();
        let view = f.with(|s| s.telegram_app(Some(&f.app))).unwrap();
        assert_eq!(
            (view.builtin, view.use_own, view.own_api_id, view.next_login),
            (true, true, Some(777), Some("own"))
        );
        assert!(
            !serde_json::to_string(&view).unwrap().contains(OWN_HASH),
            "the hash does not go to the user interface"
        );
        assert_eq!(status(&f.session, Some(&f.app)).unwrap().values, Some("own"));

        // A login with the own values. The secret keeps a copy of them.
        let own = f.login();
        f.wait(|| f.items().iter().any(|item| item.key.account == own.id));
        assert_eq!(sent_api_ids(&f), vec![(2, 777)]);

        // A change of the setting does not change the existing account.
        set(false, None).unwrap();
        f.lock();
        f.unlock();
        f.wait(|| f.runtime.auth(own.id.0) == Some(AuthState::Ready));
        assert_eq!(sent_api_ids(&f), vec![(3, 777)]);
        // The removal of the own values does not change it either. A build without PANIM's values starts it too.
        f.with(|s| s.forget_telegram_app()).unwrap();
        f.lock();
        f.with(|s| s.unlock("passcode1").unwrap());
        start_all(&f.session, &f.runtime, None);
        f.wait(|| f.runtime.auth(own.id.0) == Some(AuthState::Ready));
        assert_eq!(sent_api_ids(&f), vec![(4, 777)]);
        let view = f.with(|s| s.telegram_app(None)).unwrap();
        assert_eq!(
            (view.builtin, view.use_own, view.own_api_id, view.next_login),
            (false, false, None, None)
        );
        f.lock();

        // The setting stays after a restart of the app.
        f.with(|s| s.unlock("passcode1").unwrap());
        f.with(|s| s.set_telegram_app(false, Some(("888", OWN_HASH)))).unwrap();
        f.with(|s| s.lock()).unwrap();
        let mut session = Session::new(f.data.clone(), ProfileArea::at(f.memory.clone()), Strength::TEST);
        session.unlock("passcode1").unwrap();
        let view = session.telegram_app(Some(&f.app)).unwrap();
        assert_eq!(
            (view.use_own, view.own_api_id, view.next_login),
            (false, Some(888), Some("builtin"))
        );
        // Without PANIM's values, a new login uses the own values.
        assert_eq!(session.telegram_app(None).unwrap().next_login, Some("own"));
        session.lock().unwrap();
    }

    #[test]
    fn an_account_without_its_values_does_not_start() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        // The person has own values, but the account logged in with PANIM's values.
        f.with(|s| s.set_telegram_app(true, Some(("777", OWN_HASH)))).unwrap();
        f.lock();
        f.td.take_sent();
        f.with(|s| s.unlock("passcode1").unwrap());
        assert!(start_all(&f.session, &f.runtime, None));
        assert!(
            f.runtime.keys().is_empty(),
            "PANIM does not start the client with different values"
        );
        assert!(f.td.take_sent().is_empty());
        assert!(f.folders().is_empty(), "the folder stays sealed");
        let state = f.with(|s| s.sync_state().unwrap());
        assert_eq!(state[&account.id.0].error, Some(crate::session::VALUES_MISSING));
        // The items stay. A new login is possible with the own values.
        assert_eq!(f.items().len(), 1);
        assert_eq!(status(&f.session, None).unwrap().values, Some("own"));
        f.lock();
        // With PANIM's values, the account starts again.
        f.unlock();
        f.wait(|| f.runtime.auth(account.id.0) == Some(AuthState::Ready));
        f.lock();
    }

    #[test]
    fn a_backup_has_the_sealed_state_of_an_open_tdlib_folder() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        // TDLib writes after the last seal. The backup must have this change.
        let folder = f.folders().pop().unwrap();
        std::fs::write(folder.join("db/td.binlog"), b"the login, after the seal").unwrap();
        // A file in `files` is not sealed.
        std::fs::write(folder.join("files/photo.jpg"), b"a picture").unwrap();

        let out = tempfile::tempdir().unwrap();
        let target = out.path().join("panim.backup");
        let info = backup(&f.session, &f.runtime, Some(&f.app), &target).unwrap();
        assert!(info.files >= 4, "{info:?}");
        assert!(
            f.sent().contains(&"send 1 close".to_string()),
            "the client closes before the seal"
        );
        // The clients start again after the backup.
        f.wait(|| f.runtime.auth(account.id.0) == Some(AuthState::Ready));
        assert_eq!(f.folders().len(), 1);
        f.lock();

        // A restore on a second computer: the same passcode, the same accounts, the same items and the same folder.
        let other = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let data = other.path().join("panim");
        let mut session = Session::new(
            data.clone(),
            ProfileArea::at(memory.path().to_path_buf()),
            Strength::TEST,
        );
        assert_eq!(session.status().unwrap().phase, crate::session::Phase::FirstRun);
        assert_eq!(session.restore(&target).unwrap(), info);
        assert_eq!(session.status().unwrap().phase, crate::session::Phase::Locked);
        assert_eq!(session.restore(&target).unwrap_err().code, "exists");
        assert_eq!(session.unlock("wrong-passcode").unwrap_err().code, "wrong_passcode");
        session.unlock("passcode1").unwrap();
        assert_eq!(session.accounts().unwrap(), vec![account.clone()]);
        let items = session.timeline(&TimelineFilter::default()).unwrap();
        assert_eq!(
            items,
            f.with(|s| {
                s.unlock("passcode1").unwrap();
                let items = s.timeline(&TimelineFilter::default()).unwrap();
                s.lock().unwrap();
                items
            })
        );
        let opened = session.telegram_open_all(Some(&f.app)).unwrap();
        assert_eq!(opened.starts.len(), 1);
        let dir = &opened.starts[0].dir;
        assert_eq!(
            std::fs::read(dir.join("db/td.binlog")).unwrap(),
            b"the login, after the seal"
        );
        assert!(!dir.join("files/photo.jpg").exists());
        session.lock().unwrap();
    }

    #[test]
    fn not_configured_and_proxy_names() {
        let f = Fixture::new();
        assert_eq!(
            status(&f.session, None).unwrap(),
            StatusView {
                configured: false,
                values: None,
                proxy: false
            }
        );
        assert_eq!(
            code(login_start(&f.session, &f.runtime, None)),
            "telegram_not_configured"
        );
        assert!(f.folders().is_empty());

        let named = Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "proxy.example.org".into(),
            port: 1080,
        });
        f.with(|s| s.set_route(named).unwrap());
        assert_eq!(
            status(&f.session, Some(&f.app)).unwrap(),
            StatusView {
                configured: true,
                values: Some("builtin"),
                proxy: true
            }
        );
        assert_eq!(
            code(login_start(&f.session, &f.runtime, Some(&f.app))),
            "telegram_proxy_name"
        );
        assert!(
            f.td.take_sent().is_empty(),
            "TDLib gets no request for a proxy with a host name"
        );
        assert!(f.folders().is_empty());
    }

    #[test]
    fn a_new_route_starts_the_clients_again_with_the_proxy_first() {
        let f = Fixture::new();
        let account = f.login();
        f.wait(|| f.items().len() == 1);
        let tor = Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "localhost".into(),
            port: 9050,
        });
        f.with(|s| s.set_route(tor).unwrap());
        restart_all(&f.session, &f.runtime, Some(&f.app)).unwrap();
        f.wait(|| f.runtime.auth(account.id.0) == Some(AuthState::Ready));
        let sent = f.sent();
        let second: Vec<&String> = sent.iter().filter(|line| line.starts_with("send 2 ")).collect();
        assert_eq!(&second[..2], ["send 2 addProxy", "send 2 setTdlibParameters"]);
        let proxy =
            f.td.take_sent()
                .into_iter()
                .find(|(_, r)| r["@type"] == "addProxy")
                .unwrap()
                .1;
        assert_eq!(
            (&proxy["proxy"]["server"], &proxy["proxy"]["port"]),
            (&json!("127.0.0.1"), &json!(9050))
        );
        assert!(status(&f.session, Some(&f.app)).unwrap().proxy);
        f.lock();
    }

    #[test]
    fn each_kind_and_each_login_step_has_a_text() {
        // `pnpm check` makes sure that each language has each key. This test makes sure that the key exists.
        let texts = include_str!("../../src/lib/i18n.svelte.ts");
        // A long text starts on the next line.
        let has = |key: String| texts.contains(&format!("\n  {key}: ")) || texts.contains(&format!("\n  {key}:\n"));
        for kind in panim_telegram::KINDS {
            assert!(has(format!("tg_kind_{kind}")), "no text for the kind {kind}");
        }
        for reason in ["email_address", "email_code", "other_device", "premium", "registration"] {
            assert!(has(format!("tg_unsupported_{reason}")), "{reason}");
        }
        for via in panim_telegram::CODE_ROUTES {
            assert!(has(format!("tg_code_via_{via}")), "no text for the route {via}");
            assert!(has(format!("tg_next_{via}")), "no text for the next route {via}");
        }
        let codes = [
            "telegram_not_configured",
            "telegram_proxy_name",
            "telegram_bad_input",
            "telegram_no_client",
            "telegram_timeout",
            "telegram_flood",
            "telegram_flood_unknown",
            "telegram_code_invalid",
            "telegram_code_expired",
            "telegram_password_invalid",
            "telegram_phone_invalid",
            "telegram_banned",
            "telegram_app_refused",
            "telegram_database",
            "telegram_logged_out",
            "telegram_no_write",
            "telegram_no_chat",
            "telegram_login_none",
            "telegram_no_unread",
            "telegram_offline",
            "telegram_too_long",
            "telegram_error",
            "telegram_resend_none",
            "telegram_resend_state",
            "telegram_resend_early",
            "telegram_values_missing",
            "telegram_values_invalid",
            "telegram_values_none",
            "bad_backup",
        ];
        let api = include_str!("../../src/lib/api.ts");
        for code in codes {
            assert!(has(format!("err_{code}")), "no text for the error {code}");
            assert!(
                api.contains(&format!("case \"{code}\":")),
                "errorText does not know {code}"
            );
        }
    }

    #[test]
    fn a_telegram_account_can_make_a_proof() {
        let f = Fixture::new();
        let account = f.login();
        // 2026-11-16 12:00:00 UTC.
        let view = f.with(|s| s.make_proof(account.id, 1_794_830_400, false)).unwrap();
        assert!(!view.id_needed && !view.identity_editable);
        let block = view.block.unwrap();
        assert!(block.contains("telegram"), "{block}");
        assert!(block.contains("777001") && block.contains("sakura_r"), "{block}");

        // A username that changes in Telegram changes the handle and removes the kept proof. The id stays.
        let me = panim_telegram::Me {
            id: 777001,
            username: None,
            name: "Sakura R".into(),
        };
        assert!(f.with(|s| s.telegram_me(account.id, &me)));
        let stored = f.with(|s| s.accounts().unwrap()).into_iter().next().unwrap();
        assert_eq!(
            (stored.handle.as_str(), stored.remote_id.as_deref()),
            ("", Some("777001"))
        );
        let proofs = f.with(|s| s.proofs().unwrap());
        assert!(proofs[0].block.is_none());
        // An account with no username can make a proof too: the handle is `-`.
        let block = f
            .with(|s| s.make_proof(account.id, 1_794_830_400, false))
            .unwrap()
            .block
            .unwrap();
        assert!(block.contains("777001"), "{block}");
        f.lock();
    }

    #[test]
    fn a_locked_session_refuses_the_work() {
        let f = Fixture::new();
        let account = f.login();
        f.lock();
        assert_eq!(code(login_start(&f.session, &f.runtime, Some(&f.app))), "locked");
        assert_eq!(
            code(set_read(&f.runtime, &key(&account, 5), true)),
            "telegram_no_client"
        );
        assert_eq!(code(chat_open(&f.runtime, &key(&account, 5))), "telegram_no_client");
        assert!(!start_all(&f.session, &f.runtime, Some(&f.app)));
        assert_eq!(
            apply_event(
                &f.session,
                account.id.0,
                Event::Loaded {
                    complete: true,
                    shown: vec![]
                }
            ),
            Change::default()
        );
    }
}
