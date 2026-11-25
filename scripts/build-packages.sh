#!/usr/bin/env bash
# Build the packages of PANIM without Nix, with the bundler of Tauri (`docs/ARCHITECTURE.md` §11).
#
# - Linux: a `.deb`, an `.rpm` and an AppImage (`crates/app/src-tauri/bundle/linux.json`).
# - macOS: an `.app` and a `.dmg` (`crates/app/src-tauri/bundle/macos.json`). Only a macOS runner of GitHub made
#   them (`docs/ROADMAP.md` §10).
#
# Each package has the TDLib of PANIM from `scripts/build-tdlib.sh`, in its default prefix `target/tdlib`. Run that
# script first. The bundler configuration names this path, so a different prefix does not operate here.
#
# The binary finds `libtdjson` through a run path that is relative to the binary:
#
# - Linux: `$ORIGIN/../lib/panim`. The `.deb` and the `.rpm` put the library into `/usr/lib/panim`, next to
#   `/usr/bin/panim`. The AppImage is different: `linuxdeploy` of the bundler copies the library into `usr/lib` and
#   changes the run path to `$ORIGIN/../lib`. It finds the library through `LD_LIBRARY_PATH`, which this script sets.
# - macOS: `@executable_path/../Frameworks`. The bundler copies the library into `Contents/Frameworks`.
#
# Thus, the binary in `target/release` does not find the library before it is in a package. For a development
# build, use `cargo build` or `pnpm tauri build --no-bundle` without this script.
#
# Usage: scripts/build-packages.sh [more arguments for `tauri build`]
#
# Run `pnpm install` in `crates/app` first. The bundler runs `pnpm build` itself. The packages are in
# `target/release/bundle`. The build reads the Telegram app values as each build does (`CONTRIBUTING.md`). To
# build without them, set `PANIM_TELEGRAM_ENV` to an empty text.

set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tdlib="$root/target/tdlib"

case "$(uname -s)" in
  Linux)
    library="$tdlib/lib/libtdjson.so.1.8.68"
    config=src-tauri/bundle/linux.json
    # The run path has the text `$ORIGIN`, not a shell variable.
    # shellcheck disable=SC2016
    rpath='$ORIGIN/../lib/panim'
    ;;
  Darwin)
    library="$tdlib/lib/libtdjson.1.8.68.dylib"
    config=src-tauri/bundle/macos.json
    rpath='@executable_path/../Frameworks'
    # The minimum of PANIM is macOS 14 (`DISCUSSION.md` §51), the same value as `minimumSystemVersion` in
    # `macos.json`. Without it, rustc and clang use the version of the build computer.
    export MACOSX_DEPLOYMENT_TARGET=14.0
    ;;
  *)
    echo "The script supports only Linux and macOS." >&2
    exit 1
    ;;
esac

if [ ! -f "$library" ]; then
  echo "$library does not exist. Run scripts/build-tdlib.sh first." >&2
  exit 1
fi

export PKG_CONFIG_PATH="$tdlib/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export PANIM_TDJSON_RPATH="$rpath"
if [ "$(uname -s)" = Linux ]; then
  # `linuxdeploy` finds `libtdjson` for the AppImage here. Without it, the run path `$ORIGIN/../lib/panim` does not
  # find the library, because the AppImage has no copy in `usr/lib/panim`.
  export LD_LIBRARY_PATH="$tdlib/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

cd "$root/crates/app"
pnpm tauri build --config "$config" "$@"
