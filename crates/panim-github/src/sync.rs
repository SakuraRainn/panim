//! The sync: notifications and received events become timeline items.
//!
//! - A notification thread becomes one item with the source identifier `n/<thread id>`.
//! - A received event becomes one item with the source identifier `e/<event id>`.
//!
//! When GitHub changes a thread, the thread comes again with the same identifier and a later time. The store then
//! makes the item unread again.

use std::collections::BTreeSet;

use panim_core::{NewItem, SecretBytes};
use serde_json::Value;

use crate::api::{self, API, Error, Result, WEB};
use crate::http::{Http, Method, Response};

/// The prefix of the source identifier of a notification thread.
pub const THREAD_PREFIX: &str = "n/";
/// The prefix of the source identifier of a received event.
pub const EVENT_PREFIX: &str = "e/";

/// The number of items in one page.
const PER_PAGE: u32 = 50;
/// The sync reads no more than this number of pages of notifications, thus 500 threads.
///
/// GitHub gives no more than 50 notification threads in one page.
pub const MAX_PAGES: usize = 10;
/// The number of watched repositories in one page.
const WATCHED_PER_PAGE: u32 = 100;
/// The sync reads no more than this number of pages of watched repositories, thus 1,000 repositories.
///
/// If an account watches more repositories, the list is cut. Then the sync drops the events of the repositories
/// that are not in the list.
const MAX_WATCHED_PAGES: usize = 10;

/// The result of one request for notifications or for events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetched {
    /// The items, or `None` if GitHub answered that there is no change (status 304).
    pub items: Option<Vec<NewItem>>,
    /// The value to send with the next request: `Last-Modified` for notifications, `ETag` for events.
    pub validator: Option<String>,
    /// The minimum number of seconds before the next request, if GitHub gave it (`X-Poll-Interval`).
    pub poll_interval: Option<u64>,
    /// True if `items` has all the items of the list. False if the page limit cut the list, or if there are no items.
    pub complete: bool,
}

fn poll_interval(response: &Response) -> Option<u64> {
    response.header("x-poll-interval")?.trim().parse().ok()
}

fn array(response: &Response) -> Result<Vec<Value>> {
    match api::json(response)? {
        Value::Array(values) => Ok(values),
        _ => Err(Error::BadData("the body is not a list".into())),
    }
}

/// The URL of the next page from a `Link` header, if it is a URL of the REST API.
fn next_page(response: &Response) -> Option<String> {
    next_link(response).filter(|url| url.starts_with(&format!("{API}/")))
}

/// The URL of the next page from a `Link` header, on any host.
fn next_link(response: &Response) -> Option<String> {
    response.header("link")?.split(',').find_map(|part| {
        let (url, parameters) = part.split_once(';')?;
        let is_next = parameters
            .split(';')
            .any(|parameter| parameter.trim() == "rel=\"next\"");
        let url = url.trim().strip_prefix('<')?.strip_suffix('>')?;
        is_next.then(|| url.to_string())
    })
}

/// Gets the unread notifications of the account.
///
/// `last_modified` is the validator of the last result. With it, GitHub answers 304 if there is no change, and the
/// request does not count against the rate limit.
pub fn notifications(http: &dyn Http, token: &SecretBytes, last_modified: Option<&str>) -> Result<Fetched> {
    let mut request = api::request(
        Method::Get,
        format!("{API}/notifications?all=false&per_page={PER_PAGE}"),
        Some(token),
    )?;
    if let Some(value) = last_modified {
        request = request.header("If-Modified-Since", value);
    }
    let mut response = api::send(http, request)?;
    let poll_interval = poll_interval(&response);
    if response.status == 304 {
        return Ok(Fetched {
            items: None,
            validator: last_modified.map(str::to_string),
            poll_interval,
            complete: false,
        });
    }
    let validator = response.header("last-modified").map(str::to_string);
    let mut items = Vec::new();
    let mut complete = false;
    for page in 1..=MAX_PAGES {
        if response.status != 200 {
            return Err(Error::Status(response.status));
        }
        items.extend(array(&response)?.iter().filter_map(thread_item));
        if next_link(&response).is_none() {
            complete = true;
            break;
        }
        // The request has the token. Do not send it to a different host. Then the list is not complete.
        let Some(next) = next_page(&response) else {
            break;
        };
        if page == MAX_PAGES {
            // The limit cut the list. The caller must not mark the other threads as read.
            break;
        }
        response = api::send(http, api::request(Method::Get, next, Some(token))?)?;
    }
    Ok(Fetched {
        items: Some(items),
        validator,
        poll_interval,
        complete,
    })
}

