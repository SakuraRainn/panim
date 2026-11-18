//! The 12 recovery words of a persona, and the persona key that comes from them.
//!
//! This follows the method of ZIKARON (`crates/app/src/cryptx.rs` and `family.rs` in the ZIKARON repository), with
//! its own code:
//!
//! - The words are BIP-39 (English list): 16 random bytes give 12 words.
//! - The seed is `PBKDF2-HMAC-SHA512(words, "mnemonic", 2048)`. There is no passphrase.
//! - The key is BIP-32 at `m/44'/60'/project'/group/index`. The family derivation table gives each product its own
//!   project. ZIKARON is project 0. PANIM is project 1 (`PERSONA_PATH`).
//!
//! Each persona has its own 12 words. Thus, the words of one persona do not show or link a different persona.

use hmac::{Hmac, KeyInit, Mac};
use k256::ecdsa::SigningKey;
use k256::elliptic_curve::ff::PrimeField;
use k256::{NonZeroScalar, Scalar};
use sha2::Sha512;
use zeroize::{Zeroize, Zeroizing};

use crate::key::{KeyError, PersonaKey};
use crate::secret::SecretBytes;

const HARDENED: u32 = 0x8000_0000;

/// The project of PANIM in the family derivation table.
pub const PROJECT: u32 = 1;

/// The path of the persona key: `m/44'/60'/1'/0/0`. Each persona has its own words, so the group and the index are 0.
pub const PERSONA_PATH: [u32; 5] = [44 | HARDENED, 60 | HARDENED, PROJECT | HARDENED, 0, 0];

/// The number of recovery words.
pub const WORD_COUNT: usize = 12;

/// The 12 recovery words of a persona, kept as their 16 random bytes. The bytes are set to zero when dropped.
#[derive(Clone)]
pub struct RecoveryWords(Zeroizing<[u8; 16]>);

impl RecoveryWords {
    /// Makes new words from the random bytes of the system.
    pub fn generate() -> Result<Self, KeyError> {
        let mut entropy = Zeroizing::new([0u8; 16]);
        getrandom::fill(entropy.as_mut()).map_err(|_| KeyError::NoRandom)?;
        Ok(Self(entropy))
    }

    /// Reads 12 words. Spaces and uppercase letters are not important. The BIP-39 checksum must be correct.
    pub fn parse(text: &str) -> Option<Self> {
        let normal = text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ");
        let mnemonic = bip39::Mnemonic::parse_normalized(&normal).ok()?;
        let entropy: [u8; 16] = mnemonic.to_entropy().try_into().ok()?;
        Some(Self(Zeroizing::new(entropy)))
    }

    /// Reads the words from their 16 bytes, as a secret store keeps them.
    pub fn from_secret(secret: &SecretBytes) -> Option<Self> {
        Some(Self(Zeroizing::new(secret.expose().try_into().ok()?)))
    }

    pub fn to_secret(&self) -> SecretBytes {
        SecretBytes::new(self.0.to_vec())
    }

    /// The 12 words, in sequence.
    pub fn words(&self) -> Vec<String> {
        let mnemonic = bip39::Mnemonic::from_entropy(self.0.as_ref()).expect("16 bytes give 12 words");
        mnemonic.words().map(str::to_string).collect()
    }

    /// The persona key of these words, at `PERSONA_PATH`.
    pub fn persona_key(&self) -> Result<PersonaKey, KeyError> {
        self.key_at(&PERSONA_PATH)
    }

    /// The key at a different path. Use it to compare with other tools.
    pub fn key_at(&self, path: &[u32]) -> Result<PersonaKey, KeyError> {
        let mnemonic = bip39::Mnemonic::from_entropy(self.0.as_ref()).expect("16 bytes give 12 words");
        let seed = Zeroizing::new(mnemonic.to_seed(""));
        let secret = derive(seed.as_ref(), path)?;
        PersonaKey::from_secret(&SecretBytes::new(secret.to_vec()))
    }
}

impl std::fmt::Debug for RecoveryWords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryWords(..)")
    }
}

