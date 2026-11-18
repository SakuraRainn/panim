//! Messages in the simple text form of PANIM.
//!
//! PANIM shows the text of a text message. For each other content type, it shows a label. The connector gives the
//! kind of the label as a code, and the user interface has the text of each kind in its languages. TDLib 1.8.68 has
//! more than 100 content types. An unknown type gets the kind `other`.

use serde::Serialize;
use serde_json::Value;

/// A text in a timeline item has no more than this number of characters.
pub const ITEM_TEXT_CHARS: usize = 300;

/// A message in the chat panel has no more than this number of characters. Telegram permits 4096.
pub const MESSAGE_TEXT_CHARS: usize = 4096;

/// All the kinds that the user interface has a label for.
pub const KINDS: [&str; 30] = [
    "photo",
    "video",
    "animation",
    "audio",
    "voice",
    "video_note",
    "document",
    "sticker",
    "location",
    "contact",
    "poll",
    "call",
    "game",
    "invoice",
    "story",
    "dice",
    "checklist",
    "pin",
    "join",
    "leave",
    "chat_create",
    "chat_change",
    "joined_telegram",
    "gift",
    "payment",
    "giveaway",
    "expired",
    "screenshot",
    "unsupported",
    "other",
];

/// The kind and the text of a message content.
///
/// A text message has no kind. A media message has its kind, and its caption as the text.
pub fn content_summary(content: &Value) -> (Option<&'static str>, String) {
    let name = content["@type"].as_str().unwrap_or("");
    let caption = || content["caption"]["text"].as_str().unwrap_or("").to_string();
    let kind = match name {
        "messageText" => return (None, content["text"]["text"].as_str().unwrap_or("").to_string()),
        // An emoji that Telegram shows with an animation is text.
        "messageAnimatedEmoji" => return (None, content["emoji"].as_str().unwrap_or("").to_string()),
        "messagePhoto" => "photo",
        "messageVideo" => "video",
        "messageAnimation" => "animation",
        "messageAudio" => "audio",
        "messageVoiceNote" => "voice",
        "messageVideoNote" => "video_note",
        "messageDocument" => "document",
        "messageSticker" => "sticker",
        "messageLocation" | "messageLiveLocation" | "messageVenue" => "location",
        "messageContact" => "contact",
        "messagePoll" => "poll",
        "messageCall" | "messageGroupCall" => "call",
        "messageGame" | "messageGameScore" => "game",
        "messageInvoice" | "messagePaidMedia" => "invoice",
        "messageStory" => "story",
        "messageDice" | "messageStakeDice" => "dice",
        "messageChecklist" | "messageChecklistTasksDone" | "messageChecklistTasksAdded" => "checklist",
        "messagePinMessage" => "pin",
        "messageChatAddMembers" | "messageChatJoinByLink" | "messageChatJoinByRequest" => "join",
        "messageChatDeleteMember" => "leave",
        "messageBasicGroupChatCreate" | "messageSupergroupChatCreate" => "chat_create",
        "messageChatChangeTitle" | "messageChatChangePhoto" | "messageChatDeletePhoto" => "chat_change",
        "messageContactRegistered" => "joined_telegram",
        "messageGift"
        | "messageUpgradedGift"
        | "messageGiftedPremium"
        | "messageGiftedStars"
        | "messagePremiumGiftCode" => "gift",
        "messagePaymentSuccessful" | "messagePaymentRefunded" => "payment",
        "messageGiveaway" | "messageGiveawayCreated" | "messageGiveawayCompleted" | "messageGiveawayWinners" => {
            "giveaway"
        }
        "messageExpiredPhoto" | "messageExpiredVideo" | "messageExpiredVideoNote" | "messageExpiredVoiceNote" => {
            "expired"
        }
        "messageScreenshotTaken" => "screenshot",
        "messageUnsupported" => "unsupported",
        _ => "other",
    };
    (Some(kind), caption())
}

/// The first characters of a text.
pub fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A message in the chat panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MessageView {
    pub id: i64,
    /// The time in milliseconds after 1970-01-01 00:00:00 UTC.
    pub time: i64,
    /// True if the account of the person sent the message.
    pub outgoing: bool,
    /// The name of the sender. It is empty for an outgoing message and when TDLib did not tell the name.
    pub sender: String,
    /// The text of a text message, or the caption of a media message.
    pub text: String,
    /// The kind of the label for a message that is not text.
    pub kind: Option<&'static str>,
    /// `pending` while Telegram did not accept the message, `failed` if it refused the message.
    pub sending: Option<&'static str>,
}

/// The sender of a message: a user or a chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sender {
    User(i64),
    Chat(i64),
    Unknown,
}

pub fn sender_of(message: &Value) -> Sender {
    let sender = &message["sender_id"];
    match sender["@type"].as_str().unwrap_or("") {
        "messageSenderUser" => sender["user_id"].as_i64().map_or(Sender::Unknown, Sender::User),
        "messageSenderChat" => sender["chat_id"].as_i64().map_or(Sender::Unknown, Sender::Chat),
        _ => Sender::Unknown,
    }
}

