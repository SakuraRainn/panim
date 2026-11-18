//! The vault: one master key, two seals of it, and the sealed files.
//!
//! This follows the design of the ZIKARON local data standard (`crates/app/local-data-standard.md` in the
//! ZIKARON repository). It does not use ZIKARON code. Refer to `docs/ARCHITECTURE.md` §3.
//!
//! The vault folder contains:
//!
//! - `vault.json`: the header. It holds the two seals of the master key and the passcode settings. It holds no
//!   secret that is not sealed.
//! - `vault.lock`: the lock of the one writer.
//! - `gate.json`: the count of wrong passcode tries. It is not secret. After `MAX_WRONG_TRIES` wrong tries, the
//!   passcode does not open the vault. Then only the persona key opens it (`Vault::recover`).
//! - `data/`: the sealed files. The name of a file comes from a key, so the names do not show the places.
//!
//! The passcode opens the master key every day. The key of the persona is the recovery: it opens the master key a
//! second way, and then the person sets a new passcode.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use panim_core::{PersonaAddress, PersonaKey};
use zeroize::Zeroizing;

use crate::VaultError;
use crate::crypto::{self, KdfParams, Key};

const FORMAT: &str = "panim-vault/1";
pub(crate) const FILE_MAGIC: &[u8; 4] = b"PNL1";
pub(crate) const HEADER: &str = "vault.json";
const LOCK: &str = "vault.lock";
pub(crate) const DATA: &str = "data";
pub(crate) const GATE: &str = "gate.json";

/// After this number of wrong passcode tries, only the persona key opens the vault.
pub const MAX_WRONG_TRIES: u32 = 5;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Gate {
    wrong: u32,
}

/// A passcode must have this number of characters or more.
pub const MIN_PASSCODE_CHARS: usize = 8;

#[derive(serde::Serialize, serde::Deserialize)]
struct Header {
    format: String,
    kdf: KdfParams,
    /// The salt of Argon2id, in hexadecimal.
    salt: String,
    /// The master key, sealed with the key from the passcode, in hexadecimal.
    passcode_seal: String,
    recovery: Recovery,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Recovery {
    /// The address of the persona key that opens this seal.
    address: String,
    salt: String,
    /// The master key, sealed with a key from the persona key, in hexadecimal.
    seal: String,
}

/// An open vault. It holds the master key and the lock of the one writer.
pub struct Vault {
    dir: PathBuf,
    master: Key,
    names: Key,
    _lock: File,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault({})", self.dir.display())
    }
}

impl Vault {
    /// True if the folder has a vault.
    pub fn exists(dir: &Path) -> bool {
        dir.join(HEADER).is_file()
    }

    /// The address of the persona key that recovers the vault.
    pub fn recovery_address(dir: &Path) -> Result<PersonaAddress, VaultError> {
        let header = read_header(dir)?;
        PersonaAddress::parse(&header.recovery.address).ok_or_else(|| VaultError::Damaged(HEADER.into()))
    }

    /// Makes a new vault with a new master key. The persona key becomes the recovery key.
    pub fn create(dir: &Path, passcode: &str, recovery: &PersonaKey, kdf: KdfParams) -> Result<Vault, VaultError> {
        check_passcode(passcode)?;
        fs::create_dir_all(dir.join(DATA)).map_err(io)?;
        let lock = lock(dir)?;
        if Vault::exists(dir) {
            return Err(VaultError::Exists);
        }
        let master = Zeroizing::new(crypto::random::<32>()?);
        let header = Header {
            format: FORMAT.into(),
            kdf,
            salt: String::new(),
            passcode_seal: String::new(),
            recovery: recovery_seal(&master, recovery)?,
        };
        write_header(dir, &with_passcode(header, &master, passcode, kdf)?)?;
        Ok(Vault::from_master(dir, master, lock))
    }

    /// The number of passcode tries that are left before the gate locks. 0 means that the gate is locked.
    pub fn tries_left(dir: &Path) -> Result<u32, VaultError> {
        Ok(MAX_WRONG_TRIES.saturating_sub(read_gate(dir)?.wrong))
    }

