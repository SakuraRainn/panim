//! The key file of a persona key: the Web3 Secret Storage format, version 3 (a "V3 keystore").
//!
//! The person keeps the key file outside PANIM. With its password, it gives back the persona key. The persona
//! key then recovers the vault (`Vault::recover`). Other wallets and ZIKARON can also read this format.
//!
//! - Export uses scrypt and AES-128-CTR.
//! - Import accepts scrypt and PBKDF2-HMAC-SHA256.
//! - The MAC is Keccak-256 of the second half of the derived key and the ciphertext.

use aes::Aes128;
use ctr::Ctr128BE;
use ctr::cipher::{KeyIvInit, StreamCipher};
use panim_core::{PersonaKey, SecretBytes};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::VaultError;
use crate::crypto::{self, hex, unhex};

/// The settings of scrypt for a new key file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScryptParams {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
}

/// The largest scrypt `log_n` that import accepts: n = 2^20.
const MAX_SCRYPT_LOG_N: u32 = 20;
/// The largest scrypt `r` that import accepts.
const MAX_SCRYPT_R: u32 = 16;
/// The largest scrypt `p` that import accepts.
const MAX_SCRYPT_P: u32 = 16;
/// The largest pbkdf2 `c` (the number of rounds) that import accepts.
const MAX_PBKDF2_C: u32 = 10_000_000;

impl ScryptParams {
    /// The usual settings of wallets: n = 2^18, r = 8, p = 1. This uses 256 MiB of memory.
    pub const STANDARD: ScryptParams = ScryptParams { log_n: 18, r: 8, p: 1 };
    /// Very weak settings. Use them only in tests.
    pub const TEST: ScryptParams = ScryptParams { log_n: 4, r: 8, p: 1 };
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    address: String,
    crypto: Crypto,
    id: String,
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct Crypto {
    cipher: String,
    cipherparams: CipherParams,
    ciphertext: String,
    kdf: String,
    kdfparams: serde_json::Value,
    mac: String,
}

#[derive(Serialize, Deserialize)]
struct CipherParams {
    iv: String,
}

/// Writes the key file of a persona key, as JSON text.
pub fn export(key: &PersonaKey, password: &str, params: ScryptParams) -> Result<String, VaultError> {
    let salt: [u8; 32] = crypto::random()?;
    let iv: [u8; 16] = crypto::random()?;
    let scrypt_params = scrypt::Params::new(params.log_n, params.r, params.p)
        .map_err(|_| VaultError::BadKeyFile("the scrypt settings are not correct".into()))?;
    let mut derived = Zeroizing::new([0u8; 32]);
    scrypt::scrypt(password.as_bytes(), &salt, &scrypt_params, derived.as_mut())
        .map_err(|_| VaultError::BadKeyFile("scrypt failed".into()))?;

    let secret = key.to_secret();
    let mut ciphertext = secret.expose().to_vec();
    Ctr128BE::<Aes128>::new(&derived[..16].try_into().unwrap(), &iv.into()).apply_keystream(&mut ciphertext);
    let file = KeyFile {
        address: key.address().to_string()[2..].to_string(),
        crypto: Crypto {
            cipher: "aes-128-ctr".into(),
            cipherparams: CipherParams { iv: hex(&iv) },
            ciphertext: hex(&ciphertext),
            kdf: "scrypt".into(),
            kdfparams: serde_json::json!({
                "dklen": 32,
                "n": 1u64 << params.log_n,
                "p": params.p,
                "r": params.r,
                "salt": hex(&salt),
            }),
            mac: hex(&mac(&derived, &ciphertext)),
        },
        id: uuid_v4()?,
        version: 3,
    };
    Ok(serde_json::to_string_pretty(&file).expect("the key file is plain data"))
}

/// Reads a key file with its password and gives back the persona key.
pub fn import(text: &str, password: &str) -> Result<PersonaKey, VaultError> {
    let bad = |reason: &str| VaultError::BadKeyFile(reason.into());
    let file: KeyFile = serde_json::from_str(text).map_err(|_| bad("the file is not a V3 key file"))?;
    if file.version != 3 || file.crypto.cipher != "aes-128-ctr" {
        return Err(bad("only version 3 with aes-128-ctr is supported"));
    }
    let params = &file.crypto.kdfparams;
    let number = |name: &str| {
        params
            .get(name)
            .and_then(|v| v.as_u64())
            .ok_or_else(|| bad("a kdf setting is missing"))
    };
    let salt = params
        .get("salt")
        .and_then(|v| v.as_str())
        .and_then(unhex)
        .ok_or_else(|| bad("the salt is missing"))?;
    if number("dklen")? != 32 {
        return Err(bad("only dklen 32 is supported"));
    }

    let mut derived = Zeroizing::new([0u8; 32]);
    match file.crypto.kdf.as_str() {
        "scrypt" => {
            // Examine the settings before scrypt runs. A bad file can ask for too much memory or time.
            let n = number("n")?;
            if !n.is_power_of_two() || n < 2 {
                return Err(bad("scrypt n is not a power of two"));
            }
            let log_n = n.trailing_zeros();
            let r = u32::try_from(number("r")?).map_err(|_| bad("scrypt r is too large"))?;
            let p = u32::try_from(number("p")?).map_err(|_| bad("scrypt p is too large"))?;
            if log_n > MAX_SCRYPT_LOG_N || r > MAX_SCRYPT_R || p > MAX_SCRYPT_P {
                return Err(bad("the scrypt settings are above the limits"));
            }
            let log_n = u8::try_from(log_n).map_err(|_| bad("scrypt n is too large"))?;
            let scrypt_params =
                scrypt::Params::new(log_n, r, p).map_err(|_| bad("the scrypt settings are not correct"))?;
            scrypt::scrypt(password.as_bytes(), &salt, &scrypt_params, derived.as_mut())
                .map_err(|_| bad("scrypt failed"))?;
        }
        "pbkdf2" => {
            if params.get("prf").and_then(|v| v.as_str()) != Some("hmac-sha256") {
                return Err(bad("only pbkdf2 with hmac-sha256 is supported"));
            }
            let rounds = u32::try_from(number("c")?).map_err(|_| bad("pbkdf2 c is too large"))?;
            if rounds > MAX_PBKDF2_C {
                return Err(bad("pbkdf2 c is above the limit"));
            }
            pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, rounds, derived.as_mut());
        }
        _ => return Err(bad("the kdf is not scrypt or pbkdf2")),
    }

