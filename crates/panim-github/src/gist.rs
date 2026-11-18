//! The check of an account proof in a gist (`base/panim-proof-v1.md` §7).
//!
//! The check uses no token. Thus, GitHub does not know which account examines the proof. Without a token, GitHub
//! permits 60 requests each hour for each IP address.

use panim_core::proof::{self, ProofError};
use panim_core::{Block, Domain, Platform};
use serde::Serialize;
use serde_json::Value;

use crate::api::{self, API, Error, Result};
use crate::http::{Http, Method};

/// The host of the web pages of gists.
const GIST_HOST: &str = "gist.github.com";
/// The host of the full text of a gist file.
const RAW_HOST: &str = "gist.githubusercontent.com";

/// The check gives no more than this number of blocks. Then `GistCheck::cut` is true.
pub const MAX_BLOCKS: usize = 50;
/// A text from the gist (a file name, a login, an id or a handle) has no more than this number of characters in
/// the result. The check cuts a longer text and adds `…`.
pub const MAX_TEXT: usize = 120;

/// The account that published a gist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Owner {
    pub id: u64,
    pub login: String,
}

/// What the check found in a gist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GistResult {
    /// The gist is public and has one block or more. Each block has its own verdict.
    Blocks,
    /// The gist is public, but it has no block.
    NoBlock,
    /// The gist is secret. A proof must be in a public location, so the check did not read the files.
    Secret,
}

/// The verdict for one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The signature is correct, and the block names the account that published the gist.
    Correct,
    /// The key of the `persona` line did not make the signature.
    BadSignature,
    /// The signature is correct, but the block names a different account or a different platform.
    OtherAccount,
    /// The text starts as a block, but it does not obey the form.
    NotBlock,
}

/// One block that the check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BlockView {
    /// The name of the gist file.
    pub file: String,
    pub verdict: Verdict,
    /// The address of the persona. `None` if the text is not a block.
    pub persona: Option<String>,
    /// `proof` or `revoke`. `None` if the text is not a block.
    pub domain: Option<&'static str>,
    /// The date of the statement, as `YYYY-MM-DD`.
    pub date: Option<String>,
    /// The account identifier in the statement.
    pub id: Option<String>,
    /// The account handle in the statement.
    pub handle: Option<String>,
    /// The platform of the statement, for example `github`. `None` if the text is not a block.
    pub platform: Option<&'static str>,
    /// For a correct block: true if it is the current state of its persona and account (`base/panim-proof-v1.md`
    /// §8.2), false if a different block in the gist replaces it. `None` for a block that is not correct.
    pub current: Option<bool>,
}

/// The result of the check of a gist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GistCheck {
    /// The account that published the gist. `None` if the gist has no owner.
    pub owner: Option<Owner>,
    pub result: GistResult,
    pub blocks: Vec<BlockView>,
    /// True if the gist has more than `MAX_BLOCKS` blocks. Then `blocks` has only the first `MAX_BLOCKS`.
    pub cut: bool,
}

/// Cuts a text from the gist to `MAX_TEXT` characters.
fn clip(text: String) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text;
    }
    let mut short: String = text.chars().take(MAX_TEXT - 1).collect();
    short.push('…');
    short
}

/// Reads a gist identifier from an identifier or from a gist URL.
///
/// The URL form is `https://gist.github.com/<login>/<id>` or `https://gist.github.com/<id>`. The function refuses
/// all other hosts.
pub fn gist_id(input: &str) -> Option<String> {
    let input = input.trim();
    let is_id = |text: &str| !text.is_empty() && text.len() <= 40 && text.bytes().all(|b| b.is_ascii_hexdigit());
    if is_id(input) {
        return Some(input.to_ascii_lowercase());
    }
    let url = url::Url::parse(input).ok()?;
    let plain = url.scheme() == "https"
        && url.host_str() == Some(GIST_HOST)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !plain {
        return None;
    }
    let parts: Vec<&str> = url.path_segments()?.filter(|part| !part.is_empty()).collect();
    let id = match parts[..] {
        [id] => id,
        [login, id] if api::is_name(login) => id,
        _ => return None,
    };
    is_id(id).then(|| id.to_ascii_lowercase())
}

