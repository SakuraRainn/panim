//! The state of one TDLib client: the start, the login, the guard, the chats and the chat panel.
//!
//! `Client` has no network code and no clock. It reads the updates and the answers of TDLib, and it gives the
//! requests to send and the events for the app. The module `runtime` moves the data between TDLib and the client.
//! Thus, the tests give scripted updates and examine the requests.
//!
//! The requests that the client sends for its own work have a text in the field `@extra`, with the prefix `panim:`.
//! The runtime uses numbers for the requests of the app.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use panim_core::{Route, SecretBytes};
use serde_json::{Value, json};

use crate::Error;
use crate::chats::{ChatItem, Model, Scope};
use crate::guard::{Guard, GuardAction};
use crate::login::{AuthState, TdError, td_error};
use crate::messages::{MessageView, message_view, sender_of};
use crate::start::{AppConfig, has_proxy, parameters, proxy_request};

/// The client asks for no more than this number of pages of the chat list. One page has `CHAT_PAGE` chats.
pub const CHAT_PAGES: u32 = 10;
const CHAT_PAGE: u32 = 100;

/// The chat panel keeps no more than this number of messages.
pub const PANEL_MESSAGES: usize = 200;

/// What a client needs to start.
pub struct ClientConfig {
    /// The folder of the account in memory. TDLib writes `db/` and `files/` in it.
    pub dir: PathBuf,
    /// The database key of the account: 32 random bytes from the sealed secrets.
    pub key: SecretBytes,
    /// The route of the persona.
    pub route: Route,
    pub app: AppConfig,
    /// The language of the app, `en` or `zh`.
    pub language: String,
}

/// The account of the login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    /// The numeric user id. It does not change.
    pub id: i64,
    /// The first active username, without `@`. A Telegram account can have no username.
    pub username: Option<String>,
    /// The first name and the last name.
    pub name: String,
}

/// The connection state of a client, for the account list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Unknown,
    WaitingForNetwork,
    ConnectingToProxy,
    Connecting,
    Updating,
    Ready,
}

/// What the app must know about a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The authorization state changed.
    Auth(AuthState),
    /// The connection state changed.
    Connection(Connection),
    /// The login is complete, and this is the account.
    Me(Me),
    /// Timeline items changed. `upsert` has the new data of the items. `remove` has the chats with no item now.
    Items { upsert: Vec<ChatItem>, remove: Vec<i64> },
    /// The client has the chat list. If `complete` is true, a stored item of a chat that is not in `shown` is old.
    Loaded { complete: bool, shown: Vec<i64> },
    /// The messages of the open chat panel changed.
    Panel { chat_id: i64 },
    /// The client could not start, for example because the database key is wrong.
    Failed(TdError),
}

/// The requests to send and the events for the app.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Out {
    pub requests: Vec<Value>,
    pub events: Vec<Event>,
}

/// The open chat panel.
struct Panel {
    chat_id: i64,
    /// The messages, in the sequence of their identifiers. A larger identifier is a later message.
    messages: BTreeMap<i64, MessageView>,
    /// The newest received message that Telegram got a read for. 0 before the first read.
    seen: i64,
}

/// The data of the chat panel for the user interface.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PanelView {
    pub chat_id: i64,
    pub title: String,
    /// False if the account cannot write in the chat, for example in a channel of a different person.
    pub can_write: bool,
    /// The messages, the oldest first.
    pub messages: Vec<MessageView>,
}

pub struct Client {
    parameters: Option<Value>,
    auth: AuthState,
    connection: Connection,
    guard: Guard,
    model: Model,
    /// The items that the app has now.
    shown: BTreeMap<i64, ChatItem>,
    me: Option<Me>,
    chat_pages: u32,
    /// `Some` after the client has the chat list. The value is true if the list is complete.
    loaded: Option<bool>,
    panel: Option<Panel>,
    /// The number of authorization updates. A login step waits until this number changes.
    auth_serial: u64,
    /// The time of the last state "wait for the code". The timeout of a resend starts at this time.
    code_since: Option<Instant>,
}

fn tagged(mut request: Value, tag: &str) -> Value {
    request["@extra"] = format!("panim:{tag}").into();
    request
}

impl Client {
    /// Makes a client and its first request: the proxy request. Send it before all other requests.
    ///
    /// The function gives an error for a proxy with a host name, and it sends nothing then.
    pub fn new(config: ClientConfig) -> Result<(Self, Vec<Value>), Error> {
        let proxy = tagged(proxy_request(&config.route)?, "proxy");
        let parameters = tagged(
            parameters(&config.dir, &config.key, &config.app, &config.language)?,
            "parameters",
        );
        let client = Self {
            parameters: Some(parameters),
            auth: AuthState::Starting,
            auth_serial: 0,
            code_since: None,
            connection: Connection::Unknown,
            guard: Guard::new(has_proxy(&config.route)),
            model: Model::default(),
            shown: BTreeMap::new(),
            me: None,
            chat_pages: 0,
            loaded: None,
            panel: None,
        };
        Ok((client, vec![proxy]))
    }

    pub fn auth(&self) -> &AuthState {
        &self.auth
    }