    let ciphertext = unhex(&file.crypto.ciphertext).ok_or_else(|| bad("the ciphertext is not hexadecimal"))?;
    let expected = unhex(&file.crypto.mac).ok_or_else(|| bad("the mac is not hexadecimal"))?;
    if !equal_in_constant_time(&mac(&derived, &ciphertext), &expected) {
        return Err(VaultError::WrongKeyFilePassword);
    }
    let iv: [u8; 16] = unhex(&file.crypto.cipherparams.iv)
        .and_then(|iv| iv.try_into().ok())
        .ok_or_else(|| bad("the iv is not 16 bytes"))?;
    let mut plain = Zeroizing::new(ciphertext);
    Ctr128BE::<Aes128>::new(&derived[..16].try_into().unwrap(), &iv.into()).apply_keystream(&mut plain);
    let key = PersonaKey::from_secret(&SecretBytes::new(plain.to_vec())).map_err(|_| bad("the key is not correct"))?;
    if !file.address.is_empty() && file.address.to_ascii_lowercase() != key.address().to_string()[2..] {
        return Err(bad("the address in the file is not the address of the key"));
    }
    Ok(key)
}

fn mac(derived: &[u8; 32], ciphertext: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(&derived[16..]);
    hasher.update(ciphertext);
    hasher.finalize().into()
}

/// Compares two byte strings. The time does not change with the position of the first different byte.
///
/// The length is not secret, so a different length gives `false` immediately.
fn equal_in_constant_time(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let difference = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(difference) == 0
}

