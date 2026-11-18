//! The one receive loop for all the TDLib clients of the process.
//!
//! TDLib has one queue for the answers and the updates of all its clients, and only one thread can read it. The
//! runtime has that thread. It gives each value to the `Client` of its account, sends the requests that the client
//! makes, and gives the events to the app through the sink.
//!
//! - The loop starts with the first client and stops when no client is open.
//! - The runtime holds its lock only while a client reads a value. It does not hold the lock while it calls the
//!   sink. Thus, the sink can lock the session of the app.
//! - Before the first client, the runtime stops the log of TDLib.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::Error;
use crate::client::{Client, ClientConfig, Event, Out};
use crate::login::{AuthState, td_error};
use crate::start::log_off;
use crate::td::Td;

/// The app gets each event with the key of its client.
pub type Sink = Box<dyn Fn(u64, Event) + Send + Sync>;

/// How long one call of `receive` waits.
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(250);

/// How a client ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// `close`: the database stays, so the login stays.
    Close,
    /// `logOut`: Telegram ends the session, and TDLib deletes its database.
    LogOut,
}

struct Slot {
    key: u64,
    client: Client,
    closed: bool,
}

#[derive(Default)]
struct Inner {
    slots: HashMap<i32, Slot>,
    next_extra: u64,
    waiters: HashMap<u64, mpsc::Sender<Value>>,
    looping: bool,
}

impl Inner {
    fn id_of(&self, key: u64) -> Option<i32> {
        self.slots.iter().find(|(_, slot)| slot.key == key).map(|(id, _)| *id)
    }
}

pub struct Runtime {
    td: Arc<dyn Td>,
    sink: Sink,
    inner: Mutex<Inner>,
    changed: Condvar,
}

