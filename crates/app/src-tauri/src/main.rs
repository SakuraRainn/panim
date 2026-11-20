//! PANIM, the desktop app. Refer to `docs/ARCHITECTURE.md` §2.
//!
//! - `session` holds the state of the app, without Tauri.
//! - The commands in this file give the session to the user interface.
//! - `unread` holds the unread state of the account windows, without Tauri.
//! - `activity` limits the event `input-elsewhere`, without Tauri. On Linux, [`watch_input`] gets the input of a
//!   browser window from GTK. The automatic lock in the main window counts this input.
//! - Browser-view accounts open in their own windows. Only the main window can call the commands
//!   (`capabilities/main.json`). The account windows load sites of other parties. They can call only `page_notice`,
//!   which has no arguments (`capabilities/account.json`).
//! - All the commands that lock the session are `async`, and they run on a blocking thread (`docs/ROADMAP.md` §9,
//!   part A). Thus, the main thread never waits for the lock of the session. A seal or Argon2id can hold the lock
//!   for some seconds, but the windows do not stop.
//! - The handlers of the windows also run on the main thread. They send their changes of the unread state to one
//!   thread ([`UnreadQueue`]). That thread changes the session in the order of the changes.
//! - `github` does the work of the GitHub connector, without Tauri. The network commands do not hold the lock of the
//!   session while a request runs.
//! - The device page of GitHub and the links of timeline items open in profile windows. A profile window uses the
//!   browser profile and the route of the persona. No capability names it, so it can call no command.
//! - `telegram` does the work of the Telegram connector, without Tauri. One runtime has the receive loop for all the
//!   TDLib clients. The clients start when the vault opens, and they close before the vault locks. The commands
//!   `telegram_*` do the login and the chat panel. `set_read`, `answer` and `remove_account` also serve Telegram.
//! - The proof screen uses `proofs`, `make_proof`, `set_identity`, `set_proof_location`, `check_own_proof`,
//!   `check_pasted_proof`, `check_github_proof` and `open_gist_page`.
//! - The settings use `telegram_app`, `set_telegram_app` and `forget_telegram_app` for the Telegram app values, and
//!   `backup_vault` for the backup. The first start uses `restore_vault` (milestone 10).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod github;
mod session;
mod telegram;
mod unread;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use github::{LoginPoll, RealNet};
use panim_core::{Account, AccountId, Platform, Route, TimelineFilter};
use panim_github::GistCheck;
use panim_telegram::{AppConfig, PanelView, RealTd, Runtime};
use panim_vault::ProfileArea;
use session::{
    AppError, AppResult, ItemView, LoginView, PastedView, ProofView, Seal, Session, Settings, Status, Strength,
    SyncView, TelegramAppView,
};
use tauri::{
    AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use unread::Unread;

/// WebKitGTK sends an old Safari version by default (`docs/ARCHITECTURE.md` §9). Some sites then show an old page or
/// refuse the browser. This user agent gives a current Safari version.
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";

/// This script replaces `window.Notification` in the account windows. It sends a signal with no content.
const NOTICE_SCRIPT: &str = include_str!("unread.js");

/// The event that tells the main window that the unread state changed. It has no payload.
const UNREAD_CHANGED: &str = "unread-changed";

/// The event that tells the main window that the timeline or the state of a joined account changed. It has no
/// payload.
const TIMELINE_CHANGED: &str = "timeline-changed";

/// The event that tells the main window that the messages of the open chat panel changed. It has no payload.
const TELEGRAM_CHAT_CHANGED: &str = "telegram-chat-changed";

/// The event that tells the main window that the person used a browser window. It has no payload. The main window
/// then resets the time of the automatic lock.
const INPUT_ELSEWHERE: &str = "input-elsewhere";

/// The limit of the event `input-elsewhere`. All the browser windows use the same limit.
static INPUT_THROTTLE: Mutex<activity::Throttle> = Mutex::new(activity::Throttle::new());

/// Tells the main window about the input of the person in a browser window, no more than one time in
/// [`activity::GAP`]. Only Linux uses it until milestone 12.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn input_elsewhere(app: &AppHandle) {
    let send = INPUT_THROTTLE
        .lock()
        .map(|mut throttle| throttle.input(Instant::now()))
        .unwrap_or(false);
    if send {
        let _ = app.emit_to("main", INPUT_ELSEWHERE, ());
    }
}

/// Gets the input of the person in a browser window from GTK, for the automatic lock.
///
/// The handler connects to the signal `event` of the WebKit view. GTK sends each event of the view to this signal
/// before WebKit gets it. The handler counts only key presses, button presses and scroll events, and it does not stop
/// the event. The script of a page cannot make a GTK event. Thus, a page cannot stop the automatic lock.
#[cfg(target_os = "linux")]
fn watch_input(window: &WebviewWindow) {
    use gtk::prelude::WidgetExt;

    let app = window.app_handle().clone();
    let _ = window.with_webview(move |webview| {
        webview.inner().connect_event(move |_, event| {
            use gtk::gdk::EventType;
            if matches!(
                event.event_type(),
                EventType::KeyPress | EventType::ButtonPress | EventType::Scroll
            ) {
                input_elsewhere(&app);
            }
            gtk::glib::Propagation::Proceed
        });
    });
}

/// On other systems, the browser windows do not reset the automatic lock yet. Milestone 12 does this for macOS.
#[cfg(not(target_os = "linux"))]
fn watch_input(_window: &WebviewWindow) {}

/// The Telegram connector of the app: the runtime with the receive loop, and PANIM's values from the build.
struct Telegram {
    runtime: Arc<Runtime>,
    /// `None` if the build has no `api_id` and `api_hash`. Then only the own values of the person can start a login,
    /// and only the accounts with own values start a client.
    app: Option<AppConfig>,
}

