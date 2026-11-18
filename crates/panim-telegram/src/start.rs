//! The requests of the start of a client: the log, the proxy and the parameters.
//!
//! The order is fixed (`docs/ARCHITECTURE.md` §4b):
//!
//! 1. Before the first client: `setLogVerbosityLevel` 0 and `setLogStream` with the empty stream. The default log of
//!    TDLib goes to stderr and contains the `api_id` and the `api_hash`.
//! 2. For each client, before `setTdlibParameters`: `addProxy` with `enable: true`, or `disableProxy` for a direct
//!    route. TDLib keeps this request and runs it before its network starts. TDLib also keeps the proxy in its
//!    database, so each start must send the request again.
//! 3. `setTdlibParameters`, when the client asks for it.

use std::fmt;
use std::net::IpAddr;
use std::path::Path;

use panim_core::{ProxyKind, Route, SecretBytes};
use serde_json::{Value, json};

use crate::Error;

include!(concat!(env!("OUT_DIR"), "/telegram_app.rs"));

/// The `api_id` and the `api_hash` of the app. The `Debug` form does not show them.
///
/// The values come from the build (PANIM's values) or from the person (their own values, `docs/ARCHITECTURE.md`
/// §4b). `new` makes sure that the values have the correct form: a positive number and 32 hexadecimal digits.
#[derive(Clone, PartialEq, Eq)]
pub struct AppConfig {
    api_id: i32,
    api_hash: String,
}

/// The `api_hash` has this number of hexadecimal digits.
pub const API_HASH_DIGITS: usize = 32;

impl AppConfig {
    /// The values from the build, or `None` if the build did not have them ("Telegram is not configured").
    pub fn from_build() -> Option<Self> {
        TELEGRAM_APP.map(|(api_id, api_hash)| Self {
            api_id,
            api_hash: api_hash.to_string(),
        })
    }

    /// Values that the person gives. The `api_id` is a positive number. The `api_hash` has 32 hexadecimal digits.
    ///
    /// The hash becomes lowercase. The error does not contain the values.
    pub fn new(api_id: i32, api_hash: &str) -> Result<Self, Error> {
        if api_id <= 0 {
            return Err(Error::BadInput("the api_id is not a positive number"));
        }
        let api_hash = api_hash.trim();
        if api_hash.len() != API_HASH_DIGITS || !api_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::BadInput("the api_hash does not have 32 hexadecimal digits"));
        }
        Ok(Self {
            api_id,
            api_hash: api_hash.to_ascii_lowercase(),
        })
    }

    /// Values from the text of two fields. The `api_id` has only digits.
    pub fn parse(api_id: &str, api_hash: &str) -> Result<Self, Error> {
        let api_id = api_id.trim();
        if api_id.is_empty() || api_id.len() > 10 || !api_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::BadInput("the api_id is not a number"));
        }
        let api_id = api_id
            .parse::<i32>()
            .map_err(|_| Error::BadInput("the api_id is too large"))?;
        Self::new(api_id, api_hash)
    }

    /// The `api_id`. It is not secret: Telegram shows it in the list of sessions.
    pub fn api_id(&self) -> i32 {
        self.api_id
    }

    /// The `api_hash`. Keep it sealed. Do not show it, log it or send it to the user interface.
    pub fn api_hash(&self) -> &str {
        &self.api_hash
    }

    /// Values for a test with the fake TDLib.
    #[cfg(any(test, feature = "fake"))]
    pub fn for_test() -> Self {
        Self {
            api_id: 12345,
            api_hash: "0123456789abcdef0123456789abcdef".into(),
        }
    }
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AppConfig(..)")
    }
}

/// True if the build has the `api_id` and the `api_hash`.
pub fn is_configured() -> bool {
    TELEGRAM_APP.is_some()
}

/// The length of the database key of an account, in bytes.
pub const KEY_BYTES: usize = 32;

/// Makes a random database key. Each account has its own key in the sealed secrets.
pub fn new_database_key() -> Result<SecretBytes, Error> {
    let mut key = vec![0u8; KEY_BYTES];
    getrandom::fill(&mut key).map_err(|_| Error::NoRandom)?;
    Ok(SecretBytes::new(key))
}

