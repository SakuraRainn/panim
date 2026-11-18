//! The login states and the errors of TDLib, in the form that the app shows.
//!
//! The phone number, the login code and the password go only into the requests. They never go into an error, a
//! state or a log.

use std::fmt;

use serde::Serialize;
use serde_json::Value;

/// The authorization state of a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthState {
    /// The client did not tell its state yet, or it waits for its parameters.
    Starting,
    /// The person must give the phone number.
    WaitPhone,
    /// The person must give the login code.
    WaitCode {
        /// How the code comes. `CODE_ROUTES` has all the values.
        via: &'static str,
        /// The number of characters of the code. 0 if TDLib does not tell it.
        length: u32,
        /// A short text for the route, for example the first letter of an SMS word, or the start of the number that
        /// calls. It is empty for most routes.
        detail: String,
        /// The route of the next code after a resend. `None` if Telegram offers no other route.
        next: Option<&'static str>,
        /// The number of seconds before a resend is permitted, from the time of this state.
        timeout: u32,
    },
    /// The person must give the two-step password.
    WaitPassword {
        /// The hint that the person wrote for the password. It can be empty.
        hint: String,
    },
    /// The login needs a step that PANIM cannot do. The person cannot continue in PANIM.
    Unsupported {
        /// `email_address`, `email_code`, `other_device`, `premium` or `registration`.
        reason: &'static str,
    },
    /// The login is complete.
    Ready,
    LoggingOut,
    Closing,
    Closed,
}

impl AuthState {
    /// Reads the field `authorization_state` of `updateAuthorizationState`.
    pub fn read(state: &Value) -> Self {
        match state["@type"].as_str().unwrap_or("") {
            "authorizationStateWaitPhoneNumber" => Self::WaitPhone,
            "authorizationStateWaitCode" => {
                let info = &state["code_info"];
                let (via, length, detail) = code_route(&info["type"]).unwrap_or(("other", 0, String::new()));
                Self::WaitCode {
                    via,
                    length,
                    detail,
                    next: code_route(&info["next_type"]).map(|(next, _, _)| next),
                    timeout: info["timeout"].as_u64().unwrap_or(0).min(86_400) as u32,
                }
            }
            "authorizationStateWaitPassword" => Self::WaitPassword {
                hint: state["password_hint"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect(),
            },
            "authorizationStateWaitEmailAddress" => Self::Unsupported {
                reason: "email_address",
            },
            "authorizationStateWaitEmailCode" => Self::Unsupported { reason: "email_code" },
            "authorizationStateWaitOtherDeviceConfirmation" => Self::Unsupported { reason: "other_device" },
            "authorizationStateWaitPremiumPurchase" => Self::Unsupported { reason: "premium" },
            "authorizationStateWaitRegistration" => Self::Unsupported { reason: "registration" },
            "authorizationStateReady" => Self::Ready,
            "authorizationStateLoggingOut" => Self::LoggingOut,
            "authorizationStateClosing" => Self::Closing,
            "authorizationStateClosed" => Self::Closed,
            _ => Self::Starting,
        }
    }
}

/// All the routes of a login code. The user interface has a text for each route. `other` is for an unknown type.
pub const CODE_ROUTES: [&str; 10] = [
    "app",
    "sms",
    "sms_word",
    "sms_phrase",
    "call",
    "flash_call",
    "missed_call",
    "fragment",
    "firebase",
    "other",
];

/// Reads an `AuthenticationCodeType` of TDLib 1.8.68: the route, the length of the code and a short detail.
///
/// Returns `None` for a null value (no next route). The detail never contains the phone number of the person.
fn code_route(kind: &Value) -> Option<(&'static str, u32, String)> {
    let name = kind["@type"].as_str()?;
    let short = |field: &str| kind[field].as_str().unwrap_or("").chars().take(40).collect::<String>();
    let (via, detail) = match name {
        "authenticationCodeTypeTelegramMessage" => ("app", String::new()),
        "authenticationCodeTypeSms" => ("sms", String::new()),
        // The first letter of the word, or the first word of the phrase, if Telegram tells it.
        "authenticationCodeTypeSmsWord" => ("sms_word", short("first_letter")),
        "authenticationCodeTypeSmsPhrase" => ("sms_phrase", short("first_word")),
        "authenticationCodeTypeCall" => ("call", String::new()),
        // The number that calls is the code. The pattern shows the form of that number.
        "authenticationCodeTypeFlashCall" => ("flash_call", short("pattern")),
        // The last digits of the number that calls are the code. The detail is the start of that number.
        "authenticationCodeTypeMissedCall" => ("missed_call", short("phone_number_prefix")),
        // The code is on fragment.com, for an anonymous number. The user interface names the site, not the URL.
        "authenticationCodeTypeFragment" => ("fragment", String::new()),
        // Only the official apps on Android and iOS can do the check of Firebase.
        "authenticationCodeTypeFirebaseAndroid" | "authenticationCodeTypeFirebaseIos" => ("firebase", String::new()),
        _ => ("other", String::new()),
    };
    let length = kind["length"].as_u64().unwrap_or(0).min(32) as u32;
    Some((via, length, detail))
}

/// An error of TDLib or of Telegram, with a code that the user interface has a text for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TdError {
    /// The code, for example `telegram_code_invalid`.
    pub code: &'static str,
    /// For `telegram_flood`: the person must wait this number of seconds.
    pub seconds: Option<u64>,
    /// The message of TDLib, if it is safe to show. It is empty for a message that can contain private data.
    pub detail: String,
}

