//! The work of the GitHub connector for the app, without Tauri.
//!
//! Each function obeys one rule: it does not hold the lock of the session while a request runs. It locks the
//! session, copies what the request needs, and unlocks. Then it sends the request. Then it locks the session again
//! and writes the result. Thus, a slow network does not stop the window.
//!
//! The functions get the HTTP client from a `Net`. The tests give them a fake client.

use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use panim_core::{Account, AccountId, ItemKey, Route};
use panim_github::{Client, GistCheck, GistResult, Http, Poll, Verdict};
use serde::Serialize;

use crate::session::{AppError, AppResult, LoginStep, LoginView, ProofView, Session, SyncResult};

/// Makes the HTTP client for a route.
pub trait Net: Send + Sync {
    fn http(&self, route: &Route) -> Result<Box<dyn Http>, panim_github::Error>;
}

/// The real network.
pub struct RealNet;

impl Net for RealNet {
    fn http(&self, route: &Route) -> Result<Box<dyn Http>, panim_github::Error> {
        Ok(Box::new(Client::new(route)?))
    }
}

/// The result of one poll of the device login.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginPoll {
    /// The person did not agree yet. Poll again after `interval` seconds.
    Pending { interval: u64 },
    /// The login is complete. This is the new account.
    Done(Account),
}

/// The time now, in milliseconds after 1970-01-01 00:00:00 UTC.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .unwrap_or(0)
}

fn locked<T>(session: &Mutex<Session>, f: impl FnOnce(&mut Session) -> AppResult<T>) -> AppResult<T> {
    let mut session = session.lock().map_err(|_| AppError::new("io", "poisoned"))?;
    f(&mut session)
}

/// Starts a device login. The device code stays in the session.
pub fn login_start(session: &Mutex<Session>, net: &dyn Net, now: Instant) -> AppResult<LoginView> {
    let route = locked(session, |s| s.route())?;
    let login = panim_github::start_login(net.http(&route)?.as_ref())?;
    locked(session, |s| s.github_login_begin(login, now))
}

/// Polls the device login one time. When the person agreed, this function adds the account.
pub fn login_poll(session: &Mutex<Session>, net: &dyn Net, now: Instant) -> AppResult<LoginPoll> {
    let (device_code, route) = match locked(session, |s| s.github_login_step(now))? {
        LoginStep::Wait { interval } => return Ok(LoginPoll::Pending { interval }),
        LoginStep::Poll { device_code, route } => (device_code, route),
    };
    let http = net.http(&route)?;
    // The person can cancel the login while the request runs. Then the login is not in work.
    let cancelled = || AppError::new("github_login_none", "the login was cancelled");
    let stop = |code: &'static str, detail: String| -> AppResult<LoginPoll> {
        locked(session, |s| {
            s.github_login_drop(Some(&device_code));
            Ok(())
        })?;
        Err(AppError::new(code, detail))
    };
    let poll = match panim_github::poll_login(http.as_ref(), &device_code) {
        Ok(poll) => poll,
        // GitHub refused the login, for example because the app has no device flow.
        Err(error @ panim_github::Error::Login(_)) => return stop("github_login_failed", error.to_string()),
        // A network failure does not stop the login. The next poll tries again.
        Err(error) => return Err(error.into()),
    };
    match poll {
        Poll::Pending => {
            let interval = locked(session, |s| Ok(s.github_login_interval(&device_code, None)))?;
            Ok(LoginPoll::Pending {
                interval: interval.ok_or_else(cancelled)?,
            })
        }
        Poll::SlowDown { interval } => {
            let interval = locked(session, |s| Ok(s.github_login_interval(&device_code, Some(interval))))?;
            Ok(LoginPoll::Pending {
                interval: interval.ok_or_else(cancelled)?,
            })
        }
        Poll::Expired => stop("github_login_expired", "the code expired".into()),
        Poll::Denied => stop("github_login_denied", "the person refused".into()),
        Poll::ScopeMissing { missing } => stop("github_scope_missing", missing.join(", ")),
        Poll::Done { token } => {
            // GitHub gives the token only one time. Try the next request a second time after a network failure.
            let user = panim_github::user(http.as_ref(), &token).or_else(|error| match error {
                panim_github::Error::Network(_) => panim_github::user(http.as_ref(), &token),
                other => Err(other),
            });
            let user = match user {
                Ok(user) => user,
                Err(error) => {
                    let failed: AppError = error.into();
                    return stop(failed.code, failed.detail);
                }
            };
            let account = locked(session, |s| s.github_login_finish(&device_code, token, &user))?;
            Ok(LoginPoll::Done(account))
        }
    }
}

/// Stops the device login.
pub fn login_cancel(session: &Mutex<Session>) -> AppResult<()> {
    locked(session, |s| {
        s.github_login_drop(None);
        Ok(())
    })
}

/// Does one sync of each joined GitHub account. Returns true if the timeline or the state of an account changed.
///
/// With `force`, the sync also tries the accounts that need a new login. `now` is in milliseconds after
/// 1970-01-01 00:00:00 UTC.
pub fn sync_all(session: &Mutex<Session>, net: &dyn Net, force: bool, now: u64) -> bool {
    let jobs = match session.lock() {
        Ok(mut session) => session.sync_jobs(force, now / 1000),
        Err(_) => return false,
    };
    let mut changed = false;
    for job in jobs {
        let result = match net.http(&job.route) {
            Ok(http) => SyncResult {
                watched: panim_github::watched(http.as_ref(), &job.token, job.watched_etag.as_deref()),
                notifications: panim_github::notifications(http.as_ref(), &job.token, job.last_modified.as_deref()),
                events: panim_github::received_events(http.as_ref(), &job.token, &job.login, job.etag.as_deref()),
            },
            Err(error) => SyncResult {
                watched: Err(error.clone()),
                notifications: Err(error.clone()),
                events: Err(error),
            },
        };
        if let Ok(mut session) = session.lock() {
            changed |= session.sync_done(&job, result, now);
        }
    }
    changed
}

