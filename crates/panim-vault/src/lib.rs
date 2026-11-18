//! The vault of PANIM: sealed local data, the passcode, the recovery with the persona key, and sealed browser
//! profiles. Refer to `docs/ARCHITECTURE.md` §3 and §5.
//!
//! - `Vault` holds the master key and writes sealed files.
//! - `SealedStore` and `SealedSecrets` are the `Store` and the `SecretStore` of the core, sealed in the vault.
//! - `ProfileArea` opens sealed browser profiles into memory.
//! - `keyfile` writes and reads the key file of a persona key.
//! - `backup` writes the vault into one archive file, and restores a vault from it.
//!
//! This crate follows the design of the ZIKARON local data standard, with its own code. Later, the family
//! components `local-seal`, `passcode-gate` and `key-file` can replace it.

pub mod backup;
mod crypto;
pub mod keyfile;
mod profile;
mod store;
mod vault;

use std::fmt;

pub use backup::{BackupInfo, restore_backup, write_backup};
pub use crypto::KdfParams;
pub use keyfile::ScryptParams;
pub use profile::ProfileArea;
pub use store::{SealedSecrets, SealedStore};
pub use vault::{MAX_WRONG_TRIES, MIN_PASSCODE_CHARS, Vault};

/// An error from the vault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// The folder has no vault.
    NotFound,
    /// The folder has a vault already.
    Exists,
    /// A different process has the vault open.
    InUse,
    /// The passcode is not correct. After `tries_left` more wrong tries, the gate locks.
    WrongPasscode {
        tries_left: u32,
    },
    /// There were `MAX_WRONG_TRIES` wrong tries. Only the persona key opens the vault now.
    GateLocked,
    /// The passcode has fewer than `MIN_PASSCODE_CHARS` characters.
    PasscodeTooShort,
    /// The persona key is not the recovery key of this vault.
    WrongRecoveryKey,
    /// The file or the header is damaged, or a file is in the place of a different file. The text names it.
    Damaged(String),
    BadKeyFile(String),
    WrongKeyFilePassword,
    /// The file is not a backup that PANIM can restore. The text gives the reason.
    BadBackup(String),
    /// The system did not give random bytes.
    NoRandom,
    Io(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::NotFound => f.write_str("the folder has no vault"),
            VaultError::Exists => f.write_str("the folder has a vault already"),
            VaultError::InUse => f.write_str("a different process has the vault open"),
            VaultError::WrongPasscode { tries_left } => {
                write!(f, "the passcode is not correct; {tries_left} tries are left")
            }
            VaultError::GateLocked => f.write_str("too many wrong tries; use the key file to open the vault"),
            VaultError::PasscodeTooShort => write!(f, "the passcode must have {MIN_PASSCODE_CHARS} characters or more"),
            VaultError::WrongRecoveryKey => f.write_str("this persona key is not the recovery key of the vault"),
            VaultError::Damaged(what) => write!(f, "damaged: {what}"),
            VaultError::BadKeyFile(reason) => write!(f, "the key file is not correct: {reason}"),
            VaultError::WrongKeyFilePassword => f.write_str("the key file password is not correct"),
            VaultError::BadBackup(reason) => write!(f, "the backup is not correct: {reason}"),
            VaultError::NoRandom => f.write_str("the system did not give random bytes"),
            VaultError::Io(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for VaultError {}