impl fmt::Display for TdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// Makes the error for an answer of the type `error`.
pub fn td_error(answer: &Value) -> TdError {
    let number = answer["code"].as_i64().unwrap_or(0);
    let message = answer["message"].as_str().unwrap_or("");
    let seconds = flood_seconds(number, message);
    let has = |part: &str| message.contains(part);
    let code = if seconds.is_some() || number == 429 || has("FLOOD") {
        "telegram_flood"
    } else if has("can't be resend") || has("SEND_CODE_UNAVAILABLE") {
        // TDLib has no next route, or Telegram has no other route for this login.
        "telegram_resend_none"
    } else if has("resendAuthenticationCode unexpected") {
        "telegram_resend_state"
    } else if has("PHONE_CODE_INVALID") || has("PHONE_CODE_EMPTY") {
        "telegram_code_invalid"
    } else if has("PHONE_CODE_EXPIRED") {
        "telegram_code_expired"
    } else if has("PASSWORD_HASH_INVALID") {
        "telegram_password_invalid"
    } else if has("PHONE_NUMBER_INVALID") {
        "telegram_phone_invalid"
    } else if has("PHONE_NUMBER_BANNED") || has("USER_DEACTIVATED") || has("PHONE_NUMBER_UNOCCUPIED") {
        "telegram_banned"
    } else if has("API_ID_INVALID") || has("API_ID_PUBLISHED_FLOOD") || has("UPDATE_APP_TO_LOGIN") {
        "telegram_app_refused"
    } else if has("Wrong database encryption key") {
        "telegram_database"
    } else if number == 401 || has("AUTH_KEY_UNREGISTERED") || has("SESSION_REVOKED") || has("SESSION_EXPIRED") {
        "telegram_logged_out"
    } else if has("CHAT_WRITE_FORBIDDEN")
        || has("Have no write access")
        || has("CHAT_SEND_PLAIN_FORBIDDEN")
        || has("USER_IS_BLOCKED")
        || has("YOU_BLOCKED_USER")
        || has("CHAT_RESTRICTED")
        || has("not enough rights")
        || has("Not enough rights")
    {
        "telegram_no_write"
    } else if has("Chat not found") || has("CHANNEL_PRIVATE") {
        "telegram_no_chat"
    } else {
        "telegram_error"
    };
    TdError {
        code,
        seconds,
        detail: safe_detail(message),
    }
}

