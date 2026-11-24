#!/usr/bin/env bash
# The check of the build without Nix, in containers (`docs/ARCHITECTURE.md` §11). It needs Docker or rootless Podman.
#
# Usage: docker/check.sh [build | install | all] [--out DIR] [--values FILE]
#
# - `build` makes the image of `docker/ubuntu.Dockerfile` (Ubuntu 24.04, rustup, Node.js, pnpm, the TDLib of PANIM).
#   In a container of this image, it copies the repository, and it does the steps of `AGENTS.md` §6 and
#   `scripts/build-packages.sh`. Then it examines the binary and `libtdjson` for paths of Nix, and copies the
#   packages and a report to the output folder.
# - `install` installs the `.deb` in a clean Ubuntu 24.04 and the `.rpm` in a clean Fedora. It unpacks the
#   AppImage in Ubuntu. In each case, it examines that the binary finds all its libraries, and it starts PANIM for
#   15 seconds in Xvfb.
# - `all` (the default) does `build` and then `install`.
# - `--values FILE` is the release mode (`CONTRIBUTING.md`, "Make a release"). It applies to `build` and `all`. FILE
#   has the Telegram app values of PANIM (`PANIM_TELEGRAM_API_ID` and `PANIM_TELEGRAM_API_HASH`), normally
#   `~/.config/panim/telegram.env`. FILE must be outside the repository. Before the build, the script examines the
#   repository: it must have no changes, `HEAD` must have the tag `v<version>`, and this version must be the version
#   of `tauri.conf.json`. After the build, the script makes sure that the binary has the values.
#
# The default output folder is `target/container` of the repository. The container reads the repository only. It
# does not write into it, except the output folder. Without `--values`, the build has no Telegram app values
# (`PANIM_TELEGRAM_ENV` is empty), and the containers never read `~/.config/panim`. In the release mode, the build
# container reads only FILE, read-only. The engine mounts it when the container starts. FILE is not in the image, and
# the values are not in the environment of the container. The script, the report and the logs never show the values.
#
# Set FEDORA_IMAGE to use a different Fedora image (default `docker.io/library/fedora:latest`).
# Set TDLIB_JOBS to the number of jobs of the TDLib build. Each job can need approximately 2 GB of memory.
# Set CONTAINER_ENGINE to `docker` or `podman`. By default, the script uses `docker` if it is available, else `podman`.

set -euo pipefail

IMAGE=localhost/panim-build-ubuntu
UBUNTU_IMAGE=docker.io/library/ubuntu:24.04
FEDORA_IMAGE=${FEDORA_IMAGE:-docker.io/library/fedora:latest}
if [ -z "${CONTAINER_ENGINE:-}" ]; then
  if command -v docker > /dev/null; then CONTAINER_ENGINE=docker; else CONTAINER_ENGINE=podman; fi
fi
engine() { "$CONTAINER_ENGINE" "$@"; }
# The place of the file with the Telegram app values in the build container, in the release mode.
SECRETS=/run/panim-secrets/telegram.env