/// Gets a gist without a token and examines each block in its files.
///
/// `gist` is a gist identifier or a gist URL. `Error::NotFound` shows that GitHub has no such gist.
pub fn check_gist(http: &dyn Http, gist: &str) -> Result<GistCheck> {
    let id = gist_id(gist).ok_or(Error::BadInput("the text is not a gist identifier or a gist URL"))?;
    let response = api::send(http, api::request(Method::Get, format!("{API}/gists/{id}"), None)?)?;
    if response.status != 200 {
        return Err(Error::Status(response.status));
    }
    let body = api::json(&response)?;
    let owner = body.get("owner").and_then(|owner| {
        Some(Owner {
            id: owner.get("id")?.as_u64()?,
            login: api::text(owner, &["login"])?.to_string(),
        })
    });
    if body.get("public").and_then(Value::as_bool) != Some(true) {
        return Ok(GistCheck {
            owner: owner.map(clip_owner),
            result: GistResult::Secret,
            blocks: Vec::new(),
            cut: false,
        });
    }
    let files = body
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::BadData("no files".into()))?;
    let mut blocks = Vec::new();
    for (name, file) in files {
        let truncated = file.get("truncated").and_then(Value::as_bool) == Some(true);
        let content = if truncated {
            full_text(http, file)?
        } else {
            api::text(file, &["content"]).unwrap_or("").to_string()
        };
        blocks.extend(examine(name, &content, owner.as_ref()));
    }
    // The current state of each persona and account, from the correct blocks only (§8.2).
    let correct: Vec<Block> = blocks.iter().filter_map(|(_, block)| block.clone()).collect();
    let mut flags = proof::current_flags(&correct).into_iter();
    let mut views: Vec<BlockView> = blocks
        .into_iter()
        .map(|(mut view, block)| {
            if block.is_some() {
                view.current = flags.next();
            }
            view
        })
        .collect();
    let result = if views.is_empty() {
        GistResult::NoBlock
    } else {
        GistResult::Blocks
    };
    let cut = views.len() > MAX_BLOCKS;
    views.truncate(MAX_BLOCKS);
    let views = views
        .into_iter()
        .map(|view| BlockView {
            file: clip(view.file),
            id: view.id.map(clip),
            handle: view.handle.map(clip),
            ..view
        })
        .collect();
    Ok(GistCheck {
        owner: owner.map(clip_owner),
        result,
        blocks: views,
        cut,
    })
}

fn clip_owner(owner: Owner) -> Owner {
    Owner {
        login: clip(owner.login),
        ..owner
    }
}

/// Gets the full text of a gist file that the API response cut.
fn full_text(http: &dyn Http, file: &Value) -> Result<String> {
    let raw_url = api::text(file, &["raw_url"]).ok_or_else(|| Error::BadData("no raw_url".into()))?;
    let url = url::Url::parse(raw_url).map_err(|_| Error::BadData("the raw_url is not a URL".into()))?;
    if url.scheme() != "https" || url.host_str() != Some(RAW_HOST) || url.port().is_some() {
        return Err(Error::BadData("the raw_url is not on the host of gist files".into()));
    }
    let response = api::send(http, api::request(Method::Get, raw_url, None)?)?;
    if response.status != 200 {
        return Err(Error::Status(response.status));
    }
    String::from_utf8(response.body).map_err(|_| Error::BadData("the file is not text".into()))
}