/// Gets the first page of the events that the account received.
///
/// `etag` is the validator of the last result.
pub fn received_events(http: &dyn Http, token: &SecretBytes, login: &str, etag: Option<&str>) -> Result<Fetched> {
    if !api::is_name(login) {
        return Err(Error::BadInput("the login has the wrong form"));
    }
    let mut request = api::request(
        Method::Get,
        format!("{API}/users/{login}/received_events?per_page={PER_PAGE}"),
        Some(token),
    )?;
    if let Some(value) = etag {
        request = request.header("If-None-Match", value);
    }
    let response = api::send(http, request)?;
    let poll_interval = poll_interval(&response);
    match response.status {
        304 => Ok(Fetched {
            items: None,
            validator: etag.map(str::to_string),
            poll_interval,
            complete: false,
        }),
        200 => Ok(Fetched {
            items: Some(array(&response)?.iter().filter_map(event_item).collect()),
            validator: response.header("etag").map(str::to_string),
            poll_interval,
            complete: true,
        }),
        status => Err(Error::Status(status)),
    }
}

/// The repositories that the account watches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watched {
    /// The full names `owner/repo` in lowercase, or `None` if GitHub answered that there is no change (status 304).
    pub repos: Option<BTreeSet<String>>,
    /// The `ETag` to send with the next request. It is `None` if the list has more than one page, because the
    /// `ETag` of the first page does not show a change in a different page.
    pub validator: Option<String>,
    /// True if the list has more than `MAX_WATCHED_PAGES` pages, and the sync did not read all of them.
    pub cut: bool,
}

/// Gets the repositories that the account watches (`GET /user/subscriptions`).
///
/// The scope `public_repo` is sufficient for public repositories. `etag` is the validator of the last result.
pub fn watched(http: &dyn Http, token: &SecretBytes, etag: Option<&str>) -> Result<Watched> {
    let mut request = api::request(
        Method::Get,
        format!("{API}/user/subscriptions?per_page={WATCHED_PER_PAGE}"),
        Some(token),
    )?;
    if let Some(value) = etag {
        request = request.header("If-None-Match", value);
    }
    let mut response = api::send(http, request)?;
    if response.status == 304 {
        return Ok(Watched {
            repos: None,
            validator: etag.map(str::to_string),
            cut: false,
        });
    }
    let first_etag = response.header("etag").map(str::to_string);
    let mut repos = BTreeSet::new();
    let mut pages = 1;
    loop {
        if response.status != 200 {
            return Err(Error::Status(response.status));
        }
        for repo in array(&response)? {
            if let Some(name) = api::text(&repo, &["full_name"]).filter(|name| api::split_repo(name).is_some()) {
                repos.insert(name.to_ascii_lowercase());
            }
        }
        let Some(next) = next_page(&response) else {
            break;
        };
        if pages == MAX_WATCHED_PAGES {
            return Ok(Watched {
                repos: Some(repos),
                validator: None,
                cut: true,
            });
        }
        pages += 1;
        response = api::send(http, api::request(Method::Get, next, Some(token))?)?;
    }
    Ok(Watched {
        repos: Some(repos),
        validator: if pages == 1 { first_etag } else { None },
        cut: false,
    })
}

/// The full name of the repository of an event item, in lowercase. Returns `None` for an item that is not an event.
pub fn event_repo(raw: &[u8], source_id: &str) -> Option<String> {
    if !source_id.starts_with(EVENT_PREFIX) {
        return None;
    }
    let event: Value = serde_json::from_slice(raw).ok()?;
    Some(api::text(&event, &["repo", "name"])?.to_ascii_lowercase())
}

/// True if the item is an event in a watched repository. `watched` has the names in lowercase.
///
/// The received events also contain the public actions of the accounts that the person follows, in all
/// repositories. PANIM keeps only the events of the watched repositories. An item that is not an event is false.
pub fn is_watched_event(raw: &[u8], source_id: &str, watched: &BTreeSet<String>) -> bool {
    event_repo(raw, source_id).is_some_and(|repo| watched.contains(&repo))
}

/// Marks a notification thread as read on GitHub. `source_id` is the source identifier of the item.
pub fn mark_thread_read(http: &dyn Http, token: &SecretBytes, source_id: &str) -> Result<()> {
    let id = thread_id(source_id).ok_or(Error::BadInput("the item is not a notification"))?;
    let request = api::request(Method::Patch, format!("{API}/notifications/threads/{id}"), Some(token))?;
    api::send(http, request).map(|_| ())
}