/// Starts the client of each Telegram account. Call it after the vault opens.
fn telegram_start(app: &AppHandle) {
    let (session, tg) = (app.state::<Mutex<Session>>(), app.state::<Telegram>());
    if telegram::start_all(&session, &tg.runtime, tg.app.as_ref()) {
        timeline_changed(app);
    }
}

/// Closes all the Telegram clients. Call it before the vault locks, so that the session can seal their folders.
fn telegram_stop(app: &AppHandle) {
    if !telegram::stop_all(&app.state::<Telegram>().runtime) {
        eprintln!("panim: a Telegram client did not close in the time limit");
    }
}

/// The start of the label of a profile window. It is not the start of the label of an account window. Thus, no
/// capability names a profile window, and its page can call no command.
const PROFILE_PREFIX: &str = "profile-";

/// The label of the profile window for the device page of GitHub.
const LOGIN_LABEL: &str = "profile-github-login";

/// The label of the profile window for the page that makes a new gist.
const GIST_LABEL: &str = "profile-gist";

/// The page that makes a new gist. The URL is a constant. It does not come from the user interface.
const GIST_PAGE: &str = "https://gist.github.com/";

/// The label of the profile window of a joined account without a browser view, for example GitHub.
fn home_label(id: AccountId) -> String {
    format!("{PROFILE_PREFIX}account-{}", id.0)
}

/// The number of the next profile window for a link.
static NEXT_LINK_WINDOW: AtomicU64 = AtomicU64::new(1);

/// Only one sync runs at a time. This is not the lock of the session.
static SYNC_GUARD: Mutex<()> = Mutex::new(());

/// The app seals the open browser profile into the vault at this interval.
const SEAL_INTERVAL: Duration = Duration::from_secs(5 * 60);

fn with<T>(session: &Mutex<Session>, f: impl FnOnce(&mut Session) -> AppResult<T>) -> AppResult<T> {
    let mut session = session.lock().map_err(|_| AppError {
        code: "io",
        tries_left: None,
        detail: "poisoned".into(),
    })?;
    f(&mut session)
}

fn window_label(id: AccountId) -> String {
    format!("{}{}", unread::LABEL_PREFIX, id.0)
}

/// Changes the session without an error. Returns false if the lock of the session failed.
fn update(app: &AppHandle, f: impl FnOnce(&mut Session) -> bool) -> bool {
    let state = app.state::<Mutex<Session>>();
    state.lock().map(|mut session| f(&mut session)).unwrap_or(false)
}

/// Tells the main window that the unread state changed. The main window then calls `unread`.
fn unread_changed(app: &AppHandle) {
    let _ = app.emit_to("main", UNREAD_CHANGED, ());
}

/// A change of the unread state. It returns true if the state changed.
type UnreadChange = Box<dyn FnOnce(&mut Session) -> bool + Send>;

/// A job of the queue of the unread state.
enum UnreadJob {
    /// A change of the session.
    Change(UnreadChange),
    /// A notification of the page of an account window, at a time. The worker records it only if the window is
    /// not in focus when the job runs.
    Notice(Box<WebviewWindow>, AccountId, Instant),
}

/// The queue of the changes of the unread state.
///
/// The handlers of the windows run on the main thread, so they must not lock the session. They put their changes in
/// this queue. One thread ([`unread_worker`]) does the changes in their order. Thus, an old title of a page cannot
/// replace a newer title.
struct UnreadQueue(Mutex<mpsc::Sender<UnreadJob>>);

/// Puts a change of the unread state in the queue. This function does not lock the session.
fn queue_unread(app: &AppHandle, change: impl FnOnce(&mut Session) -> bool + Send + 'static) {
    queue_job(app, UnreadJob::Change(Box::new(change)));
}

/// Puts a job in the queue of the unread state. This function does not lock the session and does not wait.
fn queue_job(app: &AppHandle, job: UnreadJob) {
    if let Some(queue) = app.try_state::<UnreadQueue>()
        && let Ok(sender) = queue.0.lock()
    {
        let _ = sender.send(job);
    }
}

/// Does the jobs of the unread state in their order, and tells the main window about each change.
fn unread_worker(app: AppHandle, jobs: mpsc::Receiver<UnreadJob>) {
    for job in jobs {
        let change = match job {
            UnreadJob::Change(change) => change,
            UnreadJob::Notice(window, id, at) => {
                // The query of the focus waits for the main thread. The worker does not hold the lock of the session
                // here, so the main thread and the worker do not wait for each other. If the person looks at the
                // window now, the notice is not necessary. An error (for example, a closed window) records nothing.
                if window.is_focused().unwrap_or(true) {
                    continue;
                }
                Box::new(move |s: &mut Session| s.record_notice(id, at))
            }
        };
        if update(&app, change) {
            unread_changed(&app);
        }
    }
}

/// Closes the windows of all browser-view accounts and all profile windows. Do this before the browser profile
/// closes.
fn close_account_windows(app: &AppHandle) {
    let mut ids = Vec::new();
    for (label, window) in app.webview_windows() {
        if let Some(id) = unread::account_from_label(&label) {
            let _ = window.destroy();
            ids.push(AccountId(id));
        } else if label.starts_with(PROFILE_PREFIX) {
            let _ = window.destroy();
        }
    }
    let changed = update(app, |s| {
        ids.into_iter().fold(false, |changed, id| s.drop_unread(id) | changed)
    });
    if changed {
        unread_changed(app);
    }
}

/// The window events of the account windows: the focus clears the notice, and a closed window has no count.
fn account_window_event(window: &tauri::Window, event: &WindowEvent) {
    let Some(id) = unread::account_from_label(window.label()).map(AccountId) else {
        return;
    };
    // This handler runs on the main thread. It does not lock the session.
    let app = window.app_handle();
    match event {
        WindowEvent::Focused(true) => queue_unread(app, move |s| s.clear_notice(id)),
        WindowEvent::Destroyed => queue_unread(app, move |s| s.drop_unread(id)),
        _ => {}
    }
}