# The steps in the container of `build`. The repository is in /src (read only).
inside_build() {
  mkdir -p /work
  tar -C /src --exclude=./.git --exclude=./target --exclude=./result --exclude=./crates/app/node_modules \
    --exclude=./crates/app/dist --exclude=./crates/app/src-tauri/gen -cf - . | tar -C /work -xf -
  cd /work
  # The build reads the Telegram app values only from the mounted file, and only in the release mode.
  unset PANIM_TELEGRAM_API_ID PANIM_TELEGRAM_API_HASH
  local release=false
  if [ "${PANIM_RELEASE:-}" = 1 ]; then
    release=true
    if [ ! -f "$SECRETS" ] || [ ! -r "$SECRETS" ]; then
      echo "FAIL: the release mode needs the file with the values at $SECRETS" >&2
      return 1
    fi
    export PANIM_TELEGRAM_ENV="$SECRETS"
  else
    export PANIM_TELEGRAM_ENV=""
  fi
  export PKG_CONFIG_PATH=/work/target/tdlib/lib/pkgconfig
  PANIM_DATA=$(mktemp -d)
  XDG_RUNTIME_DIR=$(mktemp -d)
  export PANIM_DATA XDG_RUNTIME_DIR
  chmod 700 "$XDG_RUNTIME_DIR"
  report=/out/report.txt
  : > "$report"
  log() { echo "== $*" | tee -a "$report"; }

  log "Release mode (the build with the Telegram app values): $release"
  log "Tools"
  { rustc --version; cargo --version; node --version; pnpm --version; cmake --version | head -1; } | tee -a "$report"
  # No tool of the build comes from Nix.
  test ! -e /nix

  log "pnpm install, pnpm build, pnpm check"
  (cd crates/app && pnpm install --frozen-lockfile && pnpm build && pnpm check)

  log "cargo fmt --all --check"
  cargo fmt --all --check
  log "cargo clippy --all-targets -- -D warnings"
  cargo clippy --all-targets -- -D warnings
  log "cargo test"
  cargo test 2>&1 | tee /tmp/test.log
  grep -E '^test result:' /tmp/test.log >> "$report"
  awk '/^test result:/ { p += $4; f += $6; i += $8 } END { printf "Total: %d passed, %d failed, %d ignored\n", p, f, i }' \
    /tmp/test.log | tee -a "$report"

  log "scripts/build-packages.sh"
  scripts/build-packages.sh 2>&1 | tee /tmp/packages.log
  if [ "$release" = true ]; then
    check_values "$report"
  fi

  log "readelf -d target/release/panim (NEEDED tdjson and RUNPATH)"
  readelf -d target/release/panim | grep -E 'tdjson|RUNPATH|RPATH' | tee -a "$report"
  log "readelf -d target/tdlib/lib/libtdjson.so.1.8.68 (NEEDED, SONAME, RUNPATH)"
  readelf -d target/tdlib/lib/libtdjson.so.1.8.68 | grep -E 'NEEDED|SONAME|RUNPATH|RPATH' | tee -a "$report"
  log "Lines with /nix/store in the binary and in libtdjson (each count must be 0)"
  for file in target/release/panim target/tdlib/lib/libtdjson.so.1.8.68; do
    echo "$(grep -c -a '/nix/store' "$file" || true) $file" | tee -a "$report"
  done

  log "Packages"
  mkdir -p /out/packages
  cp target/release/bundle/deb/*.deb target/release/bundle/rpm/*.rpm target/release/bundle/appimage/*.AppImage \
    /out/packages/
  (cd /out/packages && sha256sum ./* | tee -a "$report")
  log "Files of the .deb"
  dpkg-deb -c /out/packages/*.deb | tee -a "$report"
  log "Depends of the .deb"
  dpkg-deb -f /out/packages/*.deb Depends | tee -a "$report"
}

# In the release mode, make sure that the build used the Telegram app values. Show no value. $1 is the report.
check_values() {
  local report=$1 count
  echo "== The Telegram app values in the build (the values are not shown)" | tee -a "$report"
  # `crates/panim-telegram/build.rs` gives this warning when it has no correct values.
  if grep -q 'Telegram is not configured' /tmp/packages.log; then
    echo "FAIL: the build warning \"Telegram is not configured\" is in the output" | tee -a "$report"
    return 1
  fi
  echo "The build warning \"Telegram is not configured\": no" | tee -a "$report"
  # The build script writes the `api_hash` as a string literal into the code of `panim-telegram`, so the binary has
  # it as plain text. The subshell reads the file and counts the lines of the binary with the hash. Only the count
  # comes out of the subshell. An empty hash or a hash with a wrong form gives 0, because `grep -F ""` finds each
  # line.
  count=$(
    set +u
    # shellcheck disable=SC1090
    . "$SECRETS" > /dev/null 2>&1
    api_hash=${PANIM_TELEGRAM_API_HASH:-}
    api_hash=${api_hash%$'\r'}
    if [[ "$api_hash" =~ ^[0-9a-fA-F]{32}$ ]]; then
      # `printf` is a builtin, so the hash is not in the arguments of a process.
      grep -c -a -F -f <(printf '%s\n' "$api_hash") target/release/panim || true
    else
      echo 0
    fi
  ) || count=0
  if [[ "$count" =~ ^[0-9]+$ ]] && [ "$count" -gt 0 ]; then
    echo "The binary has the values: yes" | tee -a "$report"
  else
    echo "The binary has the values: no" | tee -a "$report"
    echo "FAIL: target/release/panim does not have the api_hash of the file" | tee -a "$report"
    return 1
  fi
}

# Examine the libraries of a binary, and start it for 15 seconds in Xvfb. $1 is the binary.
check_binary() {
  local binary=$1
  echo "== ldd $binary"
  ldd "$binary" | tee /tmp/ldd.txt | grep -E 'tdjson|not found' || true
  if grep -q 'not found' /tmp/ldd.txt; then
    echo "FAIL: a library is missing"
    return 1
  fi
  if grep -q '/nix/store' /tmp/ldd.txt; then
    echo "FAIL: a library comes from /nix/store"
    return 1
  fi
  echo "== Start $binary in Xvfb for 15 seconds"
  local data runtime status=0
  data=$(mktemp -d)
  runtime=$(mktemp -d)
  chmod 700 "$runtime"
  PANIM_DATA=$data XDG_RUNTIME_DIR=$runtime timeout 15 xvfb-run -a dbus-run-session -- "$binary" \
    > /tmp/start.log 2>&1 || status=$?
  tail -n 20 /tmp/start.log
  # 124: the program still ran when `timeout` stopped it.
  if [ "$status" = 124 ]; then
    echo "PASS: PANIM ran for 15 seconds"
  else
    echo "FAIL: PANIM stopped with the status $status"
    return 1
  fi
}

# The steps in a clean Ubuntu container: the .deb and the AppImage.
inside_ubuntu() {
  export DEBIAN_FRONTEND=noninteractive
  apt-get update > /dev/null
  apt-get install -y --no-install-recommends /out/packages/*.deb xvfb xauth dbus binutils > /tmp/apt.log
  echo "== Installed the .deb"
  check_binary /usr/bin/panim
  echo "== Unpack the AppImage"
  cd /tmp
  cp /out/packages/*.AppImage panim.AppImage
  ./panim.AppImage --appimage-extract > /dev/null
  find squashfs-root -name 'libtdjson*' | tee /tmp/tdjson.txt
  # The AppImage must have one copy of the library, in `usr/lib`.
  if [ "$(wc -l < /tmp/tdjson.txt)" != 1 ] || ! grep -qx 'squashfs-root/usr/lib/libtdjson.so.1.8.68' /tmp/tdjson.txt; then
    echo "FAIL: the AppImage must have libtdjson one time, in usr/lib"
    return 1
  fi
  echo "Files in the AppImage with /nix/store: $(grep -rla '/nix/store' squashfs-root | wc -l)"
  readelf -d squashfs-root/usr/bin/panim | grep -E 'RUNPATH|RPATH'
  ldd squashfs-root/usr/bin/panim | grep -E 'tdjson|not found' || true
  if ldd squashfs-root/usr/bin/panim | grep -q 'not found'; then
    echo "FAIL: a library of the AppImage is missing"
    return 1
  fi
  echo "== Start the AppImage (unpacked) in Xvfb for 15 seconds"
  local data runtime status=0
  data=$(mktemp -d)
  runtime=$(mktemp -d)
  chmod 700 "$runtime"
  PANIM_DATA=$data XDG_RUNTIME_DIR=$runtime timeout 15 xvfb-run -a dbus-run-session -- squashfs-root/AppRun \
    > /tmp/start.log 2>&1 || status=$?
  tail -n 20 /tmp/start.log
  if [ "$status" = 124 ]; then echo "PASS: the AppImage ran for 15 seconds"; else echo "FAIL: status $status"; return 1; fi
}

# The steps in a clean Fedora container: the .rpm.
inside_fedora() {
  echo "== rpm -qpR"
  rpm -qpR /out/packages/*.rpm
  dnf install -y /out/packages/*.rpm xorg-x11-server-Xvfb xorg-x11-xauth dbus-daemon which > /tmp/dnf.log
  # shellcheck disable=SC1091
  echo "== Installed the .rpm on $(. /etc/os-release && echo "$PRETTY_NAME")"
  check_binary /usr/bin/panim
}

# Examine the conditions of the release mode on the host, before the engine starts. A release must come from a
# clean tag. $1 is the repository, and $2 is the file with the values (an absolute path).
check_release() {
  local root=$1 values=$2 status version tags ok=true
  if [ ! -f "$values" ] || [ ! -r "$values" ]; then
    echo "FAIL: $values does not exist, is not a regular file, or is not readable" >&2
    return 1
  fi
  case "$values" in
    *:*) echo "FAIL: the path $values has a \":\", and the engine cannot mount it" >&2; return 1 ;;
    "$(realpath -- "$root")"/*) echo "FAIL: the file with the values must be outside the repository" >&2; return 1 ;;
  esac
  status=$(git -C "$root" status --porcelain)
  if [ -n "$status" ]; then
    echo "FAIL: the repository has changes. A release must come from a clean tag:" >&2
    echo "$status" >&2
    ok=false
  fi
  version=$(sed -n 's/^  "version": *"\([^"]*\)".*/\1/p' "$root/crates/app/src-tauri/tauri.conf.json")
  if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "FAIL: crates/app/src-tauri/tauri.conf.json has no version of the form X.Y.Z" >&2
    return 1
  fi
  tags=$(git -C "$root" tag --points-at HEAD)
  if ! grep -qx -F -- "v$version" <<< "$tags"; then
    echo "FAIL: HEAD does not have the tag v$version (the version of tauri.conf.json)." >&2
    echo "The tags of HEAD: ${tags:-none}" >&2
    ok=false
  fi
  [ "$ok" = true ] || return 1
  echo "Release mode: the repository is clean, and HEAD has the tag v$version."
}

