//! Sealed browser profiles.
//!
//! WebKit writes plain files into a profile folder while it runs. Thus, the vault keeps each profile as one sealed
//! archive, and the app opens the profile into memory:
//!
//! - `open` unseals the archive into the profile area. On Linux, the area is in `$XDG_RUNTIME_DIR`, which is in
//!   memory and not on the disk.
//! - `seal` writes the folder to the vault again. The app calls it every few minutes, so a crash loses only a few
//!   minutes of browser state.
//! - `seal_changed` does the same, but only when a file changed after the last seal. The periodic seal uses it.
//! - `close` seals the folder and removes it from the area. Close the web views of the profile before `close`.
//!
//! Each vault has its own folder in the area (`Vault::area_name`). Thus, two copies of the app with different data
//! folders can run at the same time, and their open profiles stay apart.
//!
//! The HTTP cache (`WebKitCache`) is not sealed. The browser makes it again.
//!
//! WebKit keeps the local storage, IndexedDB and the service workers in SQLite databases, most in WAL mode. A copy
//! of a database file and of its `-wal` file at different times can give a damaged database. Thus, the seal makes
//! a copy of each SQLite database with the backup interface of SQLite (`sqlite_copy`). The archive holds the copy
//! under the name of the database, and it does not hold the `-wal`, `-shm` or `-journal` file of that database. The
//! format of the archive does not change.
//!
//! On Linux, the seal uses the SQLite library of the system (`libsqlite3.so`), not a SQLite in the binary
//! (`bundled`). WebKitGTK and other libraries in the process of PANIM load `libsqlite3.so` too. With a second SQLite in the
//! binary, the binary gives some SQLite functions to these libraries and not others, so that a library uses parts of
//! two different SQLite copies. Also, two SQLite copies in one process remove the POSIX locks of each other. One
//! SQLite in the process prevents these problems.
//!
//! On macOS, the seal uses the bundled SQLite, because the SQLite of macOS is older than 3.44. WebKit on macOS keeps
//! its databases in its own processes, so these problems do not occur there (`docs/DISCUSSION.md` §56).

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use zeroize::Zeroizing;

use crate::vault::io;
use crate::{Vault, VaultError};

/// The folders that the archive does not keep.
const NOT_SEALED: &[&str] = &["WebKitCache"];

/// The first 16 bytes of a SQLite database file.
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

/// The files that SQLite keeps next to a database. The name is the name of the database and one of these ends.
const SQLITE_FILES: &[&str] = &["-wal", "-shm", "-journal"];

/// The time that the copy of a database waits for a lock of another connection.
const SQLITE_WAIT: Duration = Duration::from_secs(2);

/// The state of the files of an open folder: the relative path, the size and the modification time of each file,
/// in the order of the paths. A folder has the size 0 and no time.
///
/// The precision of the modification time is not always high. Before Linux 6.13, `tmpfs` uses a coarse clock with
/// a tick of some milliseconds. Thus, a change in the same tick, with the same size, does not change the
/// fingerprint. `SETTLE` prevents this problem (`settled`).
type Fingerprint = Vec<(PathBuf, u64, Option<SystemTime>)>;

/// A file with a modification time less than this time ago can change again in the same tick of the clock.
const SETTLE: Duration = Duration::from_secs(2);

/// Tells if a fingerprint can let the next periodic seal skip.
///
/// If a file changed less than `SETTLE` ago, or has a modification time in the future, a second change can come in
/// the same tick of the clock and not change the fingerprint. Then the area does not keep the fingerprint, and the
/// next periodic seal occurs. The next seals occur until all the files are older than `SETTLE`.
fn settled(print: &Fingerprint) -> bool {
    let now = SystemTime::now();
    print.iter().all(|(_, _, time)| match time {
        Some(time) => now.duration_since(*time).is_ok_and(|age| age >= SETTLE),
        None => true,
    })
}

/// The folder where open profiles are, in memory.
pub struct ProfileArea {
    base: PathBuf,
    /// The fingerprint of the last seal of each open folder. The key is the path of the open folder.
    sealed: Mutex<HashMap<PathBuf, Fingerprint>>,
}