#[tauri::command]
async fn status(app: AppHandle) -> AppResult<Status> {
    locked(app, |s| s.status()).await
}

#[tauri::command]
async fn set_settings(app: AppHandle, settings: Settings) -> AppResult<()> {
    locked(app, move |s| s.set_settings(settings)).await
}

#[tauri::command]
async fn create(app: AppHandle, passcode: String) -> AppResult<()> {
    locked(app, move |s| s.create(&passcode)).await
}

/// Opens the vault with a function of the session on a blocking thread, and then starts the Telegram clients.
async fn open_vault(app: AppHandle, f: impl FnOnce(&mut Session) -> AppResult<()> + Send + 'static) -> AppResult<()> {
    blocking(app, |app, session| {
        with(session, f)?;
        telegram_start(app);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn unlock(app: AppHandle, passcode: String) -> AppResult<()> {
    open_vault(app, move |s| s.unlock(&passcode)).await
}

#[tauri::command]
async fn recover(app: AppHandle, path: PathBuf, password: String, passcode: String) -> AppResult<()> {
    open_vault(app, move |s| s.recover(&path, &password, &passcode)).await
}

#[tauri::command]
async fn recover_with_words(app: AppHandle, words: String, passcode: String) -> AppResult<()> {
    open_vault(app, move |s| s.recover_with_words(&words, &passcode)).await
}

#[tauri::command]
async fn recovery_words(app: AppHandle, passcode: Option<String>) -> AppResult<Vec<String>> {
    locked(app, move |s| s.recovery_words(passcode.as_deref())).await
}

#[tauri::command]
async fn check_words(app: AppHandle, answers: Vec<(usize, String)>) -> AppResult<()> {
    locked(app, move |s| s.check_words(&answers)).await
}

#[tauri::command]
async fn export_key_file(app: AppHandle, password: String, path: PathBuf) -> AppResult<()> {
    locked(app, move |s| s.export_key_file(&password, &path)).await
}

#[tauri::command]
async fn change_passcode(app: AppHandle, current: String, passcode: String) -> AppResult<()> {
    locked(app, move |s| s.change_passcode(&current, &passcode)).await
}

/// Locks the app. The Telegram clients close first, so that the session can seal their folders.
#[tauri::command]
async fn lock(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, session| {
        close_account_windows(app);
        telegram_stop(app);
        session.lock().map_err(|_| AppError::new("io", "poisoned"))?.lock()
    })
    .await
}

#[tauri::command]
async fn set_route(app: AppHandle, route: Route) -> AppResult<()> {
    blocking(app, move |app, session| {
        // Open windows keep the old route. Close them, so the next window uses the new route.
        close_account_windows(app);
        session
            .lock()
            .map_err(|_| AppError::new("io", "poisoned"))?
            .set_route(route)?;
        // A TDLib client gets its proxy only at its start. Start the clients again on the new route.
        let tg = app.state::<Telegram>();
        telegram::restart_all(session, &tg.runtime, tg.app.as_ref())?;
        timeline_changed(app);
        Ok(())
    })
    .await
}

#[tauri::command]
async fn accounts(app: AppHandle) -> AppResult<Vec<Account>> {
    locked(app, |s| {
        #[cfg(debug_assertions)]
        session::test_slow();
        s.accounts()
    })
    .await
}

#[tauri::command]
async fn add_browser_account(
    app: AppHandle,
    platform: Platform,
    display_name: String,
    handle: String,
) -> AppResult<Account> {
    locked(app, move |s| s.add_browser_account(platform, &display_name, &handle)).await
}

/// Removes an account. For a Telegram account, PANIM first ends the session on Telegram. The result tells if
/// Telegram did not confirm that.
#[tauri::command]
async fn remove_account(app: AppHandle, id: u64) -> AppResult<telegram::Removed> {
    blocking(app, move |app, session| {
        for label in [window_label(AccountId(id)), home_label(AccountId(id))] {
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.destroy();
            }
        }
        let removed = telegram::remove_account(session, &app.state::<Telegram>().runtime, AccountId(id))?;
        unread_changed(app);
        Ok(removed)
    })
    .await
}

#[tauri::command]
async fn timeline(app: AppHandle, account: Option<u64>, unread_only: bool) -> AppResult<Vec<ItemView>> {
    let filter = TimelineFilter {
        accounts: account.map(|id| vec![AccountId(id)]),
        unread_only,
        limit: Some(500),
        ..Default::default()
    };
    locked(app, move |s| {
        #[cfg(debug_assertions)]
        session::test_slow();
        s.timeline(&filter)
    })
    .await
}

/// The unread state of each account with an open window. The key is the account ID.
#[tauri::command]
async fn unread(app: AppHandle) -> AppResult<BTreeMap<u64, Unread>> {
    locked(app, |s| Ok(s.unread())).await
}

/// The signal of `unread.js`: the page of an account window made a notification.
///
/// The account windows can call only this command. It has no arguments, so the page cannot send data. The account
/// comes from the label of the window, not from the page.
///
/// The command is not `async`, so it runs on the main thread, as the handlers of the windows do. It only puts the
/// notice in the queue. It does not lock the session and does not wait. Thus, the queue gets the notice, the change
/// of the focus and the close of the window in the order of the main thread. The worker examines the focus when the
/// job runs (`unread_worker`).
#[tauri::command]
fn page_notice(window: WebviewWindow) {
    let Some(id) = unread::account_from_label(window.label()).map(AccountId) else {
        return;
    };
    let app = window.app_handle().clone();
    queue_job(&app, UnreadJob::Notice(Box::new(window), id, Instant::now()));
}

/// Opens the browser window of an account, or shows it if it is open.
///
/// A browser-view account opens in an account window. A joined GitHub account opens `https://github.com/` in a
/// profile window. A profile window can call no command and gets no script of PANIM.
#[tauri::command]
async fn open_account(app: AppHandle, id: u64) -> AppResult<()> {
    blocking(app, move |app, session| open_account_window(app, session, id)).await
}

/// Does the work of `open_account` on a blocking thread.
///
/// The function releases the lock of the session before it makes the window. The build of a window waits for the
/// main thread. Thus, the function does not hold the lock while it waits for the main thread.
fn open_account_window(app: &AppHandle, session: &Mutex<Session>, id: u64) -> AppResult<()> {
    let label = window_label(AccountId(id));
    let home = home_label(AccountId(id));
    for open in [&label, &home] {
        if let Some(window) = app.get_webview_window(open) {
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }
    }
    let (account, profile, route) = with(session, |s| s.browser_target(AccountId(id)))?;
    if account.browser.is_none() {
        let (url, title) = with(session, |s| s.home_page(AccountId(id)))?;
        return open_profile_window(app, session, home, url, &title);
    }
    let browser = account.browser.ok_or_else(|| AppError {
        code: "store",
        tries_left: None,
        detail: "the account has no browser view".into(),
    })?;
    let url = url::Url::parse(&browser.start_url).map_err(|e| AppError {
        code: "store",
        tries_left: None,
        detail: e.to_string(),
    })?;
    let account_id = account.id;
    // The handler reads the title of the page and keeps only the unread count. In Tauri 2.12, a title handler does
    // not change the title of the window, so the window keeps the title below.
    let profile_path = profile.clone();
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title(format!("{} · {}", account.display_name, account.handle))
        .inner_size(1100.0, 800.0)
        .data_directory(profile)
        .user_agent(USER_AGENT)
        .initialization_script(NOTICE_SCRIPT)
        .on_document_title_changed(move |window, title| {
            // This handler runs on the main thread. The queue sends the title to the thread of the unread state.
            queue_unread(window.app_handle(), move |s| s.set_unread_title(account_id, &title));
        });
    if let Route::Proxy(proxy) = route {
        let proxy = url::Url::parse(&proxy.url()).map_err(|e| AppError {
            code: "bad_proxy",
            tries_left: None,
            detail: e.to_string(),
        })?;
        builder = builder.proxy_url(proxy);
    }
    let window = builder.build().map_err(|e| AppError {
        code: "io",
        tries_left: None,
        detail: e.to_string(),
    })?;
    keep_if_open(session, window, &profile_path)
}

/// Keeps a new browser window only if its profile is still the open profile of the vault.
///
/// The lock can occur after the read of the profile and before the build of the window. Then the lock did not see
/// the window, and the profile folder is closed. Thus, the function destroys the window and gives the error
/// `locked`. It does not hold the lock of the session while it destroys the window, because that waits for the main
/// thread. If the lock occurs after this check, the lock sees the window and closes it (`close_account_windows`).
fn keep_if_open(session: &Mutex<Session>, window: WebviewWindow, profile: &Path) -> AppResult<()> {
    // A poisoned lock gives false, so that the window does not stay.
    let open = with(session, |s| Ok(s.profile_is_open(profile))).unwrap_or(false);
    if !open {
        let _ = window.destroy();
        return Err(AppError::new("locked", "the vault locked while the window opened"));
    }
    watch_input(&window);
    Ok(())
}

/// Opens a profile window: a browser window in the profile and on the route of the persona.
///
/// The window gets no script of PANIM. Its label starts with `PROFILE_PREFIX`, so its page can call no command.
/// Call this function on a blocking thread. It releases the lock of the session before it makes the window.
fn open_profile_window(
    app: &AppHandle,
    session: &Mutex<Session>,
    label: String,
    url: url::Url,
    title: &str,
) -> AppResult<()> {
    let (profile, route) = session
        .lock()
        .map_err(|_| AppError::new("io", "poisoned"))?
        .profile_target()?;
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title(title)
        .inner_size(1100.0, 800.0)
        .data_directory(profile.clone())
        .user_agent(USER_AGENT);
    if let Route::Proxy(proxy) = route {
        let proxy = url::Url::parse(&proxy.url()).map_err(|e| AppError::new("bad_proxy", e))?;
        builder = builder.proxy_url(proxy);
    }
    let window = builder.build().map_err(|e| AppError::new("io", e))?;
    keep_if_open(session, window, &profile)
}

/// Runs a function on a blocking thread. Thus, a slow network or a wait for the lock of the session does not stop
/// the main thread.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppHandle, &Mutex<Session>) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<Session>>();
        f(&app, &state)
    })
    .await
    .map_err(|e| AppError::new("io", e))?
}