/// Makes the view of a message. `sender` is the name of the sender.
pub fn message_view(message: &Value, sender: String) -> Option<MessageView> {
    let id = message["id"].as_i64()?;
    let (kind, text) = content_summary(&message["content"]);
    let outgoing = message["is_outgoing"].as_bool().unwrap_or(false);
    let sending = match message["sending_state"]["@type"].as_str() {
        Some("messageSendingStatePending") => Some("pending"),
        Some("messageSendingStateFailed") => Some("failed"),
        _ => None,
    };
    Some(MessageView {
        id,
        time: message["date"].as_i64().unwrap_or(0) * 1000,
        outgoing,
        sender: if outgoing { String::new() } else { sender },
        text: cut(&text, MESSAGE_TEXT_CHARS),
        kind,
        sending,
    })
}

/// The request that sends a plain text message. The text has no formatting, and Telegram makes no link preview.
pub fn send_text_request(chat_id: i64, text: &str) -> Value {
    serde_json::json!({
        "@type": "sendMessage",
        "chat_id": chat_id,
        "input_message_content": {
            "@type": "inputMessageText",
            "text": {"@type": "formattedText", "text": text, "entities": []},
            "link_preview_options": {"@type": "linkPreviewOptions", "is_disabled": true},
            "clear_draft": false,
        },
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn text_and_labels() {
        let text = json!({"@type": "messageText", "text": {"@type": "formattedText", "text": "hello"}});
        assert_eq!(content_summary(&text), (None, "hello".into()));
        let photo = json!({"@type": "messagePhoto", "caption": {"text": "the cat"}});
        assert_eq!(content_summary(&photo), (Some("photo"), "the cat".into()));
        let voice = json!({"@type": "messageVoiceNote", "caption": {"text": ""}});
        assert_eq!(content_summary(&voice), (Some("voice"), String::new()));
        let emoji = json!({"@type": "messageAnimatedEmoji", "emoji": "👍"});
        assert_eq!(content_summary(&emoji), (None, "👍".into()));
        let new_type = json!({"@type": "messageTypeOfTheFuture"});
        assert_eq!(content_summary(&new_type), (Some("other"), String::new()));
    }

    #[test]
    fn each_kind_is_in_the_list() {
        let types = [
            "messagePhoto",
            "messageVideo",
            "messageAnimation",
            "messageAudio",
            "messageVoiceNote",
            "messageVideoNote",
            "messageDocument",
            "messageSticker",
            "messageVenue",
            "messageContact",
            "messagePoll",
            "messageCall",
            "messageGame",
            "messageInvoice",
            "messageStory",
            "messageDice",
            "messageChecklist",
            "messagePinMessage",
            "messageChatAddMembers",
            "messageChatDeleteMember",
            "messageSupergroupChatCreate",
            "messageChatChangeTitle",
            "messageContactRegistered",
            "messageGift",
            "messagePaymentSuccessful",
            "messageGiveaway",
            "messageExpiredPhoto",
            "messageScreenshotTaken",
            "messageUnsupported",
            "messageRichMessage",
        ];
        let mut found = std::collections::BTreeSet::new();
        for name in types {
            let kind = content_summary(&json!({"@type": name})).0.unwrap();
            assert!(KINDS.contains(&kind), "{name} {kind}");
            found.insert(kind);
        }
        assert_eq!(found.len(), KINDS.len());
    }

    #[test]
    fn message_views() {
        let message = json!({"id": 7340032, "date": 1794800000, "is_outgoing": false, "chat_id": 5,
            "sender_id": {"@type": "messageSenderUser", "user_id": 42},
            "content": {"@type": "messageText", "text": {"text": "hi"}}});
        assert_eq!(sender_of(&message), Sender::User(42));
        let view = message_view(&message, "Alice".into()).unwrap();
        assert_eq!((view.id, view.time, view.outgoing), (7340032, 1794800000000, false));
        assert_eq!(
            (view.sender.as_str(), view.text.as_str(), view.kind),
            ("Alice", "hi", None)
        );

        let own = json!({"id": 8, "date": 1, "is_outgoing": true,
            "sending_state": {"@type": "messageSendingStatePending"},
            "content": {"@type": "messageSticker"}});
        let view = message_view(&own, "Me".into()).unwrap();
        assert_eq!(
            (view.sender.as_str(), view.kind, view.sending),
            ("", Some("sticker"), Some("pending"))
        );
    }

    #[test]
    fn send_request_is_plain_text() {
        let request = send_text_request(-100123, "*not bold* https://example.org");
        let content = &request["input_message_content"];
        assert_eq!(request["@type"], "sendMessage");
        assert_eq!(request["chat_id"], -100123);
        assert_eq!(content["@type"], "inputMessageText");
        assert_eq!(content["text"]["text"], "*not bold* https://example.org");
        assert_eq!(content["text"]["entities"], json!([]));
        assert_eq!(content["link_preview_options"]["is_disabled"], true);
    }
}
