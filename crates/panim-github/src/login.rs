//! The OAuth device flow, and the account of a token.
//!
//! PANIM is an OAuth App with the device flow. The flow has no client secret. Thus, the client ID is public.

use panim_core::SecretBytes;

use crate::api::{self, API, Error, Result, USER_AGENT, WEB};
use crate::http::{Http, Method, Request};

/// The client ID of the OAuth App "PANIM Desktop". It is public.
pub const CLIENT_ID: &str = "Ov23liVIHlgUHcwnpbFk";
/// The scopes that PANIM asks for.
///
/// `notifications` reads the notifications and marks them as read. `public_repo` writes comments in public
/// repositories. PANIM does not ask for access to private repositories.
pub const SCOPES: [&str; 2] = ["notifications", "public_repo"];
/// The page where the person enters the user code.
pub const DEVICE_PAGE: &str = "https://github.com/login/device";

const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// GitHub adds this number of seconds to the interval when it answers `slow_down`.
pub const SLOW_DOWN_STEP: u64 = 5;

/// A device login that waits for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceLogin {
    /// The code that PANIM sends when it polls. It is a secret.
    pub device_code: SecretBytes,
    /// The code that the person enters on the page of GitHub.
    pub user_code: String,
    /// The page where the person enters the code.
    pub verification_uri: String,
    /// The minimum number of seconds between two polls.
    pub interval: u64,
    /// The number of seconds until the codes expire.
    pub expires_in: u64,
}

/// The result of one poll.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    /// The person did not enter the code yet.
    Pending,
    /// PANIM polled too fast. `interval` is the new interval, if GitHub gave it.
    SlowDown { interval: Option<u64> },
    /// The person agreed. The token has all the scopes.
    Done { token: SecretBytes },
    /// The person agreed, but removed these scopes. PANIM does not keep the token.
    ScopeMissing { missing: Vec<String> },
    /// The codes expired. Start the login again.
    Expired,
    /// The person refused.
    Denied,
}

/// The account of a token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// The stable numeric identifier.
    pub id: u64,
    pub login: String,
}