/// Runs a function of the session on a blocking thread, with the lock of the session.
async fn locked<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&mut Session) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    blocking(app, |_, session| with(session, f)).await
}

/// Tells the main window that the timeline or the state of a joined account changed.
fn timeline_changed(app: &AppHandle) {
    let _ = app.emit_to("main", TIMELINE_CHANGED, ());
}

/// Does one sync of the joined accounts, and tells the main window if something changed.
fn sync_and_tell(app: &AppHandle, force: bool) {
    let Ok(_guard) = SYNC_GUARD.lock() else { return };
    let state = app.state::<Mutex<Session>>();
    if github::sync_all(&state, &RealNet, force, github::now_millis()) {
        timeline_changed(app);
    }
}

/// Starts the device login of GitHub. The result has the code that the person enters on the page of GitHub.
#[tauri::command]
async fn github_login_start(app: AppHandle) -> AppResult<LoginView> {
    blocking(app, |_, session| github::login_start(session, &RealNet, Instant::now())).await
}

/// Polls the device login one time. The result is `pending` with the interval, or `done` with the new account.
#[tauri::command]
async fn github_login_poll(app: AppHandle) -> AppResult<LoginPoll> {
    blocking(app, |app, session| {
        let poll = github::login_poll(session, &RealNet, Instant::now())?;
        if matches!(poll, LoginPoll::Done(_)) {
            if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
                let _ = window.destroy();
            }
            // Get the first items of the new account now.
            let app = app.clone();
            std::thread::spawn(move || sync_and_tell(&app, false));
        }
        Ok(poll)
    })
    .await
}

