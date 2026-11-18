//! Account proofs and revocations, version 1 (`base/panim-proof-v1.md`).
//!
//! Version 1 is temporary. The ZIKARON face law will replace it (`docs/DISCUSSION.md` §12). Thus, this module is
//! separate from the rest of the core. A module for the face law can come next to it.
//!
//! - `Statement` is the five lines that the persona key signs (§3).
//! - `Block` is a statement and its signature (§4). A person posts the block.
//! - `parse_block` does steps 2 to 6 of §7. `Block::is_from` does steps 7 and 8.
//! - `read_block` does only steps 2 to 4. It does not examine the signature.
//! - `current_state` applies the rule of §8. `current_flags` marks the current block of each account in a list.
//! - `proof_statement` makes the statement for an account of the person. `account_handle` and `account_id` give
//!   the handle and the id in the form of §5.
//! - `KeptProof` is the latest block that PANIM made for an account.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::account::{Account, Platform};
use crate::ids::PersonaAddress;
use crate::key::{self, Domain, KeyError, PersonaKey, PersonaSignature, VerifyError};

/// A date in UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Makes a date. Returns `None` if the date is not in the calendar.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (year >= 1 && day >= 1 && day <= days).then_some(Self { year, month, day })
    }

    /// Reads the form `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let number = |range: std::ops::Range<usize>| -> Option<u16> {
            let part = &text[range];
            part.bytes().all(|b| b.is_ascii_digit()).then(|| part.parse().ok())?
        };
        Self::new(number(0..4)?, number(5..7)? as u8, number(8..10)? as u8)
    }
}

impl Date {
    /// The date in UTC of a time in seconds after 1970-01-01 00:00:00 UTC.
    ///
    /// Returns `None` if the year is not from 1 to 9999.
    pub fn from_unix_seconds(seconds: i64) -> Option<Self> {
        // The civil calendar from a day count. The algorithm is "civil_from_days" of Howard Hinnant.
        let z = seconds.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        if !(1..=9999).contains(&year) {
            return None;
        }
        Self::new(year as u16, month as u8, day as u8)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// The name of a platform in a statement (§5).
pub fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::GitHub => "github",
        Platform::X => "x",
        Platform::Telegram => "telegram",
        Platform::Discord => "discord",
        Platform::Email => "email",
    }
}

fn platform_from_name(name: &str) -> Option<Platform> {
    Platform::ALL
        .into_iter()
        .find(|platform| platform_name(*platform) == name)
}

/// The five lines that the persona key signs (§3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    /// `Domain::Proof` for a proof, `Domain::Revoke` for a revocation.
    pub domain: Domain,
    pub persona: PersonaAddress,
    pub platform: Platform,
    /// The stable identifier of the account, in decimal digits, or `None` (`-`).
    pub id: Option<String>,
    /// The name of the account on the platform, or `None` (`-`).
    pub handle: Option<String>,
    pub date: Date,
}

impl Statement {
    /// Makes sure that the statement obeys §2 and §3.
    pub fn check(&self) -> Result<(), ProofError> {
        if self.id.is_none() && self.handle.is_none() {
            return Err(ProofError::NoAccount);
        }
        if let Some(id) = &self.id {
            let digits = !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !id.starts_with('0');
            if !digits {
                return Err(ProofError::BadId);
            }
        }
        if let Some(handle) = &self.handle {
            let printable = !handle.is_empty() && handle != "-" && handle.bytes().all(|b| (0x21..=0x7e).contains(&b));
            if !printable || handle.starts_with('@') {
                return Err(ProofError::BadHandle);
            }
        }
        if self.platform == Platform::Email {
            // §5.2: there is no id, and the address binds the proof, so it is in lowercase.
            let address_ok = self
                .handle
                .as_ref()
                .is_some_and(|h| h.contains('@') && !h.bytes().any(|b| b.is_ascii_uppercase()));
            if self.id.is_some() || !address_ok {
                return Err(ProofError::BadHandle);
            }
        }
        Ok(())
    }