fn form(url: String, fields: &[(&str, &str)]) -> Request {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    let mut request = Request::new(Method::Post, url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", USER_AGENT);
    request.body = Some(body.into_bytes());
    request
}

/// Starts the device flow.
pub fn start_login(http: &dyn Http) -> Result<DeviceLogin> {
    let scope = SCOPES.join(" ");
    let request = form(
        format!("{WEB}/login/device/code"),
        &[("client_id", CLIENT_ID), ("scope", &scope)],
    );
    let response = api::send(http, request)?;
    let body = api::json(&response)?;
    if let Some(code) = api::text(&body, &["error"]) {
        return Err(Error::Login(code.to_string()));
    }
    let field = |name: &'static str| api::text(&body, &[name]).ok_or_else(|| Error::BadData(format!("no {name}")));
    let number = |name: &'static str| body.get(name).and_then(|value| value.as_u64());
    let verification_uri = field("verification_uri")?.to_string();
    if !api::is_web_url(&verification_uri) {
        return Err(Error::BadData("the page of the code is not a page of GitHub".into()));
    }
    Ok(DeviceLogin {
        device_code: SecretBytes::new(field("device_code")?.as_bytes().to_vec()),
        user_code: field("user_code")?.to_string(),
        verification_uri,
        interval: number("interval").unwrap_or(SLOW_DOWN_STEP).max(1),
        expires_in: number("expires_in").unwrap_or(900),
    })
}

/// Asks one time if the person agreed.
///
/// Do not call this function more often than the interval of the login.
pub fn poll_login(http: &dyn Http, device_code: &SecretBytes) -> Result<Poll> {
    let device_code =
        std::str::from_utf8(device_code.expose()).map_err(|_| Error::BadInput("the device code is not text"))?;
    let request = form(
        format!("{WEB}/login/oauth/access_token"),
        &[
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", GRANT_TYPE),
        ],
    );
    let response = api::send(http, request)?;
    let body = api::json(&response)?;
    if let Some(code) = api::text(&body, &["error"]) {
        return match code {
            "authorization_pending" => Ok(Poll::Pending),
            "slow_down" => Ok(Poll::SlowDown {
                interval: body.get("interval").and_then(|value| value.as_u64()),
            }),
            "expired_token" | "token_expired" => Ok(Poll::Expired),
            "access_denied" => Ok(Poll::Denied),
            other => Err(Error::Login(other.to_string())),
        };
    }
    let token = api::text(&body, &["access_token"]).ok_or_else(|| Error::BadData("no access_token".into()))?;
    let granted: Vec<&str> = api::text(&body, &["scope"])
        .unwrap_or("")
        .split([',', ' '])
        .filter(|scope| !scope.is_empty())
        .collect();
    let missing: Vec<String> = SCOPES
        .iter()
        // The scope `repo` contains the scope `public_repo`.
        .filter(|scope| !granted.contains(scope) && !(**scope == "public_repo" && granted.contains(&"repo")))
        .map(|scope| scope.to_string())
        .collect();
    if !missing.is_empty() {
        return Ok(Poll::ScopeMissing { missing });
    }
    Ok(Poll::Done {
        token: SecretBytes::new(token.as_bytes().to_vec()),
    })
}

/// Gets the account of a token.
pub fn user(http: &dyn Http, token: &SecretBytes) -> Result<User> {
    let response = api::send(http, api::request(Method::Get, format!("{API}/user"), Some(token))?)?;
    let body = api::json(&response)?;
    let id = body
        .get("id")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| Error::BadData("no id".into()))?;
    let login = api::text(&body, &["login"]).ok_or_else(|| Error::BadData("no login".into()))?;
    if !api::is_name(login) {
        return Err(Error::BadData("the login has the wrong form".into()));
    }
    Ok(User {
        id,
        login: login.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;

    fn code() -> SecretBytes {
        SecretBytes::new(b"3584d83530557fdd1f46af8289938c8ef79f9dc5".to_vec())
    }

    fn body_text(request: &Request) -> String {
        String::from_utf8(request.body.clone().unwrap()).unwrap()
    }

    #[test]
    fn start_sends_the_client_and_the_scopes() {
        let http = FakeHttp::new();
        http.json(
            200,
            r#"{"device_code":"3584d83530557fdd1f46af8289938c8ef79f9dc5","user_code":"WDJB-MJHT",
                "verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
        );
        let login = start_login(&http).unwrap();
        assert_eq!(login.device_code, code());
        assert_eq!(
            (login.user_code.as_str(), login.interval, login.expires_in),
            ("WDJB-MJHT", 5, 900)
        );
        assert_eq!(login.verification_uri, DEVICE_PAGE);
        assert!(!format!("{login:?}").contains("3584d8"), "the device code is a secret");

        let requests = http.take_requests();
        let request = &requests[0];
        assert_eq!(
            (request.method, request.url.as_str()),
            (Method::Post, "https://github.com/login/device/code")
        );
        assert_eq!(request.header_value("Accept"), Some("application/json"));
        assert_eq!(request.header_value("Authorization"), None);
        assert_eq!(
            body_text(request),
            "client_id=Ov23liVIHlgUHcwnpbFk&scope=notifications+public_repo"
        );
    }

    #[test]
    fn start_refuses_a_page_of_a_different_site() {
        let http = FakeHttp::new();
        http.json(
            200,
            r#"{"device_code":"d","user_code":"U","verification_uri":"https://example.org/x","expires_in":900,"interval":5}"#,
        );
        assert!(matches!(start_login(&http), Err(Error::BadData(_))));
        http.json(200, r#"{"error":"device_flow_disabled"}"#);
        assert_eq!(start_login(&http), Err(Error::Login("device_flow_disabled".into())));
    }

    #[test]
    fn poll_states() {
        let http = FakeHttp::new();
        http.json(200, r#"{"error":"authorization_pending","error_description":"x"}"#)
            .json(200, r#"{"error":"slow_down","interval":10}"#)
            .json(200, r#"{"error":"slow_down"}"#)
            .json(200, r#"{"error":"expired_token"}"#)
            .json(200, r#"{"error":"access_denied"}"#)
            .json(200, r#"{"error":"incorrect_device_code"}"#)
            .json(
                200,
                r#"{"access_token":"gho_16C7e42F292c6912E7710c838347Ae178B4a","token_type":"bearer","scope":"notifications,public_repo"}"#,
            )
            .json(
                200,
                r#"{"access_token":"gho_other","token_type":"bearer","scope":"notifications"}"#,
            )
            .json(200, r#"{"access_token":"gho_other","token_type":"bearer","scope":""}"#)
            .json(
                200,
                r#"{"access_token":"gho_repo","token_type":"bearer","scope":"notifications repo public_repo"}"#,
            );
        assert_eq!(poll_login(&http, &code()), Ok(Poll::Pending));
        assert_eq!(poll_login(&http, &code()), Ok(Poll::SlowDown { interval: Some(10) }));
        assert_eq!(poll_login(&http, &code()), Ok(Poll::SlowDown { interval: None }));
        assert_eq!(poll_login(&http, &code()), Ok(Poll::Expired));
        assert_eq!(poll_login(&http, &code()), Ok(Poll::Denied));
        assert_eq!(
            poll_login(&http, &code()),
            Err(Error::Login("incorrect_device_code".into()))
        );
        assert_eq!(
            poll_login(&http, &code()),
            Ok(Poll::Done {
                token: SecretBytes::new(b"gho_16C7e42F292c6912E7710c838347Ae178B4a".to_vec())
            })
        );
        assert_eq!(
            poll_login(&http, &code()),
            Ok(Poll::ScopeMissing {
                missing: vec!["public_repo".into()]
            })
        );
        assert_eq!(
            poll_login(&http, &code()),
            Ok(Poll::ScopeMissing {
                missing: vec!["notifications".into(), "public_repo".into()]
            })
        );
        assert!(matches!(poll_login(&http, &code()), Ok(Poll::Done { .. })));

        let requests = http.take_requests();
        assert_eq!(requests[0].url, "https://github.com/login/oauth/access_token");
        assert_eq!(
            body_text(&requests[0]),
            "client_id=Ov23liVIHlgUHcwnpbFk&device_code=3584d83530557fdd1f46af8289938c8ef79f9dc5\
             &grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"
        );
    }

    #[test]
    fn poll_gives_network_and_status_errors() {
        let http = FakeHttp::new();
        http.failure("no route").json(503, "{}");
        assert_eq!(poll_login(&http, &code()), Err(Error::Network("no route".into())));
        assert_eq!(poll_login(&http, &code()), Err(Error::Status(503)));
    }

    #[test]
    fn user_of_a_token() {
        let http = FakeHttp::new();
        http.json(200, r#"{"login":"octocat","id":583231,"name":"The Octocat"}"#)
            .json(401, r#"{"message":"Bad credentials"}"#);
        let token = SecretBytes::new(b"gho_abc".to_vec());
        assert_eq!(
            user(&http, &token),
            Ok(User {
                id: 583_231,
                login: "octocat".into()
            })
        );
        assert_eq!(user(&http, &token), Err(Error::BadToken));
        let requests = http.take_requests();
        assert_eq!(requests[0].url, "https://api.github.com/user");
        assert_eq!(requests[0].header_value("Authorization"), Some("Bearer gho_abc"));
    }
}