impl ProfileArea {
    /// The area in `$XDG_RUNTIME_DIR/panim`. On Linux, this folder is in memory.
    pub fn runtime() -> Result<Self, VaultError> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .ok_or_else(|| VaultError::Io("XDG_RUNTIME_DIR is not set, so there is no folder in memory".into()))?;
        Ok(Self::at(Path::new(&runtime).join("panim")))
    }

    /// An area in a different folder. Use it for tests.
    pub fn at(base: PathBuf) -> Self {
        Self {
            base,
            sealed: Mutex::new(HashMap::new()),
        }
    }

    fn sealed(&self) -> MutexGuard<'_, HashMap<PathBuf, Fingerprint>> {
        // The map has no state that a panic can break, so a poisoned lock is not an error.
        self.sealed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The folder of an open profile of a vault.
    pub fn path(&self, vault: &Vault, folder: &str) -> PathBuf {
        self.base.join(vault.area_name()).join(folder)
    }

    /// Unseals a profile into the area and gives its folder. A new profile gives an empty folder.
    pub fn open(&self, vault: &Vault, folder: &str) -> Result<PathBuf, VaultError> {
        check_folder(folder)?;
        let own = self.base.join(vault.area_name());
        for dir in [&self.base, &own] {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(io)?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(io)?;
        }
        let path = own.join(folder);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(io)?;
        }
        fs::DirBuilder::new().mode(0o700).create(&path).map_err(io)?;
        self.sealed().remove(&path);
        if let Some(archive) = vault.read(&place(folder))? {
            tar::Archive::new(archive.as_slice()).unpack(&path).map_err(io)?;
            // The open folder is the same as the archive. Thus, a periodic seal before a change is not necessary.
            // The skip list is not known here, but the archive has no skipped entries, so the result is the same.
            if let Ok(print) = fingerprint(&path, &[])
                && settled(&print)
            {
                self.sealed().insert(path.clone(), print);
            }
        }
        Ok(path)
    }

    /// Seals the open profile into the vault. The folder stays open.
    pub fn seal(&self, vault: &Vault, folder: &str) -> Result<(), VaultError> {
        self.seal_except(vault, folder, NOT_SEALED)
    }

    /// Seals the open profile into the vault if a file changed after the last seal. The folder stays open.
    ///
    /// Returns true if the seal occurred.
    pub fn seal_changed(&self, vault: &Vault, folder: &str) -> Result<bool, VaultError> {
        self.seal_changed_except(vault, folder, NOT_SEALED)
    }

    /// Seals an open folder into the vault, without the entries of its first level that `skip` names.
    ///
    /// The Telegram connector uses it: the archive of a TDLib folder does not keep `files`.
    pub fn seal_except(&self, vault: &Vault, folder: &str, skip: &[&str]) -> Result<(), VaultError> {
        self.seal_inner(vault, folder, skip, true).map(|_| ())
    }

    /// Does `seal_except` only if a file changed after the last seal. Returns true if the seal occurred.
    ///
    /// The function compares the path, the size and the modification time of each file (`Fingerprint`).
    pub fn seal_changed_except(&self, vault: &Vault, folder: &str, skip: &[&str]) -> Result<bool, VaultError> {
        self.seal_inner(vault, folder, skip, false)
    }

    fn seal_inner(&self, vault: &Vault, folder: &str, skip: &[&str], always: bool) -> Result<bool, VaultError> {
        check_folder(folder)?;
        let path = self.path(vault, folder);
        // Make the fingerprint before the archive. If a file changes while the archive is made, the next seal
        // sees a different fingerprint and seals again.
        let print = fingerprint(&path, skip)?;
        if !always && self.sealed().get(&path) == Some(&print) {
            return Ok(false);
        }
        let mut builder = tar::Builder::new(Vec::new());
        builder.follow_symlinks(false);
        append(&mut builder, &path, Path::new(""), skip)?;
        let archive = Zeroizing::new(builder.into_inner().map_err(io)?);
        // Keep the fingerprint only after a write that passed, and only if it is settled.
        vault.write(&place(folder), &archive)?;
        if settled(&print) {
            self.sealed().insert(path, print);
        } else {
            self.sealed().remove(&path);
        }
        Ok(true)
    }

    /// Seals the profile, then removes its open folder.
    pub fn close(&self, vault: &Vault, folder: &str) -> Result<(), VaultError> {
        self.close_except(vault, folder, NOT_SEALED)
    }

    /// Seals a folder without the entries that `skip` names, then removes the open folder.
    pub fn close_except(&self, vault: &Vault, folder: &str, skip: &[&str]) -> Result<(), VaultError> {
        self.seal_except(vault, folder, skip)?;
        self.remove_open(vault, folder)
    }

    /// Removes a folder for all time: the open folder in the area and the sealed archive in the vault.
    ///
    /// It is not an error if the folder or the archive does not exist.
    pub fn discard(&self, vault: &Vault, folder: &str) -> Result<(), VaultError> {
        check_folder(folder)?;
        if self.path(vault, folder).exists() {
            self.remove_open(vault, folder)?;
        }
        vault.remove(&place(folder))
    }

    fn remove_open(&self, vault: &Vault, folder: &str) -> Result<(), VaultError> {
        let path = self.path(vault, folder);
        self.sealed().remove(&path);
        fs::remove_dir_all(path).map_err(io)?;
        // Remove the folder of the vault when it has no open profile. It is not an error if it is not empty.
        let _ = fs::remove_dir(self.base.join(vault.area_name()));
        Ok(())
    }
}