    /// The bytes of the statement: five lines, with no line feed at the end (§3, §4.1).
    pub fn text(&self) -> String {
        format!(
            "{}\npersona {}\nplatform {}\naccount {} {}\ndate {}",
            self.domain.literal(),
            self.persona,
            platform_name(self.platform),
            self.id.as_deref().unwrap_or("-"),
            self.handle.as_deref().unwrap_or("-"),
            self.date,
        )
    }

    /// The value that the persona key signs: SHA-256 of the bytes of the statement (§4.1).
    pub fn value(&self) -> [u8; 32] {
        Sha256::digest(self.text().as_bytes()).into()
    }

    /// Signs the statement with the persona key. The key must be the key of the `persona` line.
    pub fn sign(self, key: &PersonaKey) -> Result<Block, ProofError> {
        self.check()?;
        if key.address() != self.persona {
            return Err(ProofError::WrongKey);
        }
        let signature = key.sign(self.domain, &self.value()).map_err(ProofError::Key)?;
        Ok(Block {
            statement: self,
            signature,
        })
    }
}

/// A statement and its signature. A person posts the block (§4, §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub statement: Statement,
    pub signature: PersonaSignature,
}

impl Block {
    /// The text to post: the statement, a line feed and the `sig` line.
    pub fn text(&self) -> String {
        format!("{}\nsig {}", self.statement.text(), self.signature)
    }

    /// Steps 5 and 6 of §7: makes sure that the key of the `persona` line made the signature.
    pub fn check_signature(&self) -> Result<(), ProofError> {
        let statement = &self.statement;
        key::verify(statement.domain, &statement.value(), &self.signature, statement.persona)
            .map_err(ProofError::Signature)
    }

    /// Steps 7 and 8 of §7: true if the block names this account on this platform.
    ///
    /// `id` and `handle` come from the platform, for the account that published the post.
    /// If the block has an id, the ids must be equal. If not, the handles must be equal.
    pub fn is_from(&self, platform: Platform, id: Option<&str>, handle: Option<&str>) -> bool {
        let statement = &self.statement;
        if statement.platform != platform {
            return false;
        }
        match &statement.id {
            Some(own) => id == Some(own.as_str()),
            None => match (&statement.handle, handle) {
                (Some(own), Some(seen)) if platform == Platform::Email => own == &seen.to_ascii_lowercase(),
                (Some(own), Some(seen)) => own == seen,
                _ => false,
            },
        }
    }
}

/// Why a block or a statement is not correct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProofError {
    /// The text has no `panim/1-proof` line and no `panim/1-revoke` line.
    NoBlock,
    /// The text has fewer than six lines from the domain line.
    ShortBlock,
    /// A byte is not a line feed or printable ASCII (§2.1).
    BadByte,
    /// The line has the wrong form. The number is the line in the block, from 1 to 6.
    BadLine(u8),
    UnknownPlatform(String),
    BadId,
    BadHandle,
    BadDate,
    /// The `id` and the `handle` are both `-`.
    NoAccount,
    /// The signature is not correct (`zikaron-v1.md` §5.4, §5.5).
    Signature(VerifyError),
    /// The key does not belong to the persona of the statement.
    WrongKey,
    Key(KeyError),
    /// The platform has a numeric id (§5), but PANIM does not know the id of the account.
    IdMissing,
}

impl fmt::Display for ProofError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProofError::NoBlock => f.write_str("the text has no proof block"),
            ProofError::ShortBlock => f.write_str("the block has fewer than six lines"),
            ProofError::BadByte => f.write_str("the block has a byte that is not printable ASCII"),
            ProofError::BadLine(n) => write!(f, "line {n} of the block has the wrong form"),
            ProofError::UnknownPlatform(name) => write!(f, "the platform {name:?} is not known"),
            ProofError::BadId => f.write_str("the account id is not decimal digits"),
            ProofError::BadHandle => f.write_str("the account handle has the wrong form"),
            ProofError::BadDate => f.write_str("the date is not a correct YYYY-MM-DD date"),
            ProofError::NoAccount => f.write_str("the account id and the handle are both -"),
            ProofError::Signature(error) => write!(f, "the signature is not correct: {error}"),
            ProofError::WrongKey => f.write_str("the key does not belong to the persona of the statement"),
            ProofError::Key(error) => write!(f, "{error}"),
            ProofError::IdMissing => {
                f.write_str("the platform has an account id, but the id of the account is not known")
            }
        }
    }
}

