//! The cipher, the key derivations and random bytes. The other modules use only these functions.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::VaultError;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub type Key = Zeroizing<[u8; KEY_LEN]>;

/// Random bytes from the system.
pub fn random<const N: usize>() -> Result<[u8; N], VaultError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|_| VaultError::NoRandom)?;
    Ok(bytes)
}

/// Seals bytes with XChaCha20-Poly1305 and a random nonce. The result is `nonce || ciphertext || tag`.
pub fn seal(key: &Key, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, VaultError> {
    let nonce: [u8; NONCE_LEN] = random()?;
    let cipher = XChaCha20Poly1305::new(&(**key).into());
    let sealed = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad })
        .map_err(|_| VaultError::Damaged("the cipher refused the data".into()))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Opens bytes that `seal` made. Returns `None` if the key, the AAD or the bytes are not correct.
pub fn open(key: &Key, aad: &[u8], sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let (nonce, body) = sealed.split_at_checked(NONCE_LEN)?;
    let nonce: [u8; NONCE_LEN] = nonce.try_into().ok()?;
    let cipher = XChaCha20Poly1305::new(&(**key).into());
    cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: body, aad })
        .ok()
        .map(Zeroizing::new)
}

/// HKDF-SHA256 of a key that has high entropy. Use a different `info` for each use.
pub fn derive(ikm: &[u8], salt: Option<&[u8]>, info: &[u8]) -> Key {
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    Hkdf::<Sha256>::new(salt, ikm)
        .expand(info, out.as_mut())
        .expect("32 bytes is a correct HKDF length");
    out
}

/// The settings of Argon2id, the slow key derivation for the passcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl KdfParams {
    /// The settings for real use: 64 MiB of memory and three passes.
    pub const STANDARD: KdfParams = KdfParams {
        memory_kib: 64 * 1024,
        iterations: 3,
        lanes: 1,
    };
    /// Very weak settings. Use them only in tests.
    pub const TEST: KdfParams = KdfParams {
        memory_kib: 8,
        iterations: 1,
        lanes: 1,
    };
    /// The largest settings that PANIM accepts: 4 GiB of memory, 64 passes and 16 lanes.
    ///
    /// The header of the vault is not secret, and a person or a program can change it. Without a maximum, a changed
    /// header can make Argon2id use all the memory of the computer, or run for hours. These values are much larger
    /// than `STANDARD`, so a real vault does not get near them.
    pub const MAX: KdfParams = KdfParams {
        memory_kib: 4 * 1024 * 1024,
        iterations: 64,
        lanes: 16,
    };

    /// Gives `VaultError::Damaged` if one or more settings are above `MAX`. Argon2id does not run for these settings.
    pub fn check(self) -> Result<(), VaultError> {
        let max = KdfParams::MAX;
        if self.memory_kib > max.memory_kib || self.iterations > max.iterations || self.lanes > max.lanes {
            return Err(VaultError::Damaged(format!(
                "the passcode settings are above the maximum: {self:?}"
            )));
        }
        Ok(())
    }

    /// True if one or more settings are below the settings in `other`.
    pub fn is_weaker_than(self, other: KdfParams) -> bool {
        self.memory_kib < other.memory_kib || self.iterations < other.iterations || self.lanes < other.lanes
    }

    /// The larger value of each setting. The result is not weaker than `self` or `other`.
    pub fn stronger(self, other: KdfParams) -> KdfParams {
        KdfParams {
            memory_kib: self.memory_kib.max(other.memory_kib),
            iterations: self.iterations.max(other.iterations),
            lanes: self.lanes.max(other.lanes),
        }
    }
}

/// Argon2id of the passcode. Settings above `KdfParams::MAX` give `VaultError::Damaged`, before Argon2id runs.
pub fn passcode_key(passcode: &str, salt: &[u8], params: KdfParams) -> Result<Key, VaultError> {
    params.check()?;
    let params = Params::new(params.memory_kib, params.iterations, params.lanes, Some(KEY_LEN))
        .map_err(|e| VaultError::Damaged(format!("the passcode settings are not correct: {e}")))?;
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passcode.as_bytes(), salt, out.as_mut())
        .map_err(|e| VaultError::Damaged(format!("Argon2id failed: {e}")))?;
    Ok(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}