fn place(folder: &str) -> String {
    format!("profile/{folder}")
}

/// A folder name has only lowercase letters, digits and `-`, so it cannot point outside the area.
fn check_folder(folder: &str) -> Result<(), VaultError> {
    let ok = !folder.is_empty()
        && folder
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(VaultError::Io(format!(
            "{folder:?} is not a correct profile folder name"
        )))
    }
}

/// Makes the fingerprint of the files that `append` puts into the archive.
fn fingerprint(root: &Path, skip: &[&str]) -> Result<Fingerprint, VaultError> {
    let mut print = Vec::new();
    add_prints(&mut print, root, Path::new(""), skip)?;
    print.sort();
    Ok(print)
}

fn add_prints(print: &mut Fingerprint, root: &Path, relative: &Path, skip: &[&str]) -> Result<(), VaultError> {
    let entries = match fs::read_dir(root.join(relative)) {
        Ok(entries) => entries,
        // A folder that WebKit removed is not in the archive.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !relative.as_os_str().is_empty() => return Ok(()),
        Err(e) => return Err(io(e)),
    };
    for entry in entries {
        let entry = entry.map_err(io)?;
        let name = relative.join(entry.file_name());
        if skip.iter().any(|skip| name == Path::new(skip)) {
            continue;
        }
        // This does not follow a symbolic link, as `append` does not.
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(e)),
        };
        if meta.is_dir() {
            print.push((name.clone(), 0, None));
            add_prints(print, root, &name, skip)?;
        } else if meta.is_file() {
            // The copy of a database can write to its `-shm` file, and the `-shm` file has no data of its own. A
            // change of the data changes the database file or its `-wal` file. A `-shm` file without a database
            // next to it is a usual file.
            if shm_of_database(&entry.path()) {
                continue;
            }
            print.push((name, meta.len(), Some(meta.modified().map_err(io)?)));
        }
    }
    Ok(())
}

/// Tells if the file is the `-shm` file of a SQLite database: its name ends with `-shm`, and the file with the name
/// without this end starts with the header of SQLite.
fn shm_of_database(path: &Path) -> bool {
    let Some(database) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix("-shm"))
    else {
        return false;
    };
    let mut first = [0u8; 16];
    fs::File::open(path.with_file_name(database))
        .and_then(|mut file| file.read_exact(&mut first))
        .is_ok_and(|()| &first == SQLITE_HEADER)
}

fn append(builder: &mut tar::Builder<Vec<u8>>, root: &Path, relative: &Path, skip: &[&str]) -> Result<(), VaultError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root.join(relative)).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = relative.join(entry.file_name());
        if skip.iter().any(|skip| name == Path::new(skip)) {
            continue;
        }
        entries.push((entry, name));
    }
    // First, copy the SQLite databases of this folder. Then the second loop knows which `-wal`, `-shm` and
    // `-journal` files to leave out.
    let mut copies = HashMap::new();
    for (entry, name) in &entries {
        if entry.file_type().map_err(io)?.is_file()
            && needs_sqlite_copy(&entry.path())
            && let Some(copy) = sqlite_copy(&entry.path(), name)
        {
            copies.insert(entry.file_name(), copy);
        }
    }
    for (entry, name) in &entries {
        let kind = entry.file_type().map_err(io)?;
        if kind.is_dir() {
            builder.append_dir(name, entry.path()).map_err(io)?;
            append(builder, root, name, skip)?;
        } else if kind.is_file() {
            if let Some(copy) = copies.get(&entry.file_name()) {
                let mut header = tar::Header::new_gnu();
                match entry.metadata() {
                    Ok(meta) => header.set_metadata(&meta),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(io(e)),
                }
                header.set_size(copy.len() as u64);
                builder.append_data(&mut header, name, copy.as_slice()).map_err(io)?;
                continue;
            }
            if part_of_copy(&entry.file_name(), &copies) {
                continue;
            }
            // WebKit can remove a file while the archive is made. Then the archive does not keep it.
            match fs::File::open(entry.path()) {
                Ok(mut file) => builder.append_file(name, &mut file).map_err(io)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
        }
    }
    Ok(())
}