impl Runtime {
    /// Makes the runtime and stops the log of TDLib. Make it before the first client.
    pub fn new(td: Arc<dyn Td>, sink: Sink) -> Arc<Self> {
        for request in log_off() {
            td.execute(&request);
        }
        Arc::new(Self {
            td,
            sink,
            inner: Mutex::new(Inner::default()),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic in a different thread does not make the data incorrect: each change is one step.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts a client for an account. `key` names the client in all other calls.
    ///
    /// The first request of the client is the proxy request. With a proxy that has a host name, the function gives
    /// an error and makes no client.
    pub fn open(self: &Arc<Self>, key: u64, config: ClientConfig) -> Result<(), Error> {
        let (client, first) = Client::new(config)?;
        let start_loop = {
            let mut inner = self.lock();
            if let Some(old) = inner.id_of(key) {
                inner.slots.remove(&old);
            }
            let id = self.td.create_client();
            inner.slots.insert(
                id,
                Slot {
                    key,
                    client,
                    closed: false,
                },
            );
            for request in &first {
                self.td.send(id, request);
            }
            !std::mem::replace(&mut inner.looping, true)
        };
        if start_loop {
            let runtime = self.clone();
            std::thread::spawn(move || runtime.run());
        }
        Ok(())
    }

    /// The receive loop. It stops when no client is open.
    fn run(&self) {
        loop {
            {
                let mut inner = self.lock();
                if inner.slots.values().all(|slot| slot.closed) {
                    inner.looping = false;
                    return;
                }
            }
            self.pump(RECEIVE_TIMEOUT);
        }
    }

    /// Reads one value of TDLib, if one comes in the time limit, and runs the guard of each client.
    fn pump(&self, timeout: Duration) {
        let value = self.td.receive(timeout);
        let now = Instant::now();
        let mut sends: Vec<(i32, Value)> = Vec::new();
        let mut events: Vec<(u64, Event)> = Vec::new();
        {
            let mut inner = self.lock();
            if let Some(value) = value {
                let id = value["@client_id"].as_i64().unwrap_or(-1) as i32;
                if let Some(extra) = value["@extra"].as_u64() {
                    // The answer to a request of the app.
                    if let Some(waiter) = inner.waiters.remove(&extra) {
                        let _ = waiter.send(value);
                    }
                } else if let Some(slot) = inner.slots.get_mut(&id) {
                    let out = slot.client.handle(&value, now);
                    if *slot.client.auth() == AuthState::Closed {
                        slot.closed = true;
                    }
                    let key = slot.key;
                    sends.extend(out.requests.into_iter().map(|request| (id, request)));
                    events.extend(out.events.into_iter().map(|event| (key, event)));
                }
            }
            for (id, slot) in inner.slots.iter_mut().filter(|(_, slot)| !slot.closed) {
                let out = slot.client.tick(now);
                sends.extend(out.requests.into_iter().map(|request| (*id, request)));
            }
            self.changed.notify_all();
        }
        for (id, request) in &sends {
            self.td.send(*id, request);
        }
        for (key, event) in events {
            (self.sink)(key, event);
        }
    }

    /// Runs a function on the client of an account, sends its requests and gives its events to the sink.
    pub fn with<R>(&self, key: u64, f: impl FnOnce(&mut Client) -> (R, Out)) -> Result<R, Error> {
        let (id, result, out) = {
            let mut inner = self.lock();
            let id = inner.id_of(key).ok_or(Error::NoClient)?;
            let slot = inner
                .slots
                .get_mut(&id)
                .filter(|slot| !slot.closed)
                .ok_or(Error::NoClient)?;
            let (result, out) = f(&mut slot.client);
            (id, result, out)
        };
        for request in &out.requests {
            self.td.send(id, request);
        }
        for event in out.events {
            (self.sink)(key, event);
        }
        Ok(result)
    }

    /// Sends a request for an account and waits for the answer. An answer of the type `error` becomes an error.
    ///
    /// Do not hold the lock of the session of the app during this call.
    pub fn call(&self, key: u64, mut request: Value, timeout: Duration) -> Result<Value, Error> {
        let (sender, receiver) = mpsc::channel();
        let (id, extra) = {
            let mut inner = self.lock();
            let id = inner.id_of(key).ok_or(Error::NoClient)?;
            if inner.slots.get(&id).is_none_or(|slot| slot.closed) {
                return Err(Error::NoClient);
            }
            inner.next_extra += 1;
            let extra = inner.next_extra;
            inner.waiters.insert(extra, sender);
            (id, extra)
        };
        request["@extra"] = extra.into();
        self.td.send(id, &request);
        let answer = receiver.recv_timeout(timeout);
        self.lock().waiters.remove(&extra);
        let answer = answer.map_err(|_| Error::Timeout)?;
        if answer["@type"] == "error" {
            return Err(Error::Td(td_error(&answer)));
        }
        Ok(answer)
    }

    /// The authorization state of an account.
    pub fn auth(&self, key: u64) -> Option<AuthState> {
        let inner = self.lock();
        let id = inner.id_of(key)?;
        inner.slots.get(&id).map(|slot| slot.client.auth().clone())
    }

    /// Waits until the authorization state of an account agrees with `accept`. Returns the last state.
    pub fn wait_auth(&self, key: u64, accept: impl Fn(&AuthState) -> bool, timeout: Duration) -> Option<AuthState> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.lock();
        loop {
            let id = inner.id_of(key)?;
            let state = inner.slots.get(&id)?.client.auth().clone();
            let left = deadline.saturating_duration_since(Instant::now());
            if accept(&state) || left.is_zero() {
                return Some(state);
            }
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// The number of authorization updates of an account until now. `None` if the account has no client.
    pub fn auth_serial(&self, key: u64) -> Option<u64> {
        let inner = self.lock();
        let id = inner.id_of(key)?;
        inner.slots.get(&id).map(|slot| slot.client.auth_serial())
    }

    /// Waits until the account has an authorization update after `serial`. Returns the state then, or the state at the
    /// end of the time limit.
    pub fn wait_auth_after(&self, key: u64, serial: u64, timeout: Duration) -> Option<(AuthState, bool)> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.lock();
        loop {
            let id = inner.id_of(key)?;
            let client = &inner.slots.get(&id)?.client;
            let (state, new) = (client.auth().clone(), client.auth_serial() > serial);
            let left = deadline.saturating_duration_since(Instant::now());
            if new || left.is_zero() {
                return Some((state, new));
            }
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// Gives a client a new key. The login uses one key, and the account that it makes uses a different key.
    pub fn rekey(&self, old: u64, new: u64) -> bool {
        let mut inner = self.lock();
        let Some(id) = inner.id_of(old) else { return false };
        if let Some(slot) = inner.slots.get_mut(&id) {
            slot.key = new;
        }
        true
    }

    /// The keys of the open clients.
    pub fn keys(&self) -> Vec<u64> {
        let inner = self.lock();
        inner
            .slots
            .values()
            .filter(|slot| !slot.closed)
            .map(|slot| slot.key)
            .collect()
    }

    /// Ends the client of an account and waits for the state "closed". Returns true if the client ended as asked.
    ///
    /// After the time limit of `LogOut`, the function closes the client without a logout and returns false. Then the
    /// session is possibly still open on Telegram.
    pub fn end(&self, key: u64, how: End, timeout: Duration) -> bool {
        let Some(id) = self.lock().id_of(key) else { return true };
        let request = match how {
            End::Close => "close",
            End::LogOut => "logOut",
        };
        self.td.send(id, &json!({"@type": request, "@extra": "panim:end"}));
        let mut done = self.wait_closed(id, timeout);
        if !done && how == End::LogOut {
            self.td.send(id, &json!({"@type": "close", "@extra": "panim:end"}));
            self.wait_closed(id, Duration::from_secs(3).min(timeout));
            done = false;
        }
        self.lock().slots.remove(&id);
        done
    }

    fn wait_closed(&self, id: i32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut inner = self.lock();
        loop {
            let closed = inner.slots.get(&id).is_none_or(|slot| slot.closed);
            let left = deadline.saturating_duration_since(Instant::now());
            if closed || left.is_zero() {
                return closed;
            }
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }

    /// Closes all the clients, for a lock or an exit. Returns true if each client reached the state "closed".
    ///
    /// The function sends `close` to all the clients first, and then waits. Thus, the time limit is for all of them
    /// together.
    pub fn end_all(&self, timeout: Duration) -> bool {
        let ids: Vec<i32> = self.lock().slots.keys().copied().collect();
        for id in &ids {
            self.td.send(*id, &json!({"@type": "close", "@extra": "panim:end"}));
        }
        let deadline = Instant::now() + timeout;
        let mut all = true;
        for id in ids {
            all &= self.wait_closed(id, deadline.saturating_duration_since(Instant::now()));
            self.lock().slots.remove(&id);
        }
        all
    }

    /// Removes a client that TDLib closed, for example after Telegram ended the session.
    pub fn forget(&self, key: u64) {
        let mut inner = self.lock();
        if let Some(id) = inner.id_of(key) {
            inner.slots.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use panim_core::Route;

    use super::*;
    use crate::client::tests::{config, new_chat, tor};
    use crate::td::fake::FakeTd;

    type Events = Arc<Mutex<Vec<(u64, Event)>>>;

    fn runtime(td: &Arc<FakeTd>) -> (Arc<Runtime>, Events) {
        let events: Events = Arc::default();
        let sink = events.clone();
        let runtime = Runtime::new(
            td.clone(),
            Box::new(move |key, event| sink.lock().unwrap().push((key, event))),
        );
        (runtime, events)
    }

    fn auth(state: &str) -> Value {
        json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": state}})
    }

    /// A fake TDLib that acts as a client with a login: it asks for the parameters and then is ready.
    fn logged_in(td: &FakeTd) {
        td.respond(|_, request| match request["@type"].as_str().unwrap() {
            "addProxy" | "disableProxy" => vec![auth("authorizationStateWaitTdlibParameters")],
            "setTdlibParameters" => vec![json!({"@type": "ok", "@answer": true}), auth("authorizationStateReady")],
            "getMe" => vec![
                json!({"@type": "user", "id": 777001, "first_name": "Sakura", "last_name": "",
                "usernames": {"active_usernames": ["sakura_r"]}, "@answer": true}),
            ],
            "getScopeNotificationSettings" => {
                vec![json!({"@type": "scopeNotificationSettings", "mute_for": 0, "@answer": true})]
            }
            "loadChats" => vec![json!({"@type": "error", "code": 404, "message": "Not Found", "@answer": true})],
            "close" => vec![auth("authorizationStateClosing"), auth("authorizationStateClosed")],
            "logOut" => vec![auth("authorizationStateLoggingOut"), auth("authorizationStateClosed")],
            "getChatHistory" => vec![json!({"@type": "messages", "messages": [], "@answer": true})],
            "sendMessage" => vec![
                json!({"@type": "error", "code": 400, "message": "Have no write access to the chat", "@answer": true}),
            ],
            _ => vec![],
        });
    }

    fn wait_for(what: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !what() {
            assert!(Instant::now() < deadline, "the condition did not become true");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_log_stops_before_the_first_client_and_the_proxy_goes_first() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, events) = runtime(&td);
        runtime.open(7, config(tor())).unwrap();
        assert_eq!(
            runtime.wait_auth(7, |state| *state == AuthState::Ready, Duration::from_secs(5)),
            Some(AuthState::Ready)
        );
        wait_for(|| {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, e)| matches!(e, Event::Loaded { .. }))
        });

        let log = td.log();
        assert_eq!(
            &log[..5],
            [
                "execute setLogVerbosityLevel",
                "execute setLogStream",
                "create 1",
                "send 1 addProxy",
                "send 1 setTdlibParameters"
            ]
        );
        let executed = td.executed();
        assert_eq!(executed[0]["new_verbosity_level"], 0);
        assert_eq!(executed[1]["log_stream"]["@type"], "logStreamEmpty");
        // No request goes to the client before the proxy request.
        let position = |name: &str| log.iter().position(|line| line.ends_with(name)).unwrap();
        assert!(position("addProxy") < position("setTdlibParameters"));
        assert!(position("setTdlibParameters") < position("getMe"));

        let events = events.lock().unwrap();
        assert!(events.contains(&(7, Event::Auth(AuthState::Ready))));
        assert!(
            events
                .iter()
                .any(|(key, e)| *key == 7 && matches!(e, Event::Me(me) if me.id == 777001))
        );
        // The sink of this test locks `events`. Free the lock before the clients close.
        drop(events);
        assert!(runtime.end_all(Duration::from_secs(5)));
    }

    #[test]
    fn a_direct_route_sends_disable_proxy_first() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, _) = runtime(&td);
        runtime.open(1, config(Route::Direct)).unwrap();
        runtime.wait_auth(1, |state| *state == AuthState::Ready, Duration::from_secs(5));
        let log = td.log();
        assert_eq!(
            &log[2..5],
            ["create 1", "send 1 disableProxy", "send 1 setTdlibParameters"]
        );
        runtime.end_all(Duration::from_secs(5));
    }

    #[test]
    fn a_proxy_with_a_host_name_makes_no_client() {
        let td = Arc::new(FakeTd::new());
        let (runtime, _) = runtime(&td);
        let route = Route::Proxy(panim_core::ProxyEndpoint {
            kind: panim_core::ProxyKind::Http,
            host: "proxy.example.org".into(),
            port: 8080,
        });
        assert_eq!(runtime.open(1, config(route)), Err(Error::ProxyHostName));
        assert!(td.take_sent().is_empty());
        assert!(!td.log().iter().any(|line| line.starts_with("create")));
    }

    #[test]
    fn two_clients_share_one_loop_and_keep_their_updates_apart() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, events) = runtime(&td);
        runtime.open(10, config(Route::Direct)).unwrap();
        runtime.open(20, config(Route::Direct)).unwrap();
        for key in [10, 20] {
            runtime.wait_auth(key, |state| *state == AuthState::Ready, Duration::from_secs(5));
        }
        wait_for(|| {
            let events = events.lock().unwrap();
            events.iter().filter(|(_, e)| matches!(e, Event::Loaded { .. })).count() == 2
        });
        td.push(2, new_chat(5, "Alice", 1));
        wait_for(|| {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, e)| matches!(e, Event::Items { .. }))
        });
        let events = events.lock().unwrap();
        let items: Vec<u64> = events
            .iter()
            .filter(|(_, e)| matches!(e, Event::Items { .. }))
            .map(|(key, _)| *key)
            .collect();
        assert_eq!(items, [20]);
        drop(events);
        let mut keys = runtime.keys();
        keys.sort();
        assert_eq!(keys, [10, 20]);
        assert!(runtime.end_all(Duration::from_secs(5)));
        assert!(runtime.keys().is_empty());
    }

