//! The backup of a vault: one archive file with the header and the sealed files.
//!
//! Refer to `docs/ARCHITECTURE.md` §3a. The files in the vault are sealed, so the archive needs no new cipher. The
//! archive is a `tar` file with these entries, in this sequence:
//!
//! 1. `panim-backup`: the mark of the format, `panim-backup/1` and a line feed.
//! 2. `vault.json`: the header of the vault.
//! 3. `data/<32 hexadecimal digits>`: each sealed file.
//!
//! The archive does not keep `vault.lock`, `gate.json` or `settings.json`. The lock and the count of wrong tries
//! belong to one computer. The settings are not secret, and the first start sets the language again.
//!
//! The restore accepts only these entries. It refuses an entry with a different name, a link, a folder, a second
//! copy of a name, a missing mark, a missing header, and an entry or an archive that is too large. It writes the
//! files into a new folder first. When all the checks pass, it moves the files into the vault folder, and the header
//! comes last. Thus, a failed restore leaves no vault.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

use crate::vault::{self, DATA, FILE_MAGIC, GATE, HEADER, io};
use crate::{Vault, VaultError, crypto};

/// The name of the first entry of an archive.
const MARK: &str = "panim-backup";
/// The content of the first entry.
pub const BACKUP_FORMAT: &str = "panim-backup/1";

/// The limits of a restore. The tests use smaller limits.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The largest archive file, in bytes.
    pub archive: u64,
    /// The largest sealed file, in bytes.
    pub file: u64,
    /// The largest number of sealed files.
    pub files: usize,
}

impl Limits {
    /// The limits of the app. A browser profile is a few hundred megabytes. These limits are much larger.
    pub const STANDARD: Limits = Limits {
        archive: 16 << 30,
        file: 8 << 30,
        files: 100_000,
    };
}

/// The largest header (`vault.json`), in bytes. A real header has less than 2 KiB.
const MAX_HEADER: u64 = 64 << 10;

/// What a backup or a restore did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupInfo {
    /// The number of sealed files.
    pub files: usize,
    /// The number of bytes of the sealed files and the header.
    pub bytes: u64,
}