/// True if the item has a read state on GitHub. Only a notification thread has one. An event has no read state, so
/// it never counts as unread.
pub fn has_read_state(source_id: &str) -> bool {
    thread_id(source_id).is_some()
}

/// The thread identifier of a notification item. Returns `None` for other items.
pub fn thread_id(source_id: &str) -> Option<&str> {
    let id = source_id.strip_prefix(THREAD_PREFIX)?;
    (!id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// An identifier from GitHub: a text of digits, or a number.
fn id_text(value: &Value) -> Option<String> {
    let id = match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_u64()?.to_string(),
        _ => return None,
    };
    (!id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// The web page of a repository, from its full name.
fn repo_page(full_name: &str) -> Option<String> {
    let (owner, repo) = api::split_repo(full_name)?;
    Some(format!("{WEB}/{owner}/{repo}"))
}

/// The owner, the repository and the number in an API URL of an issue or of a pull request.
///
/// The form is `https://api.github.com/repos/<owner>/<repo>/issues/<number>` or `.../pulls/<number>`.
pub(crate) fn issue_in_api_url(url: &str) -> Option<(String, String, u64, bool)> {
    let path = url.strip_prefix(API)?.strip_prefix("/repos/")?;
    let parts: Vec<&str> = path.split('/').collect();
    let [owner, repo, kind, number] = parts[..] else {
        return None;
    };
    let pull = match kind {
        "issues" => false,
        "pulls" => true,
        _ => return None,
    };
    if !api::is_name(owner) || !api::is_name(repo) || number.starts_with('0') {
        return None;
    }
    let number = number
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| number.parse().ok())??;
    Some((owner.to_string(), repo.to_string(), number, pull))
}

/// Makes a timeline item from a notification thread. Returns `None` if a necessary field is missing.
fn thread_item(thread: &Value) -> Option<NewItem> {
    let id = id_text(thread.get("id")?)?;
    let full_name = api::text(thread, &["repository", "full_name"])?;
    let time = api::parse_time(api::text(thread, &["updated_at"])?)?;
    let title = api::text(thread, &["subject", "title"])?;
    let link = api::text(thread, &["subject", "url"])
        .and_then(issue_in_api_url)
        .map(|(owner, repo, number, pull)| {
            let kind = if pull { "pull" } else { "issues" };
            format!("{WEB}/{owner}/{repo}/{kind}/{number}")
        })
        .or_else(|| {
            api::text(thread, &["repository", "html_url"])
                .filter(|url| api::is_web_url(url))
                .map(str::to_string)
        })
        .or_else(|| repo_page(full_name));
    Some(NewItem {
        source_id: format!("{THREAD_PREFIX}{id}"),
        author: full_name.to_string(),
        time,
        text: title.to_string(),
        link,
        raw: serde_json::to_vec(thread).ok()?,
    })
}

/// The kind of an event that PANIM knows, and its English phrase. The kind does not change, so the user interface
/// can show the phrase in its language.
const EVENT_KINDS: [(&str, &str); 18] = [
    ("starred", "starred"),
    ("forked", "forked"),
    ("created_branch", "created a branch in"),
    ("created_tag", "created a tag in"),
    ("created", "created"),
    ("released", "published a release of"),
    ("issue_opened", "opened an issue in"),
    ("issue_closed", "closed an issue in"),
    ("issue_reopened", "reopened an issue in"),
    ("issue_changed", "changed an issue in"),
    ("commented", "wrote a comment in"),
    ("pull_opened", "opened a pull request in"),
    ("pull_closed", "closed a pull request in"),
    ("pull_reopened", "reopened a pull request in"),
    ("pull_changed", "changed a pull request in"),
    ("pushed", "pushed to"),
    ("made_public", "made public"),
    ("member_added", "added a member to"),
];

/// The kind of an event, from its type and its payload. Returns `None` for a type that PANIM does not know.
fn event_kind(kind: &str, payload: &Value) -> Option<&'static str> {
    let action = api::text(payload, &["action"]).unwrap_or("");
    Some(match kind {
        "WatchEvent" => "starred",
        "ForkEvent" => "forked",
        "CreateEvent" => match api::text(payload, &["ref_type"]) {
            Some("branch") => "created_branch",
            Some("tag") => "created_tag",
            _ => "created",
        },
        "ReleaseEvent" => "released",
        "IssuesEvent" => match action {
            "opened" => "issue_opened",
            "closed" => "issue_closed",
            "reopened" => "issue_reopened",
            _ => "issue_changed",
        },
        "IssueCommentEvent" => "commented",
        "PullRequestEvent" => match action {
            "opened" => "pull_opened",
            "closed" | "merged" => "pull_closed",
            "reopened" => "pull_reopened",
            _ => "pull_changed",
        },
        "PushEvent" => "pushed",
        "PublicEvent" => "made_public",
        "MemberEvent" => "member_added",
        _ => return None,
    })
}

/// The English phrase of an event kind.
fn event_phrase(kind: &str) -> Option<&'static str> {
    EVENT_KINDS
        .iter()
        .find(|(name, _)| *name == kind)
        .map(|(_, phrase)| *phrase)
}

