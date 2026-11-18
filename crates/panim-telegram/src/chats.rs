//! The chats of one account, in memory, and the timeline items that they give.
//!
//! The model reads the updates of TDLib. It keeps only what the timeline needs: the title, the last message, the
//! position in the main chat list, the unread count and the mute settings.
//!
//! These are the rules (`docs/ARCHITECTURE.md` §4b):
//!
//! - A chat gives one timeline item if it has a position in the main chat list, has a last message and is not
//!   muted. An archived chat has no position in the main chat list.
//! - If the chat uses the default mute setting, the setting of its scope applies (private chats, groups or
//!   channels). While the model does not know that setting, the chat gives no item.
//! - The item is unread if Telegram counts unread messages in the chat, or if the person marked the chat as unread
//!   in a Telegram app. A chat that has only an unread reaction is read.
//! - A read in PANIM applies immediately. An older update does not make the item unread again while Telegram did
//!   not confirm the read. A new message that comes in does.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use panim_core::{NewItem, Timestamp};
use serde_json::Value;

use crate::messages::{ITEM_TEXT_CHARS, KINDS, Sender, content_summary, cut};

/// The start of the source identifier of a chat item.
pub const CHAT_PREFIX: &str = "c/";

/// The source identifier of the timeline item of a chat.
pub fn source_id(chat_id: i64) -> String {
    format!("{CHAT_PREFIX}{chat_id}")
}

/// The chat of a source identifier.
pub fn chat_of(source_id: &str) -> Option<i64> {
    source_id.strip_prefix(CHAT_PREFIX)?.parse().ok()
}

/// The three groups of chats that have a default notification setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Private,
    Group,
    Channel,
}

impl Scope {
    pub const ALL: [Scope; 3] = [Scope::Private, Scope::Group, Scope::Channel];

    /// The name of the scope in the requests of TDLib.
    pub fn type_name(self) -> &'static str {
        match self {
            Scope::Private => "notificationSettingsScopePrivateChats",
            Scope::Group => "notificationSettingsScopeGroupChats",
            Scope::Channel => "notificationSettingsScopeChannelChats",
        }
    }

    fn read(value: &Value) -> Option<Self> {
        Scope::ALL.into_iter().find(|scope| value["@type"] == scope.type_name())
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Private,
    Secret,
    BasicGroup(i64),
    Supergroup { id: i64, channel: bool },
    Unknown,
}

/// The status of the account in a group or a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Creator,
    Administrator { can_post: bool },
    Member,
    Restricted { can_send: bool },
    Out,
}