    /// Opens the vault with the passcode. Each wrong passcode counts as a wrong try (`MAX_WRONG_TRIES`).
    pub fn open(dir: &Path, passcode: &str) -> Result<Vault, VaultError> {
        let lock = lock(dir)?;
        let master = open_with_passcode(dir, passcode)?;
        Ok(Vault::from_master(dir, master, lock))
    }

    /// Makes sure that the passcode is correct, for example before a change of the passcode.
    /// A wrong passcode counts as a wrong try.
    pub fn verify_passcode(&self, passcode: &str) -> Result<(), VaultError> {
        open_with_passcode(&self.dir, passcode).map(|_| ())
    }

    /// Opens the vault with the persona key, and sets a new passcode. Use this when the person lost the passcode.
    pub fn recover(dir: &Path, recovery: &PersonaKey, new_passcode: &str, kdf: KdfParams) -> Result<Vault, VaultError> {
        check_passcode(new_passcode)?;
        let lock = lock(dir)?;
        let header = read_header(dir)?;
        if header.recovery.address != recovery.address().to_string() {
            return Err(VaultError::WrongRecoveryKey);
        }
        let salt = crypto::unhex(&header.recovery.salt).ok_or_else(|| VaultError::Damaged(HEADER.into()))?;
        let sealed = crypto::unhex(&header.recovery.seal).ok_or_else(|| VaultError::Damaged(HEADER.into()))?;
        let key = recovery_key(recovery, &salt);
        let aad = aad_recovery(&header.recovery.address);
        let master = crypto::open(&key, aad.as_bytes(), &sealed).ok_or(VaultError::WrongRecoveryKey)?;
        let master = to_key(&master)?;
        write_header(dir, &with_passcode(header, &master, new_passcode, kdf)?)?;
        write_gate(dir, &Gate::default())?;
        Ok(Vault::from_master(dir, master, lock))
    }

    /// Sets a new passcode. The master key and the sealed files do not change.
    pub fn change_passcode(&self, new_passcode: &str, kdf: KdfParams) -> Result<(), VaultError> {
        check_passcode(new_passcode)?;
        let header = read_header(&self.dir)?;
        write_header(&self.dir, &with_passcode(header, &self.master, new_passcode, kdf)?)
    }

    /// Writes bytes to a place, sealed. The write is complete or it does not occur.
    pub fn write(&self, place: &str, plain: &[u8]) -> Result<(), VaultError> {
        let mut file = FILE_MAGIC.to_vec();
        file.extend_from_slice(&crypto::seal(&self.file_key(place), &aad_file(place), plain)?);
        write_atomic(&self.file_path(place), &file)
    }