/// Finds each block in the text of one file and gives its verdict. A correct block also comes back as a `Block`.
fn examine(file: &str, text: &str, owner: Option<&Owner>) -> Vec<(BlockView, Option<Block>)> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut blocks = Vec::new();
    for (start, line) in lines.iter().enumerate() {
        if !Domain::ALL.iter().any(|domain| *line == domain.literal()) {
            continue;
        }
        let candidate = lines[start..].join("\n");
        let view = match proof::read_block(&candidate) {
            Ok(block) => {
                let verdict = match block.check_signature() {
                    Err(_) => Verdict::BadSignature,
                    Ok(()) if is_from(&block, owner) => Verdict::Correct,
                    Ok(()) => Verdict::OtherAccount,
                };
                let correct = (verdict == Verdict::Correct).then(|| block.clone());
                let statement = block.statement;
                let view = BlockView {
                    file: file.to_string(),
                    verdict,
                    persona: Some(statement.persona.to_string()),
                    domain: Some(match statement.domain {
                        Domain::Proof => "proof",
                        Domain::Revoke => "revoke",
                    }),
                    date: Some(statement.date.to_string()),
                    id: statement.id,
                    handle: statement.handle,
                    platform: Some(proof::platform_name(statement.platform)),
                    current: None,
                };
                (view, correct)
            }
            Err(ProofError::NoBlock) => continue,
            Err(_) => (
                BlockView {
                    file: file.to_string(),
                    verdict: Verdict::NotBlock,
                    persona: None,
                    domain: None,
                    date: None,
                    id: None,
                    handle: None,
                    platform: None,
                    current: None,
                },
                None,
            ),
        };
        blocks.push(view);
    }
    blocks
}