/// Sets the read state of an item. For a notification that becomes read, also marks the thread as read on GitHub.
///
/// - A GitHub event has no read state. The function does nothing.
/// - GitHub cannot mark a thread as unread, so the function refuses that. Otherwise the next sync undoes it.
/// - The local state changes first. A sync does not undo it while the request to GitHub is in progress.
/// - If the request fails, the item stays read until the next sync. That sync shows the state of GitHub.
///
/// Returns true if the state of the account changed, because GitHub refused the token.
pub fn set_read(session: &Mutex<Session>, net: &dyn Net, key: &ItemKey, read: bool) -> AppResult<bool> {
    let target = locked(session, |s| {
        if !s.is_github(key.account) {
            s.set_read(key, read)?;
            return Ok(None);
        }
        if !panim_github::has_read_state(&key.source_id) {
            return Ok(None);
        }
        if !read {
            return Err(AppError::new(
                "github_no_unread",
                "GitHub cannot mark a thread as unread",
            ));
        }
        let target = s.item_target(key)?;
        s.set_read(key, true)?;
        s.local_read_start(key);
        Ok(Some(target))
    })?;
    let Some(target) = target else { return Ok(false) };
    let sent = net
        .http(&target.route)
        .and_then(|http| panim_github::mark_thread_read(http.as_ref(), &target.token, &key.source_id));
    locked(session, |s| {
        s.local_read_end(key);
        Ok(())
    })?;
    if sent == Err(panim_github::Error::BadToken) {
        return locked(session, |s| Ok(s.mark_needs_login(key.account)));
    }
    Ok(false)
}

/// Posts an answer to an item, as the account of the item. Returns the web page of the new comment.
pub fn answer(session: &Mutex<Session>, net: &dyn Net, key: &ItemKey, text: &str) -> AppResult<String> {
    if text.trim().is_empty() {
        return Err(AppError::new("answer_empty", "the answer has no text"));
    }
    let target = locked(session, |s| {
        if !s.is_github(key.account) {
            return Err(AppError::new("answer_no_target", "the account is not a GitHub account"));
        }
        s.item_target(key)
    })?;
    let to = panim_github::answer_target(&target.raw, &key.source_id)
        .ok_or_else(|| AppError::new("answer_no_target", "the item cannot get an answer"))?;
    let http = net.http(&target.route)?;
    match panim_github::post_comment(http.as_ref(), &target.token, &to, text) {
        Ok(url) => Ok(url),
        // GitHub answers 404 for a repository that the token cannot see, for example a private repository.
        Err(panim_github::Error::NotFound | panim_github::Error::Forbidden) => Err(AppError::new(
            "github_no_access",
            "the token cannot write in this repository",
        )),
        Err(error) => {
            if error == panim_github::Error::BadToken {
                locked(session, |s| Ok(s.mark_needs_login(key.account)))?;
            }
            Err(error.into())
        }
    }
}

/// Examines the account proofs in a gist. The request uses the route of the persona and no token.
pub fn check_proof(session: &Mutex<Session>, net: &dyn Net, gist: &str) -> AppResult<GistCheck> {
    let route = locked(session, |s| s.route())?;
    if panim_github::gist_id(gist).is_none() {
        return Err(AppError::new(
            "bad_gist",
            "the text is not a gist identifier or a gist URL",
        ));
    }
    Ok(panim_github::check_gist(net.http(&route)?.as_ref(), gist)?)
}

/// The result of the check of the person's own proof in a gist.
#[derive(Debug, Serialize)]
pub struct OwnCheck {
    /// The proof state of the account after the check. On a pass, the location is the gist.
    pub proof: ProofView,
    /// The result of the gist check.
    pub check: GistCheck,
}

/// Checks the person's own proof for a GitHub account in a gist, and keeps the gist as the location on a pass.
///
/// The check passes only if a block in the gist has the verdict "correct", names this persona and this account,
/// and is the current state of the account (`base/panim-proof-v1.md` §8.2). If not, the error code gives the reason.
pub fn check_own_proof(session: &Mutex<Session>, net: &dyn Net, account: AccountId, gist: &str) -> AppResult<OwnCheck> {
    let target = locked(session, |s| s.own_proof_target(account))?;
    let Some(id) = panim_github::gist_id(gist) else {
        return Err(AppError::new(
            "bad_gist",
            "the text is not a gist identifier or a gist URL",
        ));
    };
    let check = panim_github::check_gist(net.http(&target.route)?.as_ref(), gist)?;
    judge_own(&check, &target.persona.to_string(), &target.remote_id)?;
    let owner = check
        .owner
        .as_ref()
        .map(|owner| owner.login.clone())
        .unwrap_or_default();
    let location = format!("https://gist.github.com/{owner}/{id}");
    let proof = locked(session, |s| s.own_proof_passed(target.account, &location))?;
    Ok(OwnCheck { proof, check })
}

/// The rule of the own-proof check. `persona` is the address in lowercase, and `id` is the numeric id of the account.
pub(crate) fn judge_own(check: &GistCheck, persona: &str, id: &str) -> AppResult<()> {
    let fail = |code: &'static str| Err(AppError::new(code, "the check did not pass"));
    match check.result {
        GistResult::Secret => return fail("own_secret"),
        GistResult::NoBlock => return fail("own_no_block"),
        GistResult::Blocks => {}
    }
    let blocks = &check.blocks;
    let own_persona = |b: &&panim_github::BlockView| b.persona.as_deref() == Some(persona);
    let mine = blocks
        .iter()
        .filter(own_persona)
        .filter(|b| b.verdict == Verdict::Correct && b.id.as_deref() == Some(id));
    if let Some(current) = mine.clone().find(|b| b.current == Some(true)) {
        return match current.domain {
            Some("proof") => Ok(()),
            _ => fail("own_revoked"),
        };
    }
    if mine.count() > 0 {
        // The current block is not in the part of the gist that the check gives.
        return fail("own_cut");
    }
    let has = |verdict: Verdict| blocks.iter().filter(own_persona).any(|b| b.verdict == verdict);
    if has(Verdict::Correct) || has(Verdict::OtherAccount) {
        return fail("own_other_account");
    }
    if blocks.iter().any(|b| b.verdict == Verdict::BadSignature) {
        return fail("own_bad_signature");
    }
    if blocks.iter().any(|b| b.persona.is_some()) {
        return fail("own_other_persona");
    }
    fail("own_not_block")
}

/// The stored link of an item, if it is a page of GitHub. The app opens only such a link.
pub fn item_link(session: &Mutex<Session>, key: &ItemKey) -> AppResult<url::Url> {
    let link = locked(session, |s| s.item_link(key))?;
    let url = link
        .as_deref()
        .and_then(|link| url::Url::parse(link).ok())
        .filter(|url| {
            let host = url.host_str();
            url.scheme() == "https"
                && (host == Some("github.com") || host == Some("gist.github.com"))
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
        });
    url.ok_or_else(|| AppError::new("no_link", "the item has no page on GitHub"))
}

