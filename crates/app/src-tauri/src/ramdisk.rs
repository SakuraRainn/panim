//! The memory area on macOS: a RAM disk (`docs/DISCUSSION.md` §53).
//!
//! macOS has no `$XDG_RUNTIME_DIR` on `tmpfs`. Thus, at the start, PANIM makes a RAM disk and mounts it at a private
//! folder. The profile area of the vault is on it (`ProfileArea::at`).
//!
//! - The base folder is `$TMPDIR/panim-<16 hexadecimal digits>`, with mode 700. `$TMPDIR` is a folder of the user.
//!   The digits come from the path of the data folder. Thus, each data folder gets its own RAM disk.
//! - `area` in the base folder is the mount point. The volume has "Owners: Disabled", so only the base folder stops
//!   other users.
//! - `device` in the base folder holds the device of the RAM disk (for example `/dev/disk5`) for the detach.
//! - After a crash, the RAM disk stays mounted. The next start uses it again.
//! - At the exit, after the lock seals the folders, `detach` removes the RAM disk.
//!
//! The functions that parse the output of the tools compile on all systems, so that the tests on Linux examine them.
//! The functions that run `hdiutil` and `diskutil` compile only on macOS. Not tested on a Mac yet
//! (`docs/ROADMAP.md` §10).

// On Linux, only the tests use most of this module.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// The size of the RAM disk in sectors of 512 bytes: 2 GiB.
pub const SECTORS: u64 = 2 * 1024 * 1024 * 1024 / 512;

/// The name of the APFS volume. A second volume with the same name gets a different folder in `/Volumes`, but PANIM
/// finds the volume from the output of `diskutil` (`volume_from_erase`).
const VOLUME_NAME: &str = "PANIM";

const HDIUTIL: &str = "/usr/bin/hdiutil";
const DISKUTIL: &str = "/usr/sbin/diskutil";

/// The maximum time of one command of `hdiutil` or `diskutil`.
const COMMAND_TIME: Duration = Duration::from_secs(120);

/// The base folder of the RAM disk of a data folder: `<temp>/panim-<16 hexadecimal digits>`.
///
/// The digits are the first 8 bytes of SHA-256 of the bytes of the path of the data folder.
pub fn base_folder(temp: &Path, data: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    let digest = Sha256::digest(data.as_os_str().as_bytes());
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    temp.join(format!("panim-{hex}"))
}

/// The device in the output of `hdiutil attach -nomount ram://...`: the first word of the first line.
pub fn device_from_attach(output: &str) -> Option<String> {
    let word = output.lines().next()?.split_whitespace().next()?;
    let name = word.strip_prefix("/dev/")?;
    is_whole_disk(name).then(|| word.to_string())
}

/// The APFS volume in the output of `diskutil erasevolume APFS`, for example `disk6s1`.
///
/// `diskutil` writes the line "Created new APFS Volume disk6s1", and also the line "Disk from APFS operation:
/// disk6s1". The function takes the last such line.
pub fn volume_from_erase(output: &str) -> Option<String> {
    output
        .lines()
        .rev()
        .filter(|line| line.contains("APFS Volume") || line.contains("Disk from APFS operation"))
        .filter_map(|line| line.split_whitespace().next_back())
        .map(|word| word.trim_end_matches('.'))
        .find(|word| is_volume(word))
        .map(str::to_string)
}

/// The values of a string key in an XML property list (`diskutil info -plist`), in the order of the text.
///
/// The function finds each `<key>KEY</key>` and takes the `<string>` after it. It is not a full parser, but the
/// output of `diskutil` has this form.
pub fn plist_strings(text: &str, key: &str) -> Vec<String> {
    let mark = format!("<key>{key}</key>");
    let mut values = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(&mark) {
        rest = &rest[at + mark.len()..];
        if let Some(value) = rest
            .trim_start()
            .strip_prefix("<string>")
            .and_then(|tail| tail.split_once("</string>"))
            .map(|(value, _)| value)
        {
            values.push(value.to_string());
        }
    }
    values
}

/// True for a device name such as `disk5`.
fn is_whole_disk(name: &str) -> bool {
    name.strip_prefix("disk")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// True for a volume name such as `disk6s1`.
fn is_volume(name: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    name.strip_prefix("disk")
        .and_then(|rest| rest.split_once('s'))
        .is_some_and(|(disk, slice)| digits(disk) && digits(slice))
}

/// True if `path` is a mount point: its device is not the device of its parent folder.
pub fn is_mount_point(path: &Path) -> io::Result<bool> {
    let parent = path.parent().unwrap_or(path);
    Ok(fs::metadata(path)?.dev() != fs::metadata(parent)?.dev())
}

/// Makes a folder with mode 700, or sets mode 700 if the folder exists.
pub fn private_folder(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

/// Runs a program and gives its standard output. A program that does not stop in `limit` is killed.
///
/// Two threads read the output, so that a long output does not stop the program.
pub fn run(program: &str, args: &[&str], limit: Duration) -> Result<String, String> {
    let text = format!("{program} {}", args.join(" "));
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{text}: {e}"))?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{text}: no result after {} s", limit.as_secs()));
            }
            Err(e) => return Err(format!("{text}: {e}")),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if status.success() {
        Ok(out)
    } else {
        Err(format!("{text}: {status}: {}", err.trim()))
    }
}