run_build() {
  local root=$1 out=$2 values=${3:-}
  local rust
  rust=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$root/rust-toolchain.toml")
  # The build context has only the three files that the image needs. Thus, `target/` and `node_modules/` do not go
  # to the engine, and the file `.dockerignore` is not necessary.
  local context
  context=$(mktemp -d)
  mkdir -p "$context/scripts" "$context/nix"
  cp "$root/docker/ubuntu.Dockerfile" "$context/Dockerfile"
  cp "$root/scripts/build-tdlib.sh" "$context/scripts/"
  cp "$root/nix/tdlib-no-direct.patch" "$context/nix/"
  engine build --build-arg RUST_VERSION="$rust" --build-arg TDLIB_JOBS="${TDLIB_JOBS:-}" -t "$IMAGE" "$context"
  rm -rf "$context"
  # Docker runs the container as the root of the host, so the files in the output folder must go back to the
  # person. Rootless Podman maps the root of the container to the person, so a `chown` is not necessary.
  local after=true
  if [ "$CONTAINER_ENGINE" = docker ]; then
    after="chown -R $(id -u):$(id -g) /out"
  fi
  # The release mode: the engine mounts the file with the values read-only, and gives a flag with no secret.
  local release=()
  if [ -n "$values" ]; then
    release=(-v "$values:$SECRETS:ro" -e PANIM_RELEASE=1)
  fi
  engine run --rm -v "$root:/src:ro" -v "$out:/out" ${release[@]+"${release[@]}"} "$IMAGE" \
    bash -c "trap '$after' EXIT; bash /src/docker/check.sh --inside-build" 2>&1 | tee "$out/build.log"
}