    /// Reads the bytes of a place. Returns `None` if the place has no file.
    pub fn read(&self, place: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        let file = match fs::read(self.file_path(place)) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(e)),
        };
        let body = file
            .strip_prefix(FILE_MAGIC.as_slice())
            .ok_or_else(|| VaultError::Damaged(place.into()))?;
        let plain = crypto::open(&self.file_key(place), &aad_file(place), body)
            .ok_or_else(|| VaultError::Damaged(place.into()))?;
        Ok(Some(plain))
    }

    /// Removes the file of a place. It is not an error if the place has no file.
    pub fn remove(&self, place: &str) -> Result<(), VaultError> {
        match fs::remove_file(self.file_path(place)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn from_master(dir: &Path, master: Key, lock: File) -> Vault {
        let names = crypto::derive(master.as_ref(), None, b"panim-local/1 names");
        Vault {
            dir: dir.to_path_buf(),
            master,
            names,
            _lock: lock,
        }
    }

    /// The name of the folder of this vault in the profile area: 16 hexadecimal digits from the master key.
    /// Each vault has a different name, and the name is the same at each start. It does not show the data folder.
    pub fn area_name(&self) -> String {
        let key = crypto::derive(self.master.as_ref(), None, b"panim-local/1 area");
        crypto::hex(&key[..8])
    }

    fn file_key(&self, place: &str) -> Key {
        let mut info = b"panim-local/1 file\0".to_vec();
        info.extend_from_slice(place.as_bytes());
        crypto::derive(self.master.as_ref(), None, &info)
    }

    /// The name of the file of a place: 32 hexadecimal digits from a key. It does not show the place.
    fn file_path(&self, place: &str) -> PathBuf {
        let mut info = b"panim-local/1 name\0".to_vec();
        info.extend_from_slice(place.as_bytes());
        let name = crypto::derive(self.names.as_ref(), None, &info);
        self.dir.join(DATA).join(crypto::hex(&name[..16]))
    }
}

/// Opens the master key with the passcode, and keeps the count of wrong tries.
fn open_with_passcode(dir: &Path, passcode: &str) -> Result<Key, VaultError> {
    let mut gate = read_gate(dir)?;
    if gate.wrong >= MAX_WRONG_TRIES {
        return Err(VaultError::GateLocked);
    }
    let header = read_header(dir)?;
    let salt = crypto::unhex(&header.salt).ok_or_else(|| VaultError::Damaged(HEADER.into()))?;
    let key = crypto::passcode_key(passcode, &salt, header.kdf)?;
    let sealed = crypto::unhex(&header.passcode_seal).ok_or_else(|| VaultError::Damaged(HEADER.into()))?;
    match crypto::open(&key, aad_passcode().as_bytes(), &sealed) {
        Some(master) => {
            if gate.wrong > 0 {
                write_gate(dir, &Gate::default())?;
            }
            to_key(&master)
        }
        None => {
            gate.wrong += 1;
            write_gate(dir, &gate)?;
            match MAX_WRONG_TRIES.saturating_sub(gate.wrong) {
                0 => Err(VaultError::GateLocked),
                tries_left => Err(VaultError::WrongPasscode { tries_left }),
            }
        }
    }
}

fn read_gate(dir: &Path) -> Result<Gate, VaultError> {
    match fs::read_to_string(dir.join(GATE)) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| VaultError::Damaged(GATE.into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Gate::default()),
        Err(e) => Err(io(e)),
    }
}

fn write_gate(dir: &Path, gate: &Gate) -> Result<(), VaultError> {
    write_atomic(
        &dir.join(GATE),
        serde_json::to_string(gate).expect("plain data").as_bytes(),
    )
}

fn check_passcode(passcode: &str) -> Result<(), VaultError> {
    if passcode.chars().count() < MIN_PASSCODE_CHARS {
        return Err(VaultError::PasscodeTooShort);
    }
    Ok(())
}

fn aad_passcode() -> String {
    format!("{FORMAT} passcode")
}

fn aad_recovery(address: &str) -> String {
    format!("{FORMAT} recovery {address}")
}

fn aad_file(place: &str) -> Vec<u8> {
    let mut aad = FILE_MAGIC.to_vec();
    aad.extend_from_slice(place.as_bytes());
    aad
}

fn recovery_key(recovery: &PersonaKey, salt: &[u8]) -> Key {
    let secret = recovery.to_secret();
    crypto::derive(secret.expose(), Some(salt), format!("{FORMAT} recovery").as_bytes())
}

fn recovery_seal(master: &Key, recovery: &PersonaKey) -> Result<Recovery, VaultError> {
    let salt: [u8; 32] = crypto::random()?;
    let address = recovery.address().to_string();
    let seal = crypto::seal(
        &recovery_key(recovery, &salt),
        aad_recovery(&address).as_bytes(),
        master.as_ref(),
    )?;
    Ok(Recovery {
        address,
        salt: crypto::hex(&salt),
        seal: crypto::hex(&seal),
    })
}

fn with_passcode(mut header: Header, master: &Key, passcode: &str, kdf: KdfParams) -> Result<Header, VaultError> {
    let salt: [u8; 16] = crypto::random()?;
    let key = crypto::passcode_key(passcode, &salt, kdf)?;
    header.kdf = kdf;
    header.salt = crypto::hex(&salt);
    header.passcode_seal = crypto::hex(&crypto::seal(&key, aad_passcode().as_bytes(), master.as_ref())?);
    Ok(header)
}

