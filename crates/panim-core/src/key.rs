//! The persona key: a raw secp256k1 key that obeys the key rules of ZIKARON from the first day.
//!
//! In step 3, the same key becomes the ZIKARON identity of the persona. Thus, the key must obey
//! `base/zikaron-v1.md` §5 in the ZIKARON repository now:
//!
//! - The key signs only messages that have the ZIKARON shape: `D || 0x0A || hex32(value)` (§5.2).
//! - `D` is a domain literal of PANIM. No ZIKARON domain uses it (§5.6, §5.7).
//! - The digest is EIP-191 with Keccak-256 (§5.3).
//! - The nonce is RFC 6979 with HMAC-SHA256 on the digest. The signature is low-s, and `v` is 27 or 28 (§5.4, §5.7).
//!
//! The key has no function that signs other bytes. Thus, it cannot answer a `personal_sign` request or sign an
//! EIP-7702 authorization. The key must not hold money.
//!
//! This module does not use ZIKARON code. The tests use the signing vectors of ZIKARON as data.

use std::fmt;

use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use sha3::{Digest, Keccak256};

use crate::ids::PersonaAddress;
use crate::secret::SecretBytes;

/// A domain of PANIM. The list is closed: the key signs under these literals only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Domain {
    /// An account proof (`docs/DESIGN.md` §6).
    Proof,
    /// A revocation of an account proof.
    Revoke,
}

impl Domain {
    pub const ALL: [Domain; 2] = [Domain::Proof, Domain::Revoke];

    /// The domain literal. It has no line feed, and no ZIKARON domain uses it.
    pub fn literal(self) -> &'static str {
        match self {
            Domain::Proof => "panim/1-proof",
            Domain::Revoke => "panim/1-revoke",
        }
    }
}

/// A 65-byte signature `r || s || v`, with `v` 27 or 28.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersonaSignature(pub [u8; 65]);

impl PersonaSignature {
    /// Reads the text form: `0x` and 130 hexadecimal digits.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = parse_hex(text)?;
        Some(Self(bytes.try_into().ok()?))
    }
}

impl fmt::Display for PersonaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0))
    }
}

impl fmt::Debug for PersonaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PersonaSignature({self})")
    }
}

/// An error from the persona key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The secret is not a correct secp256k1 private key.
    BadSecret,
    /// The system did not give random bytes.
    NoRandom,
    /// The nonce gave an `r` that is not the x-coordinate itself. The chance is less than 2^-127.
    /// ZIKARON then takes the next RFC 6979 candidate. This signer stops with an error.
    XReduced,
}

/// Why a signature is not correct. The names follow the rejection tokens of `zikaron-v1.md` §5.4 and §5.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// `v` is not 27 or 28 (`E_SIG_V`).
    V,
    /// `r` or `s` is 0 or not less than the group order (`E_SIG_RANGE`).
    Range,
    /// `s` is more than half of the group order (`E_SIG_HIGH_S`).
    HighS,
    /// No public key comes from the signature (`E_SIG_RECOVER`).
    Recover,
    /// The signer is not the expected persona (`E_SIG_SIGNER`).
    Signer(PersonaAddress),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            KeyError::BadSecret => "the secret is not a correct secp256k1 private key",
            KeyError::NoRandom => "the system did not give random bytes",
            KeyError::XReduced => "the nonce gave an x-reduced r; sign again with a different value",
        })
    }
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::V => f.write_str("E_SIG_V"),
            VerifyError::Range => f.write_str("E_SIG_RANGE"),
            VerifyError::HighS => f.write_str("E_SIG_HIGH_S"),
            VerifyError::Recover => f.write_str("E_SIG_RECOVER"),
            VerifyError::Signer(address) => write!(f, "E_SIG_SIGNER: the signer is {address}"),
        }
    }
}

impl std::error::Error for KeyError {}
impl std::error::Error for VerifyError {}

/// The signing key of a persona.
///
/// The `Debug` form shows only the address. The private key clears its memory when it is dropped.
pub struct PersonaKey {
    key: SigningKey,
    address: PersonaAddress,
}

impl PersonaKey {
    /// Makes a new key from the random bytes of the system.
    pub fn generate() -> Result<Self, KeyError> {
        loop {
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| KeyError::NoRandom)?;
            // Of 2^256 values, about 2^128 are not correct keys. Take new bytes for these values.
            if let Ok(key) = SigningKey::from_slice(&bytes) {
                return Ok(Self::from_signing_key(key));
            }
        }
    }

    /// Reads a key from its 32-byte secret. A secret with a different length is not correct.
    pub fn from_secret(secret: &SecretBytes) -> Result<Self, KeyError> {
        // `from_slice` accepts a shorter slice and adds zeros. A stored key always has 32 bytes, so refuse it.
        if secret.expose().len() != 32 {
            return Err(KeyError::BadSecret);
        }
        let key = SigningKey::from_slice(secret.expose()).map_err(|_| KeyError::BadSecret)?;
        Ok(Self::from_signing_key(key))
    }

    /// The 32-byte secret of the key, to keep in the secret store.
    pub fn to_secret(&self) -> SecretBytes {
        SecretBytes::new(self.key.to_bytes().to_vec())
    }

    fn from_signing_key(key: SigningKey) -> Self {
        let address = address_of(key.verifying_key());
        Self { key, address }
    }

    pub fn address(&self) -> PersonaAddress {
        self.address
    }

    /// Signs a 32-byte value under a domain of PANIM.
    pub fn sign(&self, domain: Domain, value: &[u8; 32]) -> Result<PersonaSignature, KeyError> {
        sign_message(&self.key, domain.literal().as_bytes(), value)
    }
}