/// The kind and the repository of a received event, from the raw record of its item.
///
/// Returns `None` for an item that is not an event, and for an event type that PANIM does not know. The user
/// interface uses the result to show the phrase in its language. `EVENT_KINDS` has all the kinds.
pub fn event_summary(raw: &[u8], source_id: &str) -> Option<(&'static str, String)> {
    if !source_id.starts_with(EVENT_PREFIX) {
        return None;
    }
    let event: Value = serde_json::from_slice(raw).ok()?;
    let kind = event_kind(
        api::text(&event, &["type"])?,
        event.get("payload").unwrap_or(&Value::Null),
    )?;
    let repo = api::text(&event, &["repo", "name"])?;
    Some((kind, repo.to_string()))
}

/// All the kinds of events that `event_summary` can give.
pub fn event_kinds() -> impl Iterator<Item = &'static str> {
    EVENT_KINDS.iter().map(|(name, _)| *name)
}

/// The number of the issue or of the pull request of an event.
pub(crate) fn event_number(kind: &str, payload: &Value) -> Option<u64> {
    let number = |path: &[&str]| path.iter().try_fold(payload, |value, key| value.get(key))?.as_u64();
    match kind {
        "IssuesEvent" | "IssueCommentEvent" => number(&["issue", "number"]),
        "PullRequestEvent" => number(&["number"]).or_else(|| number(&["pull_request", "number"])),
        _ => None,
    }
    .filter(|number| *number > 0)
}

/// The best web page for an event.
fn event_link(kind: &str, payload: &Value, repo: &str) -> Option<String> {
    let page = |path: &[&str]| {
        api::text(payload, path)
            .filter(|url| api::is_web_url(url))
            .map(str::to_string)
    };
    let from_payload = match kind {
        "IssueCommentEvent" => page(&["comment", "html_url"]).or_else(|| page(&["issue", "html_url"])),
        "IssuesEvent" => page(&["issue", "html_url"]),
        "PullRequestEvent" => page(&["pull_request", "html_url"]),
        "ReleaseEvent" => page(&["release", "html_url"]),
        "ForkEvent" => page(&["forkee", "html_url"]),
        _ => None,
    };
    let repo_page = repo_page(repo);
    from_payload
        .or_else(|| {
            // Some events have no web URL in the payload. Make the URL from the number.
            let number = event_number(kind, payload)?;
            let part = if kind == "PullRequestEvent" { "pull" } else { "issues" };
            Some(format!("{}/{part}/{number}", repo_page.as_ref()?))
        })
        .or(repo_page)
}