/// The number of seconds of a flood wait. The forms are `FLOOD_WAIT_30`, `FLOOD_PREMIUM_WAIT_30` and
/// `Too Many Requests: retry after 30`.
fn flood_seconds(code: i64, message: &str) -> Option<u64> {
    let tail = if let Some((_, tail)) = message.split_once("retry after ") {
        tail
    } else if code == 420 || message.contains("FLOOD") {
        message.rsplit_once('_').map(|(_, tail)| tail)?
    } else {
        return None;
    };
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The message of TDLib for the detail of an error, or an empty text.
///
/// A message of Telegram is a short name, for example `PHONE_CODE_INVALID`. The function refuses a long message and
/// a message with five digits or more in sequence, because such a message can contain a phone number or a code.
fn safe_detail(message: &str) -> String {
    let allowed = message.len() <= 100
        && message
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _:.,-".contains(c));
    let mut run = 0;
    let long_number = message.chars().any(|c| {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        run >= 5
    });
    if allowed && !long_number {
        message.to_string()
    } else {
        String::new()
    }
}

/// Checks the form of a phone number and gives it with digits only, for TDLib.
///
/// The person gives the number with the country code. Spaces, `-`, `(`, `)` and one `+` at the start are permitted.
pub fn clean_phone(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text.strip_prefix('+').unwrap_or(text);
    let mut digits = String::new();
    for c in text.chars() {
        match c {
            '0'..='9' => digits.push(c),
            ' ' | '-' | '(' | ')' => {}
            _ => return None,
        }
    }
    (7..=15).contains(&digits.len()).then_some(digits)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn login_states() {
        let read = |value: Value| AuthState::read(&value);
        assert_eq!(
            read(json!({"@type": "authorizationStateWaitTdlibParameters"})),
            AuthState::Starting
        );
        assert_eq!(
            read(json!({"@type": "authorizationStateWaitPhoneNumber"})),
            AuthState::WaitPhone
        );
        assert_eq!(
            read(json!({"@type": "authorizationStateWaitCode", "code_info": {
                "phone_number": "15550001111",
                "type": {"@type": "authenticationCodeTypeTelegramMessage", "length": 5}}})),
            AuthState::WaitCode {
                via: "app",
                length: 5,
                detail: String::new(),
                next: None,
                timeout: 0
            }
        );
        assert_eq!(
            read(json!({"@type": "authorizationStateWaitCode", "code_info": {
                "type": {"@type": "authenticationCodeTypeSms", "length": 6},
                "next_type": {"@type": "authenticationCodeTypeCall", "length": 6}, "timeout": 120}})),
            AuthState::WaitCode {
                via: "sms",
                length: 6,
                detail: String::new(),
                next: Some("call"),
                timeout: 120
            }
        );
        assert_eq!(
            read(json!({"@type": "authorizationStateWaitPassword", "password_hint": "cat"})),
            AuthState::WaitPassword { hint: "cat".into() }
        );
        assert_eq!(read(json!({"@type": "authorizationStateReady"})), AuthState::Ready);
        assert_eq!(read(json!({"@type": "authorizationStateClosed"})), AuthState::Closed);
        let unsupported = [
            ("authorizationStateWaitEmailAddress", "email_address"),
            ("authorizationStateWaitEmailCode", "email_code"),
            ("authorizationStateWaitOtherDeviceConfirmation", "other_device"),
            ("authorizationStateWaitPremiumPurchase", "premium"),
            ("authorizationStateWaitRegistration", "registration"),
        ];
        for (name, reason) in unsupported {
            assert_eq!(
                read(json!({"@type": name})),
                AuthState::Unsupported { reason },
                "{name}"
            );
        }
    }

    #[test]
    fn each_route_of_the_code() {
        let route = |kind: Value| code_route(&kind).unwrap();
        let cases = [
            (
                json!({"@type": "authenticationCodeTypeTelegramMessage", "length": 5}),
                ("app", 5, ""),
            ),
            (
                json!({"@type": "authenticationCodeTypeSms", "length": 5}),
                ("sms", 5, ""),
            ),
            (
                json!({"@type": "authenticationCodeTypeSmsWord", "first_letter": "K"}),
                ("sms_word", 0, "K"),
            ),
            (
                json!({"@type": "authenticationCodeTypeSmsPhrase", "first_word": "apple"}),
                ("sms_phrase", 0, "apple"),
            ),
            (
                json!({"@type": "authenticationCodeTypeCall", "length": 5}),
                ("call", 5, ""),
            ),
            (
                json!({"@type": "authenticationCodeTypeFlashCall", "pattern": "+7*"}),
                ("flash_call", 0, "+7*"),
            ),
            (
                json!({"@type": "authenticationCodeTypeMissedCall", "phone_number_prefix": "+44 20", "length": 4}),
                ("missed_call", 4, "+44 20"),
            ),
            (
                json!({"@type": "authenticationCodeTypeFragment", "url": "https://fragment.com/x", "length": 5}),
                ("fragment", 5, ""),
            ),
            (
                json!({"@type": "authenticationCodeTypeFirebaseAndroid", "device_verification_parameters": {}, "length": 6}),
                ("firebase", 6, ""),
            ),
            (
                json!({"@type": "authenticationCodeTypeFirebaseIos", "receipt": "r", "push_timeout": 5, "length": 6}),
                ("firebase", 6, ""),
            ),
            (json!({"@type": "authenticationCodeTypeOfTheFuture"}), ("other", 0, "")),
        ];
        let mut found = std::collections::BTreeSet::new();
        for (kind, (via, length, detail)) in cases {
            assert_eq!(route(kind), (via, length, detail.to_string()));
            found.insert(via);
        }
        assert_eq!(found.len(), CODE_ROUTES.len());
        assert!(found.iter().all(|via| CODE_ROUTES.contains(via)));
        // A null next type is no route.
        assert_eq!(code_route(&Value::Null), None);
        // A state without a known type still asks for the code.
        let state = AuthState::read(&json!({"@type": "authorizationStateWaitCode", "code_info": {}}));
        assert!(matches!(
            state,
            AuthState::WaitCode {
                via: "other",
                next: None,
                timeout: 0,
                ..
            }
        ));
    }

    #[test]
    fn the_code_state_does_not_show_the_phone_number() {
        let state = AuthState::read(&json!({"@type": "authorizationStateWaitCode", "code_info": {
            "phone_number": "15550001111", "type": {"@type": "authenticationCodeTypeSms", "length": 5}}}));
        let text = serde_json::to_string(&state).unwrap();
        assert_eq!(
            text,
            r#"{"state":"wait_code","via":"sms","length":5,"detail":"","next":null,"timeout":0}"#
        );
    }

    #[test]
    fn errors_get_codes() {
        let code = |number: i64, message: &str| td_error(&json!({"code": number, "message": message})).code;
        assert_eq!(code(400, "PHONE_CODE_INVALID"), "telegram_code_invalid");
        assert_eq!(code(400, "PHONE_CODE_EXPIRED"), "telegram_code_expired");
        assert_eq!(code(400, "PASSWORD_HASH_INVALID"), "telegram_password_invalid");
        assert_eq!(code(400, "PHONE_NUMBER_INVALID"), "telegram_phone_invalid");
        assert_eq!(code(400, "PHONE_NUMBER_BANNED"), "telegram_banned");
        assert_eq!(code(401, "USER_DEACTIVATED_BAN"), "telegram_banned");
        assert_eq!(code(400, "API_ID_INVALID"), "telegram_app_refused");
        assert_eq!(code(401, "Wrong database encryption key"), "telegram_database");
        assert_eq!(code(401, "Unauthorized"), "telegram_logged_out");
        assert_eq!(code(400, "Have no write access to the chat"), "telegram_no_write");
        assert_eq!(code(400, "CHAT_WRITE_FORBIDDEN"), "telegram_no_write");
        assert_eq!(code(400, "Chat not found"), "telegram_no_chat");
        assert_eq!(code(500, "Request aborted"), "telegram_error");
        assert_eq!(code(400, "Authentication code can't be resend"), "telegram_resend_none");
        assert_eq!(code(400, "SEND_CODE_UNAVAILABLE"), "telegram_resend_none");
        assert_eq!(
            code(400, "Call to resendAuthenticationCode unexpected"),
            "telegram_resend_state"
        );
    }

    #[test]
    fn flood_wait_gives_the_seconds() {
        let error = td_error(&json!({"code": 429, "message": "Too Many Requests: retry after 37"}));
        assert_eq!((error.code, error.seconds), ("telegram_flood", Some(37)));
        let error = td_error(&json!({"code": 420, "message": "FLOOD_WAIT_86400"}));
        assert_eq!((error.code, error.seconds), ("telegram_flood", Some(86400)));
        let error = td_error(&json!({"code": 400, "message": "PHONE_PASSWORD_FLOOD"}));
        assert_eq!((error.code, error.seconds), ("telegram_flood", None));
    }

    #[test]
    fn detail_has_no_private_data() {
        let detail = |message: &str| td_error(&json!({"code": 400, "message": message})).detail;
        assert_eq!(detail("PHONE_CODE_INVALID"), "PHONE_CODE_INVALID");
        assert_eq!(detail("FLOOD_WAIT_30"), "FLOOD_WAIT_30");
        // A message with a long number can contain a phone number or a code.
        assert_eq!(detail("The number 15550001111 is not valid"), "");
        assert_eq!(detail("Code 12345 is wrong"), "");
        assert_eq!(detail("a message with \"quotes\""), "");
        assert_eq!(detail(&"A".repeat(101)), "");
    }

    #[test]
    fn phone_numbers() {
        assert_eq!(clean_phone("+1 (555) 000-1111").as_deref(), Some("15550001111"));
        assert_eq!(clean_phone(" 8613800138000 ").as_deref(), Some("8613800138000"));
        for bad in ["", "12345", "+1 555 abc", "1234567890123456", "++15550001111"] {
            assert_eq!(clean_phone(bad), None, "{bad}");
        }
    }
}