/// Stops the device login and closes the window of the device page.
#[tauri::command]
async fn github_login_cancel(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, session| {
        if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
            let _ = window.destroy();
        }
        github::login_cancel(session)
    })
    .await
}

/// Opens the device page of GitHub in a profile window. Thus, the login to GitHub stays in the profile of the
/// persona and uses its route.
#[tauri::command]
async fn open_github_login(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, session| {
        if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }
        let url = url::Url::parse(panim_github::login::DEVICE_PAGE).expect("a correct URL");
        open_profile_window(app, session, LOGIN_LABEL.into(), url, "GitHub")
    })
    .await
}

/// Opens the page of a timeline item in a profile window.
///
/// The URL comes from the store, not from the user interface. The app opens only a page of GitHub.
#[tauri::command]
async fn open_link(app: AppHandle, account: u64, source_id: String) -> AppResult<()> {
    blocking(app, move |app, session| {
        let url = github::item_link(session, &github::key(account, source_id))?;
        let label = format!(
            "{PROFILE_PREFIX}link-{}",
            NEXT_LINK_WINDOW.fetch_add(1, Ordering::Relaxed)
        );
        open_profile_window(app, session, label, url, "GitHub")
    })
    .await
}

/// Does a sync of the joined accounts now. It also tries the accounts that need a new login.
#[tauri::command]
async fn sync_now(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, _| {
        sync_and_tell(app, true);
        Ok(())
    })
    .await
}

/// The state of each joined account: the unread count and the result of the last sync. The key is the account ID.
#[tauri::command]
async fn sync_state(app: AppHandle) -> AppResult<BTreeMap<u64, SyncView>> {
    locked(app, |s| {
        #[cfg(debug_assertions)]
        session::test_slow();
        s.sync_state()
    })
    .await
}

/// Sets the read state of a timeline item. A notification that becomes read also becomes read on GitHub.
#[tauri::command]
async fn set_read(app: AppHandle, account: u64, source_id: String, read: bool) -> AppResult<()> {
    blocking(app, move |app, session| {
        let key = github::key(account, source_id);
        if is_telegram(session, key.account) {
            // The read of a chat goes to Telegram. The client then tells the new state of the item.
            return telegram::set_read(&app.state::<Telegram>().runtime, &key, read);
        }
        if github::set_read(session, &RealNet, &key, read)? {
            timeline_changed(app);
        }
        Ok(())
    })
    .await
}

/// Posts an answer to a timeline item, as the account of the item. The result is the web page of the answer.
#[tauri::command]
async fn answer(app: AppHandle, account: u64, source_id: String, text: String) -> AppResult<String> {
    blocking(app, move |app, session| {
        let key = github::key(account, source_id);
        if is_telegram(session, key.account) {
            // A Telegram message has no web page, so the result is an empty text.
            telegram::send(&app.state::<Telegram>().runtime, &key, &text)?;
            return Ok(String::new());
        }
        let result = github::answer(session, &RealNet, &key, &text);
        if result.as_ref().is_err_and(|error| error.code == "github_bad_token") {
            timeline_changed(app);
        }
        result
    })
    .await
}

fn is_telegram(session: &Mutex<Session>, account: AccountId) -> bool {
    session.lock().is_ok_and(|session| session.is_telegram(account))
}

/// Tells if PANIM can add a Telegram account, and if the persona has a proxy route.
#[tauri::command]
async fn telegram_status(app: AppHandle) -> AppResult<telegram::StatusView> {
    blocking(app, |app, session| {
        telegram::status(session, app.state::<Telegram>().app.as_ref())
    })
    .await
}

/// Starts a login of Telegram. The result is the first step for the person: the phone number.
#[tauri::command]
async fn telegram_login_start(app: AppHandle) -> AppResult<telegram::LoginView> {
    blocking(app, |app, session| {
        let tg = app.state::<Telegram>();
        telegram::login_start(session, &tg.runtime, tg.app.as_ref())
    })
    .await
}

/// Tells the main window about the new account of a complete login.
fn login_done(app: &AppHandle, view: AppResult<telegram::LoginView>) -> AppResult<telegram::LoginView> {
    if view.as_ref().is_ok_and(|view| view.account.is_some()) {
        timeline_changed(app);
    }
    view
}

/// Gives the phone number of the login to Telegram. The number has the country code.
#[tauri::command]
async fn telegram_login_phone(app: AppHandle, phone: String) -> AppResult<telegram::LoginView> {
    blocking(app, move |app, session| {
        let view = telegram::login_phone(session, &app.state::<Telegram>().runtime, &phone);
        login_done(app, view)
    })
    .await
}

/// Gives the login code to Telegram.
#[tauri::command]
async fn telegram_login_code(app: AppHandle, code: String) -> AppResult<telegram::LoginView> {
    blocking(app, move |app, session| {
        let view = telegram::login_code(session, &app.state::<Telegram>().runtime, &code);
        login_done(app, view)
    })
    .await
}

/// Gives the two-step password to Telegram.
#[tauri::command]
async fn telegram_login_password(app: AppHandle, password: String) -> AppResult<telegram::LoginView> {
    blocking(app, move |app, session| {
        let view = telegram::login_password(session, &app.state::<Telegram>().runtime, &password);
        login_done(app, view)
    })
    .await
}