impl std::error::Error for ProofError {}

/// Steps 2 to 6 of §7: finds the first block in a text and examines its form and its signature.
///
/// A correct result does not show that the account published the block. Use `Block::is_from` for that.
pub fn parse_block(text: &str) -> Result<Block, ProofError> {
    let block = read_block(text)?;
    block.check_signature()?;
    Ok(block)
}

/// Steps 2 to 4 of §7: finds the first block in a text and examines its form.
///
/// This function does not examine the signature. Use it only to show a block that has a wrong signature.
/// For all other work, use `parse_block`.
pub fn read_block(text: &str) -> Result<Block, ProofError> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let start = lines
        .iter()
        .position(|line| Domain::ALL.iter().any(|domain| *line == domain.literal()))
        .ok_or(ProofError::NoBlock)?;
    let block = lines.get(start..start + 6).ok_or(ProofError::ShortBlock)?;
    if block
        .iter()
        .any(|line| line.bytes().any(|b| !(0x20..=0x7e).contains(&b)))
    {
        return Err(ProofError::BadByte);
    }

    let domain = Domain::ALL
        .into_iter()
        .find(|d| block[0] == d.literal())
        .ok_or(ProofError::BadLine(1))?;
    let field = |n: usize, name: &str| -> Result<Vec<&str>, ProofError> {
        let mut parts = block[n].split(' ');
        if parts.next() != Some(name) {
            return Err(ProofError::BadLine(n as u8 + 1));
        }
        let rest: Vec<&str> = parts.collect();
        if rest.iter().any(|part| part.is_empty()) {
            return Err(ProofError::BadLine(n as u8 + 1));
        }
        Ok(rest)
    };
    let one = |n: usize, name: &str| -> Result<&str, ProofError> {
        match field(n, name)?.as_slice() {
            [value] => Ok(value),
            _ => Err(ProofError::BadLine(n as u8 + 1)),
        }
    };

    let persona = one(1, "persona")?;
    if persona.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(ProofError::BadLine(2));
    }
    let persona = PersonaAddress::parse(persona).ok_or(ProofError::BadLine(2))?;
    let platform_text = one(2, "platform")?;
    let platform =
        platform_from_name(platform_text).ok_or_else(|| ProofError::UnknownPlatform(platform_text.into()))?;
    let [id, handle] = field(3, "account")?[..] else {
        return Err(ProofError::BadLine(4));
    };
    let date = Date::parse(one(4, "date")?).ok_or(ProofError::BadDate)?;
    let signature_text = one(5, "sig")?;
    let lowercase_hex = signature_text.len() == 132
        && signature_text.starts_with("0x")
        && signature_text[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let signature = PersonaSignature::parse(signature_text)
        .filter(|_| lowercase_hex)
        .ok_or(ProofError::BadLine(6))?;

    let dash = |text: &str| (text != "-").then(|| text.to_string());
    let statement = Statement {
        domain,
        persona,
        platform,
        id: dash(id),
        handle: dash(handle),
        date,
    };
    statement.check()?;
    Ok(Block { statement, signature })
}

/// The rule of §8: for one persona and one account, the block with the latest date is the current state.
/// On the same date, a revocation is the current state.
///
/// The blocks must be correct (from `parse_block`) and must name the same persona and account.
/// Returns `None` if there are no blocks.
pub fn current_state<'a>(blocks: impl IntoIterator<Item = &'a Block>) -> Option<&'a Block> {
    blocks
        .into_iter()
        .max_by_key(|block| (block.statement.date, block.statement.domain == Domain::Revoke))
}

