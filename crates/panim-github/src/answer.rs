//! Answers: a comment on an issue or on a pull request.
//!
//! The token has the scope `public_repo`. Thus, an answer is possible only in a public repository.

use panim_core::SecretBytes;
use serde_json::Value;

use crate::api::{self, API, Error, Result};
use crate::http::{Http, Method};
use crate::sync::{EVENT_PREFIX, THREAD_PREFIX, event_number, issue_in_api_url};

/// The issue or the pull request that gets the answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub owner: String,
    pub repo: String,
    /// The number of the issue or of the pull request.
    pub number: u64,
}

/// Finds the target of an answer from the raw record of a timeline item.
///
/// Returns `None` if the item is not about an issue or a pull request. Then the item cannot get an answer.
pub fn answer_target(raw: &[u8], source_id: &str) -> Option<Target> {
    let record: Value = serde_json::from_slice(raw).ok()?;
    if source_id.starts_with(THREAD_PREFIX) {
        let kind = api::text(&record, &["subject", "type"])?;
        let (owner, repo, number, pull) = issue_in_api_url(api::text(&record, &["subject", "url"])?)?;
        let agrees = match kind {
            "Issue" => !pull,
            "PullRequest" => pull,
            _ => false,
        };
        agrees.then_some(Target { owner, repo, number })
    } else if source_id.starts_with(EVENT_PREFIX) {
        let kind = api::text(&record, &["type"])?;
        let number = event_number(kind, record.get("payload")?)?;
        let (owner, repo) = api::split_repo(api::text(&record, &["repo", "name"])?)?;
        Some(Target {
            owner: owner.to_string(),
            repo: repo.to_string(),
            number,
        })
    } else {
        None
    }
}

/// Posts a comment as the account of the token. Returns the web page of the new comment.
///
/// `Error::Forbidden` and `Error::NotFound` show that the token has no access, for example in a private
/// repository.
pub fn post_comment(http: &dyn Http, token: &SecretBytes, target: &Target, text: &str) -> Result<String> {
    if text.trim().is_empty() {
        return Err(Error::BadInput("the answer has no text"));
    }
    if !api::is_name(&target.owner) || !api::is_name(&target.repo) {
        return Err(Error::BadInput("the target has the wrong form"));
    }
    let url = format!(
        "{API}/repos/{}/{}/issues/{}/comments",
        target.owner, target.repo, target.number
    );
    let mut request = api::request(Method::Post, url, Some(token))?.header("Content-Type", "application/json");
    request.body = Some(serde_json::to_vec(&serde_json::json!({ "body": text })).expect("plain data"));
    let response = api::send(http, request)?;
    if response.status != 201 {
        return Err(Error::Status(response.status));
    }
    let body = api::json(&response)?;
    api::text(&body, &["html_url"])
        .filter(|url| api::is_web_url(url))
        .map(str::to_string)
        .ok_or_else(|| Error::BadData("no html_url".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use crate::sync::tests::{EVENTS, THREADS};

    fn records(json: &str) -> Vec<Vec<u8>> {
        let values: Vec<Value> = serde_json::from_str(json).unwrap();
        values.iter().map(|value| serde_json::to_vec(value).unwrap()).collect()
    }

    fn target(owner: &str, repo: &str, number: u64) -> Option<Target> {
        Some(Target {
            owner: owner.into(),
            repo: repo.into(),
            number,
        })
    }

    #[test]
    fn targets_of_notifications() {
        let threads = records(THREADS);
        assert_eq!(answer_target(&threads[0], "n/101"), target("octo-org", "parser", 5));
        assert_eq!(answer_target(&threads[1], "n/102"), target("octo-org", "parser", 7));
        assert_eq!(answer_target(&threads[2], "n/103"), None, "a release");
        assert_eq!(answer_target(&threads[3], "n/104"), None, "no subject URL");
        assert_eq!(answer_target(b"not json", "n/101"), None);
        assert_eq!(answer_target(&threads[0], "x/101"), None);

        // The type and the URL must agree.
        let odd = br#"{"subject":{"type":"Issue","url":"https://api.github.com/repos/o/r/pulls/7"}}"#;
        assert_eq!(answer_target(odd, "n/1"), None);
        let other_host =
            br#"{"subject":{"type":"Issue","url":"https://api.github.com.evil.example/repos/o/r/issues/7"}}"#;
        assert_eq!(answer_target(other_host, "n/1"), None);
        let bad_path = br#"{"subject":{"type":"Issue","url":"https://api.github.com/repos/o/../issues/7"}}"#;
        assert_eq!(answer_target(bad_path, "n/1"), None);
    }

    #[test]
    fn targets_of_events() {
        let events = records(EVENTS);
        let targets: Vec<_> = events
            .iter()
            .take(11)
            .enumerate()
            .filter_map(|(n, raw)| Some((n, answer_target(raw, "e/1")?)))
            .collect();
        assert_eq!(
            targets,
            vec![
                (2, target("me", "project", 12).unwrap()),
                (3, target("me", "project", 13).unwrap()),
                (4, target("me", "project", 4).unwrap()),
            ]
        );
    }

    #[test]
    fn comment_request_and_result() {
        let http = FakeHttp::new();
        http.json(
            201,
            r#"{"id":3,"html_url":"https://github.com/octo-org/parser/issues/5#issuecomment-3","body":"Thanks."}"#,
        );
        let token = SecretBytes::new(b"gho_abc".to_vec());
        let to = target("octo-org", "parser", 5).unwrap();
        assert_eq!(
            post_comment(&http, &token, &to, "Thanks.\n\"Quote\""),
            Ok("https://github.com/octo-org/parser/issues/5#issuecomment-3".into())
        );
        let requests = http.take_requests();
        let request = &requests[0];
        assert_eq!(
            (request.method, request.url.as_str()),
            (
                Method::Post,
                "https://api.github.com/repos/octo-org/parser/issues/5/comments"
            )
        );
        assert_eq!(request.header_value("Authorization"), Some("Bearer gho_abc"));
        assert_eq!(request.header_value("Accept"), Some("application/vnd.github+json"));
        let body: Value = serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
        assert_eq!(body, serde_json::json!({ "body": "Thanks.\n\"Quote\"" }));
    }

    #[test]
    fn comment_errors() {
        let http = FakeHttp::new();
        let token = SecretBytes::new(b"gho_abc".to_vec());
        let to = target("octo-org", "private", 5).unwrap();
        assert!(matches!(
            post_comment(&http, &token, &to, "  \n"),
            Err(Error::BadInput(_))
        ));
        assert!(http.take_requests().is_empty());

        http.json(404, r#"{"message":"Not Found"}"#)
            .json(403, r#"{"message":"Resource not accessible"}"#)
            .json(401, "{}")
            .json(422, "{}");
        assert_eq!(post_comment(&http, &token, &to, "x"), Err(Error::NotFound));
        assert_eq!(post_comment(&http, &token, &to, "x"), Err(Error::Forbidden));
        assert_eq!(post_comment(&http, &token, &to, "x"), Err(Error::BadToken));
        assert_eq!(post_comment(&http, &token, &to, "x"), Err(Error::Status(422)));
    }
}