/// The RAM disk of the memory area.
pub struct RamDisk {
    base: PathBuf,
    /// The device for the detach, for example `/dev/disk5`. `None` if it is not known or after the detach.
    device: Option<String>,
}

impl RamDisk {
    /// The mount point of the RAM disk.
    pub fn area(&self) -> PathBuf {
        self.base.join("area")
    }

    fn device_file(&self) -> PathBuf {
        self.base.join("device")
    }
}

#[cfg(target_os = "macos")]
impl RamDisk {
    /// Makes and mounts the RAM disk of the data folder `data`, or uses the RAM disk that is mounted after a crash.
    pub fn start(data: &Path) -> Result<Self, String> {
        let base = base_folder(&std::env::temp_dir(), data);
        private_folder(&base).map_err(|e| format!("{}: {e}", base.display()))?;
        let mut disk = Self { base, device: None };
        let area = disk.area();
        if area.exists() && is_mount_point(&area).map_err(|e| format!("{}: {e}", area.display()))? {
            disk.device = fs::read_to_string(disk.device_file())
                .ok()
                .and_then(|text| device_from_attach(&text))
                .or_else(|| store_of(&area.to_string_lossy()));
            return Ok(disk);
        }
        private_folder(&area).map_err(|e| format!("{}: {e}", area.display()))?;
        let attach = run(
            HDIUTIL,
            &["attach", "-nomount", &format!("ram://{SECTORS}")],
            COMMAND_TIME,
        )?;
        let device =
            device_from_attach(&attach).ok_or_else(|| format!("hdiutil attach gave no device: {:?}", attach.trim()))?;
        disk.device = Some(device.clone());
        if let Err(error) = disk.prepare(&device) {
            let _ = run(HDIUTIL, &["detach", "-force", &device], COMMAND_TIME);
            let _ = fs::remove_file(disk.device_file());
            return Err(error);
        }
        Ok(disk)
    }

    /// Makes the APFS volume on the attached device, and mounts it at the area with `nobrowse`.
    fn prepare(&self, device: &str) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let file = self.device_file();
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| f.write_all(format!("{device}\n").as_bytes()))
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let erase = run(DISKUTIL, &["erasevolume", "APFS", VOLUME_NAME, device], COMMAND_TIME)?;
        let volume = match volume_from_erase(&erase) {
            Some(volume) => volume,
            // The method of the check on the macOS runner (`spikes/macos-webview/ramdisk.sh`).
            None => {
                let info = run(
                    DISKUTIL,
                    &["info", "-plist", &format!("/Volumes/{VOLUME_NAME}")],
                    COMMAND_TIME,
                )?;
                plist_strings(&info, "DeviceIdentifier")
                    .into_iter()
                    .next()
                    .filter(|v| is_volume(v))
                    .ok_or_else(|| format!("diskutil erasevolume gave no volume: {:?}", erase.trim()))?
            }
        };
        // Make sure that the volume is on the new device, and not on a different volume with the same name.
        let store = store_of(&volume);
        if store.as_deref().is_some_and(|store| store != device) {
            return Err(format!("the volume {volume} is not on {device}"));
        }
        run(DISKUTIL, &["unmount", &volume], COMMAND_TIME)?;
        let area = self.area();
        let area_text = area.to_string_lossy();
        run(
            DISKUTIL,
            &["mount", "-mountOptions", "nobrowse", "-mountPoint", &area_text, &volume],
            COMMAND_TIME,
        )?;
        if !is_mount_point(&area).map_err(|e| format!("{}: {e}", area.display()))? {
            return Err(format!("the volume {volume} is not mounted at {area_text}"));
        }
        // The volume has no owners, so the mode is not a protection. The base folder with mode 700 is.
        let _ = fs::set_permissions(&area, fs::Permissions::from_mode(0o700));
        Ok(())
    }

    /// Detaches the RAM disk. Call it only after the lock sealed the folders in the area.
    pub fn detach(&mut self) -> Result<(), String> {
        let device = self
            .device
            .take()
            .ok_or_else(|| "the device of the RAM disk is not known".to_string())?;
        run(HDIUTIL, &["detach", &device], COMMAND_TIME)
            .or_else(|_| run(HDIUTIL, &["detach", "-force", &device], COMMAND_TIME))?;
        let _ = fs::remove_file(self.device_file());
        Ok(())
    }
}