/// Steps 7 and 8 of §7: true if the block names the owner of the gist.
fn is_from(block: &Block, owner: Option<&Owner>) -> bool {
    owner.is_some_and(|owner| block.is_from(Platform::GitHub, Some(&owner.id.to_string()), Some(&owner.login)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use panim_core::proof::{Date, Statement};
    use panim_core::{PersonaKey, SecretBytes};

    const ID: &str = "aa5a315d61ae9438b18d";

    fn key() -> PersonaKey {
        PersonaKey::from_secret(&SecretBytes::new(vec![0x46; 32])).unwrap()
    }

    fn block(domain: Domain, platform: Platform, id: &str, handle: &str, date: &str) -> String {
        let key = key();
        Statement {
            domain,
            persona: key.address(),
            platform,
            id: Some(id.into()),
            handle: Some(handle.into()),
            date: Date::parse(date).unwrap(),
        }
        .sign(&key)
        .unwrap()
        .text()
    }

    fn proof() -> String {
        block(Domain::Proof, Platform::GitHub, "583231", "octocat", "2026-11-12")
    }

    fn gist(public: bool, files: &[(&str, &str)]) -> String {
        let files: serde_json::Map<String, Value> = files
            .iter()
            .map(|(name, content)| {
                let file = serde_json::json!({ "filename": name, "truncated": false, "content": content });
                (name.to_string(), file)
            })
            .collect();
        serde_json::json!({
            "id": ID,
            "public": public,
            "owner": { "login": "octocat", "id": 583231 },
            "files": files,
        })
        .to_string()
    }

    #[test]
    fn identifiers_and_urls() {
        let long = "0123456789abcdef0123456789abcdef";
        assert_eq!(gist_id(ID).as_deref(), Some(ID));
        assert_eq!(gist_id(&format!("  {long}\n")).as_deref(), Some(long));
        assert_eq!(gist_id("AA5A315D61AE9438B18D").as_deref(), Some(ID));
        assert_eq!(gist_id("2710948").as_deref(), Some("2710948"));
        for url in [
            format!("https://gist.github.com/octocat/{ID}"),
            format!("https://gist.github.com/{ID}"),
            format!("https://gist.github.com/octocat/{ID}/"),
            format!("https://gist.github.com/octocat/{ID}#file-proof-txt"),
        ] {
            assert_eq!(gist_id(&url).as_deref(), Some(ID), "{url}");
        }
        for bad in [
            String::new(),
            "not an id".into(),
            format!("http://gist.github.com/octocat/{ID}"),
            format!("https://github.com/octocat/{ID}"),
            format!("https://gist.github.com.evil.example/octocat/{ID}"),
            format!("https://evil.example/gist.github.com/octocat/{ID}"),
            format!("https://user@gist.github.com/octocat/{ID}"),
            format!("https://gist.github.com:8443/octocat/{ID}"),
            format!("https://gist.githubusercontent.com/octocat/{ID}"),
            format!("https://gist.github.com/octocat/{ID}/raw"),
            "https://gist.github.com/octocat".into(),
            "https://gist.github.com/".into(),
            format!("{ID}/../x"),
        ] {
            assert_eq!(gist_id(&bad), None, "{bad}");
        }
    }

    #[test]
    fn correct_proof() {
        let http = FakeHttp::new();
        let text = format!("My proof:\r\n\r\n{}\r\n", proof().replace('\n', "\r\n"));
        http.json(
            200,
            &gist(true, &[("proof.txt", &text), ("notes.md", "No block here.")]),
        );
        let check = check_gist(&http, &format!("https://gist.github.com/octocat/{ID}")).unwrap();
        assert_eq!(
            check.owner,
            Some(Owner {
                id: 583_231,
                login: "octocat".into()
            })
        );
        assert_eq!(check.result, GistResult::Blocks);
        assert_eq!(
            check.blocks,
            vec![BlockView {
                file: "proof.txt".into(),
                verdict: Verdict::Correct,
                persona: Some("0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f".into()),
                domain: Some("proof"),
                date: Some("2026-11-12".into()),
                id: Some("583231".into()),
                handle: Some("octocat".into()),
                platform: Some("github"),
                current: Some(true),
            }]
        );
        assert!(!check.cut);

        let requests = http.take_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, format!("https://api.github.com/gists/{ID}"));
        assert_eq!(
            requests[0].header_value("Authorization"),
            None,
            "the check uses no token"
        );
        assert_eq!(requests[0].header_value("User-Agent"), Some("PANIM"));

        let json = serde_json::to_value(&check).unwrap();
        assert_eq!(json["result"], "blocks");
        assert_eq!(json["blocks"][0]["verdict"], "correct");
    }

    #[test]
    fn verdicts() {
        let good = proof();
        let revoke = block(Domain::Revoke, Platform::GitHub, "583231", "octocat", "2026-12-01");
        // The id binds the account. The handle is the same, but the id is different.
        let other_id = block(Domain::Proof, Platform::GitHub, "999", "octocat", "2026-11-12");
        let other_platform = block(Domain::Proof, Platform::X, "583231", "octocat", "2026-11-12");
        // A changed date after the signature.
        let bad_signature = good.replace("date 2026-11-12", "date 2026-11-13");
        let short = "panim/1-proof\npersona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f";
        let text = [&good, &revoke, &other_id, &other_platform, &bad_signature, short].map(|s| s.to_string());
        let http = FakeHttp::new();
        http.json(200, &gist(true, &[("all.txt", &text.join("\n\ntext between\n\n"))]));
        let check = check_gist(&http, ID).unwrap();
        let view: Vec<_> = check
            .blocks
            .iter()
            .map(|b| (b.verdict, b.domain, b.id.as_deref(), b.date.as_deref()))
            .collect();
        assert_eq!(
            view,
            vec![
                (Verdict::Correct, Some("proof"), Some("583231"), Some("2026-11-12")),
                (Verdict::Correct, Some("revoke"), Some("583231"), Some("2026-12-01")),
                (Verdict::OtherAccount, Some("proof"), Some("999"), Some("2026-11-12")),
                (Verdict::OtherAccount, Some("proof"), Some("583231"), Some("2026-11-12")),
                (Verdict::BadSignature, Some("proof"), Some("583231"), Some("2026-11-13")),
                (Verdict::NotBlock, None, None, None),
            ]
        );
        // The revocation has the later date, so it is the current state. Only correct blocks have a state.
        let current: Vec<_> = check.blocks.iter().map(|b| b.current).collect();
        assert_eq!(current, [Some(false), Some(true), None, None, None, None]);
    }

    #[test]
    fn current_state_across_files() {
        let old = proof();
        let new = block(Domain::Proof, Platform::GitHub, "583231", "octocat", "2027-01-05");
        let http = FakeHttp::new();
        http.json(200, &gist(true, &[("a.txt", &new), ("b.txt", &old)]));
        let check = check_gist(&http, ID).unwrap();
        let view: Vec<_> = check.blocks.iter().map(|b| (b.file.as_str(), b.current)).collect();
        assert_eq!(view, [("a.txt", Some(true)), ("b.txt", Some(false))]);
    }

    #[test]
    fn remote_text_is_limited() {
        let long_name = "f".repeat(500);
        let long_handle = format!("h{}", "a".repeat(400));
        let wide = block(Domain::Proof, Platform::X, "1", &long_handle, "2026-11-12");
        let many = vec![proof(); MAX_BLOCKS + 3].join(
            "
",
        );
        let http = FakeHttp::new();
        http.json(200, &gist(true, &[(&long_name, &wide), ("z.txt", &many)]));
        let check = check_gist(&http, ID).unwrap();
        assert!(check.cut);
        assert_eq!(check.blocks.len(), MAX_BLOCKS);
        assert_eq!(check.blocks[0].file.chars().count(), MAX_TEXT);
        assert!(check.blocks[0].file.ends_with('…'));
        assert_eq!(check.blocks[0].handle.as_ref().unwrap().chars().count(), MAX_TEXT);
        // The equal blocks have one current block. It is the last one, so the cut removed it.
        assert!(check.blocks[1..].iter().all(|b| b.current == Some(false)));
    }

    #[test]
    fn owner_with_a_different_id() {
        let http = FakeHttp::new();
        let body = gist(true, &[("proof.txt", &proof())]).replace("583231 }", "1 }");
        let body = body.replace(r#""id":583231"#, r#""id":42"#);
        http.json(200, &body);
        let check = check_gist(&http, ID).unwrap();
        assert_eq!(check.owner.as_ref().unwrap().id, 42);
        assert_eq!(check.blocks[0].verdict, Verdict::OtherAccount);

        // A gist without an owner cannot hold a correct proof.
        let mut value: Value = serde_json::from_str(&gist(true, &[("proof.txt", &proof())])).unwrap();
        value["owner"] = Value::Null;
        http.json(200, &value.to_string());
        let check = check_gist(&http, ID).unwrap();
        assert_eq!((check.owner, check.blocks[0].verdict), (None, Verdict::OtherAccount));
    }

    #[test]
    fn secret_gist_and_no_block() {
        let http = FakeHttp::new();
        http.json(200, &gist(false, &[("proof.txt", &proof())]))
            .json(
                200,
                &gist(true, &[("notes.md", "panim/1-proof is the domain.\nNo block.")]),
            )
            .json(404, r#"{"message":"Not Found"}"#)
            .response(
                403,
                &[("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", "1794568800")],
                "{}",
            );
        let secret = check_gist(&http, ID).unwrap();
        assert_eq!((secret.result, secret.blocks.len()), (GistResult::Secret, 0));
        assert_eq!(secret.owner.unwrap().login, "octocat");
        let empty = check_gist(&http, ID).unwrap();
        assert_eq!((empty.result, empty.blocks.len()), (GistResult::NoBlock, 0));
        assert_eq!(check_gist(&http, ID), Err(Error::NotFound));
        assert!(matches!(check_gist(&http, ID), Err(Error::RateLimit { .. })));

        http.take_requests();
        assert!(matches!(
            check_gist(&http, "https://example.org/octocat/aa5a315d61ae9438b18d"),
            Err(Error::BadInput(_))
        ));
        assert!(http.take_requests().is_empty(), "a refused host gets no request");
    }

    #[test]
    fn cut_file_comes_from_the_raw_host() {
        let raw = format!("https://gist.githubusercontent.com/octocat/{ID}/raw/0123/proof.txt");
        let body = |raw_url: &str| {
            serde_json::json!({
                "public": true,
                "owner": { "login": "octocat", "id": 583231 },
                "files": { "proof.txt": { "truncated": true, "content": "panim/1-pro", "raw_url": raw_url } },
            })
            .to_string()
        };
        let http = FakeHttp::new();
        http.json(200, &body(&raw)).response(200, &[], &proof());
        let check = check_gist(&http, ID).unwrap();
        assert_eq!(check.blocks[0].verdict, Verdict::Correct);
        let requests = http.take_requests();
        assert_eq!(requests[1].url, raw);
        assert_eq!(requests[1].header_value("Authorization"), None);

        for bad in [
            "https://evil.example/raw/proof.txt",
            "http://gist.githubusercontent.com/octocat/x/raw/proof.txt",
            "https://gist.githubusercontent.com.evil.example/x",
        ] {
            http.json(200, &body(bad));
            assert!(matches!(check_gist(&http, ID), Err(Error::BadData(_))), "{bad}");
            assert_eq!(http.take_requests().len(), 1, "{bad}");
        }
    }
}