/// Asks Telegram to send the login code again, on the next route. The result has the new route and timeout.
#[tauri::command]
async fn telegram_login_resend(app: AppHandle) -> AppResult<telegram::LoginView> {
    blocking(app, |app, session| {
        telegram::login_resend(session, &app.state::<Telegram>().runtime)
    })
    .await
}

/// Stops the login of Telegram and deletes its folder.
#[tauri::command]
async fn telegram_login_cancel(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, session| {
        telegram::login_cancel(session, &app.state::<Telegram>().runtime)
    })
    .await
}

/// The setting of the Telegram app values. The result never contains the `api_hash`.
#[tauri::command]
async fn telegram_app(app: AppHandle) -> AppResult<TelegramAppView> {
    blocking(app, |app, session| {
        let tg = app.state::<Telegram>();
        with(session, |s| s.telegram_app(tg.app.as_ref()))
    })
    .await
}

/// Sets the Telegram app values of new logins. With `api_id` and `api_hash`, PANIM keeps them as the own values of
/// the person. An existing account keeps the values of its login.
#[tauri::command]
async fn set_telegram_app(
    app: AppHandle,
    use_own: bool,
    api_id: Option<String>,
    api_hash: Option<String>,
) -> AppResult<TelegramAppView> {
    blocking(app, move |app, session| {
        let tg = app.state::<Telegram>();
        with(session, |s| {
            let own = match (api_id.as_deref(), api_hash.as_deref()) {
                (Some(id), Some(hash)) if !id.trim().is_empty() || !hash.trim().is_empty() => Some((id, hash)),
                _ => None,
            };
            s.set_telegram_app(use_own, own)?;
            s.telegram_app(tg.app.as_ref())
        })
    })
    .await
}

/// Removes the own Telegram app values of the person. New logins then use PANIM's values.
#[tauri::command]
async fn forget_telegram_app(app: AppHandle) -> AppResult<TelegramAppView> {
    blocking(app, |app, session| {
        let tg = app.state::<Telegram>();
        with(session, |s| {
            s.forget_telegram_app()?;
            s.telegram_app(tg.app.as_ref())
        })
    })
    .await
}

/// Writes a backup of the vault to `path`. The browser windows and the Telegram clients close first, so that the
/// archive has their exact sealed state. The Telegram clients start again after the backup.
#[tauri::command]
async fn backup_vault(app: AppHandle, path: PathBuf) -> AppResult<u64> {
    blocking(app, move |app, session| {
        close_account_windows(app);
        let tg = app.state::<Telegram>();
        let info = telegram::backup(session, &tg.runtime, tg.app.as_ref(), &path);
        timeline_changed(app);
        Ok(info?.bytes)
    })
    .await
}

/// Restores a vault from a backup. This is possible only on the first start, when the data folder has no vault.
#[tauri::command]
async fn restore_vault(app: AppHandle, path: PathBuf) -> AppResult<()> {
    locked(app, move |s| s.restore(&path).map(|_| ())).await
}

/// Opens the chat panel for the chat of a timeline item. The result has the last messages of the chat.
#[tauri::command]
async fn telegram_chat_open(app: AppHandle, account: u64, source_id: String) -> AppResult<PanelView> {
    blocking(app, move |app, _| {
        telegram::chat_open(&app.state::<Telegram>().runtime, &github::key(account, source_id))
    })
    .await
}

/// The messages of the open chat panel. The main window calls it after the event `telegram-chat-changed`.
#[tauri::command]
async fn telegram_chat(app: AppHandle, account: u64, source_id: String) -> AppResult<PanelView> {
    blocking(app, move |app, _| {
        telegram::chat_view(&app.state::<Telegram>().runtime, &github::key(account, source_id))
    })
    .await
}

/// Closes the chat panel of an account. It does not lock the session, so it can run on the main thread.
#[tauri::command]
fn telegram_chat_close(tg: State<Telegram>, account: u64) {
    telegram::chat_close(&tg.runtime, AccountId(account));
}

/// Examines the account proofs in a gist. `gist` is a gist identifier or a gist URL. The request uses no token.
#[tauri::command]
async fn check_github_proof(app: AppHandle, gist: String) -> AppResult<GistCheck> {
    blocking(app, move |_, session| github::check_proof(session, &RealNet, &gist)).await
}

/// The proof state of each account of the persona.
#[tauri::command]
async fn proofs(app: AppHandle) -> AppResult<Vec<ProofView>> {
    locked(app, |s| s.proofs()).await
}

/// Makes a proof for an account and keeps it. The statement has the date of today in UTC. With `replace`, the new
/// block replaces a different kept block.
#[tauri::command]
async fn make_proof(app: AppHandle, account: u64, replace: bool) -> AppResult<ProofView> {
    let now = github::now_millis() / 1000;
    locked(app, move |s| s.make_proof(AccountId(account), now, replace)).await
}

/// Sets the handle and the numeric id of an account without a connector.
#[tauri::command]
async fn set_identity(app: AppHandle, account: u64, handle: String, id: String) -> AppResult<Account> {
    locked(app, move |s| s.set_identity(AccountId(account), &handle, &id)).await
}

/// Sets the URL where the person posted the kept block of an account. An empty text removes it.
#[tauri::command]
async fn set_proof_location(app: AppHandle, account: u64, location: String) -> AppResult<ProofView> {
    locked(app, move |s| s.set_proof_location(AccountId(account), &location)).await
}

/// Checks the person's own proof for a GitHub account in a gist. On a pass, the gist becomes the location.
#[tauri::command]
async fn check_own_proof(app: AppHandle, account: u64, gist: String) -> AppResult<github::OwnCheck> {
    blocking(app, move |_, session| {
        github::check_own_proof(session, &RealNet, AccountId(account), &gist)
    })
    .await
}