run_install() {
  local root=$1 out=$2 status=0
  echo "### Ubuntu 24.04: the .deb and the AppImage" | tee "$out/install.txt"
  engine run --rm -v "$root/docker/check.sh:/check.sh:ro" -v "$out:/out:ro" "$UBUNTU_IMAGE" \
    bash /check.sh --inside-ubuntu 2>&1 | tee -a "$out/install.txt" || status=1
  echo "### $FEDORA_IMAGE: the .rpm" | tee -a "$out/install.txt"
  engine run --rm -v "$root/docker/check.sh:/check.sh:ro" -v "$out:/out:ro" "$FEDORA_IMAGE" \
    bash /check.sh --inside-fedora 2>&1 | tee -a "$out/install.txt" || status=1
  if [ "$status" = 0 ]; then echo "### All install checks passed"; else echo "### An install check failed"; fi
  return "$status"
}

main() {
  local root out mode=all values=""
  root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
  out="$root/target/container"
  while [ $# -gt 0 ]; do
    case "$1" in
      build|install|all) mode=$1; shift ;;
      --out) out=$2; shift 2 ;;
      --values)
        if [ -z "${2:-}" ]; then
          echo "FAIL: --values needs a file" >&2
          return 2
        fi
        values=$2; shift 2 ;;
      --inside-build) inside_build; return ;;
      --inside-ubuntu) set -o pipefail; inside_ubuntu; return ;;
      --inside-fedora) inside_fedora; return ;;
      *) sed -n '/^# Usage:/s/^# //p' "$0" >&2; return 2 ;;
    esac
  done
  # The release mode: examine everything before the engine starts.
  if [ -n "$values" ]; then
    if [ "$mode" = install ]; then
      echo "FAIL: --values applies only to build and all" >&2
      return 2
    fi
    if [ ! -f "$values" ]; then
      echo "FAIL: $values does not exist or is not a regular file" >&2
      return 1
    fi
    values=$(realpath -- "$values")
    check_release "$root" "$values"
  fi
  mkdir -p "$out"
  out=$(cd "$out" && pwd)
  case "$mode" in
    build) run_build "$root" "$out" "$values" ;;
    install) run_install "$root" "$out" ;;
    all) run_build "$root" "$out" "$values" && run_install "$root" "$out" ;;
  esac
}

main "$@"