impl fmt::Debug for PersonaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PersonaKey({})", self.address)
    }
}

/// Finds the signer of a signature under a domain of PANIM, with the checks of `zikaron-v1.md` §5.4.
pub fn recover(domain: Domain, value: &[u8; 32], signature: &PersonaSignature) -> Result<PersonaAddress, VerifyError> {
    recover_message(domain.literal().as_bytes(), value, signature)
}

/// Makes sure that `expected` signed the value under the domain (`zikaron-v1.md` §5.5).
pub fn verify(
    domain: Domain,
    value: &[u8; 32],
    signature: &PersonaSignature,
    expected: PersonaAddress,
) -> Result<(), VerifyError> {
    let signer = recover(domain, value, signature)?;
    if signer == expected {
        Ok(())
    } else {
        Err(VerifyError::Signer(signer))
    }
}

/// The message `D || 0x0A || hex32(value)` (`zikaron-v1.md` §5.2).
fn message(literal: &[u8], value: &[u8; 32]) -> Vec<u8> {
    let mut message = literal.to_vec();
    message.push(b'\n');
    message.extend_from_slice(hex(value).as_bytes());
    message
}

/// The EIP-191 digest of the message (`zikaron-v1.md` §5.3).
fn digest(literal: &[u8], value: &[u8; 32]) -> [u8; 32] {
    let message = message(literal, value);
    let mut hasher = Keccak256::new();
    hasher.update(b"\x19Ethereum Signed Message:\n");
    hasher.update(message.len().to_string().as_bytes());
    hasher.update(&message);
    hasher.finalize().into()
}

fn sign_message(key: &SigningKey, literal: &[u8], value: &[u8; 32]) -> Result<PersonaSignature, KeyError> {
    debug_assert!(!literal.contains(&b'\n'));
    let (signature, recovery) = key.sign_prehash_recoverable(&digest(literal, value));
    if recovery.is_x_reduced() {
        return Err(KeyError::XReduced);
    }
    // Make s low. Then the parity of y changes, so v changes between 27 and 28 (zikaron-v1.md §5.7).
    let mut is_y_odd = recovery.is_y_odd();
    let signature = match signature.normalize_s() {
        normalized if normalized != signature => {
            is_y_odd = !is_y_odd;
            normalized
        }
        same => same,
    };
    let mut bytes = [0u8; 65];
    bytes[..64].copy_from_slice(&signature.to_bytes());
    bytes[64] = 27 + u8::from(is_y_odd);
    Ok(PersonaSignature(bytes))
}

fn recover_message(
    literal: &[u8],
    value: &[u8; 32],
    signature: &PersonaSignature,
) -> Result<PersonaAddress, VerifyError> {
    let bytes = &signature.0;
    let is_y_odd = match bytes[64] {
        27 => false,
        28 => true,
        _ => return Err(VerifyError::V),
    };
    let parsed = Signature::from_slice(&bytes[..64]).map_err(|_| VerifyError::Range)?;
    if parsed.normalize_s() != parsed {
        return Err(VerifyError::HighS);
    }
    let key = VerifyingKey::recover_from_prehash(&digest(literal, value), &parsed, RecoveryId::new(is_y_odd, false))
        .map_err(|_| VerifyError::Recover)?;
    Ok(address_of(&key))
}

/// The last 20 bytes of Keccak-256 of the 64-byte public key without the 0x04 prefix (`zikaron-v1.md` §5.5).
fn address_of(key: &VerifyingKey) -> PersonaAddress {
    let point = key.to_sec1_point(false);
    let hash = Keccak256::digest(&point.as_bytes()[1..]);
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);
    PersonaAddress(address)
}