/// Checks the form and the signature of a pasted block. It uses no network.
#[tauri::command]
async fn check_pasted_proof(app: AppHandle, text: String) -> AppResult<PastedView> {
    locked(app, move |s| s.check_pasted_proof(&text)).await
}

/// Opens the page that makes a new gist in a profile window. The page can call no command.
#[tauri::command]
async fn open_gist_page(app: AppHandle) -> AppResult<()> {
    blocking(app, |app, session| {
        if let Some(window) = app.get_webview_window(GIST_LABEL) {
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }
        let url = url::Url::parse(GIST_PAGE).expect("a correct URL");
        open_profile_window(app, session, GIST_LABEL.into(), url, "GitHub Gist")
    })
    .await
}

/// The vault folder: `$PANIM_DATA`, or `$XDG_DATA_HOME/panim`, or `~/.local/share/panim`.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PANIM_DATA") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .expect("HOME is not set");
    base.join("panim")
}

fn main() {
    let area = ProfileArea::runtime().expect("PANIM needs XDG_RUNTIME_DIR for the browser profiles");
    let session = Session::new(data_dir(), area, Strength::STANDARD);
    let (unread_sender, unread_receiver) = mpsc::channel::<UnreadJob>();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(session))
        .manage(UnreadQueue(Mutex::new(unread_sender)))
        .on_window_event(account_window_event)
        .setup(move |app| {
            // The thread of the unread state. The handlers of the windows send their changes to it.
            let handle = app.handle().clone();
            std::thread::spawn(move || unread_worker(handle, unread_receiver));
            // The runtime stops the log of TDLib before the first client. Its sink gives the events of the clients
            // to the session and tells the main window.
            let handle = app.handle().clone();
            let sink = move |key: u64, event: panim_telegram::Event| {
                let change = telegram::apply_event(&handle.state::<Mutex<Session>>(), key, event);
                if change.timeline {
                    timeline_changed(&handle);
                }
                if change.panel {
                    let _ = handle.emit_to("main", TELEGRAM_CHAT_CHANGED, ());
                }
            };
            app.manage(Telegram {
                runtime: Runtime::new(Arc::new(RealTd), Box::new(sink)),
                app: AppConfig::from_build(),
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(SEAL_INTERVAL);
                    let state = handle.state::<Mutex<Session>>();
                    if let Ok(mut session) = state.lock()
                        && let Err(error) = session.seal_profile(Seal::Periodic)
                    {
                        eprintln!("panim: the browser profile was not sealed: {}", error.detail);
                    }
                    // TDLib writes its folder while the archive is made. The archive of the lock is the exact one.
                    if let Err(error) = telegram::seal(&state) {
                        eprintln!("panim: a Telegram folder was not sealed: {}", error.detail);
                    }
                }
            });
            // The sync of the joined accounts. It does nothing while the vault is locked.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    sync_and_tell(&handle, false);
                    let state = handle.state::<Mutex<Session>>();
                    let seconds = state.lock().map(|session| session.sync_interval());
                    std::thread::sleep(Duration::from_secs(seconds.unwrap_or(session::SYNC_SECONDS)));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status,
            set_settings,
            create,
            unlock,
            recover,
            recover_with_words,
            recovery_words,
            check_words,
            export_key_file,
            change_passcode,
            lock,
            set_route,
            accounts,
            add_browser_account,
            remove_account,
            timeline,
            open_account,
            unread,
            page_notice,
            github_login_start,
            github_login_poll,
            github_login_cancel,
            open_github_login,
            open_link,
            sync_now,
            sync_state,
            set_read,
            answer,
            check_github_proof,
            proofs,
            make_proof,
            set_identity,
            set_proof_location,
            check_own_proof,
            check_pasted_proof,
            open_gist_page,
            telegram_status,
            telegram_login_start,
            telegram_login_phone,
            telegram_login_code,
            telegram_login_password,
            telegram_login_resend,
            telegram_login_cancel,
            telegram_chat_open,
            telegram_chat,
            telegram_chat_close,
            telegram_app,
            set_telegram_app,
            forget_telegram_app,
            backup_vault,
            restore_vault,
        ])
        .build(tauri::generate_context!())
        .expect("the app did not start");

    app.run(|app, event| {
        // When the main window closes, the app stops. Seal the browser profile first.
        if let RunEvent::ExitRequested { .. } = event {
            close_account_windows(app);
            telegram_stop(app);
            let state = app.state::<Mutex<Session>>();
            if let Ok(mut session) = state.lock()
                && let Err(error) = session.lock()
            {
                eprintln!("panim: the browser profile was not sealed: {}", error.detail);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use tauri::utils::acl::RemoteUrlPattern;
    use tauri::utils::acl::capability::Capability;

    fn capability(text: &str) -> Capability {
        serde_json::from_str(text).expect("a correct capability file")
    }

    fn permissions(capability: &Capability) -> Vec<String> {
        capability
            .permissions
            .iter()
            .map(|entry| entry.identifier().get().to_string())
            .collect()
    }

    /// The commands in the app ACL manifest of `build.rs`.
    fn manifest_commands() -> Vec<String> {
        let build = include_str!("../build.rs");
        let list = build.split_once("COMMANDS: &[&str] = &[").unwrap().1;
        let list = list.split_once("];").unwrap().0;
        list.split(',')
            .map(|item| item.trim().trim_matches('"').to_string())
            .filter(|item| !item.is_empty())
            .collect()
    }

    /// The commands of the Telegram connector (milestone 7).
    pub const TELEGRAM_COMMANDS: [&str; 10] = [
        "telegram_status",
        "telegram_login_start",
        "telegram_login_phone",
        "telegram_login_code",
        "telegram_login_password",
        "telegram_login_resend",
        "telegram_login_cancel",
        "telegram_chat_open",
        "telegram_chat",
        "telegram_chat_close",
    ];

    /// The commands of the Telegram app values and of the backup (milestone 10).
    pub const RELEASE_COMMANDS: [&str; 5] = [
        "telegram_app",
        "set_telegram_app",
        "forget_telegram_app",
        "backup_vault",
        "restore_vault",
    ];

    /// The commands of the proof screen (milestone 6).
    pub const PROOF_COMMANDS: [&str; 8] = [
        "proofs",
        "make_proof",
        "set_identity",
        "set_proof_location",
        "check_own_proof",
        "check_pasted_proof",
        "check_github_proof",
        "open_gist_page",
    ];

    #[test]
    fn main_window_gets_all_commands_but_the_notice() {
        let main = capability(include_str!("../capabilities/main.json"));
        assert_eq!(main.windows, vec!["main".to_string()]);
        assert!(main.local);
        assert!(main.remote.is_none());
        let allowed = permissions(&main);
        let commands = manifest_commands();
        assert!(commands.len() > 25);
        assert!(commands.contains(&"github_login_poll".to_string()));
        for command in PROOF_COMMANDS
            .into_iter()
            .chain(TELEGRAM_COMMANDS)
            .chain(RELEASE_COMMANDS)
        {
            assert!(commands.contains(&command.to_string()), "{command}");
        }
        for command in commands {
            let permission = format!("allow-{}", command.replace('_', "-"));
            assert_eq!(allowed.contains(&permission), command != "page_notice", "{permission}");
        }
    }

    #[test]
    fn account_windows_get_only_the_notice() {
        let account = capability(include_str!("../capabilities/account.json"));
        assert_eq!(account.windows, vec!["account-*".to_string()]);
        assert!(!account.local);
        assert_eq!(permissions(&account), vec!["allow-page-notice".to_string()]);

        let urls = &account.remote.as_ref().unwrap().urls;
        let patterns: Vec<RemoteUrlPattern> = urls.iter().map(|url| url.parse().unwrap()).collect();
        let matches = |url: &str| patterns.iter().any(|p| p.test(&url.parse().unwrap()));
        assert!(matches("https://x.com/home"));
        assert!(matches("https://discord.com/channels/@me"));
        assert!(matches("https://mail.proton.me/u/0/inbox?page=2#top"));
        assert!(matches("https://a.b.example.org/"));
        assert!(!matches("http://x.com/home"));
        assert!(!matches("tauri://localhost/index.html"));
    }
}

/// These tests send commands through the real ACL of the app, with the mock runtime of Tauri.
#[cfg(test)]
mod acl {
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
    use tauri::webview::InvokeRequest;
    use tauri::{App, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

    fn app() -> App<MockRuntime> {
        mock_builder()
            // The ACL examines the command before the handler. This handler accepts each command that the ACL lets
            // through.
            .invoke_handler(|invoke| {
                invoke.resolver.resolve(());
                true
            })
            .build(tauri::generate_context!())
            .expect("the mock app")
    }

    fn window(app: &App<MockRuntime>, label: &str, url: &str) -> WebviewWindow<MockRuntime> {
        if let Some(window) = app.get_webview_window(label) {
            return window;
        }
        WebviewWindowBuilder::new(app, label, WebviewUrl::External(url.parse().unwrap()))
            .build()
            .unwrap()
    }

    fn call(window: &WebviewWindow<MockRuntime>, cmd: &str, url: &str) -> bool {
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: url.parse().unwrap(),
            body: InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        get_ipc_response(window, request).is_ok()
    }

    #[test]
    fn only_the_notice_comes_from_an_account_window() {
        let app = app();
        let account = window(&app, "account-1", "https://x.com/home");
        assert!(call(&account, "page_notice", "https://x.com/home"));
        assert!(call(&account, "page_notice", "https://discord.com/channels/@me"));
        assert!(!call(&account, "status", "https://x.com/home"));
        assert!(!call(&account, "plugin:event|listen", "https://x.com/home"));
        assert!(!call(&account, "page_notice", "http://x.com/home"));
        let guarded = || {
            super::tests::PROOF_COMMANDS
                .into_iter()
                .chain(super::tests::TELEGRAM_COMMANDS)
                .chain(super::tests::RELEASE_COMMANDS)
        };
        for command in guarded() {
            assert!(!call(&account, command, "https://x.com/home"), "{command}");
        }

        let main = window(&app, "main", "tauri://localhost");
        assert!(call(&main, "status", "tauri://localhost"));
        for command in guarded() {
            assert!(call(&main, command, "tauri://localhost"), "{command}");
        }
        assert!(!call(&main, "page_notice", "tauri://localhost"));
        assert!(
            !call(&main, "status", "https://x.com/home"),
            "the main capability has no remote URLs"
        );

        let other = window(&app, "other", "https://x.com/home");
        assert!(!call(&other, "page_notice", "https://x.com/home"));

        // A profile window loads the device page of GitHub or the page of an item. It can call no command.
        let home = super::home_label(panim_core::AccountId(1));
        for label in [super::LOGIN_LABEL, super::GIST_LABEL, "profile-link-1", home.as_str()] {
            assert!(label.starts_with(super::PROFILE_PREFIX));
            assert!(crate::unread::account_from_label(label).is_none());
            let profile = window(&app, label, "https://github.com/login/device");
            let commands = [
                "page_notice",
                "status",
                "answer",
                "github_login_poll",
                "plugin:event|listen",
            ];
            for command in commands.into_iter().chain(guarded()) {
                assert!(
                    !call(&profile, command, "https://github.com/login/device"),
                    "{label} {command}"
                );
                assert!(!call(&profile, command, "tauri://localhost"), "{label} {command}");
            }
        }
    }
}
