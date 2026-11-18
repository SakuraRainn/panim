//! The parts that all REST requests use: the headers, the errors and the time form.

use std::fmt;

use panim_core::{SecretBytes, Timestamp};

use crate::http::{Http, HttpError, Method, Request, Response};

/// The base URL of the REST API.
pub const API: &str = "https://api.github.com";
/// The base URL of the web pages.
pub const WEB: &str = "https://github.com";

const ACCEPT: &str = "application/vnd.github+json";
const API_VERSION: &str = "2022-11-28";
/// The user agent. GitHub refuses a request without one.
pub const USER_AGENT: &str = "PANIM";

/// An error from the connector. No variant contains a token or a device code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The request did not get a response.
    Network(String),
    /// Status 401. The token is not valid. The account needs a new login.
    BadToken,
    /// Status 403 or 429 with rate limit headers.
    RateLimit {
        /// The time when the limit ends, in seconds after 1970-01-01 00:00:00 UTC.
        reset: Option<i64>,
        /// The number of seconds to wait, if the server gave it.
        retry_after: Option<u64>,
    },
    /// Status 403 without rate limit headers. The token has no access.
    Forbidden,
    /// Status 404.
    NotFound,
    /// A different status that is not a success.
    Status(u16),
    /// The response does not have the form that the documentation of GitHub gives.
    BadData(String),
    /// The device flow gave an error code that has no variant of its own.
    Login(String),
    /// The input of the caller is not correct.
    BadInput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(reason) => write!(f, "the request did not get a response: {reason}"),
            Error::BadToken => f.write_str("the token is not valid"),
            Error::RateLimit { reset: Some(reset), .. } => write!(f, "rate limit until {reset}"),
            Error::RateLimit {
                retry_after: Some(seconds),
                ..
            } => write!(f, "rate limit for {seconds} seconds"),
            Error::RateLimit { .. } => f.write_str("rate limit"),
            Error::Forbidden => f.write_str("the token has no access"),
            Error::NotFound => f.write_str("GitHub did not find the item"),
            Error::Status(status) => write!(f, "GitHub gave the status {status}"),
            Error::BadData(reason) => write!(f, "the response has the wrong form: {reason}"),
            Error::Login(code) => write!(f, "the login failed: {code}"),
            Error::BadInput(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for Error {}

impl From<HttpError> for Error {
    fn from(error: HttpError) -> Self {
        Error::Network(error.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Makes a request to the REST API, with the headers that each request must have.
///
/// Without a token, GitHub does not know which account sends the request.
pub fn request(method: Method, url: impl Into<String>, token: Option<&SecretBytes>) -> Result<Request> {
    let mut request = Request::new(method, url)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", API_VERSION)
        .header("User-Agent", USER_AGENT);
    if let Some(token) = token {
        let token = std::str::from_utf8(token.expose()).map_err(|_| Error::BadInput("the token is not text"))?;
        if token.is_empty() || token.bytes().any(|b| !(0x21..=0x7e).contains(&b)) {
            return Err(Error::BadInput("the token has the wrong form"));
        }
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    Ok(request)
}

/// Sends a request and changes an error status into an `Error`. A status from 200 to 399 is a correct result.
pub fn send(http: &dyn Http, request: Request) -> Result<Response> {
    check(http.send(request)?)
}

/// Changes an error status into an `Error`.
pub fn check(response: Response) -> Result<Response> {
    let number = |name: &str| response.header(name).and_then(|value| value.trim().parse::<i64>().ok());
    match response.status {
        200..=399 => Ok(response),
        401 => Err(Error::BadToken),
        403 | 429 => {
            let retry_after = number("retry-after").and_then(|seconds| u64::try_from(seconds).ok());
            let spent = number("x-ratelimit-remaining") == Some(0);
            if spent || retry_after.is_some() || response.status == 429 {
                Err(Error::RateLimit {
                    reset: number("x-ratelimit-reset").filter(|_| spent),
                    retry_after,
                })
            } else {
                Err(Error::Forbidden)
            }
        }
        404 => Err(Error::NotFound),
        status => Err(Error::Status(status)),
    }
}

/// Reads the JSON body of a response.
pub fn json(response: &Response) -> Result<serde_json::Value> {
    serde_json::from_slice(&response.body).map_err(|_| Error::BadData("the body is not JSON".into()))
}

/// Reads a time in the form `YYYY-MM-DDTHH:MM:SSZ`. A fraction of a second is permitted.
pub fn parse_time(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || !text.is_ascii() {
        return None;
    }
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators.iter().any(|(place, byte)| bytes[*place] != *byte) {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = &text[range];
        part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok())?
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        let padded = format!("{:0<3}", &fraction[..digits.min(3)]);
        millis = padded.parse().ok()?;
        rest = &fraction[digits..];
    }
    let in_range = (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 61;
    if rest != "Z" || !in_range {
        return None;
    }
    // The number of days from 1970-01-01, with the proleptic Gregorian calendar.
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = shifted_year.div_euclid(400);
    let year_of_era = shifted_year.rem_euclid(400);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(Timestamp(
        ((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000 + millis,
    ))
}

/// True if the text can be the name of an owner or of a repository in a URL path.
pub fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text != "."
        && text != ".."
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Reads `owner/repo`.
pub fn split_repo(full_name: &str) -> Option<(&str, &str)> {
    let (owner, repo) = full_name.split_once('/')?;
    (is_name(owner) && is_name(repo)).then_some((owner, repo))
}

/// A text field of a JSON object.
pub fn text<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(value, |value, key| value.get(key))?.as_str()
}

/// True if the URL is a web page of GitHub.
pub fn is_web_url(url: &str) -> bool {
    url.strip_prefix(WEB).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, headers: &[(&str, &str)]) -> Response {
        Response {
            status,
            headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
            body: Vec::new(),
        }
    }

    #[test]
    fn each_request_has_the_headers() {
        let token = SecretBytes::new(b"gho_abc".to_vec());
        let with = request(Method::Get, "https://api.github.com/user", Some(&token)).unwrap();
        assert_eq!(with.header_value("accept"), Some("application/vnd.github+json"));
        assert_eq!(with.header_value("X-GitHub-Api-Version"), Some("2022-11-28"));
        assert_eq!(with.header_value("User-Agent"), Some("PANIM"));
        assert_eq!(with.header_value("Authorization"), Some("Bearer gho_abc"));
        assert!(!format!("{with:?}").contains("gho_abc"));

        let without = request(Method::Get, "https://api.github.com/gists/1", None).unwrap();
        assert_eq!(without.header_value("Authorization"), None);

        let bad = SecretBytes::new(b"gho\r\nX: y".to_vec());
        assert!(matches!(
            request(Method::Get, "https://api.github.com/user", Some(&bad)),
            Err(Error::BadInput(_))
        ));
    }

    #[test]
    fn statuses_become_errors() {
        assert!(check(response(200, &[])).is_ok());
        assert!(check(response(304, &[])).is_ok());
        assert_eq!(check(response(401, &[])), Err(Error::BadToken));
        assert_eq!(check(response(404, &[])), Err(Error::NotFound));
        assert_eq!(check(response(500, &[])), Err(Error::Status(500)));
        assert_eq!(
            check(response(403, &[("X-RateLimit-Remaining", "4999")])),
            Err(Error::Forbidden)
        );
        assert_eq!(
            check(response(
                403,
                &[("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", "1794560400")]
            )),
            Err(Error::RateLimit {
                reset: Some(1_794_560_400),
                retry_after: None
            })
        );
        assert_eq!(
            check(response(403, &[("Retry-After", "60"), ("X-RateLimit-Remaining", "10")])),
            Err(Error::RateLimit {
                reset: None,
                retry_after: Some(60)
            })
        );
        assert_eq!(
            check(response(429, &[])),
            Err(Error::RateLimit {
                reset: None,
                retry_after: None
            })
        );
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(Timestamp(0)));
        assert_eq!(parse_time("2026-11-13T10:20:30Z"), Some(Timestamp(1_794_565_230_000)));
        assert_eq!(
            parse_time("2026-11-13T10:20:30.25Z"),
            Some(Timestamp(1_794_565_230_250))
        );
        assert_eq!(parse_time("2024-02-29T23:59:59Z"), Some(Timestamp(1_709_251_199_000)));
        assert_eq!(parse_time("2026-11-13T10:20:30+01:00"), None);
        assert_eq!(parse_time("2026-11-13 10:20:30Z"), None);
        assert_eq!(parse_time("2026-13-13T10:20:30Z"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn names() {
        assert_eq!(
            split_repo("octo-org/hello.world_1"),
            Some(("octo-org", "hello.world_1"))
        );
        assert_eq!(split_repo("octo/../x"), None);
        assert_eq!(split_repo("octo/a/b"), None);
        assert_eq!(split_repo("octo/a?b"), None);
        assert!(is_web_url("https://github.com/o/r"));
        assert!(!is_web_url("https://github.com.evil.example/o/r"));
        assert!(!is_web_url("http://github.com/o/r"));
    }
}