fn uuid_v4() -> Result<String, VaultError> {
    let mut b: [u8; 16] = crypto::random()?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex(&b);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The test vectors of the Web3 Secret Storage Definition. The password is "testpassword".
    //
    // - PBKDF2: from ethereum.org. Its derived key and MAC agree with the intermediates on that page.
    // - Scrypt: from the original Ethereum wiki (n = 2^18, r = 1, p = 8). The scrypt vector on ethereum.org
    //   (2026-11-13) is not correct: its derived key does not come from its salt and settings. This test uses the
    //   wiki vector. The scrypt crate also agrees with RFC 7914 (test `scrypt_matches_rfc_7914`).
    const PBKDF2_VECTOR: &str = r#"{"crypto":{"cipher":"aes-128-ctr","cipherparams":{"iv":"6087dab2f9fdbbfaddc31a909735c1e6"},"ciphertext":"5318b4d5bcd28de64ee5559e671353e16f075ecae9f99c7a79a38af5f869aa46","kdf":"pbkdf2","kdfparams":{"c":262144,"dklen":32,"prf":"hmac-sha256","salt":"ae3cd4e7013836a3df6bd7241b12db061dbe2c6785853cce422d148a624ce0bd"},"mac":"517ead924a9d0dc3124507e3393d175ce3ff7c1e96529c6c555ce9e51205e9b2"},"id":"3198bc9c-6672-5ab3-d995-4942343ae5b6","version":3,"address":""}"#;
    const SCRYPT_VECTOR: &str = r#"{"crypto":{"cipher":"aes-128-ctr","cipherparams":{"iv":"83dbcc02d8ccb40e466191a123791e0e"},"ciphertext":"d172bf743a674da9cdad04534d56926ef8358534d458fffccd4e6ad2fbde479c","kdf":"scrypt","kdfparams":{"dklen":32,"n":262144,"r":1,"p":8,"salt":"ab0c7876052600dd703518d6fc3fe8984592145b591fc8fb5c6d43190334ba19"},"mac":"2103ac29920d71da29f15d75b4a16dbe95cfd7ff8faea1056c33131d846e3097"},"id":"3198bc9c-6672-5ab3-d995-4942343ae5b6","version":3,"address":""}"#;
    const VECTOR_KEY: &str = "7a28b5ba57c53603b0b07b56bba752f7784bf506fa95edc395f5cf6c7514fe9d";

    #[test]
    fn web3_secret_storage_vectors() {
        for vector in [PBKDF2_VECTOR, SCRYPT_VECTOR] {
            let key = import(vector, "testpassword").unwrap();
            assert_eq!(hex(key.to_secret().expose()), VECTOR_KEY);
        }
        assert_eq!(
            import(PBKDF2_VECTOR, "wrong").unwrap_err(),
            VaultError::WrongKeyFilePassword
        );
    }

    #[test]
    fn scrypt_matches_rfc_7914() {
        // RFC 7914 §12: P = "password", S = "NaCl", N = 1024, r = 8, p = 16.
        let mut out = [0u8; 64];
        scrypt::scrypt(b"password", b"NaCl", &scrypt::Params::new(10, 8, 16).unwrap(), &mut out).unwrap();
        assert_eq!(
            hex(&out),
            "fdbabe1c9d3472007856e7190d01e9fe7c6ad7cbc8237830e77376634b3731622eaf30d92e22a3886ff109279d9830dac727afb94a83ee6d8360cbdfa2cc0640"
        );
    }

    /// Replaces one kdf setting in a key file. The value is JSON text.
    fn with_setting(text: &str, name: &str, value: &str) -> String {
        let mut file: serde_json::Value = serde_json::from_str(text).unwrap();
        file["crypto"]["kdfparams"][name] = serde_json::from_str(value).unwrap();
        file.to_string()
    }

    fn assert_refused_quickly(text: &str) {
        let start = std::time::Instant::now();
        assert!(matches!(import(text, "testpassword"), Err(VaultError::BadKeyFile(_))));
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn settings_above_the_limits_are_refused() {
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "n", "2097152")); // 2^21
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "n", "9223372036854775808")); // 2^63
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "r", "17"));
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "p", "17"));
        assert_refused_quickly(&with_setting(PBKDF2_VECTOR, "c", "10000001"));
    }

    #[test]
    fn values_above_u32_are_refused() {
        // Before this repair, `as u32` changed 2^32 + 1 to 1.
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "r", "4294967297"));
        assert_refused_quickly(&with_setting(SCRYPT_VECTOR, "p", "4294967297"));
        assert_refused_quickly(&with_setting(PBKDF2_VECTOR, "c", "4294967297"));
    }

    #[test]
    fn the_standard_settings_are_in_the_limits() {
        let standard = ScryptParams::STANDARD;
        assert!(u32::from(standard.log_n) <= MAX_SCRYPT_LOG_N);
        assert!(standard.r <= MAX_SCRYPT_R);
        assert!(standard.p <= MAX_SCRYPT_P);
    }

    #[test]
    fn a_wrong_mac_is_refused() {
        let key = PersonaKey::generate().unwrap();
        let text = export(&key, "pw", ScryptParams::TEST).unwrap();
        let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mac = file["crypto"]["mac"].as_str().unwrap().to_string();
        let first = if mac.starts_with('0') { "1" } else { "0" };
        file["crypto"]["mac"] = format!("{first}{}", &mac[1..]).into();
        assert_eq!(
            import(&file.to_string(), "pw").unwrap_err(),
            VaultError::WrongKeyFilePassword
        );
        file["crypto"]["mac"] = mac[..62].into();
        assert_eq!(
            import(&file.to_string(), "pw").unwrap_err(),
            VaultError::WrongKeyFilePassword
        );
    }

    #[test]
    fn constant_time_compare() {
        assert!(equal_in_constant_time(&[1, 2, 3], &[1, 2, 3]));
        assert!(!equal_in_constant_time(&[1, 2, 3], &[1, 2, 4]));
        assert!(!equal_in_constant_time(&[1, 2, 3], &[1, 2]));
    }

    #[test]
    fn export_then_import() {
        let key = PersonaKey::generate().unwrap();
        let text = export(&key, "key file password", ScryptParams::TEST).unwrap();
        assert!(!text.contains(&hex(key.to_secret().expose())));
        assert_eq!(import(&text, "key file password").unwrap().address(), key.address());
        assert_eq!(import(&text, "other").unwrap_err(), VaultError::WrongKeyFilePassword);
    }

    #[test]
    fn address_in_the_file_must_match() {
        let key = PersonaKey::generate().unwrap();
        let text = export(&key, "pw", ScryptParams::TEST).unwrap();
        let other = PersonaKey::generate().unwrap().address().to_string();
        let changed = text.replace(&key.address().to_string()[2..], &other[2..]);
        assert!(matches!(import(&changed, "pw"), Err(VaultError::BadKeyFile(_))));
    }
}