/// Marks the current block of each persona and account in a list (§8.2).
///
/// The result has one flag for each block. A flag is true for the block that `current_state` gives for its persona,
/// platform and account. The account is the id, or the handle if the block has no id. If two blocks are equal, only
/// one of them is current. Use only correct blocks (from `parse_block`).
pub fn current_flags(blocks: &[Block]) -> Vec<bool> {
    let group = |block: &Block| {
        let statement = &block.statement;
        let account = match &statement.id {
            Some(id) => (true, id.clone()),
            None => (false, statement.handle.clone().unwrap_or_default()),
        };
        (statement.persona, platform_name(statement.platform), account)
    };
    blocks
        .iter()
        .map(|block| {
            let key = group(block);
            let same = blocks.iter().filter(|other| group(other) == key);
            current_state(same).is_some_and(|current| std::ptr::eq(current, block))
        })
        .collect()
}

/// True if an account of the platform has a numeric id in a statement (§5). Only email has no id.
pub fn has_id(platform: Platform) -> bool {
    platform != Platform::Email
}

/// The handle of an account in the form of §5, from the handle that PANIM stores.
///
/// - The function removes the spaces at the start and at the end, and one `@` at the start.
/// - For email, the handle is the address, in lowercase. A Discord username is in lowercase too.
/// - Only a Telegram account can have no handle (`None`, which is `-` in the statement).
///
/// Returns `ProofError::BadHandle` if the result is not a correct handle.
pub fn account_handle(platform: Platform, stored: &str) -> Result<Option<String>, ProofError> {
    let trimmed = stored.trim();
    let handle = match platform {
        Platform::Email => trimmed.to_ascii_lowercase(),
        Platform::Discord => trimmed.strip_prefix('@').unwrap_or(trimmed).to_ascii_lowercase(),
        _ => trimmed.strip_prefix('@').unwrap_or(trimmed).to_string(),
    };
    if handle.is_empty() {
        return match platform {
            Platform::Telegram => Ok(None),
            _ => Err(ProofError::BadHandle),
        };
    }
    let printable = handle != "-" && handle.bytes().all(|b| (0x21..=0x7e).contains(&b)) && !handle.starts_with('@');
    if !printable || handle.len() > MAX_HANDLE {
        return Err(ProofError::BadHandle);
    }
    if platform == Platform::Email {
        let address_ok = handle
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.contains('@'));
        if !address_ok {
            return Err(ProofError::BadHandle);
        }
    }
    Ok(Some(handle))
}

/// The longest handle that `account_handle` accepts. An email address has 254 characters or fewer.
pub const MAX_HANDLE: usize = 254;

/// The longest id that `account_id` accepts. The largest 64-bit number has 20 digits.
pub const MAX_ID: usize = 20;

/// The id of an account in the form of §5, from the id that PANIM stores.
///
/// - Email has no id. The result is `None` (`-`), and the function ignores the stored id.
/// - All other platforms have a numeric id. The id must be decimal digits with no zero at the start.
///
/// Returns `ProofError::IdMissing` if the platform has an id and there is no stored id, and `ProofError::BadId` if
/// the id is not correct.
pub fn account_id(platform: Platform, stored: Option<&str>) -> Result<Option<String>, ProofError> {
    if !has_id(platform) {
        return Ok(None);
    }
    let id = stored
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(ProofError::IdMissing)?;
    let digits = id.len() <= MAX_ID && id.bytes().all(|b| b.is_ascii_digit()) && !id.starts_with('0');
    if !digits {
        return Err(ProofError::BadId);
    }
    Ok(Some(id.to_string()))
}

