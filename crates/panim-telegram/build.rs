//! The build script of the Telegram connector. It does two tasks.
//!
//! 1. It finds `libtdjson` through pkg-config (`tdjson.pc`) and tells the linker where the library is. The dev
//!    shell of `flake.nix` supplies the TDLib of PANIM, with the patch `nix/tdlib-no-direct.patch`. Without Nix,
//!    `scripts/build-tdlib.sh` builds the same TDLib with the same patch. Then `PKG_CONFIG_PATH` must contain its
//!    `lib/pkgconfig` folder.
//!
//!    The script also sets the run path, so that the program finds the library without `LD_LIBRARY_PATH`. By
//!    default, the run path is the folder of the library. `PANIM_TDJSON_RPATH` replaces it, for example with
//!    `$ORIGIN/../lib/panim` for a package (`scripts/build-packages.sh`). The value is a list with `:` between the
//!    entries. Cargo gives the run path to the crate `panim` as `DEP_TDJSON_RPATH` (the key `links` in
//!    `Cargo.toml`), because a link argument of this crate does not go to the binaries of other crates.
//! 2. It reads the `api_id` and the `api_hash` of PANIM at compile time and writes them to a Rust file in `OUT_DIR`.
//!    The environment has priority (`PANIM_TELEGRAM_API_ID`, `PANIM_TELEGRAM_API_HASH`). If a value is not there,
//!    the script reads `~/.config/panim/telegram.env`. `PANIM_TELEGRAM_ENV` can name a different file. An empty
//!    name means "no file".
//!
//! The script does not print the values. It does not use `cargo:rustc-env`, because Cargo keeps those lines in the
//! `output` file of the build script and shows them with `cargo build -vv`.
//!
//! Without the values, the build passes with one warning. The connector then answers "Telegram is not configured".

use std::path::PathBuf;
use std::{env, fs};

const ID: &str = "PANIM_TELEGRAM_API_ID";
const HASH: &str = "PANIM_TELEGRAM_API_HASH";
const FILE: &str = "PANIM_TELEGRAM_ENV";
const RPATH: &str = "PANIM_TDJSON_RPATH";

fn main() {
    let library = pkg_config::Config::new()
        .atleast_version("1.8.68")
        .probe("tdjson")
        .expect(
            "pkg-config must find tdjson (`tdjson.pc`). The shell of `nix develop` supplies it. Without Nix, run \
             `scripts/build-tdlib.sh`, and add its `lib/pkgconfig` folder to `PKG_CONFIG_PATH`",
        );
    // Put the run path into the test binaries of this crate. Thus, they find `libtdjson` without
    // `LD_LIBRARY_PATH`. The compiler wrapper of Nix does the same for a path in the Nix store.
    println!("cargo:rerun-if-env-changed={RPATH}");
    let rpath = match env::var(RPATH) {
        Ok(rpath) if !rpath.is_empty() => rpath,
        _ => library
            .link_paths
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(":"),
    };
    for entry in rpath.split(':').filter(|entry| !entry.is_empty()) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{entry}");
    }
    // The crate `panim` reads this value as `DEP_TDJSON_RPATH`.
    println!("cargo:rpath={rpath}");

    println!("cargo:rerun-if-env-changed={ID}");
    println!("cargo:rerun-if-env-changed={HASH}");
    println!("cargo:rerun-if-env-changed={FILE}");
    let file = env_file();
    if let Some(file) = &file {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let text = file.and_then(|file| fs::read_to_string(file).ok()).unwrap_or_default();
    let id = value(ID, &text)
        .and_then(|id| id.parse::<i32>().ok())
        .filter(|id| *id > 0);
    let hash = value(HASH, &text).filter(|hash| hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit()));

    let code = match (id, hash) {
        (Some(id), Some(hash)) => format!("const TELEGRAM_APP: Option<(i32, &str)> = Some(({id}, \"{hash}\"));\n"),
        _ => {
            println!("cargo:warning=Telegram is not configured: {ID} and {HASH} are not set");
            "const TELEGRAM_APP: Option<(i32, &str)> = None;\n".to_string()
        }
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("telegram_app.rs");
    fs::write(out, code).expect("write telegram_app.rs");
}

/// The file with the values.
fn env_file() -> Option<PathBuf> {
    if let Some(file) = env::var_os(FILE) {
        return (!file.is_empty()).then(|| PathBuf::from(file));
    }
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("panim/telegram.env"))
}

/// The value of a name. The environment has priority. The file has lines of the form `NAME=value`.
fn value(name: &str, file: &str) -> Option<String> {
    if let Ok(value) = env::var(name) {
        return Some(value);
    }
    file.lines().find_map(|line| {
        let line = line.trim();
        let (key, value) = line.strip_prefix("export ").unwrap_or(line).split_once('=')?;
        (key.trim() == name).then(|| value.trim().trim_matches(['"', '\'']).to_string())
    })
}