    #[test]
    fn calls_get_their_answers_and_errors() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, _) = runtime(&td);
        runtime.open(1, config(Route::Direct)).unwrap();
        runtime.wait_auth(1, |state| *state == AuthState::Ready, Duration::from_secs(5));

        let history = json!({"@type": "getChatHistory", "chat_id": 5});
        let answer = runtime.call(1, history, Duration::from_secs(5)).unwrap();
        assert_eq!(answer["@type"], "messages");
        let send = crate::messages::send_text_request(5, "hello");
        let error = runtime.call(1, send, Duration::from_secs(5)).unwrap_err();
        assert_eq!(error.code(), "telegram_no_write");
        // TDLib does not answer this request.
        let silent = json!({"@type": "openChat", "chat_id": 5});
        assert_eq!(runtime.call(1, silent, Duration::from_millis(50)), Err(Error::Timeout));
        assert_eq!(
            runtime.call(99, json!({"@type": "getMe"}), Duration::from_secs(1)),
            Err(Error::NoClient)
        );
        runtime.end_all(Duration::from_secs(5));
    }

    #[test]
    fn close_and_logout_wait_for_the_closed_state() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, events) = runtime(&td);
        runtime.open(1, config(Route::Direct)).unwrap();
        runtime.open(2, config(Route::Direct)).unwrap();
        runtime.wait_auth(2, |state| *state == AuthState::Ready, Duration::from_secs(5));
        assert!(runtime.end(1, End::Close, Duration::from_secs(5)));
        assert!(runtime.end(2, End::LogOut, Duration::from_secs(5)));
        let sent: Vec<String> = td
            .log()
            .into_iter()
            .filter(|l| l.ends_with("close") || l.ends_with("logOut"))
            .collect();
        assert_eq!(sent, ["send 1 close", "send 2 logOut"]);
        assert!(
            events
                .lock()
                .unwrap()
                .contains(&(2, Event::Auth(AuthState::LoggingOut)))
        );
        assert_eq!(
            runtime.call(1, json!({"@type": "getMe"}), Duration::from_secs(1)),
            Err(Error::NoClient)
        );

        // A new client can start after all the clients are closed: the loop starts again.
        runtime.open(3, config(Route::Direct)).unwrap();
        assert_eq!(
            runtime.wait_auth(3, |state| *state == AuthState::Ready, Duration::from_secs(5)),
            Some(AuthState::Ready)
        );
        runtime.end_all(Duration::from_secs(5));
    }

    #[test]
    fn a_logout_that_does_not_complete_closes_the_client() {
        let td = Arc::new(FakeTd::new());
        // Telegram does not answer the logout. The client closes only on `close`.
        td.respond(|_, request| match request["@type"].as_str().unwrap() {
            "disableProxy" => vec![auth("authorizationStateWaitTdlibParameters")],
            "setTdlibParameters" => vec![auth("authorizationStateReady")],
            "logOut" => vec![auth("authorizationStateLoggingOut")],
            "close" => vec![auth("authorizationStateClosed")],
            _ => vec![],
        });
        let (runtime, _) = runtime(&td);
        runtime.open(1, config(Route::Direct)).unwrap();
        runtime.wait_auth(1, |state| *state == AuthState::Ready, Duration::from_secs(5));
        assert!(!runtime.end(1, End::LogOut, Duration::from_millis(100)));
        let log = td.log();
        assert!(log.contains(&"send 1 logOut".to_string()) && log.contains(&"send 1 close".to_string()));
        assert!(runtime.keys().is_empty());
    }

    #[test]
    fn the_login_key_changes_to_the_account_key() {
        let td = Arc::new(FakeTd::new());
        logged_in(&td);
        let (runtime, events) = runtime(&td);
        runtime.open(0, config(Route::Direct)).unwrap();
        runtime.wait_auth(0, |state| *state == AuthState::Ready, Duration::from_secs(5));
        wait_for(|| {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, e)| matches!(e, Event::Loaded { .. }))
        });
        assert!(runtime.rekey(0, 42));
        assert!(!runtime.rekey(0, 43));
        td.push(1, new_chat(5, "Alice", 1));
        wait_for(|| {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|(key, e)| *key == 42 && matches!(e, Event::Items { .. }))
        });
        let read = runtime.with(42, |client| ((), client.mark_read(5)));
        assert_eq!(read, Ok(()));
        wait_for(|| td.log().contains(&"send 1 viewMessages".to_string()));
        assert_eq!(
            runtime.with(0, |client| ((), client.mark_read(5))),
            Err(Error::NoClient)
        );
        runtime.end_all(Duration::from_secs(5));
    }
}