/// The proof statement for an account of the person (§3, §5).
///
/// `persona` is the address of the persona that has the account. `date` is the date of the signature, in UTC.
pub fn proof_statement(persona: PersonaAddress, account: &Account, date: Date) -> Result<Statement, ProofError> {
    let statement = Statement {
        domain: Domain::Proof,
        persona,
        platform: account.platform,
        id: account_id(account.platform, account.remote_id.as_deref())?,
        handle: account_handle(account.platform, &account.handle)?,
        date,
    };
    statement.check()?;
    Ok(statement)
}

/// The latest block that PANIM made for an account, and where the person posted it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeptProof {
    /// The text of the block (§4), six lines.
    pub block: String,
    /// The URL where the person posted the block, if the person gave it.
    pub location: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{AccountId, PersonaId};
    use crate::secret::SecretBytes;

    // The worked example of base/panim-proof-v1.md §9.
    const EXAMPLE_PROOF: &str = "panim/1-proof
persona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f
platform github
account 583231 octocat
date 2026-11-12
sig 0xb566ce41a13d26ab1ed686f0f3032e296d6eb7953aa0ccef96065dbc33313ba771817564b371933a0cbaa3cda6cbe6a661e49eef56faddbf52033e3f5f389a2f1c";
    const EXAMPLE_REVOKE: &str = "panim/1-revoke