/// The EIP-55 form of an address, with uppercase and lowercase letters as a checksum. Use it to show an address to
/// a person. ZIKARON keeps the lowercase form (`PersonaAddress` shows this form).
pub fn checksum_address(address: PersonaAddress) -> String {
    let lower = address.to_string();
    let hash = Keccak256::digest(&lower.as_bytes()[2..]);
    let mut text = String::from("0x");
    for (i, digit) in lower[2..].chars().enumerate() {
        let nibble = (hash[i / 2] >> if i % 2 == 0 { 4 } else { 0 }) & 0x0f;
        text.push(if nibble >= 8 { digit.to_ascii_uppercase() } else { digit });
    }
    text
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + 2 * bytes.len());
    text.push_str("0x");
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("0x")?;
    if digits.len() % 2 != 0 {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("zikaron_sign_vectors.txt");

    struct Vector {
        name: String,
        literal: Vec<u8>,
        key: SigningKey,
        value: [u8; 32],
        digest: [u8; 32],
        signature: PersonaSignature,
        signer: PersonaAddress,
    }

    fn vectors() -> Vec<Vector> {
        VECTORS
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let f: Vec<&str> = line.split(' ').collect();
                Vector {
                    name: f[0].into(),
                    literal: parse_hex(&format!("0x{}", f[1])).unwrap(),
                    key: SigningKey::from_slice(&parse_hex(f[2]).unwrap()).unwrap(),
                    value: parse_hex(f[3]).unwrap().try_into().unwrap(),
                    digest: parse_hex(f[4]).unwrap().try_into().unwrap(),
                    signature: PersonaSignature::parse(f[5]).unwrap(),
                    signer: PersonaAddress::parse(f[6]).unwrap(),
                }
            })
            .collect()
    }

    #[test]
    fn signatures_match_the_zikaron_vectors() {
        let vectors = vectors();
        assert_eq!(vectors.len(), 42);
        for v in vectors {
            assert_eq!(digest(&v.literal, &v.value), v.digest, "{}: digest", v.name);
            assert_eq!(
                sign_message(&v.key, &v.literal, &v.value),
                Ok(v.signature),
                "{}: signature",
                v.name
            );
            assert_eq!(address_of(v.key.verifying_key()), v.signer, "{}: address", v.name);
            assert_eq!(
                recover_message(&v.literal, &v.value, &v.signature),
                Ok(v.signer),
                "{}: recover",
                v.name
            );
        }
    }

    #[test]
    fn domains_are_not_zikaron_domains() {
        for domain in Domain::ALL {
            let literal = domain.literal();
            assert!(!literal.contains('\n') && literal.is_ascii(), "{literal}");
            assert!(!["zikaron/1", "zikaron/1-adoption"].contains(&literal), "{literal}");
        }
        assert_ne!(Domain::Proof.literal(), Domain::Revoke.literal());
    }

    #[test]
    fn sign_and_verify() {
        let key = PersonaKey::generate().unwrap();
        let value = [7u8; 32];
        let signature = key.sign(Domain::Proof, &value).unwrap();
        assert_eq!(verify(Domain::Proof, &value, &signature, key.address()), Ok(()));
        // The same key and value give the same signature (RFC 6979).
        assert_eq!(key.sign(Domain::Proof, &value), Ok(signature));
        // A signature under one domain is not a signature under a different domain.
        assert!(matches!(
            verify(Domain::Revoke, &value, &signature, key.address()),
            Err(VerifyError::Signer(_))
        ));
        let other = PersonaKey::generate().unwrap();
        assert_eq!(
            verify(Domain::Proof, &value, &signature, other.address()),
            Err(VerifyError::Signer(key.address()))
        );
    }

    #[test]
    fn secret_goes_both_ways() {
        let key = PersonaKey::generate().unwrap();
        let again = PersonaKey::from_secret(&key.to_secret()).unwrap();
        assert_eq!(again.address(), key.address());
        assert_eq!(
            PersonaKey::from_secret(&SecretBytes::new(vec![0; 32])).unwrap_err(),
            KeyError::BadSecret
        );
        assert_eq!(
            PersonaKey::from_secret(&SecretBytes::new(vec![1; 31])).unwrap_err(),
            KeyError::BadSecret
        );
    }

    #[test]
    fn verify_refuses_bad_signatures() {
        let key = PersonaKey::generate().unwrap();
        let value = [3u8; 32];
        let good = key.sign(Domain::Proof, &value).unwrap();

        let mut bad_v = good;
        bad_v.0[64] = 29;
        assert_eq!(recover(Domain::Proof, &value, &bad_v), Err(VerifyError::V));

        let mut zero_r = good;
        zero_r.0[..32].fill(0);
        assert_eq!(recover(Domain::Proof, &value, &zero_r), Err(VerifyError::Range));

        // The malleability twin (r, n - s, 55 - v) has a high s.
        use k256::elliptic_curve::ff::PrimeField;
        let parsed = Signature::from_slice(&good.0[..64]).unwrap();
        let (r, s) = parsed.split_scalars();
        let twin = Signature::from_scalars(r.to_repr(), (-*s).to_repr()).unwrap();
        let mut high = PersonaSignature([0; 65]);
        high.0[..64].copy_from_slice(&twin.to_bytes());
        high.0[64] = 55 - good.0[64];
        assert_eq!(recover(Domain::Proof, &value, &high), Err(VerifyError::HighS));
    }

    #[test]
    fn debug_form_does_not_show_the_key() {
        let key = PersonaKey::generate().unwrap();
        assert_eq!(format!("{key:?}"), format!("PersonaKey({})", key.address()));
    }

    #[test]
    fn eip55_checksum() {
        // Test vectors from EIP-55.
        for text in [
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ] {
            assert_eq!(checksum_address(PersonaAddress::parse(text).unwrap()), text);
        }
    }
}