impl Status {
    fn read(status: &Value) -> Option<Self> {
        Some(match status["@type"].as_str()? {
            "chatMemberStatusCreator" => Status::Creator,
            "chatMemberStatusAdministrator" => Status::Administrator {
                can_post: status["rights"]["can_post_messages"].as_bool().unwrap_or(false),
            },
            "chatMemberStatusMember" => Status::Member,
            "chatMemberStatusRestricted" => Status::Restricted {
                can_send: status["is_member"].as_bool().unwrap_or(false)
                    && status["permissions"]["can_send_basic_messages"]
                        .as_bool()
                        .unwrap_or(false),
            },
            _ => Status::Out,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Last {
    id: i64,
    /// Seconds after 1970-01-01 00:00:00 UTC.
    date: i64,
    outgoing: bool,
    kind: Option<&'static str>,
    text: String,
}

#[derive(Clone, Debug)]
struct Chat {
    title: String,
    kind: Kind,
    last: Option<Last>,
    in_main: bool,
    unread_count: u32,
    marked_unread: bool,
    use_default_mute: bool,
    mute_for: i64,
    can_send_basic: bool,
    /// The last message that the person read in PANIM, until Telegram confirms the read.
    pending_read: Option<i64>,
}

/// The data of one chat item, as the connector gives it to the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatItem {
    pub chat_id: i64,
    pub title: String,
    /// The time of the last message, in milliseconds after 1970-01-01 00:00:00 UTC.
    pub time: i64,
    /// The text of the last message, or its caption.
    pub text: String,
    /// The kind of the label, if the last message is not text.
    pub kind: Option<&'static str>,
    /// The unread count of Telegram for the chat.
    pub unread: u32,
    /// True if the person marked the chat as unread in a Telegram app.
    pub marked: bool,
    /// True if the item is read in PANIM.
    pub read: bool,
    /// True if the account sent the last message.
    pub outgoing: bool,
    /// True if the account can write in the chat.
    pub can_write: bool,
}

impl ChatItem {
    /// The item for the local store. The raw record is a small JSON object that `item_summary` reads.
    pub fn to_new_item(&self) -> NewItem {
        let raw = serde_json::json!({
            "v": 1,
            "kind": self.kind,
            "unread": self.unread,
            "marked": self.marked,
            "outgoing": self.outgoing,
            "can_write": self.can_write,
        });
        NewItem {
            source_id: source_id(self.chat_id),
            author: self.title.clone(),
            time: Timestamp(self.time),
            text: self.text.clone(),
            link: None,
            raw: raw.to_string().into_bytes(),
        }
    }
}

/// What the user interface needs from the raw record of a chat item.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ItemSummary {
    /// The kind of the label of the last message. `None` for a text message.
    pub kind: Option<&'static str>,
    /// The number of unread messages in the chat.
    pub unread: u32,
    /// True if the person marked the chat as unread in a Telegram app.
    pub marked: bool,
    /// True if the account sent the last message.
    pub outgoing: bool,
    /// True if the account can write in the chat.
    pub can_write: bool,
}

/// Reads the raw record of a chat item.
pub fn item_summary(raw: &[u8]) -> Option<ItemSummary> {
    let value: Value = serde_json::from_slice(raw).ok()?;
    if value["v"] != 1 {
        return None;
    }
    let kind = value["kind"]
        .as_str()
        .map(|kind| KINDS.into_iter().find(|known| *known == kind).unwrap_or("other"));
    Some(ItemSummary {
        kind,
        unread: value["unread"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        marked: value["marked"].as_bool().unwrap_or(false),
        outgoing: value["outgoing"].as_bool().unwrap_or(false),
        can_write: value["can_write"].as_bool().unwrap_or(false),
    })
}

/// The chats of one account.
#[derive(Default)]
pub struct Model {
    chats: BTreeMap<i64, Chat>,
    /// The default mute time of each scope, in seconds. `None` until TDLib tells it.
    scope_mute: [Option<i64>; 3],
    users: HashMap<i64, String>,
    supergroups: HashMap<i64, Status>,
    basic_groups: HashMap<i64, Status>,
    /// The chats that changed after the last call of `take_dirty`.
    dirty: BTreeSet<i64>,
}

/// An `int64` of TDLib is a text in JSON. An `int53` is a number.
fn int(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

/// True if the list of positions has a position in the main chat list.
fn in_main(positions: &Value) -> bool {
    positions.as_array().is_some_and(|positions| {
        positions
            .iter()
            .any(|position| position["list"]["@type"] == "chatListMain" && int(&position["order"]).unwrap_or(0) != 0)
    })
}

fn read_last(message: &Value) -> Option<Last> {
    let id = message["id"].as_i64()?;
    let (kind, text) = content_summary(&message["content"]);
    Some(Last {
        id,
        date: message["date"].as_i64().unwrap_or(0),
        outgoing: message["is_outgoing"].as_bool().unwrap_or(false),
        kind,
        text: cut(&text, ITEM_TEXT_CHARS),
    })
}

impl Model {
    /// Reads one update of TDLib. Returns false if the model does not use this type of update.
    pub fn apply(&mut self, update: &Value) -> bool {
        let chat_id = update["chat_id"].as_i64();
        match update["@type"].as_str().unwrap_or("") {
            "updateNewChat" => {
                let chat = &update["chat"];
                let Some(id) = chat["id"].as_i64() else { return true };
                let kind = &chat["type"];
                let kind = match kind["@type"].as_str().unwrap_or("") {
                    "chatTypePrivate" => Kind::Private,
                    "chatTypeSecret" => Kind::Secret,
                    "chatTypeBasicGroup" => kind["basic_group_id"].as_i64().map_or(Kind::Unknown, Kind::BasicGroup),
                    "chatTypeSupergroup" => match kind["supergroup_id"].as_i64() {
                        Some(id) => Kind::Supergroup {
                            id,
                            channel: kind["is_channel"].as_bool().unwrap_or(false),
                        },
                        None => Kind::Unknown,
                    },
                    _ => Kind::Unknown,
                };
                let settings = &chat["notification_settings"];
                self.chats.insert(
                    id,
                    Chat {
                        title: chat["title"].as_str().unwrap_or("").to_string(),
                        kind,
                        last: read_last(&chat["last_message"]),
                        in_main: in_main(&chat["positions"]),
                        unread_count: chat["unread_count"].as_u64().unwrap_or(0) as u32,
                        marked_unread: chat["is_marked_as_unread"].as_bool().unwrap_or(false),
                        use_default_mute: settings["use_default_mute_for"].as_bool().unwrap_or(true),
                        mute_for: settings["mute_for"].as_i64().unwrap_or(0),
                        can_send_basic: chat["permissions"]["can_send_basic_messages"].as_bool().unwrap_or(true),
                        pending_read: None,
                    },
                );
                self.dirty.insert(id);
            }
            "updateChatTitle" => self.change(chat_id, |chat| {
                chat.title = update["title"].as_str().unwrap_or("").to_string();
            }),
            "updateChatLastMessage" => self.change(chat_id, |chat| {
                chat.last = read_last(&update["last_message"]);
                chat.in_main = in_main(&update["positions"]);
                // A new message that comes in is unread, so the local read no longer applies.
                if let (Some(last), Some(pending)) = (&chat.last, chat.pending_read)
                    && !last.outgoing
                    && last.id > pending
                {
                    chat.pending_read = None;
                }
            }),
            "updateChatDraftMessage" => self.change(chat_id, |chat| chat.in_main = in_main(&update["positions"])),
            "updateChatPosition" => self.change(chat_id, |chat| {
                let position = &update["position"];
                if position["list"]["@type"] == "chatListMain" {
                    chat.in_main = int(&position["order"]).unwrap_or(0) != 0;
                }
            }),
            "updateChatReadInbox" => self.change(chat_id, |chat| {
                chat.unread_count = update["unread_count"].as_u64().unwrap_or(0) as u32;
                let read_to = update["last_read_inbox_message_id"].as_i64().unwrap_or(0);
                if chat.pending_read.is_some_and(|pending| pending <= read_to) {
                    chat.pending_read = None;
                }
            }),
            "updateChatIsMarkedAsUnread" => self.change(chat_id, |chat| {
                chat.marked_unread = update["is_marked_as_unread"].as_bool().unwrap_or(false);
            }),
            "updateChatNotificationSettings" => self.change(chat_id, |chat| {
                let settings = &update["notification_settings"];
                chat.use_default_mute = settings["use_default_mute_for"].as_bool().unwrap_or(true);
                chat.mute_for = settings["mute_for"].as_i64().unwrap_or(0);
            }),
            "updateChatPermissions" => self.change(chat_id, |chat| {
                chat.can_send_basic = update["permissions"]["can_send_basic_messages"]
                    .as_bool()
                    .unwrap_or(true);
            }),
            "updateScopeNotificationSettings" => {
                self.set_scope(&update["scope"], &update["notification_settings"]);
            }
            "updateUser" => {
                let user = &update["user"];
                if let Some(id) = user["id"].as_i64() {
                    let first = user["first_name"].as_str().unwrap_or("");
                    let last = user["last_name"].as_str().unwrap_or("");
                    self.users.insert(id, format!("{first} {last}").trim().to_string());
                }
            }
            "updateSupergroup" => {
                let group = &update["supergroup"];
                if let (Some(id), Some(status)) = (group["id"].as_i64(), Status::read(&group["status"])) {
                    self.supergroups.insert(id, status);
                    self.mark_all_dirty();
                }
            }
            "updateBasicGroup" => {
                let group = &update["basic_group"];
                if let (Some(id), Some(status)) = (group["id"].as_i64(), Status::read(&group["status"])) {
                    let status = if group["is_active"].as_bool().unwrap_or(true) {
                        status
                    } else {
                        Status::Out
                    };
                    self.basic_groups.insert(id, status);
                    self.mark_all_dirty();
                }
            }
            _ => return false,
        }
        true
    }

    /// Sets the default notification setting of a scope. `scope` has the type name of the scope.
    pub fn set_scope(&mut self, scope: &Value, settings: &Value) {
        if let Some(scope) = Scope::read(scope) {
            self.scope_mute[scope.index()] = Some(settings["mute_for"].as_i64().unwrap_or(0));
            self.mark_all_dirty();
        }
    }

    fn change(&mut self, chat_id: Option<i64>, f: impl FnOnce(&mut Chat)) {
        if let Some(id) = chat_id
            && let Some(chat) = self.chats.get_mut(&id)
        {
            f(chat);
            self.dirty.insert(id);
        }
    }

    fn mark_all_dirty(&mut self) {
        self.dirty.extend(self.chats.keys().copied());
    }

    /// The chats that changed after the last call.
    pub fn take_dirty(&mut self) -> BTreeSet<i64> {
        std::mem::take(&mut self.dirty)
    }

    fn scope_of(kind: Kind) -> Scope {
        match kind {
            Kind::Supergroup { channel: true, .. } => Scope::Channel,
            Kind::BasicGroup(_) | Kind::Supergroup { .. } => Scope::Group,
            Kind::Private | Kind::Secret | Kind::Unknown => Scope::Private,
        }
    }

    /// True if the chat is muted. `None` if the default setting of its scope is not known yet.
    fn muted(&self, chat: &Chat) -> Option<bool> {
        let mute_for = if chat.use_default_mute {
            self.scope_mute[Self::scope_of(chat.kind).index()]?
        } else {
            chat.mute_for
        };
        Some(mute_for > 0)
    }

    /// True if the account can write a text message in the chat. TDLib can still refuse a message.
    pub fn can_write(&self, chat_id: i64) -> bool {
        let Some(chat) = self.chats.get(&chat_id) else {
            return false;
        };
        let by_status = |status: Option<&Status>, channel: bool| match status {
            Some(Status::Creator) => true,
            Some(Status::Administrator { can_post }) => !channel || *can_post,
            Some(Status::Member) | None => !channel && chat.can_send_basic,
            Some(Status::Restricted { can_send }) => !channel && *can_send && chat.can_send_basic,
            Some(Status::Out) => false,
        };
        match chat.kind {
            Kind::Private => true,
            // PANIM does not use secret chats.
            Kind::Secret | Kind::Unknown => false,
            Kind::BasicGroup(id) => by_status(self.basic_groups.get(&id), false),
            Kind::Supergroup { id, channel } => by_status(self.supergroups.get(&id), channel),
        }
    }

    /// The timeline item of a chat, or `None` if the chat gives no item.
    pub fn item(&self, chat_id: i64) -> Option<ChatItem> {
        let chat = self.chats.get(&chat_id)?;
        let last = chat.last.as_ref()?;
        if !chat.in_main || self.muted(chat) != Some(false) {
            return None;
        }
        let unread = chat.unread_count > 0 || chat.marked_unread;
        Some(ChatItem {
            chat_id,
            title: chat.title.clone(),
            time: last.date * 1000,
            text: last.text.clone(),
            kind: last.kind,
            unread: chat.unread_count,
            marked: chat.marked_unread,
            read: !unread || chat.pending_read.is_some(),
            outgoing: last.outgoing,
            can_write: self.can_write(chat_id),
        })
    }

    /// The title of a chat.
    pub fn title(&self, chat_id: i64) -> Option<&str> {
        self.chats.get(&chat_id).map(|chat| chat.title.as_str())
    }

    /// True if the model has the chat.
    pub fn has_chat(&self, chat_id: i64) -> bool {
        self.chats.contains_key(&chat_id)
    }

    /// The name of the sender of a message.
    pub fn sender_name(&self, sender: Sender) -> String {
        match sender {
            Sender::User(id) => self.users.get(&id).cloned().unwrap_or_default(),
            Sender::Chat(id) => self.title(id).unwrap_or_default().to_string(),
            Sender::Unknown => String::new(),
        }
    }

    /// Records that the person read the chat in PANIM.
    ///
    /// Returns the last message and the "marked as unread" flag, if the chat is unread on Telegram. The caller then
    /// tells Telegram. Returns `None` if there is nothing to tell.
    pub fn read_locally(&mut self, chat_id: i64) -> Option<(i64, bool)> {
        let chat = self.chats.get_mut(&chat_id)?;
        let last = chat.last.as_ref()?.id;
        if chat.unread_count == 0 && !chat.marked_unread {
            return None;
        }
        chat.pending_read = Some(last);
        let marked = chat.marked_unread;
        self.dirty.insert(chat_id);
        Some((last, marked))
    }

    /// Records that Telegram refused the read of a chat. The item shows the state of Telegram again.
    pub fn read_failed(&mut self, chat_id: i64) {
        self.change(Some(chat_id), |chat| chat.pending_read = None);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const MAIN: i64 = 9_221_294_780_000_000_000;

    fn text_message(id: i64, date: i64, text: &str, outgoing: bool) -> Value {
        json!({"@type": "message", "id": id, "date": date, "is_outgoing": outgoing, "chat_id": 0,
            "sender_id": {"@type": "messageSenderUser", "user_id": 42},
            "content": {"@type": "messageText", "text": {"@type": "formattedText", "text": text}}})
    }

    fn main_position() -> Value {
        // TDLib writes an `int64` as a text.
        json!([{"@type": "chatPosition", "list": {"@type": "chatListMain"}, "order": MAIN.to_string(), "is_pinned": false}])
    }

    fn new_chat(id: i64, title: &str, kind: Value, unread: u32) -> Value {
        json!({"@type": "updateNewChat", "chat": {
            "@type": "chat", "id": id, "title": title, "type": kind,
            "permissions": {"can_send_basic_messages": true},
            "last_message": text_message(100, 1794800000, "hello", false),
            "positions": main_position(), "unread_count": unread, "is_marked_as_unread": false,
            "notification_settings": {"use_default_mute_for": true, "mute_for": 0}}})
    }

    fn private() -> Value {
        json!({"@type": "chatTypePrivate", "user_id": 42})
    }

    fn channel(id: i64) -> Value {
        json!({"@type": "chatTypeSupergroup", "supergroup_id": id, "is_channel": true})
    }

    fn scope(model: &mut Model, scope: Scope, mute_for: i64) {
        model.apply(&json!({"@type": "updateScopeNotificationSettings",
            "scope": {"@type": scope.type_name()}, "notification_settings": {"mute_for": mute_for}}));
    }

    fn model() -> Model {
        let mut model = Model::default();
        for s in Scope::ALL {
            scope(&mut model, s, 0);
        }
        model
    }

    #[test]
    fn a_chat_in_the_main_list_gives_an_item() {
        let mut model = model();
        assert!(model.apply(&new_chat(5, "Alice", private(), 3)));
        let item = model.item(5).unwrap();
        assert_eq!(
            (item.title.as_str(), item.text.as_str(), item.kind),
            ("Alice", "hello", None)
        );
        assert_eq!((item.time, item.unread, item.read), (1794800000000, 3, false));
        assert!(item.can_write);
        assert_eq!(model.take_dirty().into_iter().collect::<Vec<_>>(), vec![5]);
        assert!(model.take_dirty().is_empty());

        let new = item.to_new_item();
        assert_eq!((new.source_id.as_str(), new.author.as_str()), ("c/5", "Alice"));
        assert_eq!(new.time, Timestamp(1794800000000));
        let summary = item_summary(&new.raw).unwrap();
        assert_eq!((summary.kind, summary.unread, summary.can_write), (None, 3, true));
        assert_eq!(chat_of("c/5"), Some(5));
        assert_eq!(chat_of("c/-1001234"), Some(-1001234));
        assert_eq!(chat_of("n/5"), None);
    }

    #[test]
    fn the_scope_setting_is_necessary_for_a_default_chat() {
        let mut model = Model::default();
        model.apply(&new_chat(5, "Alice", private(), 1));
        assert_eq!(model.item(5), None, "the default setting is not known yet");
        scope(&mut model, Scope::Group, 0);
        assert_eq!(model.item(5), None, "this is the setting of a different scope");
        scope(&mut model, Scope::Private, 0);
        assert!(model.item(5).is_some());
        assert!(model.take_dirty().contains(&5));
    }

    #[test]
    fn muted_chats_give_no_item() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 1));
        model.apply(&new_chat(-1007, "News", channel(7), 9));

        // The chat has its own setting: muted.
        model.apply(&json!({"@type": "updateChatNotificationSettings", "chat_id": 5,
            "notification_settings": {"use_default_mute_for": false, "mute_for": 600}}));
        assert_eq!(model.item(5), None);
        // The person removes the mute.
        model.apply(&json!({"@type": "updateChatNotificationSettings", "chat_id": 5,
            "notification_settings": {"use_default_mute_for": false, "mute_for": 0}}));
        assert!(model.item(5).is_some());

        // The scope of the channels becomes muted. The private chat stays.
        scope(&mut model, Scope::Channel, 2_000_000_000);
        assert_eq!(model.item(-1007), None);
        assert!(model.item(5).is_some());
        // A channel with its own setting "not muted" shows when its scope is muted.
        model.apply(&json!({"@type": "updateChatNotificationSettings", "chat_id": -1007,
            "notification_settings": {"use_default_mute_for": false, "mute_for": 0}}));
        assert!(model.item(-1007).is_some());
    }

    #[test]
    fn archived_chats_and_chats_out_of_the_list_give_no_item() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 1));
        // The person moves the chat to the archive: the main list position goes away.
        model.apply(&json!({"@type": "updateChatPosition", "chat_id": 5, "position":
            {"list": {"@type": "chatListMain"}, "order": "0"}}));
        model.apply(&json!({"@type": "updateChatPosition", "chat_id": 5, "position":
            {"list": {"@type": "chatListArchive"}, "order": MAIN.to_string()}}));
        assert_eq!(model.item(5), None);
        // A new message moves the chat to the main list again.
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5,
            "last_message": text_message(101, 1794800100, "are you there?", false), "positions": main_position()}));
        assert_eq!(model.item(5).unwrap().text, "are you there?");
        // The chat has no last message, for example after the history was cleared.
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5, "last_message": null, "positions": []}));
        assert_eq!(model.item(5), None);

        // A chat that TDLib tells without a position is not in a list.
        let mut update = new_chat(6, "Bob", private(), 0);
        update["chat"]["positions"] = json!([]);
        model.apply(&update);
        assert_eq!(model.item(6), None);
        model.apply(&json!({"@type": "updateChatPosition", "chat_id": 6, "position":
            {"list": {"@type": "chatListMain"}, "order": MAIN}}));
        assert!(model.item(6).is_some());
    }

    #[test]
    fn read_state_follows_telegram() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 2));
        assert!(!model.item(5).unwrap().read);
        // The person reads the chat in a Telegram app.
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 100, "unread_count": 0}));
        let item = model.item(5).unwrap();
        assert!(item.read && item.unread == 0);
        // A new message comes.
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5,
            "last_message": text_message(101, 1794800100, "one more", false), "positions": main_position()}));
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 100, "unread_count": 1}));
        let item = model.item(5).unwrap();
        assert!(!item.read && item.unread == 1 && item.text == "one more");
    }

    #[test]
    fn a_local_read_stays_until_telegram_confirms_it() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 2));
        assert_eq!(model.read_locally(5), Some((100, false)));
        assert!(model.item(5).unwrap().read, "the read applies immediately");
        // An update from before the read does not make the item unread again.
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 99, "unread_count": 1}));
        assert!(model.item(5).unwrap().read);
        // Telegram confirms the read.
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 100, "unread_count": 0}));
        assert!(model.item(5).unwrap().read);
        assert_eq!(model.read_locally(5), None, "there is nothing to tell Telegram");

        // A message that comes after a local read is unread.
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 100, "unread_count": 1}));
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5,
            "last_message": text_message(101, 1794800100, "new", false), "positions": main_position()}));
        assert_eq!(model.read_locally(5), Some((101, false)));
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5,
            "last_message": text_message(102, 1794800200, "newer", false), "positions": main_position()}));
        assert!(!model.item(5).unwrap().read);
    }

    #[test]
    fn a_refused_read_shows_the_state_of_telegram_again() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 2));
        model.read_locally(5);
        model.read_failed(5);
        assert!(!model.item(5).unwrap().read);
    }

    #[test]
    fn a_chat_marked_as_unread_is_unread() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 0));
        assert!(model.item(5).unwrap().read);
        model.apply(&json!({"@type": "updateChatIsMarkedAsUnread", "chat_id": 5, "is_marked_as_unread": true}));
        let item = model.item(5).unwrap();
        assert!(!item.read && item.marked && item.unread == 0);
        assert_eq!(model.read_locally(5), Some((100, true)));
        model.apply(&json!({"@type": "updateChatIsMarkedAsUnread", "chat_id": 5, "is_marked_as_unread": false}));
        // The read inbox did not change, so the read needs no other confirmation.
        model.apply(&json!({"@type": "updateChatReadInbox", "chat_id": 5,
            "last_read_inbox_message_id": 100, "unread_count": 0}));
        assert!(model.item(5).unwrap().read);
    }

    #[test]
    fn labels_for_messages_that_are_not_text() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 1));
        let photo = json!({"@type": "message", "id": 101, "date": 1794800100, "is_outgoing": true,
            "content": {"@type": "messagePhoto", "caption": {"text": "the cat"}}});
        model.apply(
            &json!({"@type": "updateChatLastMessage", "chat_id": 5, "last_message": photo,
            "positions": main_position()}),
        );
        let item = model.item(5).unwrap();
        assert_eq!(
            (item.kind, item.text.as_str(), item.outgoing),
            (Some("photo"), "the cat", true)
        );
        let summary = item_summary(&item.to_new_item().raw).unwrap();
        assert_eq!((summary.kind, summary.outgoing), (Some("photo"), true));
        // A kind that this version does not know becomes the default kind.
        assert_eq!(
            item_summary(br#"{"v":1,"kind":"hologram","unread":2}"#).unwrap().kind,
            Some("other")
        );
        assert_eq!(item_summary(b"not json"), None);
        assert_eq!(item_summary(br#"{"v":2}"#), None);
    }

    #[test]
    fn long_texts_are_cut() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 1));
        let long = "界".repeat(1000);
        model.apply(&json!({"@type": "updateChatLastMessage", "chat_id": 5,
            "last_message": text_message(101, 1, &long, false), "positions": main_position()}));
        assert_eq!(model.item(5).unwrap().text.chars().count(), ITEM_TEXT_CHARS);
    }

    #[test]
    fn who_can_write() {
        let mut model = model();
        model.apply(&new_chat(5, "Alice", private(), 0));
        model.apply(&new_chat(-1007, "News", channel(7), 0));
        model.apply(&new_chat(
            -1008,
            "Team",
            json!({"@type": "chatTypeSupergroup", "supergroup_id": 8, "is_channel": false}),
            0,
        ));
        model.apply(&new_chat(
            -9,
            "Old group",
            json!({"@type": "chatTypeBasicGroup", "basic_group_id": 9}),
            0,
        ));
        model.apply(&new_chat(
            77,
            "Secret",
            json!({"@type": "chatTypeSecret", "secret_chat_id": 1, "user_id": 42}),
            0,
        ));
        let supergroup =
            |id: i64, status: Value| json!({"@type": "updateSupergroup", "supergroup": {"id": id, "status": status}});

        assert!(model.can_write(5));
        assert!(!model.can_write(77));
        assert!(!model.can_write(404));

        // A reader of a channel cannot write. An administrator with the right to post can.
        model.apply(&supergroup(7, json!({"@type": "chatMemberStatusMember"})));
        assert!(!model.can_write(-1007));
        assert!(!model.item(-1007).unwrap().can_write);
        model.apply(&supergroup(
            7,
            json!({"@type": "chatMemberStatusAdministrator", "rights": {"can_post_messages": false}}),
        ));
        assert!(!model.can_write(-1007));
        model.apply(&supergroup(
            7,
            json!({"@type": "chatMemberStatusAdministrator", "rights": {"can_post_messages": true}}),
        ));
        assert!(model.can_write(-1007));

        // A member of a group can write if the group permits text messages.
        model.apply(&supergroup(8, json!({"@type": "chatMemberStatusMember"})));
        assert!(model.can_write(-1008));
        model.apply(&json!({"@type": "updateChatPermissions", "chat_id": -1008,
            "permissions": {"can_send_basic_messages": false}}));
        assert!(!model.can_write(-1008));
        model.apply(&supergroup(8, json!({"@type": "chatMemberStatusCreator"})));
        assert!(model.can_write(-1008));
        model.apply(&supergroup(
            8,
            json!({"@type": "chatMemberStatusRestricted", "is_member": true,
            "permissions": {"can_send_basic_messages": false}}),
        ));
        assert!(!model.can_write(-1008));
        model.apply(&supergroup(8, json!({"@type": "chatMemberStatusBanned"})));
        assert!(!model.can_write(-1008));

        model.apply(
            &json!({"@type": "updateBasicGroup", "basic_group": {"id": 9, "is_active": true,
            "status": {"@type": "chatMemberStatusMember"}}}),
        );
        assert!(model.can_write(-9));
        model.apply(
            &json!({"@type": "updateBasicGroup", "basic_group": {"id": 9, "is_active": false,
            "status": {"@type": "chatMemberStatusMember"}}}),
        );
        assert!(!model.can_write(-9));
    }

    #[test]
    fn names_of_senders() {
        let mut model = model();
        model.apply(&new_chat(-1007, "News", channel(7), 0));
        model.apply(&json!({"@type": "updateUser", "user": {"id": 42, "first_name": "Alice", "last_name": ""}}));
        model.apply(&json!({"@type": "updateUser", "user": {"id": 43, "first_name": "Bob", "last_name": "Stone"}}));
        assert_eq!(model.sender_name(Sender::User(42)), "Alice");
        assert_eq!(model.sender_name(Sender::User(43)), "Bob Stone");
        assert_eq!(model.sender_name(Sender::Chat(-1007)), "News");
        assert_eq!(model.sender_name(Sender::User(1)), "");
    }

    #[test]
    fn unknown_updates_are_not_used() {
        let mut model = model();
        assert!(!model.apply(&json!({"@type": "updateOption", "name": "version"})));
        // An update for a chat that the model does not have changes nothing.
        assert!(model.apply(&json!({"@type": "updateChatTitle", "chat_id": 1, "title": "x"})));
        assert!(model.take_dirty().is_empty());
    }
}