fn to_key(bytes: &[u8]) -> Result<Key, VaultError> {
    let array: [u8; 32] = bytes.try_into().map_err(|_| VaultError::Damaged(HEADER.into()))?;
    Ok(Zeroizing::new(array))
}

fn read_header(dir: &Path) -> Result<Header, VaultError> {
    let text = match fs::read_to_string(dir.join(HEADER)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(VaultError::NotFound),
        Err(e) => return Err(io(e)),
    };
    let header: Header = serde_json::from_str(&text).map_err(|_| VaultError::Damaged(HEADER.into()))?;
    if header.format != FORMAT {
        return Err(VaultError::Damaged(format!(
            "{HEADER}: unknown format {:?}",
            header.format
        )));
    }
    Ok(header)
}

fn write_header(dir: &Path, header: &Header) -> Result<(), VaultError> {
    let text = serde_json::to_string_pretty(header).expect("the header is plain data");
    write_atomic(&dir.join(HEADER), text.as_bytes())
}

/// Takes the lock of the one writer. Another process that has the vault open keeps the lock.
pub(crate) fn lock(dir: &Path) -> Result<File, VaultError> {
    if !dir.is_dir() {
        return Err(VaultError::NotFound);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK))
        .map_err(io)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(VaultError::InUse),
        Err(std::fs::TryLockError::Error(e)) => Err(io(e)),
    }
}

