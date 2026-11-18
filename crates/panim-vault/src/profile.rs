//! Sealed browser profiles.
//!
//! WebKit writes plain files into a profile folder while it runs. Thus, the vault keeps each profile as one sealed
//! archive, and the app opens the profile into memory:
//!
//! - `open` unseals the archive into the profile area. On Linux, the area is in `$XDG_RUNTIME_DIR`, which is in
//!   memory and not on the disk.
//! - `seal` writes the folder to the vault again. The app calls it every few minutes, so a crash loses only a few
//!   minutes of browser state.
//! - `close` seals the folder and removes it from the area. Close the web views of the profile before `close`.
//!
//! Each vault has its own folder in the area (`Vault::area_name`). Thus, two copies of the app with different data
//! folders can run at the same time, and their open profiles stay apart.
//!
//! The HTTP cache (`WebKitCache`) is not sealed. The browser makes it again.

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::vault::io;
use crate::{Vault, VaultError};

/// The folders that the archive does not keep.
const NOT_SEALED: &[&str] = &["WebKitCache"];

/// The folder where open profiles are, in memory.
pub struct ProfileArea {
    base: PathBuf,
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
        Self { base }
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
        if let Some(archive) = vault.read(&place(folder))? {
            tar::Archive::new(archive.as_slice()).unpack(&path).map_err(io)?;
        }
        Ok(path)
    }

    /// Seals the open profile into the vault. The folder stays open.
    pub fn seal(&self, vault: &Vault, folder: &str) -> Result<(), VaultError> {
        self.seal_except(vault, folder, NOT_SEALED)
    }

    /// Seals an open folder into the vault, without the entries of its first level that `skip` names.
    ///
    /// The Telegram connector uses it: the archive of a TDLib folder does not keep `files`.
    pub fn seal_except(&self, vault: &Vault, folder: &str, skip: &[&str]) -> Result<(), VaultError> {
        check_folder(folder)?;
        let path = self.path(vault, folder);
        let mut builder = tar::Builder::new(Vec::new());
        builder.follow_symlinks(false);
        append(&mut builder, &path, Path::new(""), skip)?;
        let archive = Zeroizing::new(builder.into_inner().map_err(io)?);
        vault.write(&place(folder), &archive)
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
        fs::remove_dir_all(self.path(vault, folder)).map_err(io)?;
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

fn append(builder: &mut tar::Builder<Vec<u8>>, root: &Path, relative: &Path, skip: &[&str]) -> Result<(), VaultError> {
    for entry in fs::read_dir(root.join(relative)).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = relative.join(entry.file_name());
        let kind = entry.file_type().map_err(io)?;
        if skip.iter().any(|skip| name == Path::new(skip)) {
            continue;
        }
        if kind.is_dir() {
            builder.append_dir(&name, entry.path()).map_err(io)?;
            append(builder, root, &name, skip)?;
        } else if kind.is_file() {
            // WebKit can remove a file while the archive is made. Then the archive does not keep it.
            match fs::File::open(entry.path()) {
                Ok(mut file) => builder.append_file(&name, &mut file).map_err(io)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
        }
    }
    Ok(())
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