/// The device of the APFS store of a volume or a mount point, for example `/dev/disk5`. `None` if `diskutil info`
/// does not tell it.
#[cfg(target_os = "macos")]
fn store_of(volume: &str) -> Option<String> {
    let info = run(DISKUTIL, &["info", "-plist", volume], COMMAND_TIME).ok()?;
    plist_strings(&info, "APFSPhysicalStore")
        .into_iter()
        .find(|store| is_whole_disk(store))
        .map(|store| format!("/dev/{store}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_is_2_gib() {
        assert_eq!(SECTORS, 4_194_304);
    }

    #[test]
    fn each_data_folder_gets_its_own_base_folder() {
        let temp = Path::new("/var/folders/xy/T");
        let one = base_folder(temp, Path::new("/Users/a/Library/Application Support/panim"));
        assert_eq!(
            one,
            base_folder(temp, Path::new("/Users/a/Library/Application Support/panim"))
        );
        assert_ne!(one, base_folder(temp, Path::new("/Users/a/other")));
        assert_eq!(one.parent(), Some(temp));
        let name = one.file_name().unwrap().to_str().unwrap();
        let hex = name.strip_prefix("panim-").unwrap();
        assert_eq!(hex.len(), 16);
        assert!(hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    }

    #[test]
    fn the_device_is_the_first_word_of_the_attach() {
        let out = "/dev/disk5          \t                               \t\n";
        assert_eq!(device_from_attach(out).as_deref(), Some("/dev/disk5"));
        assert_eq!(device_from_attach("/dev/disk12\n").as_deref(), Some("/dev/disk12"));
        assert_eq!(device_from_attach(""), None);
        assert_eq!(device_from_attach("hdiutil: attach failed\n"), None);
        assert_eq!(device_from_attach("/dev/disk5s1\n"), None);
        assert_eq!(device_from_attach("/dev/\n"), None);
    }

    #[test]
    fn the_volume_comes_from_the_output_of_erasevolume() {
        let out = "Started erase on disk5\n\
                   Unmounting disk\n\
                   Creating the partition map\n\
                   Creating a new empty APFS Container\n\
                   Unmounting Volumes\n\
                   Switching disk5 to APFS\n\
                   Creating APFS Container\n\
                   Created new APFS Container disk6\n\
                   Preparing APFS Container disk6 for addition of the new APFS Volume\n\
                   Creating APFS Volume\n\
                   Created new APFS Volume disk6s1\n\
                   Mounting APFS Volume\n\
                   Setting volume permissions\n\
                   Disk from APFS operation: disk6s1\n\
                   Finished APFS operation on disk5\n\
                   Finished erase on disk5\n";
        assert_eq!(volume_from_erase(out).as_deref(), Some("disk6s1"));
        assert_eq!(
            volume_from_erase("Created new APFS Volume disk7s1.\n").as_deref(),
            Some("disk7s1")
        );
        assert_eq!(
            volume_from_erase("Started erase on disk5\nFinished erase on disk5\n"),
            None
        );
        assert_eq!(
            volume_from_erase("Preparing APFS Container disk6 for the new APFS Volume\n"),
            None
        );
    }

    #[test]
    fn the_names_of_disks_and_volumes() {
        assert!(is_whole_disk("disk5"));
        assert!(!is_whole_disk("disk"));
        assert!(!is_whole_disk("disk5s1"));
        assert!(is_volume("disk6s1"));
        assert!(is_volume("disk12s10"));
        assert!(!is_volume("disk6"));
        assert!(!is_volume("disks1"));
        assert!(!is_volume("disk6s"));
        assert!(!is_volume("Volume"));
    }

    #[test]
    fn the_plist_gives_the_strings_of_a_key() {
        let text = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
	<key>APFSPhysicalStores</key>
	<array>
		<dict>
			<key>APFSPhysicalStore</key>
			<string>disk5</string>
		</dict>
	</array>
	<key>DeviceIdentifier</key>
	<string>disk6s1</string>
	<key>Writable</key>
	<true/>
</dict>
</plist>"#;
        assert_eq!(plist_strings(text, "APFSPhysicalStore"), ["disk5"]);
        assert_eq!(plist_strings(text, "DeviceIdentifier"), ["disk6s1"]);
        assert!(plist_strings(text, "Writable").is_empty());
        assert!(plist_strings(text, "MountPoint").is_empty());
    }

    #[test]
    fn a_folder_on_the_same_device_is_not_a_mount_point() {
        let temp = tempfile::tempdir().unwrap();
        let area = temp.path().join("base/area");
        private_folder(&area).unwrap();
        assert!(!is_mount_point(&area).unwrap());
        assert_eq!(fs::metadata(&area).unwrap().permissions().mode() & 0o777, 0o700);
        fs::set_permissions(&area, fs::Permissions::from_mode(0o755)).unwrap();
        private_folder(&area).unwrap();
        assert_eq!(fs::metadata(&area).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn run_gives_the_output_or_the_error() {
        let limit = Duration::from_secs(10);
        assert_eq!(
            run("/bin/sh", &["-c", "echo /dev/disk5"], limit).unwrap(),
            "/dev/disk5\n"
        );
        let error = run("/bin/sh", &["-c", "echo bad >&2; exit 3"], limit).unwrap_err();
        assert!(error.contains("bad"), "{error}");
        assert!(run("/no/such/program", &[], limit).is_err());
        let error = run("/bin/sh", &["-c", "sleep 5"], Duration::from_millis(200)).unwrap_err();
        assert!(error.contains("no result"), "{error}");
    }
}