/// Makes a timeline item from a received event. Returns `None` if a necessary field is missing.
fn event_item(event: &Value) -> Option<NewItem> {
    let id = id_text(event.get("id")?)?;
    let kind = api::text(event, &["type"])?;
    let actor = api::text(event, &["actor", "login"])?;
    let repo = api::text(event, &["repo", "name"])?;
    let time = api::parse_time(api::text(event, &["created_at"])?)?;
    let empty = Value::Null;
    let payload = event.get("payload").unwrap_or(&empty);
    let text = match event_kind(kind, payload).and_then(event_phrase) {
        Some(phrase) => format!("{phrase} {repo}"),
        None => format!("{kind} {repo}"),
    };
    Some(NewItem {
        source_id: format!("{EVENT_PREFIX}{id}"),
        author: actor.to_string(),
        time,
        text,
        link: event_link(kind, payload, repo),
        raw: serde_json::to_vec(event).ok()?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use panim_core::Timestamp;

    pub(crate) const THREADS: &str = r#"[
      {"id":"101","unread":true,"reason":"mention","updated_at":"2026-11-13T10:20:30Z",
       "subject":{"title":"The parser stops on an empty line","type":"Issue",
                  "url":"https://api.github.com/repos/octo-org/parser/issues/5",
                  "latest_comment_url":"https://api.github.com/repos/octo-org/parser/issues/comments/9"},
       "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}},
      {"id":"102","unread":true,"reason":"review_requested","updated_at":"2026-11-13T10:00:00Z",
       "subject":{"title":"Add a test","type":"PullRequest",
                  "url":"https://api.github.com/repos/octo-org/parser/pulls/7"},
       "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}},
      {"id":"103","unread":true,"reason":"subscribed","updated_at":"2026-11-12T08:00:00Z",
       "subject":{"title":"v1.2.0","type":"Release",
                  "url":"https://api.github.com/repos/octo-org/parser/releases/44"},
       "repository":{"full_name":"octo-org/parser","html_url":"https://github.com/octo-org/parser"}},
      {"id":"104","unread":true,"reason":"security_alert","updated_at":"2026-11-11T08:00:00Z",
       "subject":{"title":"An alert","type":"RepositoryVulnerabilityAlert","url":null},
       "repository":{"full_name":"octo-org/tools","html_url":"https://evil.example/x"}},
      {"id":"bad","updated_at":"2026-11-11T08:00:00Z"}
    ]"#;

    pub(crate) const EVENTS: &str = r#"[
      {"id":"9001","type":"WatchEvent","actor":{"login":"alice"},"repo":{"name":"me/project"},
       "payload":{"action":"started"},"created_at":"2026-11-13T09:00:00Z"},
      {"id":"9002","type":"ForkEvent","actor":{"login":"bob"},"repo":{"name":"me/project"},
       "payload":{"forkee":{"html_url":"https://github.com/bob/project"}},"created_at":"2026-11-13T08:00:00Z"},
      {"id":"9003","type":"IssueCommentEvent","actor":{"login":"carol"},"repo":{"name":"me/project"},
       "payload":{"action":"created","issue":{"number":12,"html_url":"https://github.com/me/project/issues/12"},
                  "comment":{"html_url":"https://github.com/me/project/issues/12#issuecomment-3"}},
       "created_at":"2026-11-13T07:00:00Z"},
      {"id":"9004","type":"PullRequestEvent","actor":{"login":"dave"},"repo":{"name":"me/project"},
       "payload":{"action":"opened","number":13,"pull_request":{"number":13}},"created_at":"2026-11-13T06:00:00Z"},
      {"id":"9005","type":"IssuesEvent","actor":{"login":"erin"},"repo":{"name":"me/project"},
       "payload":{"action":"closed","issue":{"number":4,"html_url":"https://github.com/me/project/issues/4"}},
       "created_at":"2026-11-13T05:00:00Z"},
      {"id":"9006","type":"ReleaseEvent","actor":{"login":"frank"},"repo":{"name":"octo-org/parser"},
       "payload":{"action":"published","release":{"html_url":"https://github.com/octo-org/parser/releases/tag/v1"}},
       "created_at":"2026-11-13T04:00:00Z"},
      {"id":"9007","type":"CreateEvent","actor":{"login":"gina"},"repo":{"name":"gina/new"},
       "payload":{"ref_type":"repository"},"created_at":"2026-11-13T03:00:00Z"},
      {"id":"9008","type":"PushEvent","actor":{"login":"hal"},"repo":{"name":"me/project"},
       "payload":{"ref":"refs/heads/main"},"created_at":"2026-11-13T02:00:00Z"},
      {"id":"9009","type":"PublicEvent","actor":{"login":"ivy"},"repo":{"name":"ivy/tool"},
       "payload":{},"created_at":"2026-11-13T01:00:00Z"},
      {"id":"9010","type":"MemberEvent","actor":{"login":"jo"},"repo":{"name":"jo/team"},
       "payload":{"action":"added"},"created_at":"2026-11-13T00:30:00Z"},
      {"id":"9011","type":"SponsorshipEvent","actor":{"login":"kim"},"repo":{"name":"kim/x"},
       "payload":{"issue":{"html_url":"https://evil.example/"}},"created_at":"2026-11-13T00:00:00Z"},
      {"type":"WatchEvent"}
    ]"#;

    fn token() -> SecretBytes {
        SecretBytes::new(b"gho_abc".to_vec())
    }

    #[test]
    fn notifications_become_items() {
        let http = FakeHttp::new();
        http.response(
            200,
            &[
                ("Last-Modified", "Fri, 13 Nov 2026 10:20:30 GMT"),
                ("X-Poll-Interval", "90"),
            ],
            THREADS,
        );
        let fetched = notifications(&http, &token(), None).unwrap();
        assert_eq!(fetched.validator.as_deref(), Some("Fri, 13 Nov 2026 10:20:30 GMT"));
        assert_eq!(fetched.poll_interval, Some(90));
        assert!(fetched.complete, "the response has no next page");
        let items = fetched.items.unwrap();
        let view: Vec<_> = items
            .iter()
            .map(|item| {
                (
                    item.source_id.as_str(),
                    item.author.as_str(),
                    item.text.as_str(),
                    item.link.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            view,
            vec![
                (
                    "n/101",
                    "octo-org/parser",
                    "The parser stops on an empty line",
                    "https://github.com/octo-org/parser/issues/5"
                ),
                (
                    "n/102",
                    "octo-org/parser",
                    "Add a test",
                    "https://github.com/octo-org/parser/pull/7"
                ),
                (
                    "n/103",
                    "octo-org/parser",
                    "v1.2.0",
                    "https://github.com/octo-org/parser"
                ),
                (
                    "n/104",
                    "octo-org/tools",
                    "An alert",
                    "https://github.com/octo-org/tools"
                ),
            ]
        );
        assert_eq!(items[0].time, Timestamp(1_794_565_230_000));
        let raw: Value = serde_json::from_slice(&items[0].raw).unwrap();
        assert_eq!(raw["reason"], "mention");

        let requests = http.take_requests();
        assert_eq!(
            requests[0].url,
            "https://api.github.com/notifications?all=false&per_page=50"
        );
        assert_eq!(requests[0].method, Method::Get);
        assert_eq!(requests[0].header_value("Authorization"), Some("Bearer gho_abc"));
        assert_eq!(requests[0].header_value("If-Modified-Since"), None);
    }

    #[test]
    fn no_change_gives_304() {
        let http = FakeHttp::new();
        http.response(304, &[("X-Poll-Interval", "60")], "");
        let fetched = notifications(&http, &token(), Some("Fri, 13 Nov 2026 10:20:30 GMT")).unwrap();
        assert_eq!(
            fetched,
            Fetched {
                items: None,
                validator: Some("Fri, 13 Nov 2026 10:20:30 GMT".into()),
                poll_interval: Some(60),
                complete: false,
            }
        );
        let requests = http.take_requests();
        assert_eq!(
            requests[0].header_value("If-Modified-Since"),
            Some("Fri, 13 Nov 2026 10:20:30 GMT")
        );
    }

    #[test]
    fn notifications_follow_the_next_pages() {
        let page = |id: u32| {
            format!(
                r#"[{{"id":"{id}","updated_at":"2026-11-13T10:00:00Z","subject":{{"title":"t","type":"Issue","url":null}},
                   "repository":{{"full_name":"o/r","html_url":"https://github.com/o/r"}}}}]"#
            )
        };
        let next =
            |n: u32| format!("<https://api.github.com/notifications?all=false&per_page=50&page={n}>; rel=\"next\"");
        let http = FakeHttp::new();
        // Two pages. The second page has only a previous page, so the list is complete.
        http.response(200, &[("Link", &next(2))], &page(1)).response(
            200,
            &[("Link", "<https://api.github.com/x>; rel=\"prev\"")],
            &page(2),
        );
        let fetched = notifications(&http, &token(), None).unwrap();
        assert_eq!((fetched.items.unwrap().len(), fetched.complete), (2, true));
        let requests = http.take_requests();
        assert!(requests[1].url.ends_with("page=2"));
        assert_eq!(requests[1].header_value("Authorization"), Some("Bearer gho_abc"));

        // Each page has a next page. The sync stops at the limit, and the list is not complete.
        for n in 1..=MAX_PAGES as u32 {
            http.response(200, &[("Link", &next(n + 1))], &page(n));
        }
        let fetched = notifications(&http, &token(), None).unwrap();
        assert_eq!((fetched.items.unwrap().len(), fetched.complete), (MAX_PAGES, false));
        assert_eq!(http.take_requests().len(), MAX_PAGES);

        // A next page on a different host does not get the token, and the list is not complete.
        http.response(
            200,
            &[("Link", "<https://evil.example/n?page=2>; rel=\"next\"")],
            &page(1),
        );
        let fetched = notifications(&http, &token(), None).unwrap();
        assert_eq!((fetched.items.unwrap().len(), fetched.complete), (1, false));
        assert_eq!(http.take_requests().len(), 1);
    }

    #[test]
    fn events_become_items() {
        let http = FakeHttp::new();
        http.response(200, &[("ETag", "W/\"abc\""), ("X-Poll-Interval", "60")], EVENTS)
            .response(304, &[], "");
        let fetched = received_events(&http, &token(), "me", None).unwrap();
        assert_eq!(fetched.validator.as_deref(), Some("W/\"abc\""));
        assert_eq!(fetched.poll_interval, Some(60));
        let items = fetched.items.unwrap();
        let view: Vec<_> = items
            .iter()
            .map(|item| {
                (
                    item.source_id.as_str(),
                    item.author.as_str(),
                    item.text.as_str(),
                    item.link.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            view,
            vec![
                ("e/9001", "alice", "starred me/project", "https://github.com/me/project"),
                ("e/9002", "bob", "forked me/project", "https://github.com/bob/project"),
                (
                    "e/9003",
                    "carol",
                    "wrote a comment in me/project",
                    "https://github.com/me/project/issues/12#issuecomment-3"
                ),
                (
                    "e/9004",
                    "dave",
                    "opened a pull request in me/project",
                    "https://github.com/me/project/pull/13"
                ),
                (
                    "e/9005",
                    "erin",
                    "closed an issue in me/project",
                    "https://github.com/me/project/issues/4"
                ),
                (
                    "e/9006",
                    "frank",
                    "published a release of octo-org/parser",
                    "https://github.com/octo-org/parser/releases/tag/v1"
                ),
                ("e/9007", "gina", "created gina/new", "https://github.com/gina/new"),
                ("e/9008", "hal", "pushed to me/project", "https://github.com/me/project"),
                ("e/9009", "ivy", "made public ivy/tool", "https://github.com/ivy/tool"),
                (
                    "e/9010",
                    "jo",
                    "added a member to jo/team",
                    "https://github.com/jo/team"
                ),
                ("e/9011", "kim", "SponsorshipEvent kim/x", "https://github.com/kim/x"),
            ]
        );
        assert_eq!(items[0].time, api::parse_time("2026-11-13T09:00:00Z").unwrap());

        // The user interface gets the kind of each known event, and shows the phrase in its language.
        let kinds: Vec<_> = items
            .iter()
            .map(|item| event_summary(&item.raw, &item.source_id).map(|(kind, _)| kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                Some("starred"),
                Some("forked"),
                Some("commented"),
                Some("pull_opened"),
                Some("issue_closed"),
                Some("released"),
                Some("created"),
                Some("pushed"),
                Some("made_public"),
                Some("member_added"),
                None,
            ]
        );
        assert_eq!(
            event_summary(&items[5].raw, &items[5].source_id),
            Some(("released", "octo-org/parser".to_string()))
        );
        assert_eq!(
            event_summary(&items[0].raw, "n/9001"),
            None,
            "a notification is not an event"
        );
        assert!(event_kinds().all(|kind| event_phrase(kind).is_some()));

        let again = received_events(&http, &token(), "me", Some("W/\"abc\"")).unwrap();
        assert_eq!((again.items, again.validator.as_deref()), (None, Some("W/\"abc\"")));
        let requests = http.take_requests();
        assert_eq!(
            requests[0].url,
            "https://api.github.com/users/me/received_events?per_page=50"
        );
        assert_eq!(requests[0].header_value("If-None-Match"), None);
        assert_eq!(requests[1].header_value("If-None-Match"), Some("W/\"abc\""));

        assert!(matches!(
            received_events(&http, &token(), "me/../x", None),
            Err(Error::BadInput(_))
        ));
        assert!(http.take_requests().is_empty());
    }

    #[test]
    fn errors_of_a_sync() {
        let http = FakeHttp::new();
        http.json(401, r#"{"message":"Bad credentials"}"#)
            .response(
                403,
                &[("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", "1794568800")],
                r#"{"message":"API rate limit exceeded"}"#,
            )
            .json(404, r#"{"message":"Not Found"}"#)
            .failure("timeout")
            .json(200, r#"{"message":"not a list"}"#);
        assert_eq!(notifications(&http, &token(), None), Err(Error::BadToken));
        assert_eq!(
            notifications(&http, &token(), None),
            Err(Error::RateLimit {
                reset: Some(1_794_568_800),
                retry_after: None
            })
        );
        assert_eq!(received_events(&http, &token(), "me", None), Err(Error::NotFound));
        assert_eq!(
            received_events(&http, &token(), "me", None),
            Err(Error::Network("timeout".into()))
        );
        assert!(matches!(notifications(&http, &token(), None), Err(Error::BadData(_))));
    }

    fn repos(names: &[&str]) -> String {
        let list: Vec<Value> = names
            .iter()
            .map(|name| serde_json::json!({ "full_name": name }))
            .collect();
        Value::Array(list).to_string()
    }

    #[test]
    fn watched_repositories() {
        let http = FakeHttp::new();
        http.response(
            200,
            &[("ETag", "\"w1\"")],
            &repos(&["Me/Project", "octo-org/parser", "bad/../x"]),
        )
        .response(304, &[], "");
        let first = watched(&http, &token(), None).unwrap();
        let names: Vec<_> = first.repos.clone().unwrap().into_iter().collect();
        assert_eq!(names, vec!["me/project", "octo-org/parser"]);
        assert_eq!((first.validator.as_deref(), first.cut), (Some("\"w1\""), false));
        let again = watched(&http, &token(), Some("\"w1\"")).unwrap();
        assert_eq!((again.repos, again.validator.as_deref()), (None, Some("\"w1\"")));
        let requests = http.take_requests();
        assert_eq!(
            requests[0].url,
            "https://api.github.com/user/subscriptions?per_page=100"
        );
        assert_eq!(requests[0].header_value("Authorization"), Some("Bearer gho_abc"));
        assert_eq!(requests[0].header_value("If-None-Match"), None);
        assert_eq!(requests[1].header_value("If-None-Match"), Some("\"w1\""));

        http.json(401, "{}").json(403, "{}");
        assert_eq!(watched(&http, &token(), None), Err(Error::BadToken));
        assert_eq!(watched(&http, &token(), None), Err(Error::Forbidden));
    }

    #[test]
    fn watched_pages_and_the_limit() {
        let next =
            |n: usize| format!("<https://api.github.com/user/subscriptions?per_page=100&page={n}>; rel=\"next\"");
        let http = FakeHttp::new();
        http.response(200, &[("ETag", "\"p1\""), ("Link", &next(2))], &repos(&["o/a"]))
            .response(200, &[], &repos(&["o/b"]));
        let two = watched(&http, &token(), None).unwrap();
        assert_eq!(two.repos.unwrap().len(), 2);
        assert_eq!(
            (two.validator, two.cut),
            (None, false),
            "no validator for more than one page"
        );
        http.take_requests();

        for n in 1..=MAX_WATCHED_PAGES {
            http.response(200, &[("Link", &next(n + 1))], &repos(&[&format!("o/r{n}")]));
        }
        let cut = watched(&http, &token(), None).unwrap();
        assert_eq!((cut.repos.unwrap().len(), cut.cut), (MAX_WATCHED_PAGES, true));
        assert_eq!(http.take_requests().len(), MAX_WATCHED_PAGES);

        // A next page on a different host does not get the token.
        http.response(
            200,
            &[("Link", "<https://evil.example/s?page=2>; rel=\"next\"")],
            &repos(&["o/a"]),
        );
        assert_eq!(watched(&http, &token(), None).unwrap().repos.unwrap().len(), 1);
        assert_eq!(http.take_requests().len(), 1);
    }

    #[test]
    fn only_events_of_watched_repositories_stay() {
        let event = |repo: &str| {
            serde_json::to_vec(&serde_json::json!({
                "id": "1", "type": "WatchEvent", "actor": {"login": "friend"}, "repo": {"name": repo},
            }))
            .unwrap()
        };
        let watched: BTreeSet<String> = ["me/project".to_string()].into();
        assert!(is_watched_event(&event("me/project"), "e/1", &watched));
        assert!(
            is_watched_event(&event("Me/PROJECT"), "e/1", &watched),
            "the case does not matter"
        );
        // A followed account starred a repository that the person does not watch.
        assert!(!is_watched_event(&event("friend/other"), "e/1", &watched));
        assert!(
            !is_watched_event(&event("me/project"), "n/1", &watched),
            "a notification is not an event"
        );
        assert!(!is_watched_event(b"not json", "e/1", &watched));
        assert_eq!(event_repo(&event("Me/Project"), "e/1").as_deref(), Some("me/project"));
    }

    #[test]
    fn only_threads_have_a_read_state() {
        assert!(has_read_state("n/101"));
        assert!(!has_read_state("e/9001"));
        assert!(!has_read_state("n/"));
    }

    #[test]
    fn a_thread_becomes_read() {
        let http = FakeHttp::new();
        http.response(205, &[], "").json(404, "{}");
        mark_thread_read(&http, &token(), "n/101").unwrap();
        let requests = http.take_requests();
        assert_eq!(
            (requests[0].method, requests[0].url.as_str()),
            (Method::Patch, "https://api.github.com/notifications/threads/101")
        );
        assert_eq!(mark_thread_read(&http, &token(), "n/101"), Err(Error::NotFound));
        http.take_requests();
        for bad in ["e/9001", "n/", "n/1/../2", "101"] {
            assert!(matches!(
                mark_thread_read(&http, &token(), bad),
                Err(Error::BadInput(_))
            ));
        }
        assert!(http.take_requests().is_empty(), "an event has no read state on GitHub");
    }
}