/// Writes the header and the sealed files of an open vault into one archive at `target`.
///
/// The write is complete or it does not occur: the function writes a temporary file in the folder of `target`,
/// then renames it. Seal the browser profile and the TDLib folders before this, and keep the session locked, so
/// that no sealed file changes while the archive is made. The target must not be in the vault folder.
pub fn write_backup(vault: &Vault, target: &Path) -> Result<BackupInfo, VaultError> {
    let dir = vault.dir();
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = target
        .file_name()
        .ok_or_else(|| VaultError::Io("the backup path has no file name".into()))?;
    let (parent_real, dir_real) = (
        fs::canonicalize(parent).map_err(io)?,
        fs::canonicalize(dir).map_err(io)?,
    );
    if parent_real.starts_with(&dir_real) {
        return Err(VaultError::Io("the backup cannot be in the vault folder".into()));
    }
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        crypto::hex(&crypto::random::<8>()?)
    ));
    let result = (|| {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(io)?;
        let mut builder = tar::Builder::new(BufWriter::new(file));
        let mut info = BackupInfo { files: 0, bytes: 0 };
        append(&mut builder, MARK, format!("{BACKUP_FORMAT}\n").as_bytes())?;
        let header = fs::read(dir.join(HEADER)).map_err(io)?;
        info.bytes += header.len() as u64;
        append(&mut builder, HEADER, &header)?;
        let mut names = Vec::new();
        for entry in fs::read_dir(dir.join(DATA)).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            // A temporary file of an interrupted write has a different name. The archive does not keep it.
            if entry.file_type().map_err(io)?.is_file() && is_data_name(&name) {
                names.push(name);
            }
        }
        names.sort();
        for name in names {
            let mut file = File::open(dir.join(DATA).join(&name)).map_err(io)?;
            let size = file.metadata().map_err(io)?.len();
            let mut tar_header = plain_header(size);
            builder
                .append_data(&mut tar_header, format!("{DATA}/{name}"), &mut file)
                .map_err(io)?;
            info.files += 1;
            info.bytes += size;
        }
        let file = builder
            .into_inner()
            .map_err(io)?
            .into_inner()
            .map_err(|e| io(e.into_error()))?;
        file.sync_all().map_err(io)?;
        fs::rename(&temp, target).map_err(io)?;
        File::open(parent).and_then(|dir| dir.sync_all()).map_err(io)?;
        Ok(info)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Writes the files of an archive into `dir`, which must have no vault. Then `Vault::open` opens it with the
/// passcode of the vault in the archive, or `Vault::recover` with its persona key.
pub fn restore_backup(archive: &Path, dir: &Path) -> Result<BackupInfo, VaultError> {
    restore_with(archive, dir, Limits::STANDARD)
}

/// `restore_backup` with other limits. The tests use it.
pub fn restore_with(archive: &Path, dir: &Path, limits: Limits) -> Result<BackupInfo, VaultError> {
    let file = File::open(archive).map_err(io)?;
    if file.metadata().map_err(io)?.len() > limits.archive {
        return Err(bad("the archive is too large"));
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io)?;
    // The lock of the one writer. A different process cannot make or open a vault here during the restore.
    let _lock = vault::lock(dir)?;
    if Vault::exists(dir) {
        return Err(VaultError::Exists);
    }
    let data = dir.join(DATA);
    if data.is_dir() && fs::read_dir(&data).map_err(io)?.next().is_some() {
        return Err(VaultError::Exists);
    }
    let staging = dir.join(format!(".restore-{}", crypto::hex(&crypto::random::<8>()?)));
    fs::DirBuilder::new().mode(0o700).create(&staging).map_err(io)?;
    let result = (|| {
        let info = unpack(file, &staging, limits)?;
        // The header must be a header that the vault can read.
        Vault::recovery_address(&staging)?;
        if data.is_dir() {
            fs::remove_dir(&data).map_err(io)?;
        }
        // A count of wrong tries from an earlier vault in this folder does not apply to the restored vault.
        match fs::remove_file(dir.join(GATE)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io(e)),
            _ => {}
        }
        fs::rename(staging.join(DATA), &data).map_err(io)?;
        // The header comes last. Before it exists, the folder has no vault.
        fs::rename(staging.join(HEADER), dir.join(HEADER)).map_err(io)?;
        File::open(dir).and_then(|dir| dir.sync_all()).map_err(io)?;
        Ok(info)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Reads the entries of the archive into `staging`, with all the checks.
fn unpack(file: File, staging: &Path, limits: Limits) -> Result<BackupInfo, VaultError> {
    let mut archive = tar::Archive::new(BufReader::new(file));
    let mut entries = archive.entries().map_err(|_| bad("the file is not an archive"))?;
    let mut info = BackupInfo { files: 0, bytes: 0 };

    // 1. The mark of the format.
    let mut mark = entries
        .next()
        .ok_or_else(|| bad("the archive is empty"))?
        .map_err(|_| bad("the file is not an archive"))?;
    if mark.path_bytes().as_ref() != MARK.as_bytes()
        || mark.header().entry_type() != tar::EntryType::Regular
        || mark.size() > 64
    {
        return Err(bad("the archive is not a PANIM backup"));
    }
    let mut text = String::new();
    mark.read_to_string(&mut text)
        .map_err(|_| bad("the archive is not a PANIM backup"))?;
    if text.trim_end() != BACKUP_FORMAT {
        return Err(bad("the backup has a format that this version does not know"));
    }

    // 2. The header, then the sealed files.
    fs::DirBuilder::new()
        .mode(0o700)
        .create(staging.join(DATA))
        .map_err(io)?;
    let mut header = false;
    for entry in entries {
        let mut entry = entry.map_err(|_| bad("the archive is damaged"))?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Err(bad("the archive has an entry that is not a file"));
        }
        let path = String::from_utf8(entry.path_bytes().into_owned()).map_err(|_| bad("the archive has a bad name"))?;
        let size = entry.size();
        let (target, limit) = if path == HEADER {
            if header {
                return Err(bad("the archive has two headers"));
            }
            header = true;
            (staging.join(HEADER), MAX_HEADER)
        } else if let Some(name) = path.strip_prefix(&format!("{DATA}/")).filter(|name| is_data_name(name)) {
            info.files += 1;
            if info.files > limits.files {
                return Err(bad("the archive has too many files"));
            }
            (staging.join(DATA).join(name), limits.file)
        } else {
            return Err(bad("the archive has an entry that is not part of a vault"));
        };
        if size > limit {
            return Err(bad("an entry of the archive is too large"));
        }
        info.bytes += size;
        if info.bytes > limits.archive {
            return Err(bad("the archive is too large"));
        }
        let mut out = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&target)
        {
            Ok(out) => out,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                return Err(bad("the archive has the same file two times"));
            }
            Err(e) => return Err(io(e)),
        };
        // Read one byte more than the size. Thus, an entry cannot give more data than its header tells.
        let copied = io::copy(&mut (&mut entry).take(size + 1), &mut out).map_err(|_| bad("the archive is damaged"))?;
        if copied != size {
            return Err(bad("the archive is damaged"));
        }
        if path != HEADER {
            let mut start = [0u8; 4];
            let mut check = File::open(&target).map_err(io)?;
            if check.read_exact(&mut start).is_err() || &start != FILE_MAGIC {
                return Err(bad("a file in the archive is not a sealed file"));
            }
        }
        out.sync_all().map_err(io)?;
    }
    if !header {
        return Err(bad("the archive has no vault header"));
    }
    if info.files == 0 {
        return Err(bad("the archive has no sealed files"));
    }
    File::open(staging.join(DATA))
        .and_then(|dir| dir.sync_all())
        .map_err(io)?;
    Ok(info)
}