/// Makes a random name for the folder of an account: `telegram-` and 16 hexadecimal digits.
///
/// The name does not come from the account, so it does not show the phone number or the user id.
pub fn new_folder_name() -> Result<String, Error> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| Error::NoRandom)?;
    Ok(format!("telegram-{:016x}", u64::from_be_bytes(bytes)))
}

/// The two synchronous requests that stop the log of TDLib. Send them before the first client.
pub fn log_off() -> [Value; 2] {
    [
        json!({"@type": "setLogVerbosityLevel", "new_verbosity_level": 0}),
        json!({"@type": "setLogStream", "log_stream": {"@type": "logStreamEmpty"}}),
    ]
}

/// The proxy request for the route of the persona. It must go to TDLib before `setTdlibParameters`.
///
/// TDLib gets only an IP address. With a host name, TDLib must look up the name, and a lookup does not use the
/// route. Thus, a route with a host name gives an error for Telegram. The names `localhost` and `localhost.` are
/// the exception: they become `127.0.0.1` here, without a lookup.
pub fn proxy_request(route: &Route) -> Result<Value, Error> {
    let Route::Proxy(proxy) = route else {
        return Ok(json!({"@type": "disableProxy"}));
    };
    let host = proxy.host.trim();
    let literal = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let address: IpAddr = match literal.parse() {
        Ok(address) => address,
        Err(_) if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.") => {
            IpAddr::from([127, 0, 0, 1])
        }
        Err(_) => return Err(Error::ProxyHostName),
    };
    if proxy.port == 0 {
        return Err(Error::ProxyHostName);
    }
    let kind = match proxy.kind {
        ProxyKind::Socks5 => json!({"@type": "proxyTypeSocks5", "username": "", "password": ""}),
        // `http_only: false` makes TDLib use CONNECT for a TCP connection through the proxy.
        ProxyKind::Http => json!({"@type": "proxyTypeHttp", "username": "", "password": "", "http_only": false}),
    };
    Ok(json!({
        "@type": "addProxy",
        "proxy": {"@type": "proxy", "server": address.to_string(), "port": proxy.port, "type": kind},
        "enable": true,
        "comment": "",
    }))
}

/// True if the route is a proxy. The guard runs only then.
pub fn has_proxy(route: &Route) -> bool {
    matches!(route, Route::Proxy(_))
}

/// The request `setTdlibParameters`.
///
/// - The three databases are off. TDLib then writes only `td.binlog`, which holds the login.
/// - The key is never empty. With an empty key, TDLib uses a fixed key.
/// - The device model and the system version are fixed texts. TDLib does not get the name of the computer.
pub fn parameters(dir: &Path, key: &SecretBytes, app: &AppConfig, language: &str) -> Result<Value, Error> {
    if key.expose().len() != KEY_BYTES {
        return Err(Error::BadInput("the database key does not have 32 bytes"));
    }
    let language = if language.is_empty() { "en" } else { language };
    Ok(json!({
        "@type": "setTdlibParameters",
        "use_test_dc": false,
        "database_directory": dir.join("db").to_string_lossy(),
        "files_directory": dir.join("files").to_string_lossy(),
        "database_encryption_key": base64(key.expose()),
        "use_file_database": false,
        "use_chat_info_database": false,
        "use_message_database": false,
        "use_secret_chats": false,
        "api_id": app.api_id,
        "api_hash": app.api_hash,
        "system_language_code": language,
        "device_model": "PANIM",
        "system_version": "Linux",
        "application_version": env!("CARGO_PKG_VERSION"),
    }))
}

