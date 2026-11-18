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
            let n = number("n")?;
            if !n.is_power_of_two() || n < 2 {
                return Err(bad("scrypt n is not a power of two"));
            }
            let scrypt_params = scrypt::Params::new(n.trailing_zeros() as u8, number("r")? as u32, number("p")? as u32)
                .map_err(|_| bad("the scrypt settings are not correct"))?;
            scrypt::scrypt(password.as_bytes(), &salt, &scrypt_params, derived.as_mut())
                .map_err(|_| bad("scrypt failed"))?;
        }
        "pbkdf2" => {
            if params.get("prf").and_then(|v| v.as_str()) != Some("hmac-sha256") {
                return Err(bad("only pbkdf2 with hmac-sha256 is supported"));
            }
            let rounds = u32::try_from(number("c")?).map_err(|_| bad("pbkdf2 c is too large"))?;
            pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, rounds, derived.as_mut());
        }
        _ => return Err(bad("the kdf is not scrypt or pbkdf2")),
    }

    let ciphertext = unhex(&file.crypto.ciphertext).ok_or_else(|| bad("the ciphertext is not hexadecimal"))?;
    let expected = unhex(&file.crypto.mac).ok_or_else(|| bad("the mac is not hexadecimal"))?;
    if mac(&derived, &ciphertext).as_slice() != expected.as_slice() {
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