/// The name of a sealed file: 32 lowercase hexadecimal digits (`Vault::file_path`).
fn is_data_name(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn bad(reason: &str) -> VaultError {
    VaultError::BadBackup(reason.into())
}

/// A header of a file entry with no owner and no time. The archive does not show the user or the times.
fn plain_header(size: u64) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_entry_type(tar::EntryType::Regular);
    header
}

fn append<W: Write>(builder: &mut tar::Builder<W>, path: &str, bytes: &[u8]) -> Result<(), VaultError> {
    let mut header = plain_header(bytes.len() as u64);
    builder.append_data(&mut header, path, bytes).map_err(io)
}

/// The path of the folder where a restore writes its files first. Only the tests use it.
#[cfg(test)]
fn staging_folders(dir: &Path) -> Vec<std::path::PathBuf> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(".restore-"))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{KdfParams, ProfileArea};
    use panim_core::PersonaKey;

    fn new_vault(dir: &Path) -> Vault {
        Vault::create(dir, "passcode1", &PersonaKey::generate().unwrap(), KdfParams::TEST).unwrap()
    }

    #[test]
    fn a_backup_and_a_restore_give_the_same_vault() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let memory = tempfile::tempdir().unwrap();
        let vault = new_vault(old.path());
        vault.write("store/meta", b"the accounts").unwrap();
        vault.write("secrets", b"a token").unwrap();
        let area = ProfileArea::at(memory.path().to_path_buf());
        let profile = area.open(&vault, "persona-1").unwrap();
        fs::write(profile.join("cookies"), "session=alice").unwrap();
        area.seal(&vault, "persona-1").unwrap();
        // Files that are not sealed files of the vault stay out.
        fs::write(old.path().join("settings.json"), "{}").unwrap();
        fs::write(old.path().join(DATA).join(".x.tmp"), "half").unwrap();

        let target = out.path().join("panim.backup");
        let info = write_backup(&vault, &target).unwrap();
        assert_eq!(info.files, 3);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1, "no temporary file stays");
        drop(vault);

        let dir = new.path().join("panim");
        assert_eq!(restore_backup(&target, &dir).unwrap(), info);
        assert!(staging_folders(&dir).is_empty());
        assert!(!dir.join("settings.json").exists() && !dir.join(GATE).exists());
        let vault = Vault::open(&dir, "passcode1", KdfParams::TEST).unwrap();
        assert_eq!(vault.read("store/meta").unwrap().unwrap().as_slice(), b"the accounts");
        assert_eq!(vault.read("secrets").unwrap().unwrap().as_slice(), b"a token");
        let profile = ProfileArea::at(memory.path().join("again"))
            .open(&vault, "persona-1")
            .unwrap();
        assert_eq!(fs::read_to_string(profile.join("cookies")).unwrap(), "session=alice");
        assert_eq!(
            Vault::recovery_address(&dir).unwrap(),
            Vault::recovery_address(old.path()).unwrap()
        );

        // A second restore into the same folder is refused, and the vault does not change.
        assert_eq!(restore_backup(&target, &dir).unwrap_err(), VaultError::InUse);
        drop(vault);
        assert_eq!(restore_backup(&target, &dir).unwrap_err(), VaultError::Exists);
        assert!(Vault::open(&dir, "passcode1", KdfParams::TEST).is_ok());
    }

    #[test]
    fn the_backup_is_not_in_the_vault_folder() {
        let old = tempfile::tempdir().unwrap();
        let vault = new_vault(old.path());
        vault.write("a", b"1").unwrap();
        for target in [old.path().join("x.backup"), old.path().join(DATA).join("x.backup")] {
            assert!(write_backup(&vault, &target).is_err(), "{}", target.display());
            assert!(!target.exists());
        }
    }

    /// One entry of a test archive: the raw name, the type and the data.
    type Entry<'a> = (&'a [u8], tar::EntryType, Vec<u8>);

    /// An archive entry with a raw name. `tar::Builder` refuses some names, so the test writes the name itself.
    fn raw_entry(builder: &mut tar::Builder<Vec<u8>>, name: &[u8], kind: tar::EntryType, data: &[u8]) {
        let mut header = tar::Header::new_old();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_size(data.len() as u64);
        header.set_mode(0o600);
        header.set_entry_type(kind);
        if kind == tar::EntryType::Symlink {
            header.set_link_name("/etc/passwd").unwrap();
        }
        header.set_cksum();
        builder.append(&header, data).unwrap();
    }

    /// Makes an archive from a list of entries in `work`, and gives the result of a restore with small limits.
    fn restore_of(work: &Path, entries: &[Entry]) -> (Result<BackupInfo, VaultError>, PathBuf) {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, kind, data) in entries {
            raw_entry(&mut builder, name, *kind, data);
        }
        let archive = work.join("a.backup");
        fs::write(&archive, builder.into_inner().unwrap()).unwrap();
        let dir = work.join("panim");
        let limits = Limits {
            archive: 10240,
            file: 3000,
            files: 3,
        };
        (restore_with(&archive, &dir, limits), dir)
    }

    #[test]
    fn a_hostile_or_damaged_archive_is_refused() {
        let real = tempfile::tempdir().unwrap();
        let vault = new_vault(real.path());
        let header = fs::read(real.path().join(HEADER)).unwrap();
        let mark = format!("{BACKUP_FORMAT}\n").into_bytes();
        let sealed = |n: usize| {
            let mut data = FILE_MAGIC.to_vec();
            data.resize(n, 7);
            data
        };
        let name = b"data/0123456789abcdef0123456789abcdef";
        let other = b"data/fedcba9876543210fedcba9876543210";
        let file = tar::EntryType::Regular;
        let good: Vec<Entry> = vec![
            (b"panim-backup", file, mark.clone()),
            (b"vault.json", file, header.clone()),
            (name, file, sealed(100)),
        ];
        let work = tempfile::tempdir().unwrap();
        let (result, dir) = restore_of(work.path(), &good);
        assert!(result.is_ok(), "{result:?}");
        assert!(Vault::exists(&dir));
        drop(vault);

        let mut cases: Vec<(&str, Vec<Entry>)> = vec![
            ("empty", vec![]),
            ("no mark", good[1..].to_vec()),
            (
                "a mark of a different format",
                [
                    vec![(b"panim-backup".as_slice(), file, b"panim-backup/9\n".to_vec())],
                    good[1..].to_vec(),
                ]
                .concat(),
            ),
            ("no header", vec![good[0].clone(), good[2].clone()]),
            ("no sealed file", good[..2].to_vec()),
            (
                "a parent folder",
                [good.clone(), vec![(b"../x".as_slice(), file, sealed(10))]].concat(),
            ),
            (
                "a path in the data folder that leaves it",
                [good.clone(), vec![(b"data/../../x".as_slice(), file, sealed(10))]].concat(),
            ),
            (
                "an absolute path",
                [good.clone(), vec![(b"/tmp/panim-x".as_slice(), file, sealed(10))]].concat(),
            ),
            (
                "a link",
                [good.clone(), vec![(other.as_slice(), tar::EntryType::Symlink, vec![])]].concat(),
            ),
            (
                "a folder",
                [
                    good.clone(),
                    vec![(b"data/".as_slice(), tar::EntryType::Directory, vec![])],
                ]
                .concat(),
            ),
            (
                "a different file",
                [good.clone(), vec![(b"gate.json".as_slice(), file, b"{}".to_vec())]].concat(),
            ),
            ("two headers", [good.clone(), vec![good[1].clone()]].concat()),
            (
                "the same file two times",
                [good.clone(), vec![good[2].clone()]].concat(),
            ),
            (
                "a file that is too large",
                [good[..2].to_vec(), vec![(name.as_slice(), file, sealed(4000))]].concat(),
            ),
            (
                "a file that is not sealed",
                [good[..2].to_vec(), vec![(name.as_slice(), file, vec![1; 50])]].concat(),
            ),
            (
                "a header that is not a vault header",
                vec![good[0].clone(), (b"vault.json", file, b"{}".to_vec()), good[2].clone()],
            ),
        ];
        // Too many files, and an archive that is too large.
        let names: Vec<Vec<u8>> = (0..4).map(|i| format!("data/{i:032x}").into_bytes()).collect();
        let mut many = good[..2].to_vec();
        for name in &names {
            many.push((name.as_slice(), file, sealed(10)));
        }
        cases.push(("too many files", many));
        let mut large = good[..2].to_vec();
        for name in &names[..3] {
            large.push((name.as_slice(), file, sealed(2900)));
        }
        cases.push(("too many bytes", large));

        for (what, entries) in cases {
            let work = tempfile::tempdir().unwrap();
            let (result, dir) = restore_of(work.path(), &entries);
            assert!(
                matches!(result, Err(VaultError::BadBackup(_)) | Err(VaultError::Damaged(_))),
                "{what}: {result:?}"
            );
            assert!(!Vault::exists(&dir), "{what}: a failed restore leaves no vault");
            assert!(staging_folders(&dir).is_empty(), "{what}: the staging folder goes away");
            assert!(!dir.parent().unwrap().join("x").exists(), "{what}");
            assert!(!Path::new("/tmp/panim-x").exists(), "{what}");
        }

        // A file that is not an archive.
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("a.backup");
        fs::write(&archive, b"this is not an archive").unwrap();
        assert!(restore_backup(&archive, &work.path().join("panim")).is_err());
        assert!(!Vault::exists(&work.path().join("panim")));
    }
}