    /// The number of authorization updates until now.
    pub fn auth_serial(&self) -> u64 {
        self.auth_serial
    }

    /// The request that sends the login code again, on the next route of Telegram.
    ///
    /// TDLib permits it only in the state "wait for the code", only with a next route, and only after the timeout of
    /// Telegram. The function makes the same checks first, so that the person gets a clear error.
    pub fn resend_request(&self, now: Instant) -> Result<Value, Error> {
        let refuse = |code: &'static str, seconds: Option<u64>| {
            Err(Error::Td(TdError {
                code,
                seconds,
                detail: String::new(),
            }))
        };
        let AuthState::WaitCode { next, timeout, .. } = &self.auth else {
            return refuse("telegram_resend_state", None);
        };
        if next.is_none() {
            return refuse("telegram_resend_none", None);
        }
        let ready = self.code_since.unwrap_or(now) + Duration::from_secs(u64::from(*timeout));
        if now < ready {
            let left = (ready - now).as_millis().div_ceil(1000) as u64;
            return refuse("telegram_resend_early", Some(left));
        }
        Ok(json!({"@type": "resendAuthenticationCode", "reason": {"@type": "resendCodeReasonUserRequest"}}))
    }

    pub fn connection(&self) -> Connection {
        self.connection
    }

    pub fn me(&self) -> Option<&Me> {
        self.me.as_ref()
    }

    /// The events that tell all the items of the client again.
    ///
    /// The login uses it: the first items come before the account exists in the store.
    pub fn replay(&self) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.shown.is_empty() {
            events.push(Event::Items {
                upsert: self.shown.values().cloned().collect(),
                remove: Vec::new(),
            });
        }
        if let Some(complete) = self.loaded {
            events.push(Event::Loaded {
                complete,
                shown: self.shown.keys().copied().collect(),
            });
        }
        events
    }

    /// Reads one update or one answer of TDLib for this client.
    pub fn handle(&mut self, value: &Value, now: Instant) -> Out {
        let mut out = Out::default();
        if let Some(tag) = value["@extra"].as_str().and_then(|extra| extra.strip_prefix("panim:")) {
            self.answer(tag, value, &mut out);
        } else {
            self.update(value, now, &mut out);
        }
        self.items(&mut out);
        out
    }

    /// Runs the guard. Call it frequently.
    pub fn tick(&mut self, now: Instant) -> Out {
        let mut out = Out::default();
        let network = match self.guard.tick(now) {
            GuardAction::Nothing => return out,
            GuardAction::NetworkOff => "networkTypeNone",
            GuardAction::NetworkOn => "networkTypeOther",
        };
        out.requests.push(tagged(
            json!({"@type": "setNetworkType", "type": {"@type": network}}),
            "guard",
        ));
        out
    }

    fn update(&mut self, update: &Value, now: Instant, out: &mut Out) {
        match update["@type"].as_str().unwrap_or("") {
            "updateAuthorizationState" => {
                let raw = &update["authorization_state"];
                if raw["@type"] == "authorizationStateWaitTdlibParameters" {
                    // TDLib has the proxy request already. Now it can get the parameters.
                    if let Some(parameters) = self.parameters.take() {
                        out.requests.push(parameters);
                    }
                    return;
                }
                let state = AuthState::read(raw);
                // A resend or a new phone number can give the same state again. The serial and the time change all
                // the same, so that a login step sees the new state.
                self.auth_serial += 1;
                self.code_since = matches!(state, AuthState::WaitCode { .. }).then_some(now);
                if state == self.auth {
                    return;
                }
                self.auth = state.clone();
                if state == AuthState::Ready {
                    out.requests.push(tagged(json!({"@type": "getMe"}), "me"));
                    for scope in Scope::ALL {
                        out.requests.push(tagged(
                            json!({"@type": "getScopeNotificationSettings", "scope": {"@type": scope.type_name()}}),
                            &format!("scope:{}", scope.type_name()),
                        ));
                    }
                    // Remove the proxies of earlier routes from the database of TDLib.
                    out.requests.push(tagged(json!({"@type": "getProxies"}), "proxies"));
                    out.requests.push(self.load_chats());
                }
                out.events.push(Event::Auth(state));
            }
            "updateConnectionState" => {
                let connection = match update["state"]["@type"].as_str().unwrap_or("") {
                    "connectionStateWaitingForNetwork" => Connection::WaitingForNetwork,
                    "connectionStateConnectingToProxy" => Connection::ConnectingToProxy,
                    "connectionStateConnecting" => Connection::Connecting,
                    "connectionStateUpdating" => Connection::Updating,
                    "connectionStateReady" => Connection::Ready,
                    _ => Connection::Unknown,
                };
                self.guard.on_state(connection == Connection::Connecting, now);
                if connection != self.connection {
                    self.connection = connection;
                    out.events.push(Event::Connection(connection));
                }
            }
            "updateNewMessage" => self.panel_message(&update["message"], None, out),
            "updateMessageSendSucceeded" | "updateMessageSendFailed" => {
                self.panel_message(&update["message"], update["old_message_id"].as_i64(), out)
            }
            "updateMessageContent" => {
                let (chat, id) = (update["chat_id"].as_i64(), update["message_id"].as_i64());
                if let (Some(panel), Some(id)) = (self.panel.as_mut().filter(|p| Some(p.chat_id) == chat), id)
                    && let Some(message) = panel.messages.get_mut(&id)
                {
                    let (kind, text) = crate::messages::content_summary(&update["new_content"]);
                    message.kind = kind;
                    message.text = crate::messages::cut(&text, crate::messages::MESSAGE_TEXT_CHARS);
                    out.events.push(Event::Panel { chat_id: panel.chat_id });
                }
            }
            "updateDeleteMessages" => {
                let chat = update["chat_id"].as_i64();
                // `from_cache` tells that TDLib removed the message only from its memory.
                let permanent = update["is_permanent"].as_bool().unwrap_or(false)
                    && !update["from_cache"].as_bool().unwrap_or(false);
                if let Some(panel) = self.panel.as_mut().filter(|p| Some(p.chat_id) == chat && permanent) {
                    let before = panel.messages.len();
                    for id in update["message_ids"].as_array().into_iter().flatten() {
                        if let Some(id) = id.as_i64() {
                            panel.messages.remove(&id);
                        }
                    }
                    if panel.messages.len() != before {
                        out.events.push(Event::Panel { chat_id: panel.chat_id });
                    }
                }
            }
            _ => {
                self.model.apply(update);
            }
        }
    }

    fn answer(&mut self, tag: &str, answer: &Value, out: &mut Out) {
        let failed = answer["@type"] == "error";
        match tag {
            "parameters" if failed => out.events.push(Event::Failed(td_error(answer))),
            "me" if !failed => {
                let Some(id) = answer["id"].as_i64() else { return };
                let first = answer["first_name"].as_str().unwrap_or("");
                let last = answer["last_name"].as_str().unwrap_or("");
                let username = answer["usernames"]["active_usernames"][0].as_str();
                let me = Me {
                    id,
                    username: username.filter(|name| !name.is_empty()).map(str::to_string),
                    name: format!("{first} {last}").trim().to_string(),
                };
                self.me = Some(me.clone());
                out.events.push(Event::Me(me));
            }
            "proxies" if !failed => {
                for proxy in answer["proxies"].as_array().into_iter().flatten() {
                    if proxy["is_enabled"] != true
                        && let Some(id) = proxy["id"].as_i64()
                    {
                        out.requests
                            .push(tagged(json!({"@type": "removeProxy", "proxy_id": id}), "proxy"));
                    }
                }
            }
            "chats" => {
                // The error 404 tells that TDLib has all the chats of the list.
                let complete = failed && answer["code"] == 404;
                if !failed && self.chat_pages < CHAT_PAGES {
                    out.requests.push(self.load_chats());
                } else {
                    // Make the items first, so that `shown` has the chats of the last update.
                    self.items(out);
                    self.loaded = Some(complete);
                    out.events.push(Event::Loaded {
                        complete,
                        shown: self.shown.keys().copied().collect(),
                    });
                }
            }
            _ => {
                if let Some(scope) = tag.strip_prefix("scope:") {
                    if !failed {
                        self.model.set_scope(&json!({"@type": scope}), answer);
                    }
                } else if let Some(chat) = tag.strip_prefix("read:").and_then(|chat| chat.parse().ok())
                    && failed
                {
                    self.model.read_failed(chat);
                }
            }
        }
    }

    fn load_chats(&mut self) -> Value {
        self.chat_pages += 1;
        tagged(
            json!({"@type": "loadChats", "chat_list": {"@type": "chatListMain"}, "limit": CHAT_PAGE}),
            "chats",
        )
    }

    /// Compares the items of the changed chats with the items that the app has, and tells the differences.
    fn items(&mut self, out: &mut Out) {
        let dirty = self.model.take_dirty();
        if self.auth != AuthState::Ready {
            // Keep the chats for the time after the login. TDLib sends chats only with a login.
            return;
        }
        let (mut upsert, mut remove) = (Vec::new(), Vec::new());
        for chat_id in dirty {
            match self.model.item(chat_id) {
                Some(item) => {
                    if self.shown.get(&chat_id) != Some(&item) {
                        self.shown.insert(chat_id, item.clone());
                        upsert.push(item);
                    }
                }
                None => {
                    if self.shown.remove(&chat_id).is_some() {
                        remove.push(chat_id);
                    }
                }
            }
        }
        if !upsert.is_empty() || !remove.is_empty() {
            out.events.push(Event::Items { upsert, remove });
        }
    }

    /// The person read a chat in PANIM. The result has the requests that tell Telegram, and the item change.
    ///
    /// `viewMessages` with `force_read` marks the messages as read when the chat is not open. Telegram keeps the
    /// read state as one position in the chat, so the last message is sufficient.
    pub fn mark_read(&mut self, chat_id: i64) -> Out {
        let mut out = Out::default();
        if let Some((last, marked)) = self.model.read_locally(chat_id) {
            out.requests.push(tagged(
                json!({"@type": "viewMessages", "chat_id": chat_id, "message_ids": [last], "force_read": true}),
                &format!("read:{chat_id}"),
            ));
            if marked {
                out.requests.push(tagged(
                    json!({"@type": "toggleChatIsMarkedAsUnread", "chat_id": chat_id, "is_marked_as_unread": false}),
                    "unmark",
                ));
            }
        }
        self.items(&mut out);
        out
    }

    /// True if the client has the chat.
    pub fn has_chat(&self, chat_id: i64) -> bool {
        self.model.has_chat(chat_id)
    }

    /// True if the account can write in the chat.
    pub fn can_write(&self, chat_id: i64) -> bool {
        self.model.can_write(chat_id)
    }

    /// Opens the chat panel for a chat. A panel that was open closes first.
    ///
    /// `openChat` tells TDLib that the person looks at the chat. For a channel or a large group, TDLib then gets all
    /// the updates of the chat.
    pub fn panel_open(&mut self, chat_id: i64) -> Vec<Value> {
        let mut requests = self.panel_close();
        self.panel = Some(Panel {
            chat_id,
            messages: BTreeMap::new(),
            seen: 0,
        });
        requests.push(tagged(json!({"@type": "openChat", "chat_id": chat_id}), "open"));
        requests
    }

    /// Closes the chat panel.
    pub fn panel_close(&mut self) -> Vec<Value> {
        match self.panel.take() {
            Some(panel) => vec![tagged(json!({"@type": "closeChat", "chat_id": panel.chat_id}), "close")],
            None => Vec::new(),
        }
    }

    /// The chat of the open panel.
    pub fn panel_chat(&self) -> Option<i64> {
        self.panel.as_ref().map(|panel| panel.chat_id)
    }

    /// The request for one page of the history of the open panel, before its oldest message.
    pub fn panel_history_request(&self, limit: usize) -> Option<Value> {
        let panel = self.panel.as_ref()?;
        let from = panel.messages.keys().next().copied().unwrap_or(0);
        Some(json!({
            "@type": "getChatHistory",
            "chat_id": panel.chat_id,
            "from_message_id": from,
            "offset": 0,
            "limit": limit.clamp(1, 100),
            "only_local": false,
        }))
    }

    /// Adds the messages of an answer to `getChatHistory`. Returns the number of new messages.
    pub fn panel_history(&mut self, chat_id: i64, answer: &Value) -> usize {
        let mut added = 0;
        for message in answer["messages"].as_array().into_iter().flatten() {
            if self.panel_insert(chat_id, message) {
                added += 1;
            }
        }
        added
    }

    fn panel_insert(&mut self, chat_id: i64, message: &Value) -> bool {
        let sender = self.model.sender_name(sender_of(message));
        let Some(panel) = self.panel.as_mut().filter(|panel| panel.chat_id == chat_id) else {
            return false;
        };
        let Some(view) = message_view(message, sender) else {
            return false;
        };
        let new = panel.messages.insert(view.id, view).is_none();
        while panel.messages.len() > PANEL_MESSAGES {
            panel.messages.pop_first();
        }
        new
    }

    /// A new message, or a sent message that has its final identifier now.
    fn panel_message(&mut self, message: &Value, old_id: Option<i64>, out: &mut Out) {
        let Some(chat_id) = message["chat_id"].as_i64() else {
            return;
        };
        let Some(panel) = self.panel.as_mut().filter(|panel| panel.chat_id == chat_id) else {
            return;
        };
        if let Some(old) = old_id {
            panel.messages.remove(&old);
        }
        self.panel_insert(chat_id, message);
        out.events.push(Event::Panel { chat_id });
    }

    /// The data of the open panel, if it shows this chat.
    pub fn panel_view(&self, chat_id: i64) -> Option<PanelView> {
        let panel = self.panel.as_ref().filter(|panel| panel.chat_id == chat_id)?;
        Some(PanelView {
            chat_id,
            title: self.model.title(chat_id).unwrap_or_default().to_string(),
            can_write: self.model.can_write(chat_id),
            messages: panel.messages.values().cloned().collect(),
        })
    }

    /// The request that tells Telegram that the person sees the messages of the open panel.
    ///
    /// The client sends `viewMessages` only for a received message that is newer than the last one it told. Thus,
    /// each refresh of the panel does not send a request.
    pub fn panel_seen(&mut self, chat_id: i64) -> Out {
        let mut out = Out::default();
        let Some(panel) = self.panel.as_mut().filter(|panel| panel.chat_id == chat_id) else {
            return out;
        };
        let newest = panel
            .messages
            .values()
            .rev()
            .find(|message| !message.outgoing)
            .map(|message| message.id)
            .filter(|id| *id > panel.seen);
        if let Some(newest) = newest {
            panel.seen = newest;
            // Record the read in the model too, so that the timeline item becomes read immediately.
            self.model.read_locally(chat_id);
            out.requests.push(tagged(
                json!({"@type": "viewMessages", "chat_id": chat_id, "message_ids": [newest], "force_read": true}),
                &format!("read:{chat_id}"),
            ));
        }
        self.items(&mut out);
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use panim_core::{ProxyEndpoint, ProxyKind};

    use super::*;

    pub fn config(route: Route) -> ClientConfig {
        ClientConfig {
            dir: PathBuf::from("/memory/telegram-ab"),
            key: SecretBytes::new(vec![9; 32]),
            route,
            app: AppConfig::for_test(),
            language: "en".into(),
        }
    }

    pub fn tor() -> Route {
        Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: 9050,
        })
    }

    fn auth(state: &str) -> Value {
        json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": state}})
    }

    fn types(requests: &[Value]) -> Vec<&str> {
        requests.iter().map(|r| r["@type"].as_str().unwrap()).collect()
    }

    /// A client after the login, with the scope settings.
    pub fn ready(route: Route) -> Client {
        let now = Instant::now();
        let (mut client, _) = Client::new(config(route)).unwrap();
        client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        client.handle(&auth("authorizationStateReady"), now);
        for scope in Scope::ALL {
            let extra = format!("panim:scope:{}", scope.type_name());
            client.handle(
                &json!({"@type": "scopeNotificationSettings", "mute_for": 0, "@extra": extra}),
                now,
            );
        }
        client
    }

    pub fn message(chat: i64, id: i64, text: &str, outgoing: bool) -> Value {
        json!({"@type": "message", "id": id, "chat_id": chat, "date": 1794800000 + id, "is_outgoing": outgoing,
            "sender_id": {"@type": "messageSenderUser", "user_id": 42},
            "content": {"@type": "messageText", "text": {"text": text}}})
    }

    pub fn new_chat(id: i64, title: &str, unread: u32) -> Value {
        json!({"@type": "updateNewChat", "chat": {"id": id, "title": title,
            "type": {"@type": "chatTypePrivate", "user_id": 42},
            "permissions": {"can_send_basic_messages": true},
            "last_message": message(id, 100, "hello", false),
            "positions": [{"list": {"@type": "chatListMain"}, "order": "9221294780000000000"}],
            "unread_count": unread, "is_marked_as_unread": false,
            "notification_settings": {"use_default_mute_for": true, "mute_for": 0}}})
    }

    #[test]
    fn the_proxy_request_goes_before_the_parameters() {
        let now = Instant::now();
        let (mut client, first) = Client::new(config(tor())).unwrap();
        assert_eq!(types(&first), ["addProxy"]);
        assert_eq!(first[0]["enable"], true);
        assert_eq!(first[0]["proxy"]["server"], "127.0.0.1");

        // The parameters go only when TDLib asks for them, and only one time.
        let out = client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        assert_eq!(types(&out.requests), ["setTdlibParameters"]);
        assert!(out.events.is_empty());
        let out = client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        assert!(out.requests.is_empty());
    }

    #[test]
    fn a_direct_route_stops_a_kept_proxy_first() {
        let (_, first) = Client::new(config(Route::Direct)).unwrap();
        assert_eq!(types(&first), ["disableProxy"]);
    }

    #[test]
    fn a_proxy_with_a_host_name_sends_nothing() {
        let route = Route::Proxy(ProxyEndpoint {
            kind: ProxyKind::Socks5,
            host: "proxy.example.org".into(),
            port: 1080,
        });
        assert_eq!(Client::new(config(route)).err(), Some(Error::ProxyHostName));
    }

    #[test]
    fn a_wrong_database_key_gives_an_event() {
        let now = Instant::now();
        let (mut client, _) = Client::new(config(Route::Direct)).unwrap();
        client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        let out = client.handle(
            &json!({"@type": "error", "code": 401, "message": "Wrong database encryption key",
                "@extra": "panim:parameters"}),
            now,
        );
        let [Event::Failed(error)] = out.events.as_slice() else {
            panic!("{out:?}")
        };
        assert_eq!(error.code, "telegram_database");
    }

    #[test]
    fn login_states_become_events() {
        let now = Instant::now();
        let (mut client, _) = Client::new(config(Route::Direct)).unwrap();
        client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        let out = client.handle(&auth("authorizationStateWaitPhoneNumber"), now);
        assert_eq!(out.events, [Event::Auth(AuthState::WaitPhone)]);
        assert!(out.requests.is_empty());
        // The same state a second time is not an event.
        assert!(
            client
                .handle(&auth("authorizationStateWaitPhoneNumber"), now)
                .events
                .is_empty()
        );

        let out = client.handle(&auth("authorizationStateReady"), now);
        assert_eq!(out.events, [Event::Auth(AuthState::Ready)]);
        assert_eq!(
            types(&out.requests),
            [
                "getMe",
                "getScopeNotificationSettings",
                "getScopeNotificationSettings",
                "getScopeNotificationSettings",
                "getProxies",
                "loadChats"
            ]
        );
        assert_eq!(out.requests[5]["chat_list"]["@type"], "chatListMain");

        let out = client.handle(
            &json!({"@type": "user", "id": 777001, "first_name": "Sakura", "last_name": "",
                "usernames": {"active_usernames": ["sakura_r"]}, "@extra": "panim:me"}),
            now,
        );
        let me = Me {
            id: 777001,
            username: Some("sakura_r".into()),
            name: "Sakura".into(),
        };
        assert_eq!(out.events, [Event::Me(me.clone())]);
        assert_eq!(client.me(), Some(&me));

        // An account without a username.
        let out = client.handle(
            &json!({"@type": "user", "id": 5, "first_name": "A", "last_name": "B", "usernames": null,
                "@extra": "panim:me"}),
            now,
        );
        assert_eq!(
            out.events,
            [Event::Me(Me {
                id: 5,
                username: None,
                name: "A B".into()
            })]
        );
    }

    fn code_state(next: Option<&str>, timeout: u32) -> Value {
        let next = next.map_or(Value::Null, |next| json!({"@type": next, "length": 5}));
        json!({"@type": "updateAuthorizationState", "authorization_state": {"@type": "authorizationStateWaitCode",
            "code_info": {"phone_number": "15550001111", "type": {"@type": "authenticationCodeTypeTelegramMessage", "length": 5},
                "next_type": next, "timeout": timeout}}})
    }

    #[test]
    fn resend_needs_the_code_state_a_next_route_and_the_timeout() {
        let start = Instant::now();
        let (mut client, _) = Client::new(config(Route::Direct)).unwrap();
        client.handle(&auth("authorizationStateWaitTdlibParameters"), start);
        let code = |result: Result<Value, Error>| match result {
            Err(Error::Td(error)) => (error.code, error.seconds),
            other => panic!("{other:?}"),
        };
        client.handle(&auth("authorizationStateWaitPhoneNumber"), start);
        assert_eq!(code(client.resend_request(start)), ("telegram_resend_state", None));

        // Telegram offers no other route.
        client.handle(&code_state(None, 0), start);
        assert_eq!(code(client.resend_request(start)), ("telegram_resend_none", None));

        // A next route, after a timeout of 60 seconds.
        let serial = client.auth_serial();
        client.handle(&code_state(Some("authenticationCodeTypeSms"), 60), start);
        assert!(client.auth_serial() > serial);
        let state = client.auth().clone();
        assert!(matches!(
            state,
            AuthState::WaitCode {
                via: "app",
                next: Some("sms"),
                timeout: 60,
                ..
            }
        ));
        let early = start + Duration::from_millis(30_500);
        assert_eq!(code(client.resend_request(early)), ("telegram_resend_early", Some(30)));
        let request = client.resend_request(start + Duration::from_secs(60)).unwrap();
        assert_eq!(request["@type"], "resendAuthenticationCode");
        assert_eq!(request["reason"]["@type"], "resendCodeReasonUserRequest");

        // The same state again starts the timeout again, and the serial changes.
        let later = start + Duration::from_secs(100);
        let serial = client.auth_serial();
        let out = client.handle(&code_state(Some("authenticationCodeTypeSms"), 60), later);
        assert!(out.events.is_empty(), "the same state is not a new event");
        assert_eq!(client.auth_serial(), serial + 1);
        assert_eq!(code(client.resend_request(later)), ("telegram_resend_early", Some(60)));
    }

    #[test]
    fn old_proxies_are_removed() {
        let now = Instant::now();
        let mut client = ready(tor());
        let out = client.handle(
            &json!({"@type": "addedProxies", "@extra": "panim:proxies", "proxies": [
                {"id": 2, "is_enabled": false}, {"id": 3, "is_enabled": true}, {"id": 4, "is_enabled": false}]}),
            now,
        );
        assert_eq!(types(&out.requests), ["removeProxy", "removeProxy"]);
        assert_eq!(
            (&out.requests[0]["proxy_id"], &out.requests[1]["proxy_id"]),
            (&json!(2), &json!(4))
        );
    }

    #[test]
    fn chats_become_items_only_after_the_login() {
        let now = Instant::now();
        let (mut client, _) = Client::new(config(Route::Direct)).unwrap();
        client.handle(&auth("authorizationStateWaitTdlibParameters"), now);
        assert!(client.handle(&new_chat(5, "Alice", 2), now).events.is_empty());

        let mut client = ready(Route::Direct);
        let out = client.handle(&new_chat(5, "Alice", 2), now);
        let [Event::Items { upsert, remove }] = out.events.as_slice() else {
            panic!("{out:?}")
        };
        assert_eq!((upsert.len(), remove.len()), (1, 0));
        assert_eq!((upsert[0].chat_id, upsert[0].unread, upsert[0].read), (5, 2, false));

        // An update that does not change the item is not an event.
        let same =
            json!({"@type": "updateChatReadInbox", "chat_id": 5, "last_read_inbox_message_id": 99, "unread_count": 2});
        assert!(client.handle(&same, now).events.is_empty());

        // The chat becomes muted: the item goes away.
        let muted = json!({"@type": "updateChatNotificationSettings", "chat_id": 5,
            "notification_settings": {"use_default_mute_for": false, "mute_for": 3600}});
        assert_eq!(
            client.handle(&muted, now).events,
            [Event::Items {
                upsert: vec![],
                remove: vec![5]
            }]
        );
    }

    #[test]
    fn the_chat_list_loads_in_pages() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        client.handle(&new_chat(5, "Alice", 0), now);
        let ok = json!({"@type": "ok", "@extra": "panim:chats"});
        let out = client.handle(&ok, now);
        assert_eq!(types(&out.requests), ["loadChats"]);
        assert!(out.events.is_empty());
        let end = json!({"@type": "error", "code": 404, "message": "Not Found", "@extra": "panim:chats"});
        let out = client.handle(&end, now);
        assert!(out.requests.is_empty());
        assert_eq!(
            out.events,
            [Event::Loaded {
                complete: true,
                shown: vec![5]
            }]
        );

        // The page limit stops the requests. Then the list is not complete.
        let mut client = ready(Route::Direct);
        let mut last = Out::default();
        for _ in 0..CHAT_PAGES {
            last = client.handle(&ok, now);
        }
        assert!(last.requests.is_empty());
        assert_eq!(
            last.events,
            [Event::Loaded {
                complete: false,
                shown: vec![]
            }]
        );
    }

    #[test]
    fn replay_tells_the_items_again() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        assert!(client.replay().is_empty());
        client.handle(&new_chat(5, "Alice", 2), now);
        client.handle(&new_chat(6, "Bob", 0), now);
        let events = client.replay();
        assert!(
            matches!(events.as_slice(), [Event::Items { upsert, remove }] if upsert.len() == 2 && remove.is_empty())
        );
        client.handle(
            &json!({"@type": "error", "code": 404, "message": "Not Found", "@extra": "panim:chats"}),
            now,
        );
        let events = client.replay();
        assert_eq!(
            events[1],
            Event::Loaded {
                complete: true,
                shown: vec![5, 6]
            }
        );
    }

    #[test]
    fn a_read_in_panim_tells_telegram() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        client.handle(&new_chat(5, "Alice", 2), now);
        let out = client.mark_read(5);
        assert_eq!(types(&out.requests), ["viewMessages"]);
        assert_eq!(out.requests[0]["chat_id"], 5);
        assert_eq!(out.requests[0]["message_ids"], json!([100]));
        assert_eq!(out.requests[0]["force_read"], true);
        let [Event::Items { upsert, .. }] = out.events.as_slice() else {
            panic!("{out:?}")
        };
        assert!(upsert[0].read);

        // A second read sends nothing while the first is in progress.
        let confirm =
            json!({"@type": "updateChatReadInbox", "chat_id": 5, "last_read_inbox_message_id": 100, "unread_count": 0});
        client.handle(&confirm, now);
        assert_eq!(client.mark_read(5), Out::default());
        // A chat that the client does not have sends nothing.
        assert_eq!(client.mark_read(404), Out::default());
    }

    #[test]
    fn a_refused_read_makes_the_item_unread_again() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        client.handle(&new_chat(5, "Alice", 2), now);
        let out = client.mark_read(5);
        let extra = out.requests[0]["@extra"].clone();
        let out = client.handle(
            &json!({"@type": "error", "code": 400, "message": "Chat not found", "@extra": extra}),
            now,
        );
        let [Event::Items { upsert, .. }] = out.events.as_slice() else {
            panic!("{out:?}")
        };
        assert!(!upsert[0].read);
    }

    #[test]
    fn a_chat_marked_as_unread_gets_two_requests() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        client.handle(&new_chat(5, "Alice", 0), now);
        client.handle(
            &json!({"@type": "updateChatIsMarkedAsUnread", "chat_id": 5, "is_marked_as_unread": true}),
            now,
        );
        let out = client.mark_read(5);
        assert_eq!(types(&out.requests), ["viewMessages", "toggleChatIsMarkedAsUnread"]);
        assert_eq!(out.requests[1]["is_marked_as_unread"], false);
    }

    #[test]
    fn the_guard_runs_only_for_a_proxy_route() {
        let start = Instant::now();
        let connecting = json!({"@type": "updateConnectionState", "state": {"@type": "connectionStateConnecting"}});
        let later = start + std::time::Duration::from_secs(4);

        let mut direct = ready(Route::Direct);
        let out = direct.handle(&connecting, start);
        assert_eq!(out.events, [Event::Connection(Connection::Connecting)]);
        assert_eq!(direct.tick(later), Out::default());

        let mut proxied = ready(tor());
        proxied.handle(&connecting, start);
        assert_eq!(proxied.tick(start + std::time::Duration::from_secs(3)), Out::default());
        let out = proxied.tick(later);
        assert_eq!(types(&out.requests), ["setNetworkType"]);
        assert_eq!(out.requests[0]["type"]["@type"], "networkTypeNone");
        let out = proxied.tick(later + crate::guard::PAUSE);
        assert_eq!(out.requests[0]["type"]["@type"], "networkTypeOther");

        // A connection that becomes ready in time needs no action.
        let mut proxied = ready(tor());
        proxied.handle(&connecting, start);
        let ready_state = json!({"@type": "updateConnectionState", "state": {"@type": "connectionStateReady"}});
        proxied.handle(&ready_state, start + std::time::Duration::from_secs(2));
        assert_eq!(proxied.tick(later), Out::default());
        assert_eq!(proxied.connection(), Connection::Ready);
    }

    #[test]
    fn the_chat_panel_shows_history_and_new_messages() {
        let now = Instant::now();
        let mut client = ready(Route::Direct);
        client.handle(
            &json!({"@type": "updateUser", "user": {"id": 42, "first_name": "Alice", "last_name": ""}}),
            now,
        );
        client.handle(&new_chat(5, "Alice", 2), now);

        assert_eq!(types(&client.panel_open(5)), ["openChat"]);
        let request = client.panel_history_request(50).unwrap();
        assert_eq!(
            (&request["from_message_id"], &request["limit"]),
            (&json!(0), &json!(50))
        );
        // TDLib gives the messages with the most recent first, and it can give fewer than the limit.
        let page =
            json!({"@type": "messages", "messages": [message(5, 100, "hello", false), message(5, 90, "hi", true)]});
        assert_eq!(client.panel_history(5, &page), 2);
        let request = client.panel_history_request(50).unwrap();
        assert_eq!(request["from_message_id"], 90);
        let page = json!({"@type": "messages", "messages": [message(5, 80, "first", false)]});
        assert_eq!(client.panel_history(5, &page), 1);
        assert_eq!(
            client.panel_history(5, &json!({"@type": "messages", "messages": []})),
            0
        );

        let view = client.panel_view(5).unwrap();
        assert_eq!((view.title.as_str(), view.can_write), ("Alice", true));
        let texts: Vec<_> = view
            .messages
            .iter()
            .map(|m| (m.id, m.text.as_str(), m.sender.as_str()))
            .collect();
        assert_eq!(texts, [(80, "first", "Alice"), (90, "hi", ""), (100, "hello", "Alice")]);

        // The person sees the messages: Telegram gets the read, and the item becomes read.
        let out = client.panel_seen(5);
        assert_eq!(types(&out.requests), ["viewMessages"]);
        assert_eq!(out.requests[0]["message_ids"], json!([100]));
        assert!(matches!(out.events.as_slice(), [Event::Items { upsert, .. }] if upsert[0].read));
        // A second refresh with no new received message sends nothing.
        assert!(client.panel_seen(5).requests.is_empty());

        // A new message of this chat shows in the panel. A message of a different chat does not.
        let out = client.handle(
            &json!({"@type": "updateNewMessage", "message": message(5, 110, "new", false)}),
            now,
        );
        assert_eq!(out.events, [Event::Panel { chat_id: 5 }]);
        let out = client.handle(
            &json!({"@type": "updateNewMessage", "message": message(6, 111, "other", false)}),
            now,
        );
        assert!(out.events.is_empty());
        assert_eq!(client.panel_view(5).unwrap().messages.len(), 4);
        assert_eq!(client.panel_view(6), None);
        // The new received message gets its own read.
        let out = client.panel_seen(5);
        assert_eq!(out.requests[0]["message_ids"], json!([110]));

        // A sent message has a temporary identifier until Telegram accepts it.
        let mut pending = message(5, 120, "answer", true);
        pending["sending_state"] = json!({"@type": "messageSendingStatePending"});
        client.handle(&json!({"@type": "updateNewMessage", "message": pending}), now);
        assert_eq!(
            client.panel_view(5).unwrap().messages.last().unwrap().sending,
            Some("pending")
        );
        let sent = json!({"@type": "updateMessageSendSucceeded", "old_message_id": 120, "message": message(5, 130, "answer", true)});
        client.handle(&sent, now);
        let view = client.panel_view(5).unwrap();
        let last = view.messages.last().unwrap();
        assert_eq!((last.id, last.sending, view.messages.len()), (130, None, 5));

        // A message that the sender deleted goes away.
        let deleted = json!({"@type": "updateDeleteMessages", "chat_id": 5, "message_ids": [80], "is_permanent": true, "from_cache": false});
        assert_eq!(client.handle(&deleted, now).events, [Event::Panel { chat_id: 5 }]);
        let cache = json!({"@type": "updateDeleteMessages", "chat_id": 5, "message_ids": [90], "is_permanent": false, "from_cache": true});
        assert!(client.handle(&cache, now).events.is_empty());
        assert_eq!(client.panel_view(5).unwrap().messages.len(), 4);

        assert_eq!(types(&client.panel_close()), ["closeChat"]);
        assert!(client.panel_close().is_empty());
        assert_eq!(client.panel_view(5), None);
    }

    #[test]
    fn a_second_panel_closes_the_first() {
        let mut client = ready(Route::Direct);
        client.panel_open(5);
        let requests = client.panel_open(6);
        assert_eq!(types(&requests), ["closeChat", "openChat"]);
        assert_eq!(
            (&requests[0]["chat_id"], &requests[1]["chat_id"]),
            (&json!(5), &json!(6))
        );
        assert_eq!(client.panel_chat(), Some(6));
    }

    #[test]
    fn the_panel_keeps_a_limited_number_of_messages() {
        let mut client = ready(Route::Direct);
        client.panel_open(5);
        let messages: Vec<Value> = (1..=(PANEL_MESSAGES as i64 + 50))
            .map(|id| message(5, id, "x", false))
            .collect();
        client.panel_history(5, &json!({"messages": messages}));
        let view = client.panel_view(5).unwrap();
        assert_eq!(view.messages.len(), PANEL_MESSAGES);
        assert_eq!(view.messages[0].id, 51);
    }
}