persona 0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f
platform github
account 583231 octocat
date 2026-12-01
sig 0x13071f1ed3021cfefc6248699a3d60f97de86dfc4809dca953d1516a0b34dde672fa07cb713c2aa6b0b95e53786d712a93c1551c413e1feb68969a6f52affe9f1c";

    fn test_key() -> PersonaKey {
        PersonaKey::from_secret(&SecretBytes::new(vec![0x46; 32])).unwrap()
    }

    fn statement(domain: Domain, date: &str) -> Statement {
        Statement {
            domain,
            persona: test_key().address(),
            platform: Platform::GitHub,
            id: Some("583231".into()),
            handle: Some("octocat".into()),
            date: Date::parse(date).unwrap(),
        }
    }

    fn with_line(block: &str, n: usize, line: &str) -> String {
        let mut lines: Vec<&str> = block.lines().collect();
        lines[n - 1] = line;
        lines.join("\n")
    }

    #[test]
    fn worked_example_matches_the_specification() {
        let proof = statement(Domain::Proof, "2026-11-12");
        assert_eq!(proof.text().len(), 119);
        assert_eq!(
            proof.value(),
            <[u8; 32]>::try_from(
                (0..32)
                    .map(|i| u8::from_str_radix(
                        &"05e41222a4740f7c6789d1ea5535197a28f38a24a0c75b37d74c7672ed52c447"[2 * i..2 * i + 2],
                        16
                    )
                    .unwrap())
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        assert_eq!(proof.sign(&test_key()).unwrap().text(), EXAMPLE_PROOF);
        assert_eq!(
            statement(Domain::Revoke, "2026-12-01")
                .sign(&test_key())
                .unwrap()
                .text(),
            EXAMPLE_REVOKE
        );
        for example in [EXAMPLE_PROOF, EXAMPLE_REVOKE] {
            assert_eq!(parse_block(example).unwrap().text(), example);
        }
    }

    #[test]
    fn block_inside_other_text_with_crlf() {
        let post = format!(
            "My PANIM proof:\r\n\r\n{}\r\n\r\nThanks!",
            EXAMPLE_PROOF.replace('\n', "\r\n")
        );
        assert_eq!(parse_block(&post).unwrap().text(), EXAMPLE_PROOF);
    }

    #[test]
    fn longest_x_block_fits_in_one_post() {
        let block = Statement {
            domain: Domain::Proof,
            persona: test_key().address(),
            platform: Platform::X,
            id: Some("1234567890123456789".into()),
            handle: Some("abcdefghijklmno".into()),
            date: Date::new(2026, 11, 12).unwrap(),
        }
        .sign(&test_key())
        .unwrap();
        assert_eq!(block.text().chars().count(), 272);
    }

    #[test]
    fn parse_refuses_bad_blocks() {
        let cases: Vec<(String, ProofError)> = vec![
            ("no block here".into(), ProofError::NoBlock),
            (
                EXAMPLE_PROOF.lines().take(5).collect::<Vec<_>>().join("\n"),
                ProofError::ShortBlock,
            ),
            (
                with_line(EXAMPLE_PROOF, 4, "account 583231 octocät"),
                ProofError::BadByte,
            ),
            (
                with_line(EXAMPLE_PROOF, 2, "persona 0x9D8A62F656A8D1615C1294FD71E9CFB3E4855A4F"),
                ProofError::BadLine(2),
            ),
            (
                with_line(EXAMPLE_PROOF, 2, "persona  0x9d8a62f656a8d1615c1294fd71e9cfb3e4855a4f"),
                ProofError::BadLine(2),
            ),
            (
                with_line(EXAMPLE_PROOF, 3, "platform facebook"),
                ProofError::UnknownPlatform("facebook".into()),
            ),
            (
                with_line(EXAMPLE_PROOF, 4, "account 0583231 octocat"),
                ProofError::BadId,
            ),
            (
                with_line(EXAMPLE_PROOF, 4, "account 583231 @octocat"),
                ProofError::BadHandle,
            ),
            (with_line(EXAMPLE_PROOF, 4, "account - -"), ProofError::NoAccount),
            (with_line(EXAMPLE_PROOF, 4, "account 583231"), ProofError::BadLine(4)),
            (with_line(EXAMPLE_PROOF, 5, "date 2026-02-30"), ProofError::BadDate),
            (
                with_line(
                    EXAMPLE_PROOF,
                    6,
                    &EXAMPLE_PROOF
                        .lines()
                        .nth(5)
                        .unwrap()
                        .to_uppercase()
                        .replace("SIG 0X", "sig 0x"),
                ),
                ProofError::BadLine(6),
            ),
            (with_line(EXAMPLE_PROOF, 6, "sig 0x00"), ProofError::BadLine(6)),
            (
                with_line(EXAMPLE_PROOF, 5, "date 2026-11-13"),
                ProofError::Signature(VerifyError::Signer(key_for_changed_date())),
            ),
            (format!("{EXAMPLE_PROOF} "), ProofError::BadLine(6)),
        ];
        for (text, error) in cases {
            assert_eq!(parse_block(&text), Err(error), "{text}");
        }
    }

    // The signer that a changed statement gives with the old signature. It is not the persona.
    fn key_for_changed_date() -> PersonaAddress {
        let block = parse_block(EXAMPLE_PROOF).unwrap();
        let mut changed = block.statement.clone();
        changed.date = Date::new(2026, 11, 13).unwrap();
        key::recover(Domain::Proof, &changed.value(), &block.signature).unwrap()
    }

    #[test]
    fn domain_cannot_change_after_the_signature() {
        let changed = EXAMPLE_PROOF.replacen("panim/1-proof", "panim/1-revoke", 1);
        assert!(matches!(
            parse_block(&changed),
            Err(ProofError::Signature(VerifyError::Signer(_)))
        ));
    }

    #[test]
    fn signing_needs_the_key_of_the_persona() {
        let mut other = statement(Domain::Proof, "2026-11-12");
        other.persona = PersonaAddress([9; 20]);
        assert_eq!(other.sign(&test_key()), Err(ProofError::WrongKey));
    }

    #[test]
    fn email_statements() {
        let mut mail = statement(Domain::Proof, "2026-11-12");
        mail.platform = Platform::Email;
        mail.id = None;
        mail.handle = Some("me@proton.me".into());
        let block = mail.clone().sign(&test_key()).unwrap();
        assert!(block.is_from(Platform::Email, None, Some("Me@Proton.me")));
        assert!(!block.is_from(Platform::Email, None, Some("you@proton.me")));
        mail.handle = Some("Me@proton.me".into());
        assert_eq!(mail.check(), Err(ProofError::BadHandle));
        mail.handle = Some("me@proton.me".into());
        mail.id = Some("1".into());
        assert_eq!(mail.check(), Err(ProofError::BadHandle));
    }

    #[test]
    fn the_id_binds_the_account() {
        let block = parse_block(EXAMPLE_PROOF).unwrap();
        assert!(block.is_from(Platform::GitHub, Some("583231"), Some("octocat-renamed")));
        assert!(!block.is_from(Platform::GitHub, Some("1"), Some("octocat")));
        assert!(!block.is_from(Platform::X, Some("583231"), Some("octocat")));
    }

    #[test]
    fn latest_date_wins_and_revocation_wins_a_tie() {
        let proof = parse_block(EXAMPLE_PROOF).unwrap();
        let revoke = parse_block(EXAMPLE_REVOKE).unwrap();
        assert_eq!(current_state([&proof, &revoke]), Some(&revoke));
        let same_day = statement(Domain::Revoke, "2026-11-12").sign(&test_key()).unwrap();
        assert_eq!(current_state([&proof, &same_day]), Some(&same_day));
        assert_eq!(current_state([&same_day, &proof]), Some(&same_day));
        let later = statement(Domain::Proof, "2027-01-01").sign(&test_key()).unwrap();
        assert_eq!(current_state([&revoke, &later, &proof]), Some(&later));
        assert_eq!(current_state([]), None);
    }

    #[test]
    fn dates() {
        assert_eq!(
            Date::parse("2024-02-29").map(|d| d.to_string()),
            Some("2024-02-29".into())
        );
        for bad in [
            "2023-02-29",
            "1900-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-1-01",
            "26-01-01",
            "2026/01/01",
            "+026-01-01",
        ] {
            assert_eq!(Date::parse(bad), None, "{bad}");
        }
        assert!(Date::parse("2000-02-29").is_some());
    }

    fn account(platform: Platform, handle: &str, remote_id: Option<&str>) -> Account {
        Account {
            id: AccountId(1),
            persona: PersonaId(1),
            platform,
            display_name: "name".into(),
            handle: handle.into(),
            remote_id: remote_id.map(Into::into),
            joined: None,
            browser: None,
        }
    }

    #[test]
    fn statement_for_a_github_account_is_the_worked_example() {
        let octocat = account(Platform::GitHub, "octocat", Some("583231"));
        let date = Date::from_unix_seconds(1_794_441_600 + 86_399).unwrap();
        let block = proof_statement(test_key().address(), &octocat, date)
            .unwrap()
            .sign(&test_key())
            .unwrap();
        assert_eq!(block.text(), EXAMPLE_PROOF);
        block.check_signature().unwrap();
        assert!(block.is_from(Platform::GitHub, Some("583231"), Some("octocat")));
        // The same date gives the same block, because the signature is deterministic.
        let again = proof_statement(test_key().address(), &octocat, date).unwrap();
        assert_eq!(again.sign(&test_key()).unwrap(), block);
    }

    #[test]
    fn handles_in_the_form_of_section_5() {
        let ok = |platform, stored: &str, expected: Option<&str>| {
            assert_eq!(
                account_handle(platform, stored),
                Ok(expected.map(Into::into)),
                "{platform} {stored:?}"
            );
        };
        ok(Platform::GitHub, "octocat", Some("octocat"));
        ok(Platform::GitHub, " @octocat ", Some("octocat"));
        ok(Platform::X, "@Jack_Dorsey", Some("Jack_Dorsey"));
        ok(Platform::Discord, "@Some.Name", Some("some.name"));
        ok(Platform::Telegram, "", None);
        ok(Platform::Telegram, "@durov", Some("durov"));
        ok(Platform::Email, " Me@Proton.ME ", Some("me@proton.me"));
        let bad = |platform, stored: &str| {
            assert_eq!(
                account_handle(platform, stored),
                Err(ProofError::BadHandle),
                "{platform} {stored:?}"
            );
        };
        bad(Platform::GitHub, "");
        bad(Platform::X, "@");
        bad(Platform::X, "@@name");
        bad(Platform::X, "two words");
        bad(Platform::X, "-");
        bad(Platform::X, "naïve");
        bad(Platform::Discord, "  ");
        bad(Platform::Email, "me");
        bad(Platform::Email, "@proton.me");
        bad(Platform::Email, "me@proton");
        bad(Platform::Email, "me@a@proton.me");
        bad(Platform::Email, "");
        bad(Platform::GitHub, &"a".repeat(MAX_HANDLE + 1));
    }

    #[test]
    fn id_rule_for_each_platform() {
        for platform in [Platform::GitHub, Platform::X, Platform::Telegram, Platform::Discord] {
            assert!(has_id(platform));
            assert_eq!(account_id(platform, Some(" 583231 ")), Ok(Some("583231".into())));
            assert_eq!(account_id(platform, None), Err(ProofError::IdMissing), "{platform}");
            assert_eq!(account_id(platform, Some("")), Err(ProofError::IdMissing), "{platform}");
            for bad in ["0583231", "0", "12a", "-", "-5", "1 2", "+1", &"9".repeat(MAX_ID + 1)] {
                assert_eq!(
                    account_id(platform, Some(bad)),
                    Err(ProofError::BadId),
                    "{platform} {bad}"
                );
            }
        }
        assert_eq!(
            account_id(Platform::Discord, Some("80351110224678912")),
            Ok(Some("80351110224678912".into()))
        );
        assert!(!has_id(Platform::Email));
        assert_eq!(account_id(Platform::Email, None), Ok(None));
        assert_eq!(account_id(Platform::Email, Some("5")), Ok(None));

        // A platform with an id never gets a statement with the id `-`.
        let date = Date::new(2026, 11, 15).unwrap();
        let x = account(Platform::X, "@me", None);
        assert_eq!(
            proof_statement(test_key().address(), &x, date),
            Err(ProofError::IdMissing)
        );
        let mail = account(Platform::Email, "Me@Proton.me", None);
        let statement = proof_statement(test_key().address(), &mail, date).unwrap();
        assert!(statement.text().contains("\nplatform email\naccount - me@proton.me\n"));
        let block = statement.sign(&test_key()).unwrap();
        assert_eq!(parse_block(&block.text()), Ok(block));
    }

    #[test]
    fn dates_from_unix_seconds() {
        let date = |seconds| Date::from_unix_seconds(seconds).map(|d| d.to_string());
        assert_eq!(date(0).as_deref(), Some("1970-01-01"));
        assert_eq!(date(1_794_441_600).as_deref(), Some("2026-11-12"));
        assert_eq!(date(1_794_441_599).as_deref(), Some("2026-11-11"));
        assert_eq!(date(951_868_799).as_deref(), Some("2000-02-29"));
        assert_eq!(date(951_868_800).as_deref(), Some("2000-03-01"));
        assert_eq!(date(-1).as_deref(), Some("1969-12-31"));
        assert_eq!(date(253_402_300_800), None, "the year 10000");
    }

    #[test]
    fn current_flags_mark_one_block_for_each_account() {
        let proof = parse_block(EXAMPLE_PROOF).unwrap();
        let revoke = parse_block(EXAMPLE_REVOKE).unwrap();
        let later = statement(Domain::Proof, "2027-01-01").sign(&test_key()).unwrap();
        let mut other = statement(Domain::Proof, "2026-11-12");
        other.id = Some("1".into());
        let other = other.sign(&test_key()).unwrap();
        assert_eq!(
            current_flags(&[proof.clone(), revoke.clone(), other.clone()]),
            [false, true, true]
        );
        assert_eq!(
            current_flags(&[revoke.clone(), later.clone(), proof.clone()]),
            [false, true, false]
        );
        assert_eq!(current_flags(&[proof.clone(), proof.clone()]), [false, true]);
        assert_eq!(current_flags(&[]), Vec::<bool>::new());
    }
}