/// Writes a file completely or not at all: a temporary file, `fsync`, a rename, then `fsync` of the folder.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let dir = path
        .parent()
        .ok_or_else(|| VaultError::Io("the path has no folder".into()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = dir.join(format!(".{name}.{}.tmp", crypto::hex(&crypto::random::<8>()?)));
    let result = (|| {
        let mut file = File::create_new(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(io)
}

pub(crate) fn io(error: std::io::Error) -> VaultError {
    VaultError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PersonaKey {
        PersonaKey::generate().unwrap()
    }

    #[test]
    fn create_open_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let persona = key();
        let vault = Vault::create(dir.path(), "passcode1", &persona, KdfParams::TEST).unwrap();
        vault.write("store/meta", b"hello").unwrap();
        drop(vault);

        let vault = Vault::open(dir.path(), "passcode1").unwrap();
        assert_eq!(vault.read("store/meta").unwrap().unwrap().as_slice(), b"hello");
        assert_eq!(vault.read("store/other").unwrap(), None);
        assert_eq!(Vault::recovery_address(dir.path()).unwrap(), persona.address());
    }

    #[test]
    fn standard_settings_work() {
        let dir = tempfile::tempdir().unwrap();
        drop(Vault::create(dir.path(), "passcode1", &key(), KdfParams::STANDARD).unwrap());
        assert!(Vault::open(dir.path(), "passcode1").is_ok());
    }

    #[test]
    fn wrong_passcode_and_short_passcode() {
        let dir = tempfile::tempdir().unwrap();
        let persona = key();
        assert_eq!(
            Vault::create(dir.path(), "short", &persona, KdfParams::TEST).unwrap_err(),
            VaultError::PasscodeTooShort
        );
        drop(Vault::create(dir.path(), "passcode1", &persona, KdfParams::TEST).unwrap());
        assert_eq!(
            Vault::open(dir.path(), "passcode2").unwrap_err(),
            VaultError::WrongPasscode { tries_left: 4 }
        );
        assert_eq!(
            Vault::create(dir.path(), "passcode1", &persona, KdfParams::TEST).unwrap_err(),
            VaultError::Exists
        );
    }

    #[test]
    fn gate_locks_after_five_wrong_tries_and_the_key_opens_it() {
        let dir = tempfile::tempdir().unwrap();
        let persona = key();
        drop(Vault::create(dir.path(), "passcode1", &persona, KdfParams::TEST).unwrap());
        assert_eq!(Vault::tries_left(dir.path()).unwrap(), 5);
        for left in (1..5).rev() {
            assert_eq!(
                Vault::open(dir.path(), "wrong-one").unwrap_err(),
                VaultError::WrongPasscode { tries_left: left }
            );
        }
        // A correct passcode resets the count.
        drop(Vault::open(dir.path(), "passcode1").unwrap());
        assert_eq!(Vault::tries_left(dir.path()).unwrap(), 5);
        for _ in 0..4 {
            let _ = Vault::open(dir.path(), "wrong-one");
        }
        assert_eq!(
            Vault::open(dir.path(), "wrong-one").unwrap_err(),
            VaultError::GateLocked
        );
        // The correct passcode does not open a locked gate.
        assert_eq!(
            Vault::open(dir.path(), "passcode1").unwrap_err(),
            VaultError::GateLocked
        );
        assert_eq!(Vault::tries_left(dir.path()).unwrap(), 0);
        drop(Vault::recover(dir.path(), &persona, "passcode2", KdfParams::TEST).unwrap());
        assert_eq!(Vault::tries_left(dir.path()).unwrap(), 5);
        assert!(Vault::open(dir.path(), "passcode2").is_ok());
    }

    #[test]
    fn verify_passcode_counts_wrong_tries() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), "passcode1", &key(), KdfParams::TEST).unwrap();
        assert!(vault.verify_passcode("passcode1").is_ok());
        assert_eq!(
            vault.verify_passcode("nope-nope").unwrap_err(),
            VaultError::WrongPasscode { tries_left: 4 }
        );
        assert_eq!(Vault::tries_left(dir.path()).unwrap(), 4);
    }

    #[test]
    fn one_writer_only() {
        let dir = tempfile::tempdir().unwrap();
        let _vault = Vault::create(dir.path(), "passcode1", &key(), KdfParams::TEST).unwrap();
        assert_eq!(Vault::open(dir.path(), "passcode1").unwrap_err(), VaultError::InUse);
    }

    #[test]
    fn recovery_with_the_persona_key_sets_a_new_passcode() {
        let dir = tempfile::tempdir().unwrap();
        let persona = key();
        let vault = Vault::create(dir.path(), "passcode1", &persona, KdfParams::TEST).unwrap();
        vault.write("secrets", b"token").unwrap();
        drop(vault);

        assert_eq!(
            Vault::recover(dir.path(), &key(), "passcode2", KdfParams::TEST).unwrap_err(),
            VaultError::WrongRecoveryKey
        );
        let vault = Vault::recover(dir.path(), &persona, "passcode2", KdfParams::TEST).unwrap();
        assert_eq!(vault.read("secrets").unwrap().unwrap().as_slice(), b"token");
        drop(vault);
        assert_eq!(
            Vault::open(dir.path(), "passcode1").unwrap_err(),
            VaultError::WrongPasscode { tries_left: 4 }
        );
        assert!(Vault::open(dir.path(), "passcode2").is_ok());
    }

    #[test]
    fn change_passcode_keeps_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), "passcode1", &key(), KdfParams::TEST).unwrap();
        vault.write("a", b"1").unwrap();
        vault.change_passcode("passcode2", KdfParams::TEST).unwrap();
        drop(vault);
        let vault = Vault::open(dir.path(), "passcode2").unwrap();
        assert_eq!(vault.read("a").unwrap().unwrap().as_slice(), b"1");
    }

    #[test]
    fn a_file_cannot_take_the_place_of_a_different_file() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), "passcode1", &key(), KdfParams::TEST).unwrap();
        vault.write("a", b"1").unwrap();
        vault.write("b", b"2").unwrap();
        fs::copy(vault.file_path("a"), vault.file_path("b")).unwrap();
        assert_eq!(vault.read("b").unwrap_err(), VaultError::Damaged("b".into()));
    }

    #[test]
    fn file_names_and_header_do_not_show_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path(), "passcode1", &key(), KdfParams::TEST).unwrap();
        vault.write("store/items/account-1", b"secret text").unwrap();
        for entry in fs::read_dir(dir.path().join(DATA)).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_name().to_string_lossy().contains("account"));
            assert!(!fs::read(entry.path()).unwrap().windows(11).any(|w| w == b"secret text"));
        }
        let header = fs::read_to_string(dir.path().join(HEADER)).unwrap();
        assert!(!header.contains("passcode1"));
    }
}