/// Tells if the file is a SQLite database that needs a copy with the backup interface.
///
/// A database in WAL mode without a `-wal` file has no open connection, so the file is complete. A connection that
/// is read-only makes an empty `-wal` file and a `-shm` file for such a database, and it cannot remove them. Thus,
/// the seal copies such a database as a usual file.
fn needs_sqlite_copy(path: &Path) -> bool {
    let mut first = [0u8; 20];
    if fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut first))
        .is_err()
        || &first[..16] != SQLITE_HEADER
    {
        return false;
    }
    // The bytes 18 and 19 of the header are 2 in WAL mode.
    let wal_mode = first[18] == 2;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    !wal_mode || Path::new(&wal).exists()
}

/// Tells if the file is the `-wal`, `-shm` or `-journal` file of a database in `copies`.
fn part_of_copy(file: &OsString, copies: &HashMap<OsString, Zeroizing<Vec<u8>>>) -> bool {
    let file = file.to_string_lossy();
    SQLITE_FILES.iter().any(|end| {
        file.strip_suffix(end)
            .is_some_and(|database| copies.contains_key(&OsString::from(database)))
    })
}

/// Makes a correct copy of a SQLite database with the backup interface of SQLite. The copy has all the data of
/// the `-wal` file, so it is complete without it.
///
/// The copy goes into a database in memory. Thus, no temporary file with the data is on the disk. If the copy
/// fails, the function logs the relative path and gives `None`. Then the seal copies the files as before.
fn sqlite_copy(path: &Path, name: &Path) -> Option<Zeroizing<Vec<u8>>> {
    let copy = || -> Result<Zeroizing<Vec<u8>>, String> {
        let text = |e: rusqlite::Error| e.to_string();
        // A connection that is read-only does not change the database. It can write the read marks of WAL mode
        // into the `-shm` file, as each reader of SQLite does.
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let source = Connection::open_with_flags(path, flags).map_err(text)?;
        source.busy_timeout(SQLITE_WAIT).map_err(text)?;
        let mut target = Connection::open_in_memory().map_err(text)?;
        {
            // One step copies all the pages in one read transaction, so the copy is one state of the database.
            let backup = Backup::new(&source, &mut target).map_err(text)?;
            match backup.step(-1).map_err(text)? {
                StepResult::Done => {}
                other => return Err(format!("the backup did not complete: {other:?}")),
            }
        }
        Ok(Zeroizing::new(target.serialize("main").map_err(text)?.to_vec()))
    };
    match copy() {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            eprintln!(
                "panim: the SQLite copy of {} failed, so the seal copies the file: {e}",
                name.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KdfParams;
    use panim_core::PersonaKey;

    #[test]
    fn profile_goes_to_memory_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path(),
            "passcode1",
            &PersonaKey::generate().unwrap(),
            KdfParams::TEST,
        )
        .unwrap();
        let area = ProfileArea::at(memory.path().join("panim"));

        let path = area.open(&vault, "persona-1").unwrap();
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
        fs::write(path.join("cookies"), "session=alice").unwrap();
        fs::create_dir_all(path.join("localstorage/site")).unwrap();
        fs::write(path.join("localstorage/site/data"), "value").unwrap();
        fs::create_dir_all(path.join("WebKitCache")).unwrap();
        fs::write(path.join("WebKitCache/big"), "cache").unwrap();
        area.close(&vault, "persona-1").unwrap();
        assert!(!path.exists());
        assert!(!memory.path().join("panim").join(vault.area_name()).exists());

        let path = area.open(&vault, "persona-1").unwrap();
        assert_eq!(fs::read_to_string(path.join("cookies")).unwrap(), "session=alice");
        assert_eq!(
            fs::read_to_string(path.join("localstorage/site/data")).unwrap(),
            "value"
        );
        assert!(!path.join("WebKitCache").exists());
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);

        // The sealed archive on the disk does not show the cookie.
        for entry in fs::read_dir(dir.path().join("data")).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap();
            assert!(!bytes.windows(13).any(|w| w == b"session=alice"));
        }
    }

    #[test]
    fn a_folder_with_a_skip_list_and_its_removal() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path(),
            "passcode1",
            &PersonaKey::generate().unwrap(),
            KdfParams::TEST,
        )
        .unwrap();
        let area = ProfileArea::at(memory.path().to_path_buf());
        let files_before = fs::read_dir(dir.path().join("data")).unwrap().count();

        let path = area.open(&vault, "telegram-0a1b").unwrap();
        fs::create_dir_all(path.join("db")).unwrap();
        fs::write(path.join("db/td.binlog"), "login").unwrap();
        fs::create_dir_all(path.join("files/photos")).unwrap();
        fs::write(path.join("files/photos/a.jpg"), "picture").unwrap();
        // A folder with the same name at a lower level is not in the skip list.
        fs::create_dir_all(path.join("db/files")).unwrap();
        fs::write(path.join("db/files/x"), "kept").unwrap();

        area.seal_except(&vault, "telegram-0a1b", &["files"]).unwrap();
        assert!(
            path.join("files/photos/a.jpg").exists(),
            "the open folder does not change"
        );
        area.close_except(&vault, "telegram-0a1b", &["files"]).unwrap();
        assert!(!path.exists());

        let path = area.open(&vault, "telegram-0a1b").unwrap();
        assert_eq!(fs::read_to_string(path.join("db/td.binlog")).unwrap(), "login");
        assert_eq!(fs::read_to_string(path.join("db/files/x")).unwrap(), "kept");
        assert!(!path.join("files").exists());

        // The removal deletes the open folder and the archive. A second removal is not an error.
        assert_eq!(fs::read_dir(dir.path().join("data")).unwrap().count(), files_before + 1);
        area.discard(&vault, "telegram-0a1b").unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_dir(dir.path().join("data")).unwrap().count(), files_before);
        area.discard(&vault, "telegram-0a1b").unwrap();
        assert_eq!(
            fs::read_dir(area.open(&vault, "telegram-0a1b").unwrap())
                .unwrap()
                .count(),
            0
        );
        assert!(area.discard(&vault, "../x").is_err());
    }

    fn test_vault(dir: &Path) -> Vault {
        Vault::create(dir, "passcode1", &PersonaKey::generate().unwrap(), KdfParams::TEST).unwrap()
    }

    /// Sets the modification time of all the files in a folder to `SETTLE` and more before now. Then a fingerprint
    /// of the folder is settled, and the test does not wait.
    fn age(root: &Path) {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                age(&entry.path());
            } else {
                set_time(&entry.path(), SETTLE * 5);
            }
        }
    }

    /// Sets the modification time of a file to `ago` before now.
    fn set_time(path: &Path, ago: Duration) {
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - ago).unwrap();
    }

    #[test]
    fn a_periodic_seal_occurs_only_after_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        fs::write(path.join("cookies"), "session=alice").unwrap();
        fs::create_dir_all(path.join("storage")).unwrap();
        fs::write(path.join("storage/a"), "aaaa").unwrap();
        fs::create_dir_all(path.join("WebKitCache")).unwrap();
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap(), "the first seal occurs");

        // No change: no archive and no write. The test hook stays set, so the next write fails.
        Vault::fail_next_write();
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());
        // A change in a skipped folder is not a change.
        fs::write(path.join("WebKitCache/big"), "cache").unwrap();
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());
        assert!(area.seal(&vault, "persona-1").is_err(), "the hook was not used");

        // A change of the size.
        fs::write(path.join("cookies"), "session=alice2").unwrap();
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A change with the same size and a new modification time.
        fs::write(path.join("storage/a"), "bbbb").unwrap();
        set_time(&path.join("storage/a"), SETTLE * 3);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A new file, a new folder and a removed file.
        fs::write(path.join("storage/b"), "").unwrap();
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        fs::create_dir_all(path.join("empty")).unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        fs::remove_file(path.join("storage/a")).unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A failed write does not keep the fingerprint, so the next seal tries again.
        fs::write(path.join("cookies"), "session=bob").unwrap();
        age(&path);
        Vault::fail_next_write();
        assert!(area.seal_changed(&vault, "persona-1").is_err());
        assert!(area.seal_changed(&vault, "persona-1").unwrap());

        // The close always seals.
        Vault::fail_next_write();
        assert!(area.close(&vault, "persona-1").is_err());
        area.close(&vault, "persona-1").unwrap();

        // An open folder is the same as its archive. Thus, a periodic seal is not necessary.
        let path = area.open(&vault, "persona-1").unwrap();
        assert_eq!(fs::read_to_string(path.join("cookies")).unwrap(), "session=bob");
        assert!(path.join("storage/b").exists() && !path.join("storage/a").exists());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());
        fs::write(path.join("cookies"), "session=carol").unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
    }

    #[test]
    fn a_periodic_seal_with_a_skip_list() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "telegram-0a1b").unwrap();
        fs::create_dir_all(path.join("db")).unwrap();
        fs::write(path.join("db/td.binlog"), "login").unwrap();
        age(&path);
        assert!(area.seal_changed_except(&vault, "telegram-0a1b", &["files"]).unwrap());
        fs::create_dir_all(path.join("files")).unwrap();
        fs::write(path.join("files/a.jpg"), "picture").unwrap();
        assert!(!area.seal_changed_except(&vault, "telegram-0a1b", &["files"]).unwrap());
        fs::write(path.join("db/td.binlog"), "login 2").unwrap();
        assert!(area.seal_changed_except(&vault, "telegram-0a1b", &["files"]).unwrap());
        // A seal without a check always writes.
        Vault::fail_next_write();
        assert!(area.seal_except(&vault, "telegram-0a1b", &["files"]).is_err());
    }

    #[test]
    fn a_recent_change_makes_the_next_periodic_seal_occur() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();

        // The file changed now, so a second change in the same tick can come. The seals occur without a change.
        fs::write(path.join("cookies"), "session=alice").unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        // When the file is older than `SETTLE`, a folder without a change does not get a seal.
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A modification time in the future is not settled.
        let file = fs::File::options().write(true).open(path.join("cookies")).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(60)).unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // An open folder with recent files does not keep a fingerprint either.
        fs::write(path.join("cookies"), "session=bob").unwrap();
        area.close(&vault, "persona-1").unwrap();
        let path = area.open(&vault, "persona-1").unwrap();
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());
    }

    #[test]
    fn only_the_shm_file_of_a_database_is_not_in_the_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        // A file with the end `-shm` and no file without the end, and one next to a file that is not a database.
        fs::write(path.join("state-shm"), "aaaa").unwrap();
        fs::write(path.join("text"), "not a database").unwrap();
        fs::write(path.join("text-shm"), "aaaa").unwrap();
        // A database in WAL mode with an open connection has a `-shm` file.
        let db = wal_database(&path.join("local.db"));
        db.execute("INSERT INTO t VALUES (1, 'value')", []).unwrap();
        assert!(path.join("local.db-shm").exists());
        age(&path);
        assert!(area.seal_changed(&vault, "persona-1").unwrap());
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A change of the `-shm` file of the database alone does not make a seal.
        set_time(&path.join("local.db-shm"), SETTLE * 3);
        assert!(!area.seal_changed(&vault, "persona-1").unwrap());

        // A change of the other files with the same size makes a seal.
        for file in ["state-shm", "text-shm"] {
            fs::write(path.join(file), "bbbb").unwrap();
            set_time(&path.join(file), SETTLE * 3);
            assert!(area.seal_changed(&vault, "persona-1").unwrap(), "{file}");
            assert!(!area.seal_changed(&vault, "persona-1").unwrap(), "{file}");
        }
        assert_eq!(archived(&vault, "persona-1")["state-shm"], b"bbbb");
        drop(db);
    }

    /// The files in the archive of a folder, with their bytes.
    fn archived(vault: &Vault, folder: &str) -> HashMap<String, Vec<u8>> {
        let archive = vault.read(&place(folder)).unwrap().unwrap();
        let mut files = HashMap::new();
        for entry in tar::Archive::new(archive.as_slice()).entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            files.insert(name, bytes);
        }
        files
    }

    /// Writes the bytes of a database to a new folder, and gives the result of `integrity_check` and the number of
    /// rows in the table `t`.
    fn check_copy(bytes: &[u8]) -> (String, i64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy.db");
        fs::write(&path, bytes).unwrap();
        let db = Connection::open(&path).unwrap();
        let check: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0)).unwrap();
        let rows = db.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        (check, rows)
    }

    fn wal_database(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        let mode: String = db.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0)).unwrap();
        assert_eq!(mode, "wal");
        db.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE IF NOT EXISTS t (n INTEGER, text TEXT);")
            .unwrap();
        db
    }

    #[test]
    fn the_seal_copies_a_sqlite_database_with_its_wal_data() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        fs::create_dir_all(path.join("storage")).unwrap();

        // The writer stays open, so the rows stay in the `-wal` file and not in the database file.
        let db = wal_database(&path.join("storage/local.db"));
        for n in 0..500 {
            db.execute("INSERT INTO t VALUES (?1, 'value')", [n]).unwrap();
        }
        assert!(fs::metadata(path.join("storage/local.db-wal")).unwrap().len() > 0);
        assert!(path.join("storage/local.db-shm").exists());
        // A database in the usual journal mode.
        let plain = Connection::open(path.join("plain.db")).unwrap();
        // A page size that is not the default of SQLite.
        plain
            .execute_batch("PRAGMA page_size=16384; CREATE TABLE t (n INTEGER); INSERT INTO t VALUES (1), (2);")
            .unwrap();
        drop(plain);
        assert!(sqlite_copy(&path.join("plain.db"), Path::new("plain.db")).is_some());
        assert!(sqlite_copy(&path.join("storage/local.db"), Path::new("storage/local.db")).is_some());
        // A database in WAL mode that no connection has open: it has no `-wal` file.
        let closed = wal_database(&path.join("closed.db"));
        closed.execute("INSERT INTO t VALUES (1, 'closed')", []).unwrap();
        drop(closed);
        assert!(!path.join("closed.db-wal").exists());
        // A file with such an end, but with no database, stays in the archive.
        fs::write(path.join("other-wal"), "not a database").unwrap();

        // The database file alone does not have the table: it is only in the `-wal` file.
        let alone = tempfile::tempdir().unwrap();
        fs::copy(path.join("storage/local.db"), alone.path().join("alone.db")).unwrap();
        let tables: i64 = Connection::open(alone.path().join("alone.db"))
            .unwrap()
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tables, 0);

        area.seal(&vault, "persona-1").unwrap();
        let files = archived(&vault, "persona-1");
        for part in [
            "storage/local.db-wal",
            "storage/local.db-shm",
            "storage/local.db-journal",
        ] {
            assert!(!files.contains_key(part), "{part}");
        }
        assert_eq!(files["other-wal"], b"not a database");
        assert_eq!(check_copy(&files["storage/local.db"]), ("ok".into(), 500));
        assert_eq!(check_copy(&files["plain.db"]), ("ok".into(), 2));
        assert_eq!(check_copy(&files["closed.db"]), ("ok".into(), 1));
        // The seal does not change the source.
        assert!(!path.join("closed.db-wal").exists() && !path.join("closed.db-shm").exists());
        assert!(!path.join("plain.db-journal").exists());
        db.execute("INSERT INTO t VALUES (500, 'value')", []).unwrap();
        let rows: i64 = db.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 501);

        // The archive opens into a new area, and the database there has all the rows.
        drop(db);
        let other = ProfileArea::at(memory.path().join("other"));
        let again = other.open(&vault, "persona-1").unwrap();
        assert!(!again.join("storage/local.db-wal").exists());
        let db = Connection::open(again.join("storage/local.db")).unwrap();
        let check: String = db.query_row("PRAGMA integrity_check", [], |row| row.get(0)).unwrap();
        assert_eq!(check, "ok");
        let rows: i64 = db.query_row("SELECT count(*) FROM t", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 500);
    }

    #[test]
    fn the_seal_copies_a_database_while_a_thread_writes_to_it() {
        const ROWS: i64 = 25;
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        let file = path.join("local.db");
        drop(wal_database(&file));

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let (stop, file) = (stop.clone(), file.clone());
            std::thread::spawn(move || {
                let mut db = Connection::open(&file).unwrap();
                db.busy_timeout(Duration::from_secs(5)).unwrap();
                // The usual checkpoint occurs, so the copy sees data in the database file and in the `-wal` file.
                db.execute_batch("PRAGMA wal_autocheckpoint=100").unwrap();
                let mut written = 0;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let tx = db.transaction().unwrap();
                    for n in 0..ROWS {
                        tx.execute("INSERT INTO t VALUES (?1, 'a text that fills the page')", [n])
                            .unwrap();
                    }
                    tx.commit().unwrap();
                    written += 1;
                }
                written
            })
        };
        let mut counts = Vec::new();
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(20));
            area.seal(&vault, "persona-1").unwrap();
            let files = archived(&vault, "persona-1");
            assert!(!files.contains_key("local.db-wal"));
            let (check, rows) = check_copy(&files["local.db"]);
            assert_eq!(check, "ok");
            assert_eq!(rows % ROWS, 0, "the copy has no part of a transaction");
            counts.push(rows);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let written = writer.join().unwrap();
        assert!(written > 0);
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]), "{counts:?}");
        assert!(
            counts.last() > counts.first(),
            "the writer wrote during the seals: {counts:?}"
        );
    }

    #[test]
    fn a_damaged_database_is_copied_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        let mut bad = SQLITE_HEADER.to_vec();
        bad.extend_from_slice(&[0xab; 4080]);
        fs::write(path.join("bad.db"), &bad).unwrap();
        fs::write(path.join("bad.db-wal"), b"wal bytes").unwrap();
        fs::write(path.join("bad.db-journal"), b"journal bytes").unwrap();
        // A short file with a part of the header.
        fs::write(path.join("short"), &SQLITE_HEADER[..10]).unwrap();
        let good = wal_database(&path.join("good.db"));
        good.execute("INSERT INTO t VALUES (1, 'good')", []).unwrap();

        area.seal(&vault, "persona-1").unwrap();
        let files = archived(&vault, "persona-1");
        assert_eq!(files["bad.db"], bad);
        assert_eq!(files["bad.db-wal"], b"wal bytes");
        assert_eq!(files["bad.db-journal"], b"journal bytes");
        assert_eq!(files["short"], &SQLITE_HEADER[..10]);
        assert_eq!(check_copy(&files["good.db"]), ("ok".into(), 1));
        assert!(!files.contains_key("good.db-wal"));
    }

    /// An archive from release 0.1.2 has the files as they were. It opens as before.
    #[test]
    fn an_archive_of_release_0_1_2_opens() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = test_vault(dir.path());
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(5);
        header.set_mode(0o600);
        builder
            .append_data(&mut header, "cookies", b"alice".as_slice())
            .unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(9);
        header.set_mode(0o600);
        builder
            .append_data(&mut header, "storage/local.db-wal", b"old bytes".as_slice())
            .unwrap();
        vault
            .write(&place("persona-1"), &builder.into_inner().unwrap())
            .unwrap();

        let area = ProfileArea::at(memory.path().to_path_buf());
        let path = area.open(&vault, "persona-1").unwrap();
        assert_eq!(fs::read(path.join("cookies")).unwrap(), b"alice");
        assert_eq!(fs::read(path.join("storage/local.db-wal")).unwrap(), b"old bytes");
    }

    #[test]
    fn profiles_are_apart() {
        let dir = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path(),
            "passcode1",
            &PersonaKey::generate().unwrap(),
            KdfParams::TEST,
        )
        .unwrap();
        let area = ProfileArea::at(memory.path().to_path_buf());
        let alice = area.open(&vault, "persona-1").unwrap();
        fs::write(alice.join("cookies"), "alice").unwrap();
        area.close(&vault, "persona-1").unwrap();
        let bob = area.open(&vault, "persona-2").unwrap();
        assert!(!bob.join("cookies").exists());
    }

    #[test]
    fn two_vaults_in_one_area_stay_apart() {
        let real = tempfile::tempdir().unwrap();
        let test = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let area = ProfileArea::at(memory.path().to_path_buf());
        let new_vault =
            |dir: &Path| Vault::create(dir, "passcode1", &PersonaKey::generate().unwrap(), KdfParams::TEST).unwrap();
        let (real, test) = (new_vault(real.path()), new_vault(test.path()));
        assert_ne!(real.area_name(), test.area_name());

        // The two vaults open the same profile folder name at the same time.
        let real_path = area.open(&real, "persona-1").unwrap();
        let test_path = area.open(&test, "persona-1").unwrap();
        assert_ne!(real_path, test_path);
        fs::write(real_path.join("cookies"), "real login").unwrap();
        fs::write(test_path.join("cookies"), "test login").unwrap();
        area.close(&test, "persona-1").unwrap();
        assert_eq!(fs::read_to_string(real_path.join("cookies")).unwrap(), "real login");
        area.close(&real, "persona-1").unwrap();

        let again = area.open(&real, "persona-1").unwrap();
        assert_eq!(fs::read_to_string(again.join("cookies")).unwrap(), "real login");
    }

    #[test]
    fn folder_names_cannot_leave_the_area() {
        let area = ProfileArea::at(PathBuf::from("/tmp/unused"));
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(
            dir.path(),
            "passcode1",
            &PersonaKey::generate().unwrap(),
            KdfParams::TEST,
        )
        .unwrap();
        for bad in ["../x", "a/b", "", "Persona"] {
            assert!(area.open(&vault, bad).is_err(), "{bad}");
        }
    }
}