/// The key of an item from the arguments of a command.
pub fn key(account: u64, source_id: String) -> ItemKey {
    ItemKey {
        account: AccountId(account),
        source_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use panim_core::{Platform, ProxyEndpoint, ProxyKind, TimelineFilter};
    use panim_github::http::fake::FakeHttp;
    use panim_github::{HttpError, Request, Response};
    use panim_vault::ProfileArea;

    use crate::session::Strength;

    const DEVICE: &str = r#"{"device_code":"dc_secret_1","user_code":"WDJB-MJHT",
        "verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#;
    const TOKEN: &str =
        r#"{"access_token":"gho_secret_token","token_type":"bearer","scope":"notifications,public_repo"}"#;
    const USER: &str = r#"{"login":"octocat","id":583231}"#;
    const THREADS: &str = r#"[
      {"id":"101","updated_at":"2026-11-13T10:20:30Z",
       "subject":{"title":"The parser stops","type":"Issue","url":"https://api.github.com/repos/octo-org/parser/issues/5"},
       "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}},
      {"id":"103","updated_at":"2026-11-12T08:00:00Z",
       "subject":{"title":"v1.2.0","type":"Release","url":"https://api.github.com/repos/octo-org/parser/releases/44"},
       "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}}
    ]"#;
    const WATCHED: &str = r#"[{"full_name":"octocat/project"},{"full_name":"octo-org/parser"}]"#;
    const EVENTS: &str = r#"[
      {"id":"9001","type":"WatchEvent","actor":{"login":"alice"},"repo":{"name":"octocat/project"},
       "payload":{"action":"started"},"created_at":"2026-11-13T09:00:00Z"}
    ]"#;

    /// A fake network. Each request makes sure that the lock of the session is free.
    struct FakeNet {
        http: Arc<FakeHttp>,
        session: Arc<Mutex<Session>>,
        routes: Mutex<Vec<Route>>,
    }

    struct Checked(Arc<FakeHttp>, Arc<Mutex<Session>>);

    impl Http for Checked {
        fn send(&self, request: Request) -> Result<Response, HttpError> {
            assert!(self.1.try_lock().is_ok(), "a request ran with the lock of the session");
            self.0.send(request)
        }
    }

    impl Net for FakeNet {
        fn http(&self, route: &Route) -> Result<Box<dyn Http>, panim_github::Error> {
            self.routes.lock().unwrap().push(route.clone());
            Ok(Box::new(Checked(self.http.clone(), self.session.clone())))
        }
    }

    struct Fixture {
        _dirs: (tempfile::TempDir, tempfile::TempDir),
        data: PathBuf,
        session: Arc<Mutex<Session>>,
        http: Arc<FakeHttp>,
        net: FakeNet,
        start: Instant,
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
            let http = Arc::new(FakeHttp::new());
            Self {
                data: data.path().join("panim"),
                _dirs: (data, memory),
                net: FakeNet {
                    http: http.clone(),
                    session: session.clone(),
                    routes: Mutex::new(Vec::new()),
                },
                session,
                http,
                start: Instant::now(),
            }
        }

        fn at(&self, seconds: u64) -> Instant {
            self.start + Duration::from_secs(seconds)
        }

        /// Does a full device login and gives the new account.
        fn login(&self) -> Account {
            self.http.json(200, DEVICE).json(200, TOKEN).json(200, USER);
            login_start(&self.session, &self.net, self.at(0)).unwrap();
            match login_poll(&self.session, &self.net, self.at(5)).unwrap() {
                LoginPoll::Done(account) => account,
                other => panic!("{other:?}"),
            }
        }

        fn sync(&self, force: bool, now: u64) -> bool {
            sync_all(&self.session, &self.net, force, now)
        }

        fn with<T>(&self, f: impl FnOnce(&mut Session) -> T) -> T {
            f(&mut self.session.lock().unwrap())
        }

        fn items(&self) -> Vec<crate::session::ItemView> {
            self.with(|s| s.timeline(&TimelineFilter::default()).unwrap())
        }
    }

    /// The timeline items of a list of threads, as the connector makes them.
    fn thread_items(threads: &str) -> Vec<panim_core::NewItem> {
        let http = FakeHttp::new();
        http.response(200, &[], threads);
        let token = panim_core::SecretBytes::new(b"t".to_vec());
        panim_github::notifications(&http, &token, None).unwrap().items.unwrap()
    }

    fn code(error: AppResult<impl std::fmt::Debug>) -> &'static str {
        error.unwrap_err().code
    }

    #[test]
    fn device_login_adds_the_account() {
        let f = Fixture::new();
        f.http
            .json(200, DEVICE)
            .json(200, r#"{"error":"authorization_pending"}"#)
            .json(200, r#"{"error":"slow_down","interval":10}"#)
            .json(200, TOKEN)
            .json(200, USER);
        let view = login_start(&f.session, &f.net, f.at(0)).unwrap();
        assert_eq!(
            (view.user_code.as_str(), view.interval, view.expires_in),
            ("WDJB-MJHT", 5, 900)
        );
        let json = serde_json::to_string(&view).unwrap();
        assert!(
            !json.contains("dc_secret_1"),
            "the device code does not go to the user interface"
        );

        // A poll before the interval sends no request.
        assert_eq!(
            login_poll(&f.session, &f.net, f.at(1)).unwrap(),
            LoginPoll::Pending { interval: 5 }
        );
        assert_eq!(f.http.take_requests().len(), 1);
        assert_eq!(
            login_poll(&f.session, &f.net, f.at(5)).unwrap(),
            LoginPoll::Pending { interval: 5 }
        );
        assert_eq!(
            login_poll(&f.session, &f.net, f.at(10)).unwrap(),
            LoginPoll::Pending { interval: 10 }
        );
        let done = login_poll(&f.session, &f.net, f.at(20)).unwrap();
        let LoginPoll::Done(account) = done else {
            panic!("{done:?}")
        };
        assert_eq!(
            (account.platform, account.handle.as_str(), account.display_name.as_str()),
            (Platform::GitHub, "octocat", "octocat")
        );
        assert_eq!(account.remote_id.as_deref(), Some("583231"));
        assert!(account.joined.is_some() && account.browser.is_none());
        assert_eq!(f.http.answers_left(), 0);

        let requests = f.http.take_requests();
        let body = String::from_utf8(requests[0].body.clone().unwrap()).unwrap();
        assert!(body.contains("device_code=dc_secret_1"));
        assert_eq!(
            requests[3].header_value("Authorization"),
            Some("Bearer gho_secret_token")
        );

        // The login is complete, so a new poll finds no login.
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(30))), "github_login_none");

        // The token is sealed. It stays after a lock, and the file of the secrets does not show it.
        f.with(|s| s.lock().unwrap());
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
                let found = bytes.windows(16).any(|window| window == b"gho_secret_token");
                assert!(!found, "{}", path.display());
            }
        }
        assert!(files > 2);
        f.with(|s| s.unlock("passcode1").unwrap());
        let key = key(account.id.0, "n/1".into());
        assert_eq!(code(f.with(|s| s.item_target(&key).map(|_| ()))), "store");
        let jobs = f.with(|s| s.sync_jobs(false, 0));
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].token.expose(), b"gho_secret_token");
        assert_eq!(jobs[0].login, "octocat");
    }

    #[test]
    fn second_account_with_the_same_id_is_refused() {
        let f = Fixture::new();
        let first = f.login();
        let with_token = f.with(|s| s.secret_count());
        f.http.json(200, DEVICE).json(200, TOKEN).json(200, USER);
        login_start(&f.session, &f.net, f.at(100)).unwrap();
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(105))), "account_exists");
        assert_eq!(f.with(|s| s.accounts().unwrap()), vec![first.clone()]);
        assert_eq!(f.with(|s| s.secret_count()), with_token, "the second token is not kept");

        // The removal of the account removes its token. Then the same account can come again.
        f.with(|s| s.remove_account(first.id).unwrap());
        assert_eq!(f.with(|s| s.secret_count()), with_token - 1);
        assert!(f.with(|s| s.sync_jobs(true, 0)).is_empty());
        let again = f.login();
        assert_ne!(again.id, first.id);
        assert_eq!(f.with(|s| s.secret_count()), with_token);
    }

    #[test]
    fn login_stops_on_refusal_expiry_scope_and_cancel() {
        let f = Fixture::new();
        for (answer, expected) in [
            (r#"{"error":"access_denied"}"#, "github_login_denied"),
            (r#"{"error":"expired_token"}"#, "github_login_expired"),
            (
                r#"{"access_token":"gho_x","token_type":"bearer","scope":"notifications"}"#,
                "github_scope_missing",
            ),
            (r#"{"error":"device_flow_disabled"}"#, "github_login_failed"),
        ] {
            f.http.json(200, DEVICE).json(200, answer);
            login_start(&f.session, &f.net, f.at(0)).unwrap();
            let error = login_poll(&f.session, &f.net, f.at(5)).unwrap_err();
            assert_eq!(error.code, expected);
            assert!(!error.detail.contains("gho_x") && !error.detail.contains("dc_secret_1"));
            assert_eq!(code(login_poll(&f.session, &f.net, f.at(10))), "github_login_none");
        }
        assert!(f.with(|s| s.accounts().unwrap()).is_empty());

        // The code expires without an answer from GitHub.
        f.http.json(200, DEVICE);
        login_start(&f.session, &f.net, f.at(0)).unwrap();
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(900))), "github_login_expired");
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(901))), "github_login_none");

        // A cancel and a lock remove the device code.
        f.http.json(200, DEVICE).json(200, DEVICE);
        login_start(&f.session, &f.net, f.at(0)).unwrap();
        login_cancel(&f.session).unwrap();
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(5))), "github_login_none");
        login_start(&f.session, &f.net, f.at(0)).unwrap();
        f.with(|s| s.lock().unwrap());
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(5))), "locked");
        f.with(|s| s.unlock("passcode1").unwrap());
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(5))), "github_login_none");
        assert_eq!(code(login_start(&f.session, &f.net, f.at(0))), "network");
        assert_eq!(f.http.answers_left(), 0);
    }

    #[test]
    fn network_failure_does_not_stop_the_login() {
        let f = Fixture::new();
        f.http
            .json(200, DEVICE)
            .failure("no route")
            .json(200, TOKEN)
            .failure("reset")
            .json(200, USER);
        login_start(&f.session, &f.net, f.at(0)).unwrap();
        assert_eq!(code(login_poll(&f.session, &f.net, f.at(5))), "network");
        // The request for the user fails one time. The second try gets the user.
        assert!(matches!(
            login_poll(&f.session, &f.net, f.at(10)),
            Ok(LoginPoll::Done(_))
        ));
    }

    #[test]
    fn sync_puts_items_and_keeps_the_cursors() {
        let f = Fixture::new();
        let proxy = Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 9050,
        });
        f.with(|s| s.set_route(proxy.clone()).unwrap());
        let account = f.login();
        f.http.take_requests();
        assert_eq!(f.with(|s| s.sync_interval()), 60);

        f.http
            .response(200, &[("ETag", "\"w1\"")], WATCHED)
            .response(
                200,
                &[
                    ("Last-Modified", "Fri, 13 Nov 2026 10:20:30 GMT"),
                    ("X-Poll-Interval", "120"),
                ],
                THREADS,
            )
            .response(200, &[("ETag", "\"e1\"")], EVENTS);
        assert!(f.sync(false, 1_000_000));
        assert!(f.net.routes.lock().unwrap().iter().all(|route| *route == proxy));
        let requests = f.http.take_requests();
        assert_eq!(
            requests[0].url,
            "https://api.github.com/user/subscriptions?per_page=100"
        );
        let items = f.items();
        let view: Vec<_> = items
            .iter()
            .map(|i| {
                (
                    i.key.source_id.as_str(),
                    i.read,
                    i.has_read_state,
                    i.can_answer,
                    i.has_link,
                )
            })
            .collect();
        // The event has no read state. It shows as read.
        assert_eq!(
            view,
            vec![
                ("n/101", false, true, true, true),
                ("e/9001", true, false, false, true),
                ("n/103", false, true, false, true),
            ]
        );
        // An event comes with its kind, so the user interface can show the phrase in its language.
        let events: Vec<_> = items.iter().map(|i| i.event.clone()).collect();
        assert_eq!(
            events,
            vec![
                None,
                Some(crate::session::EventView {
                    kind: "starred",
                    repo: "octocat/project".into()
                }),
                None,
            ]
        );
        let json = serde_json::to_string(&items).unwrap();
        assert!(
            !json.contains("latest_comment_url") && !json.contains("https://"),
            "{json}"
        );
        // The count is the count of the inbox. The event does not count.
        let state = f.with(|s| s.sync_state().unwrap());
        assert_eq!(
            state[&account.id.0],
            crate::session::SyncView {
                unread: 2,
                unread_more: false,
                last_success: Some(1_000_000),
                error: None,
                needs_login: false
            }
        );
        assert_eq!(f.with(|s| s.sync_interval()), 120);
        let unread_only = TimelineFilter {
            unread_only: true,
            ..Default::default()
        };
        let unread: Vec<_> = f.with(|s| s.timeline(&unread_only).unwrap());
        assert_eq!(unread.len(), 2, "an event is not in the list of unread items");

        // The second sync sends the validators. GitHub answers 304, and nothing changes.
        f.http
            .response(304, &[], "")
            .response(304, &[], "")
            .response(304, &[], "");
        assert!(!f.sync(false, 1_100_000));
        let requests = f.http.take_requests();
        assert_eq!(requests[0].header_value("If-None-Match"), Some("\"w1\""));
        assert_eq!(
            requests[1].header_value("If-Modified-Since"),
            Some("Fri, 13 Nov 2026 10:20:30 GMT")
        );
        assert_eq!(requests[2].header_value("If-None-Match"), Some("\"e1\""));
        assert_eq!(
            requests[2].url,
            "https://api.github.com/users/octocat/received_events?per_page=50"
        );

        // A thread that is in the list is unread on GitHub. A local read without GitHub does not stay.
        let first = key(account.id.0, "n/101".into());
        f.with(|s| s.set_read(&first, true).unwrap());
        f.http
            .response(304, &[], "")
            .response(200, &[], THREADS)
            .response(304, &[], "");
        assert!(f.sync(false, 1_200_000));
        assert!(!f.items()[0].read);

        // The person reads thread 101 on github.com. It is not in the next complete list, so it becomes read.
        let only_103 = r#"[{"id":"103","updated_at":"2026-11-12T08:00:00Z",
           "subject":{"title":"v1.2.0","type":"Release","url":"https://api.github.com/repos/octo-org/parser/releases/44"},
           "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}}]"#;
        f.http
            .response(304, &[], "")
            .response(200, &[], only_103)
            .response(304, &[], "");
        assert!(f.sync(false, 1_250_000));
        assert!(f.items()[0].read);
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread, 1);

        // GitHub changes the thread. The item becomes unread again.
        let later = THREADS.replace("2026-11-13T10:20:30Z", "2026-11-13T11:00:00Z");
        f.http
            .response(304, &[], "")
            .response(200, &[], &later)
            .response(304, &[], "");
        assert!(f.sync(false, 1_270_000));
        assert!(!f.items()[0].read);

        // A lock removes the cursors and the state.
        f.http.take_requests();
        f.with(|s| s.lock().unwrap());
        assert!(!f.sync(false, 1_300_000), "a locked vault gets no sync");
        assert!(f.http.take_requests().is_empty());
        f.with(|s| s.unlock("passcode1").unwrap());
        let jobs = f.with(|s| s.sync_jobs(false, 0));
        assert!(jobs[0].last_modified.is_none() && jobs[0].etag.is_none() && jobs[0].watched_etag.is_none());
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].last_success, None);
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread, 2);
    }

    #[test]
    fn full_request_for_notifications() {
        let f = Fixture::new();
        f.login();
        let notifications = |f: &Fixture| {
            let requests = f.http.take_requests();
            requests[1].header_value("If-Modified-Since").map(str::to_string)
        };
        let lm = [("Last-Modified", "Fri, 13 Nov 2026 10:20:30 GMT")];
        f.http
            .response(200, &[], WATCHED)
            .response(200, &lm, "[]")
            .response(200, &[], "[]");
        f.sync(false, 1_000_000);
        assert_eq!(notifications(&f), None, "the first request is full");

        f.http
            .response(200, &[], WATCHED)
            .response(304, &[], "")
            .response(200, &[], "[]");
        f.sync(false, 1_060_000);
        assert!(notifications(&f).is_some(), "a usual request is conditional");

        // "Sync now" sends no validator.
        f.http
            .response(200, &[], WATCHED)
            .response(200, &lm, "[]")
            .response(200, &[], "[]");
        f.sync(true, 1_120_000);
        assert_eq!(notifications(&f), None);

        // A 304 does not count as a full list. After five minutes without a full list, the request is full.
        f.http
            .response(200, &[], WATCHED)
            .response(304, &[], "")
            .response(200, &[], "[]");
        f.sync(false, 1_400_000);
        assert!(notifications(&f).is_some());
        f.http
            .response(200, &[], WATCHED)
            .response(200, &lm, "[]")
            .response(200, &[], "[]");
        f.sync(false, 1_420_000);
        assert_eq!(notifications(&f), None);
    }

    #[test]
    fn a_cut_list_changes_no_read_state() {
        let f = Fixture::new();
        let account = f.login();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], "[]");
        f.sync(false, 1_000_000);
        f.http.take_requests();

        // Each page has a next page. Thread 101 is not in the cut list, but it stays unread.
        let next = |n: usize| format!("<https://api.github.com/notifications?page={n}>; rel=\"next\"");
        f.http.response(200, &[], WATCHED);
        for n in 1..=panim_github::sync::MAX_PAGES {
            f.http.response(200, &[("Link", &next(n + 1))], "[]");
        }
        f.http.response(200, &[], "[]");
        assert!(f.sync(true, 1_100_000), "the cut changes the state");
        let state = f.with(|s| s.sync_state().unwrap())[&account.id.0].clone();
        assert_eq!((state.unread, state.unread_more), (2, true));
        assert!(f.items().iter().all(|i| !i.read || !i.has_read_state));
    }

    #[test]
    fn a_local_read_in_flight_stays() {
        let f = Fixture::new();
        let account = f.login();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], "[]");
        f.sync(false, 1_000_000);
        let thread = key(account.id.0, "n/101".into());

        // A sync starts. Then the person reads the thread, and the request to GitHub ends.
        let job = f.with(|s| s.sync_jobs(false, 1100)).remove(0);
        f.http.response(205, &[], "");
        set_read(&f.session, &f.net, &thread, true).unwrap();
        // The list of the sync came before the read, so it has the thread. The item stays read.
        let result = SyncResult {
            watched: Err(panim_github::Error::Network("x".into())),
            notifications: Ok(panim_github::Fetched {
                items: Some(f.with(|_| thread_items(THREADS))),
                validator: None,
                poll_interval: None,
                complete: true,
            }),
            events: Ok(panim_github::Fetched::default()),
        };
        f.with(|s| s.sync_done(&job, result, 1_100_000));
        assert!(f.items()[0].read);

        // A sync that starts after the read sees the state of GitHub.
        f.http.take_requests();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], "[]");
        f.sync(false, 1_200_000);
        assert!(!f.items()[0].read, "the thread is still unread on GitHub");
    }

    #[test]
    fn read_changes_follow_github() {
        use crate::session::read_changes;
        let stored = vec![
            ("n/1".to_string(), false),
            ("n/2".to_string(), false),
            ("n/3".to_string(), true),
            ("n/4".to_string(), true),
            ("e/5".to_string(), false),
        ];
        let unread: BTreeSet<String> = ["n/1".to_string(), "n/3".to_string(), "n/4".to_string()].into();
        let changes = read_changes(&stored, &unread, |id| id == "n/4");
        assert_eq!(changes, vec![("n/2".to_string(), true), ("n/3".to_string(), false)]);
    }

    #[test]
    fn only_events_of_watched_repositories_stay() {
        let f = Fixture::new();
        let account = f.login();
        let events = r#"[
          {"id":"1","type":"WatchEvent","actor":{"login":"a"},"repo":{"name":"Octocat/Project"},
           "payload":{},"created_at":"2026-11-13T09:00:00Z"},
          {"id":"2","type":"WatchEvent","actor":{"login":"friend"},"repo":{"name":"friend/other"},
           "payload":{},"created_at":"2026-11-13T08:00:00Z"}
        ]"#;
        let ids = |f: &Fixture| f.items().into_iter().map(|i| i.key.source_id).collect::<Vec<_>>();

        // Without a list of watched repositories, the sync adds no event and records the error.
        f.http
            .failure("no route")
            .response(200, &[], THREADS)
            .response(200, &[], events);
        assert!(f.sync(false, 1_000_000));
        assert_eq!(ids(&f), vec!["n/101", "n/103"]);
        assert_eq!(
            f.with(|s| s.sync_state().unwrap())[&account.id.0].error,
            Some("network")
        );

        // With the list, only the event of the watched repository stays. The case of the name does not matter.
        f.http
            .response(200, &[], WATCHED)
            .response(304, &[], "")
            .response(200, &[], events);
        assert!(f.sync(false, 1_060_000));
        assert_eq!(ids(&f), vec!["n/101", "e/1", "n/103"]);

        // The list fails. The sync uses the last list.
        f.http
            .json(403, "{}")
            .response(304, &[], "")
            .response(200, &[("ETag", "\"x\"")], events);
        f.sync(false, 1_120_000);
        assert_eq!(ids(&f), vec!["n/101", "e/1", "n/103"]);

        // An old event of a repository that is not watched goes away after a good list. Threads stay.
        let old = panim_core::NewItem {
            source_id: "e/3".into(),
            author: "friend".into(),
            time: panim_core::Timestamp(1),
            text: "starred friend/other".into(),
            link: None,
            raw: br#"{"type":"WatchEvent","repo":{"name":"friend/other"}}"#.to_vec(),
        };
        f.with(|s| s.put_test_items(account.id, vec![old]));
        assert_eq!(ids(&f).len(), 4);
        f.http
            .response(200, &[], WATCHED)
            .response(304, &[], "")
            .response(304, &[], "");
        assert!(f.sync(false, 1_180_000));
        assert_eq!(ids(&f), vec!["n/101", "e/1", "n/103"]);

        // The person unwatches the repository. Its events go at the next sync.
        f.http
            .response(200, &[], "[]")
            .response(304, &[], "")
            .response(304, &[], "");
        assert!(f.sync(false, 1_240_000));
        assert_eq!(ids(&f), vec!["n/101", "n/103"]);
        assert!(
            f.with(|s| s.sync_jobs(false, 1300))[0].etag.is_none(),
            "a new list gets all events again"
        );

        // A bad token in the list still marks the account.
        f.http.json(401, "{}").failure("no route").failure("no route");
        f.sync(false, 1_300_000);
        assert!(f.with(|s| s.sync_state().unwrap())[&account.id.0].needs_login);
    }

    #[test]
    fn sync_errors_mark_the_account() {
        let f = Fixture::new();
        let account = f.login();
        let state = |f: &Fixture| f.with(|s| s.sync_state().unwrap())[&account.id.0].clone();

        // A network failure.
        f.http.failure("no route").failure("no route").failure("no route");
        assert!(f.sync(false, 1_000_000), "the state of the account changed");
        assert_eq!((state(&f).error, state(&f).needs_login), (Some("network"), false));
        f.http.failure("no route").failure("no route").failure("no route");
        assert!(!f.sync(false, 1_060_000), "the same error is not a change");

        // The rate limit. The sync waits until the limit ends.
        f.http
            .response(200, &[], WATCHED)
            .response(
                403,
                &[("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", "2000")],
                "{}",
            )
            .response(304, &[], "");
        assert!(f.sync(false, 1_120_000));
        assert_eq!(state(&f).error, Some("github_rate_limit"));
        f.http.take_requests();
        assert!(!f.sync(true, 1_999_000));
        assert!(f.http.take_requests().is_empty(), "no request before the limit ends");

        // GitHub refuses the token. The account needs a new login, and the usual sync does not try it again.
        f.http.json(401, "{}").json(401, "{}").json(401, "{}");
        assert!(f.sync(false, 2_000_000));
        assert_eq!(
            (state(&f).error, state(&f).needs_login),
            (Some("github_bad_token"), true)
        );
        assert!(!f.sync(false, 2_060_000));
        assert_eq!(f.http.take_requests().len(), 3);
        assert_eq!(f.with(|s| s.accounts().unwrap()).len(), 1, "the account stays");
        assert_eq!(f.with(|s| s.sync_jobs(true, 3000)).len(), 1, "the token stays");

        // "Sync now" tries again. A good result clears the mark.
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], "[]")
            .response(200, &[], "[]");
        assert!(f.sync(true, 3_000_000));
        assert_eq!(
            state(&f),
            crate::session::SyncView {
                unread: 0,
                unread_more: false,
                last_success: Some(3_000_000),
                error: None,
                needs_login: false
            }
        );

        // The session ignores a result from before a lock.
        let job = f.with(|s| s.sync_jobs(false, 4000)).remove(0);
        f.with(|s| {
            s.lock().unwrap();
            s.unlock("passcode1").unwrap();
        });
        f.http.response(200, &[], THREADS);
        let fetched = panim_github::notifications(f.http.as_ref(), &job.token, None);
        let result = SyncResult {
            watched: Err(panim_github::Error::NotFound),
            notifications: fetched,
            events: Err(panim_github::Error::NotFound),
        };
        assert!(!f.with(|s| s.sync_done(&job, result, 4_000_000)));
        assert!(f.items().is_empty());
    }

    #[test]
    fn read_state_goes_to_github() {
        let f = Fixture::new();
        let account = f.login();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], EVENTS);
        f.sync(false, 1_000_000);
        f.http.take_requests();
        let thread = key(account.id.0, "n/101".into());
        let event = key(account.id.0, "e/9001".into());

        f.http.response(205, &[], "");
        assert_eq!(set_read(&f.session, &f.net, &thread, true), Ok(false));
        let requests = f.http.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://api.github.com/notifications/threads/101");
        assert_eq!(requests[0].method, panim_github::Method::Patch);

        // An event has no read state. GitHub cannot mark a thread as unread, so PANIM refuses it. No request.
        assert_eq!(set_read(&f.session, &f.net, &event, false), Ok(false));
        assert_eq!(code(set_read(&f.session, &f.net, &thread, false)), "github_no_unread");
        assert!(f.http.take_requests().is_empty());
        let read: Vec<_> = f.items().iter().map(|i| (i.key.source_id.clone(), i.read)).collect();
        assert_eq!(
            read,
            vec![("n/101".into(), true), ("e/9001".into(), true), ("n/103".into(), false)]
        );

        // A network failure does not undo the local state.
        f.http.failure("no route");
        assert_eq!(set_read(&f.session, &f.net, &thread, true), Ok(false));
        assert!(f.items()[0].read);

        // A refused token marks the account.
        let other = key(account.id.0, "n/103".into());
        f.http.json(401, "{}");
        assert_eq!(set_read(&f.session, &f.net, &other, true), Ok(true));
        assert!(f.with(|s| s.sync_state().unwrap())[&account.id.0].needs_login);

        let missing = key(account.id.0, "n/999".into());
        assert_eq!(code(set_read(&f.session, &f.net, &missing, true)), "store");
    }

    #[test]
    fn answers() {
        let f = Fixture::new();
        let account = f.login();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], EVENTS);
        f.sync(false, 1_000_000);
        f.http.take_requests();
        let issue = key(account.id.0, "n/101".into());
        let release = key(account.id.0, "n/103".into());

        assert_eq!(code(answer(&f.session, &f.net, &issue, "  \n ")), "answer_empty");
        assert_eq!(code(answer(&f.session, &f.net, &release, "Thanks")), "answer_no_target");
        assert!(f.http.take_requests().is_empty());

        f.http.json(
            201,
            r#"{"html_url":"https://github.com/octo-org/parser/issues/5#issuecomment-3"}"#,
        );
        assert_eq!(
            answer(&f.session, &f.net, &issue, "Thanks").unwrap(),
            "https://github.com/octo-org/parser/issues/5#issuecomment-3"
        );
        let requests = f.http.take_requests();
        assert_eq!(
            requests[0].url,
            "https://api.github.com/repos/octo-org/parser/issues/5/comments"
        );
        assert_eq!(requests[0].body.as_deref(), Some(br#"{"body":"Thanks"}"#.as_slice()));
        assert_eq!(
            requests[0].header_value("Authorization"),
            Some("Bearer gho_secret_token")
        );

        f.http
            .json(404, "{}")
            .json(403, "{}")
            .json(401, "{}")
            .failure("no route");
        assert_eq!(code(answer(&f.session, &f.net, &issue, "x")), "github_no_access");
        assert_eq!(code(answer(&f.session, &f.net, &issue, "x")), "github_no_access");
        assert_eq!(code(answer(&f.session, &f.net, &issue, "x")), "github_bad_token");
        assert!(f.with(|s| s.sync_state().unwrap())[&account.id.0].needs_login);
        assert_eq!(code(answer(&f.session, &f.net, &issue, "x")), "network");
    }

    #[test]
    fn each_event_kind_has_a_text() {
        // `pnpm check` makes sure that each language has each key. This test makes sure that the key exists.
        let texts = include_str!("../../src/lib/i18n.svelte.ts");
        for kind in panim_github::event_kinds() {
            assert!(
                texts.contains(&format!("\n  event_{kind}: ")),
                "no text for the event kind {kind}"
            );
        }
    }

    #[test]
    fn home_page_of_a_github_account() {
        let f = Fixture::new();
        let account = f.login();
        let (url, title) = f.with(|s| s.home_page(account.id)).unwrap();
        assert_eq!(url.as_str(), "https://github.com/");
        assert_eq!(title, "octocat · octocat");

        // A browser-view account has its own window, and no home page.
        let x = f.with(|s| s.add_browser_account(Platform::X, "", "me")).unwrap();
        assert_eq!(code(f.with(|s| s.home_page(x.id))), "no_window");
        assert_eq!(code(f.with(|s| s.home_page(AccountId(999)))), "store");
        f.with(|s| s.lock().unwrap());
        assert_eq!(code(f.with(|s| s.home_page(account.id))), "locked");
    }

    #[test]
    fn links_and_proofs() {
        let f = Fixture::new();
        let account = f.login();
        f.http
            .response(200, &[], WATCHED)
            .response(200, &[], THREADS)
            .response(200, &[], EVENTS);
        f.sync(false, 1_000_000);
        f.http.take_requests();
        let issue = key(account.id.0, "n/101".into());
        assert_eq!(
            item_link(&f.session, &issue).unwrap().as_str(),
            "https://github.com/octo-org/parser/issues/5"
        );
        assert_eq!(code(item_link(&f.session, &key(account.id.0, "n/999".into()))), "store");

        // The check of a proof uses no token.
        f.http.json(
            200,
            r#"{"public":true,"owner":{"login":"octocat","id":583231},"files":{"a.txt":{"content":"No block."}}}"#,
        );
        let check = check_proof(
            &f.session,
            &f.net,
            "https://gist.github.com/octocat/aa5a315d61ae9438b18d",
        )
        .unwrap();
        assert_eq!(check.result, panim_github::GistResult::NoBlock);
        let requests = f.http.take_requests();
        assert_eq!(requests[0].url, "https://api.github.com/gists/aa5a315d61ae9438b18d");
        assert_eq!(requests[0].header_value("Authorization"), None);
        assert_eq!(
            code(check_proof(&f.session, &f.net, "https://example.org/x")),
            "bad_gist"
        );
        f.http.json(404, "{}");
        assert_eq!(
            code(check_proof(&f.session, &f.net, "aa5a315d61ae9438b18d")),
            "github_not_found"
        );
        f.with(|s| s.lock().unwrap());
        assert_eq!(code(check_proof(&f.session, &f.net, "aa5a315d61ae9438b18d")), "locked");
    }

    /// A gist with one file, as the API gives it.
    fn gist_json(public: bool, owner_login: &str, owner_id: u64, content: &str) -> String {
        serde_json::json!({
            "public": public,
            "owner": { "login": owner_login, "id": owner_id },
            "files": { "proof.txt": { "truncated": false, "content": content } },
        })
        .to_string()
    }

    const GIST: &str = "https://gist.github.com/octocat/aa5a315d61ae9438b18d";
    const NOV_12: u64 = 1_794_441_600;

    #[test]
    fn own_proof_in_a_gist() {
        let f = Fixture::new();
        let account = f.login();
        let made = f.with(|s| s.make_proof(account.id, NOV_12, false)).unwrap();
        let block = made.block.unwrap();
        assert!(block.contains("\nplatform github\naccount 583231 octocat\n"));
        assert!(!made.id_needed && !made.identity_editable);
        let own = |gist: &str| check_own_proof(&f.session, &f.net, account.id, gist);

        // Each failure gives its reason, and keeps no location.
        let failures = [
            (gist_json(false, "octocat", 583231, &block), "own_secret"),
            (gist_json(true, "octocat", 583231, "No block."), "own_no_block"),
            (gist_json(true, "someone", 42, &block), "own_other_account"),
            (
                gist_json(
                    true,
                    "octocat",
                    583231,
                    &block.replace("date 2026-11-12", "date 2026-11-13"),
                ),
                "own_bad_signature",
            ),
            (
                gist_json(true, "octocat", 583231, crate::session::tests::EXAMPLE_PROOF),
                "own_other_persona",
            ),
            (
                gist_json(true, "octocat", 583231, "panim/1-proof\nshort"),
                "own_not_block",
            ),
        ];
        for (body, reason) in failures {
            f.http.json(200, &body);
            assert_eq!(code(own(GIST)), reason);
        }
        f.http.take_requests();
        assert_eq!(code(own("https://example.org/x")), "bad_gist");
        assert!(f.http.take_requests().is_empty());
        assert_eq!(f.with(|s| s.proofs()).unwrap()[0].location, None);

        // A pass keeps the gist as the location. The request has no token.
        f.http.json(
            200,
            &gist_json(true, "octocat", 583231, &format!("My proof\n\n{block}\n")),
        );
        let passed = own("aa5a315d61ae9438b18d").unwrap();
        assert_eq!(passed.proof.location.as_deref(), Some(GIST));
        assert_eq!(passed.check.blocks[0].verdict, Verdict::Correct);
        assert_eq!(f.http.take_requests()[0].header_value("Authorization"), None);
        assert_eq!(f.with(|s| s.proofs()).unwrap()[0].location.as_deref(), Some(GIST));

        // A browser-view account has no gist check. An account without a kept proof has nothing to check.
        let x = f.with(|s| s.add_browser_account(Platform::X, "", "me")).unwrap();
        assert_eq!(
            code(check_own_proof(&f.session, &f.net, x.id, GIST)),
            "proof_not_github"
        );
        f.with(|s| s.remove_account(account.id)).unwrap();
        let account = f.login();
        assert_eq!(code(own_again(&f, account.id)), "proof_none");
        f.with(|s| s.lock().unwrap());
        assert_eq!(code(own_again(&f, account.id)), "locked");
    }

    fn own_again(f: &Fixture, id: AccountId) -> AppResult<OwnCheck> {
        check_own_proof(&f.session, &f.net, id, GIST)
    }

    #[test]
    fn own_check_needs_the_current_state() {
        let persona = "0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f";
        let view = |domain: &'static str, current: Option<bool>| panim_github::BlockView {
            file: "a.txt".into(),
            verdict: Verdict::Correct,
            persona: Some(persona.into()),
            domain: Some(domain),
            date: Some("2026-11-12".into()),
            id: Some("583231".into()),
            handle: Some("octocat".into()),
            platform: Some("github"),
            current,
        };
        let check = |blocks| GistCheck {
            owner: None,
            result: GistResult::Blocks,
            blocks,
            cut: false,
        };
        let judge = |c: &GistCheck| judge_own(c, persona, "583231").map_err(|e| e.code);
        assert_eq!(judge(&check(vec![view("proof", Some(true))])), Ok(()));
        assert_eq!(
            judge(&check(vec![view("proof", Some(false)), view("revoke", Some(true))])),
            Err("own_revoked")
        );
        assert_eq!(judge(&check(vec![view("proof", Some(false))])), Err("own_cut"));
        assert_eq!(
            judge_own(&check(vec![view("proof", Some(true))]), persona, "1").map_err(|e| e.code),
            Err("own_other_account")
        );
    }

    #[test]
    fn many_events_do_not_hide_the_unread_threads() {
        let f = Fixture::new();
        let account = f.login();
        let threads = thread_items(THREADS);
        let newest = threads.iter().map(|item| item.time.0).max().unwrap();
        // 600 events are more recent than the two threads. Events have no read state, so they stay unread.
        let events = (0..600).map(|n| panim_core::NewItem {
            source_id: format!("e/{}", 10_000 + n),
            author: "alice".into(),
            time: panim_core::Timestamp(newest + 1 + n),
            text: "starred octocat/project".into(),
            link: None,
            raw: br#"{"type":"WatchEvent","repo":{"name":"octocat/project"}}"#.to_vec(),
        });
        f.with(|s| s.put_test_items(account.id, threads.into_iter().chain(events).collect()));

        // The window asks for the timeline with the limit of 500.
        let filter = |unread_only| TimelineFilter {
            accounts: Some(vec![account.id]),
            unread_only,
            limit: Some(500),
            ..Default::default()
        };
        let unread = f.with(|s| s.timeline(&filter(true)).unwrap());
        let ids: Vec<_> = unread.iter().map(|item| item.key.source_id.as_str()).collect();
        assert_eq!(ids, ["n/101", "n/103"], "the older unread threads show");

        // The normal timeline keeps the limit. The most recent items are first, so it has only events.
        let all = f.with(|s| s.timeline(&filter(false)).unwrap());
        assert_eq!(all.len(), 500);
        assert!(all.iter().all(|item| item.key.source_id.starts_with("e/")));
        assert_eq!(all[0].key.source_id, "e/10599");

        // A smaller limit applies to the unread items too.
        let one = TimelineFilter {
            limit: Some(1),
            ..filter(true)
        };
        let first = f.with(|s| s.timeline(&one).unwrap());
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].key.source_id, "n/101");

        // The count is the count of the inbox. The events do not count.
        assert_eq!(f.with(|s| s.sync_state().unwrap())[&account.id.0].unread, 2);
    }
}