/// BIP-32 derivation of a private key from a seed, along a path.
fn derive(seed: &[u8], path: &[u32]) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    let (mut key, mut chain) = split(hmac(b"Bitcoin seed", &[seed]));
    to_scalar(&key)?;
    for &index in path {
        let mut data = Zeroizing::new(Vec::with_capacity(37));
        if index & HARDENED != 0 {
            data.push(0);
            data.extend_from_slice(key.as_ref());
        } else {
            let signing = SigningKey::from_slice(key.as_ref()).map_err(|_| KeyError::BadSecret)?;
            data.extend_from_slice(signing.verifying_key().to_sec1_point(true).as_bytes());
        }
        data.extend_from_slice(&index.to_be_bytes());
        let (tweak, next_chain) = split(hmac(chain.as_ref(), &[&data]));
        // The chance that the tweak is not a correct scalar is less than 2^-127. BIP-32 then takes the next index.
        // This code stops with an error, as the key code does for an x-reduced nonce.
        let sum = to_scalar(&tweak)? + to_scalar(&key)?;
        let sum = Option::<NonZeroScalar>::from(NonZeroScalar::new(sum)).ok_or(KeyError::BadSecret)?;
        key.copy_from_slice(&sum.to_repr());
        chain = next_chain;
    }
    Ok(key)
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> Zeroizing<[u8; 64]> {
    let mut mac = <Hmac<Sha512> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for part in parts {
        mac.update(part);
    }
    let mut out = Zeroizing::new([0u8; 64]);
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn split(mut both: Zeroizing<[u8; 64]>) -> (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>) {
    let mut left = Zeroizing::new([0u8; 32]);
    let mut right = Zeroizing::new([0u8; 32]);
    left.copy_from_slice(&both[..32]);
    right.copy_from_slice(&both[32..]);
    both.zeroize();
    (left, right)
}

fn to_scalar(bytes: &[u8; 32]) -> Result<Scalar, KeyError> {
    let scalar = Option::<Scalar>::from(Scalar::from_repr((*bytes).into())).ok_or(KeyError::BadSecret)?;
    if bool::from(scalar.is_zero()) {
        return Err(KeyError::BadSecret);
    }
    Ok(scalar)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hardhat_test_words_give_the_well_known_address() {
        // "test test ... junk" is the default phrase of Hardhat and Foundry. Its address at m/44'/60'/0'/0/0 is
        // published by these tools.
        let words = RecoveryWords::parse("test test test test test test test test test test test junk").unwrap();
        let key = words.key_at(&[44 | HARDENED, 60 | HARDENED, HARDENED, 0, 0]).unwrap();
        assert_eq!(
            crate::key::checksum_address(key.address()),
            "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
        );
    }

    #[test]
    fn bip32_test_vector_1() {
        // BIP-32 test vector 1: seed 000102...0f, chain m/0'/1/2'/2/1000000000.
        let seed: Vec<u8> = (0u8..16).collect();
        let path = [HARDENED, 1, 2 | HARDENED, 2, 1_000_000_000];
        let key = derive(&seed, &path).unwrap();
        assert_eq!(
            hex(key.as_ref()),
            "471b76e389e528d6de6d816857e012c5455051cad6660850e58372a6c3e6e7c8"
        );
        assert_eq!(
            hex(derive(&seed, &[]).unwrap().as_ref()),
            "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35"
        );
    }

    #[test]
    fn bip39_vector_and_round_trip() {
        // BIP-39: 16 zero bytes give "abandon" eleven times and "about".
        let words = RecoveryWords::from_secret(&SecretBytes::new(vec![0; 16])).unwrap();
        let text = words.words().join(" ");
        assert_eq!(
            text,
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        );
        let again = RecoveryWords::parse(
            "  ABANDON abandon abandon abandon abandon abandon\nabandon abandon abandon abandon abandon about ",
        )
        .unwrap();
        assert_eq!(again.words().join(" "), text);
        // A wrong last word breaks the checksum.
        assert!(RecoveryWords::parse(&text.replace("about", "abandon")).is_none());
        assert!(RecoveryWords::parse("abandon abandon").is_none());
    }

    #[test]
    fn persona_key_is_at_project_1() {
        let words = RecoveryWords::generate().unwrap();
        assert_eq!(words.words().len(), WORD_COUNT);
        let persona = words.persona_key().unwrap();
        let zikaron = words.key_at(&[44 | HARDENED, 60 | HARDENED, HARDENED, 0, 0]).unwrap();
        assert_ne!(persona.address(), zikaron.address());
        assert_eq!(words.key_at(&PERSONA_PATH).unwrap().address(), persona.address());
        let stored = RecoveryWords::from_secret(&words.to_secret()).unwrap();
        assert_eq!(stored.persona_key().unwrap().address(), persona.address());
    }
}