/// The JSON interface of TDLib takes bytes in the base64 form.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                text.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use panim_core::ProxyEndpoint;

    use super::*;

    fn route(kind: ProxyKind, host: &str, port: u16) -> Route {
        Route::Proxy(ProxyEndpoint {
            kind,
            host: host.into(),
            port,
        })
    }

    #[test]
    fn direct_route_stops_a_kept_proxy() {
        assert_eq!(proxy_request(&Route::Direct).unwrap(), json!({"@type": "disableProxy"}));
    }

    #[test]
    fn proxy_route_gives_an_ip_address() {
        let request = proxy_request(&route(ProxyKind::Socks5, "127.0.0.1", 9050)).unwrap();
        assert_eq!(request["@type"], "addProxy");
        assert_eq!(request["enable"], true);
        assert_eq!(request["proxy"]["server"], "127.0.0.1");
        assert_eq!(request["proxy"]["port"], 9050);
        assert_eq!(request["proxy"]["type"]["@type"], "proxyTypeSocks5");

        let request = proxy_request(&route(ProxyKind::Http, "[::1]", 8080)).unwrap();
        assert_eq!(request["proxy"]["server"], "::1");
        assert_eq!(request["proxy"]["type"]["@type"], "proxyTypeHttp");
        assert_eq!(request["proxy"]["type"]["http_only"], false);
    }

    #[test]
    fn loopback_name_becomes_an_address_without_a_lookup() {
        for name in ["localhost", "LOCALHOST", "localhost."] {
            let request = proxy_request(&route(ProxyKind::Socks5, name, 9050)).unwrap();
            assert_eq!(request["proxy"]["server"], "127.0.0.1", "{name}");
        }
    }

    #[test]
    fn host_name_is_refused() {
        for name in ["proxy.example.org", "tor", "localhost.example.org", ""] {
            assert_eq!(
                proxy_request(&route(ProxyKind::Socks5, name, 9050)),
                Err(Error::ProxyHostName),
                "{name}"
            );
        }
        assert_eq!(
            proxy_request(&route(ProxyKind::Socks5, "127.0.0.1", 0)),
            Err(Error::ProxyHostName)
        );
    }

    #[test]
    fn parameters_have_the_agreed_values() {
        let key = SecretBytes::new((0u8..32).collect());
        let app = AppConfig::for_test();
        let request = parameters(Path::new("/run/user/1/panim/x/telegram-ab"), &key, &app, "zh").unwrap();
        assert_eq!(request["use_test_dc"], false);
        for flag in [
            "use_file_database",
            "use_chat_info_database",
            "use_message_database",
            "use_secret_chats",
        ] {
            assert_eq!(request[flag], false, "{flag}");
        }
        assert_eq!(request["database_directory"], "/run/user/1/panim/x/telegram-ab/db");
        assert_eq!(request["files_directory"], "/run/user/1/panim/x/telegram-ab/files");
        assert_eq!(
            request["database_encryption_key"],
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
        );
        assert_eq!(request["system_language_code"], "zh");
        assert_eq!(request["device_model"], "PANIM");
        assert_eq!(request["system_version"], "Linux");
        assert_eq!(request["api_id"], 12345);
    }

    #[test]
    fn empty_or_short_key_is_refused() {
        let app = AppConfig::for_test();
        for length in [0, 16, 31, 33] {
            let key = SecretBytes::new(vec![7; length]);
            assert!(parameters(Path::new("/x"), &key, &app, "en").is_err(), "{length}");
        }
    }

    #[test]
    fn new_keys_are_random_and_have_32_bytes() {
        let (a, b) = (new_database_key().unwrap(), new_database_key().unwrap());
        assert_eq!(a.expose().len(), KEY_BYTES);
        assert_ne!(a.expose(), b.expose());
    }

    #[test]
    fn folder_names_are_random() {
        let (a, b) = (new_folder_name().unwrap(), new_folder_name().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.len(), 25);
        assert!(a.starts_with("telegram-"));
        assert!(
            a.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        );
    }

    #[test]
    fn debug_form_does_not_show_the_values() {
        assert_eq!(format!("{:?}", AppConfig::for_test()), "AppConfig(..)");
    }

    #[test]
    fn own_values_must_have_the_correct_form() {
        let hash = "0123456789ABCDEF0123456789abcdef";
        let app = AppConfig::parse(" 12345 ", hash).unwrap();
        assert_eq!(app.api_id(), 12345);
        assert_eq!(app.api_hash(), "0123456789abcdef0123456789abcdef");
        assert_eq!(app, AppConfig::new(12345, &format!(" {hash} ")).unwrap());
        for id in ["", "0", "-5", "12a", "1.5", "99999999999", "2147483648", "+7"] {
            assert!(AppConfig::parse(id, hash).is_err(), "{id:?}");
        }
        for bad in [
            "",
            "0123",
            "0123456789abcdef0123456789abcdeg",
            "0123456789abcdef0123456789abcdef0",
        ] {
            let error = AppConfig::parse("12345", bad).unwrap_err();
            assert!(matches!(error, Error::BadInput(_)), "{bad:?}");
            assert!(!error.to_string().contains("0123"), "the error does not show the value");
        }
    }

    #[test]
    fn base64_form() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
