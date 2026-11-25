#!/usr/bin/env bash
# Build the TDLib of PANIM without Nix (`docs/ARCHITECTURE.md` §11, "The build without Nix").
#
# On macOS, the script first builds a static OpenSSL 3.5.9 from source for macOS 14, and examines its objects.
# Then, on all the systems, the script does these steps:
#
# 1. It downloads the source of TDLib 1.8.68 at the commit of `nix/tdlib-no-direct.patch`.
# 2. It compares the SHA-256 of the archive with the value below. If the value is different, it stops.
# 3. It applies `nix/tdlib-no-direct.patch`. The Nix package applies the same file.
# 4. It builds only the shared library `tdjson` with CMake.
# 5. It installs the library, the two headers of the JSON interface and `tdjson.pc` into the prefix.
#
# The result is the same set of files as the Nix package `tdlib-panim`: no static libraries and no CMake files.
# The default prefix is `target/tdlib` in the repository. `scripts/build-packages.sh` uses this prefix.
#
# Usage: scripts/build-tdlib.sh [--prefix DIR] [--work DIR] [--jobs N]
#
# Then tell pkg-config where `tdjson.pc` is:
#
#   export PKG_CONFIG_PATH="$PWD/target/tdlib/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
#
# Linux needs a C++ compiler, CMake 3.10 or later, gperf, make, curl, and the headers of OpenSSL 3 and zlib.
# On Ubuntu: `apt install build-essential cmake gperf curl libssl-dev zlib1g-dev`.
#
# macOS: a macOS runner of GitHub ran these steps, but nobody ran them on a Mac. The script expects the Xcode
# command line tools and `brew install cmake gperf`. macOS supplies perl, make and zlib. The library is
# `libtdjson.1.8.68.dylib`, and its install name is `@rpath/libtdjson.1.8.68.dylib`. The library contains OpenSSL
# (static link), because the users of the `.app` do not have Homebrew. The script builds this OpenSSL from source,
# because the objects of the OpenSSL of Homebrew are for a newer macOS than 14 (`DISCUSSION.md` §52). After the
# build, the script uses `otool -L` to make sure that the library needs no file from Homebrew or Nix.

set -euo pipefail

# The commit of TDLib 1.8.68. `nix/tdlib-no-direct.patch` is for this commit, and nixpkgs uses the same commit.
TD_COMMIT=c15d3f5a5de6e3ba5839822c451152e5e18bb700
TD_VERSION=1.8.68
# The SHA-256 of the archive from GitHub. The unpacked archive has the same Nix hash as the source of the
# `tdlib` package of nixpkgs (sha256-OnITeci0oM6flxdrBZ5w7Gk4BGiNer96MYnD2P6fsMQ=).
TD_SHA256=59602db556c42fce6b08144946ed71660e0915018f7a950d9e75db2a883e6215
TD_URL="https://github.com/tdlib/td/archive/${TD_COMMIT}.tar.gz"

# The OpenSSL of `libtdjson` on macOS (a 3.5 LTS release). Linux uses the OpenSSL of the system. The SHA-256 is the
# value of the file `openssl-3.5.9.tar.gz.sha256` of the GitHub release. To update OpenSSL, change these three values.
OPENSSL_VERSION=3.5.9
OPENSSL_SHA256=603f5602e2eef00d77fbd429d34dcd5822bb301757a1bc9cdb24c670f1eb859a
OPENSSL_URL="https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
patch_file="$root/nix/tdlib-no-direct.patch"
prefix="$root/target/tdlib"
work="$root/target/tdlib-build"
jobs=""

usage() {
  sed -n '/^# Usage:/s/^# //p' "$0"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) prefix=$2; shift 2 ;;
    --work) work=$2; shift 2 ;;
    --jobs) jobs=$2; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

system=$(uname -s)
tools=(curl tar patch cmake gperf)
# On macOS, the build of OpenSSL also needs perl and make.
[ "$system" != Darwin ] || tools+=(perl make)
for tool in "${tools[@]}"; do
  if ! command -v "$tool" > /dev/null; then
    echo "The script needs '$tool'. Install it first." >&2
    exit 1
  fi
done

cmake_args=(
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_INSTALL_PREFIX="$prefix"
  -DCMAKE_INSTALL_LIBDIR=lib
  -DCMAKE_INSTALL_INCLUDEDIR=include
  -DTD_E2E_ONLY=OFF
)
case "$system" in
  Linux)
    library="libtdjson.so.$TD_VERSION"
    links=(libtdjson.so)
    sha256() { sha256sum "$1" | cut -d' ' -f1; }
    if [ -z "$jobs" ]; then
      # Some files of TDLib need approximately 2 GB of memory each. Thus, the number of jobs also depends on the
      # free memory.
      cores=$(nproc)
      memory_jobs=$(awk '/^MemAvailable:/ { n = int($2 / 2500000); print (n < 1 ? 1 : n) }' /proc/meminfo)
      jobs=$(( cores < memory_jobs ? cores : memory_jobs ))
    fi
    ;;
  Darwin)
    # The steps follow the build instructions of TDLib for macOS. The first run on a macOS runner of GitHub built the
    # library (`docs/ROADMAP.md` §10). Nobody started it on macOS 14.
    library="libtdjson.$TD_VERSION.dylib"
    links=(libtdjson.dylib)
    sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
    # The minimum of PANIM is macOS 14 (`DISCUSSION.md` §51). Without this value, clang uses the version of the
    # build computer, and the library does not start on an older macOS.
    export MACOSX_DEPLOYMENT_TARGET=14.0
    cmake_args+=(-DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET")
    # Link the static OpenSSL of `build_openssl` (below). The exact paths make sure that CMake does not use an
    # OpenSSL of Homebrew or of the system.
    openssl_dir="$work/openssl"
    cmake_args+=(
      -DOPENSSL_USE_STATIC_LIBS=TRUE
      -DOPENSSL_ROOT_DIR="$openssl_dir"
      -DOPENSSL_INCLUDE_DIR="$openssl_dir/include"
      -DOPENSSL_CRYPTO_LIBRARY="$openssl_dir/lib/libcrypto.a"
      -DOPENSSL_SSL_LIBRARY="$openssl_dir/lib/libssl.a"
    )
    [ -n "$jobs" ] || jobs=$(sysctl -n hw.ncpu)
    ;;
  *)
    echo "The script supports only Linux and macOS, not $system." >&2
    exit 1
    ;;
esac

# Usage: download NAME URL ARCHIVE SHA256
# Download URL into ARCHIVE, if ARCHIVE is not there with the correct SHA-256. Stop if the SHA-256 is wrong.
download() {
  local name=$1 url=$2 file=$3 expected=$4 actual
  if [ ! -f "$file" ] || [ "$(sha256 "$file")" != "$expected" ]; then
    echo "Download $name"
    curl -fL --retry 3 -o "$file.part" "$url"
    mv "$file.part" "$file"
  fi
  actual=$(sha256 "$file")
  if [ "$actual" != "$expected" ]; then
    echo "The SHA-256 of $file is wrong." >&2
    echo "  expected: $expected" >&2
    echo "  actual:   $actual" >&2
    exit 1
  fi
}

# Build the static OpenSSL for macOS into "$openssl_dir". Only the Darwin part calls this function.
build_openssl() {
  local stamp="$openssl_dir/panim-stamp" arch target value values max
  arch=$(uname -m)
  case "$arch" in
    arm64) target=darwin64-arm64-cc ;;
    x86_64) target=darwin64-x86_64-cc ;;
    *) echo "The script cannot build OpenSSL for $arch." >&2; exit 1 ;;
  esac
  # The stamp records the version, the SHA-256, the minimum of macOS and the architecture of the last build.
  value="$OPENSSL_VERSION $OPENSSL_SHA256 macos$MACOSX_DEPLOYMENT_TARGET $target"
  if [ -f "$openssl_dir/lib/libcrypto.a" ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$value" ]; then
    echo "OpenSSL $OPENSSL_VERSION is already in $openssl_dir"
  else
    local archive="$work/openssl-$OPENSSL_VERSION.tar.gz" src="$work/openssl-$OPENSSL_VERSION"
    download "OpenSSL $OPENSSL_VERSION" "$OPENSSL_URL" "$archive" "$OPENSSL_SHA256"
    # Always start from a clean source and a clean install folder.
    rm -rf "$src" "$openssl_dir"
    tar -xzf "$archive" -C "$work"
    # OPENSSLDIR goes into the library. It is the folder of `openssl.cnf`, and of the default certificates
    # (`cert.pem` and `certs/`). TDLib uses these default certificates on macOS (`tdnet/td/net/SslCtx.cpp`), for
    # example when it gets its configuration through HTTPS. macOS supplies `/etc/ssl/cert.pem`. A missing or
    # incorrect `openssl.cnf` does not stop OpenSSL, because it loads the file with the flag
    # `CONF_MFLAGS_IGNORE_RETURN_CODES`.
    echo "Build OpenSSL $OPENSSL_VERSION for $target with $jobs jobs"
    (
      cd "$src"
      perl ./Configure "$target" no-shared no-tests no-docs no-apps \
        --prefix="$openssl_dir" --libdir=lib --openssldir=/etc/ssl \
        "-mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
      make -j "$jobs" build_libs
      make install_dev
    )
    echo "$value" > "$stamp"
  fi

  # Examine the minimum of macOS of each object. An object for a newer macOS can call functions that macOS 14
  # does not have. The first macOS run (`ROADMAP.md` §10) showed this problem with the OpenSSL of Homebrew.
  values=$(otool -l "$openssl_dir/lib/libcrypto.a" "$openssl_dir/lib/libssl.a" | awk '
    $1 == "cmd" { cmd = $2 }
    cmd == "LC_BUILD_VERSION" && $1 == "minos" { print $2 }
    cmd == "LC_VERSION_MIN_MACOSX" && $1 == "version" { print $2 }
  ' | sort -V -u)
  if [ -z "$values" ]; then
    echo "otool found no minimum of macOS in the objects of OpenSSL." >&2
    exit 1
  fi
  max=$(printf '%s\n%s\n' "$values" "$MACOSX_DEPLOYMENT_TARGET" | sort -V | tail -n 1)
  echo "The minimum of macOS in the objects of OpenSSL: $(echo "$values" | paste -s -d ' ' -)"
  if [ "$max" != "$MACOSX_DEPLOYMENT_TARGET" ]; then
    echo "An object of OpenSSL is for macOS $max, above $MACOSX_DEPLOYMENT_TARGET." >&2
    exit 1
  fi
}

mkdir -p "$work"
[ "$system" != Darwin ] || build_openssl
archive="$work/td-$TD_COMMIT.tar.gz"
download "TDLib $TD_VERSION ($TD_COMMIT)" "$TD_URL" "$archive" "$TD_SHA256"

# Always start from a clean source, so that the patch applies only one time.
source_dir="$work/td-$TD_COMMIT"
rm -rf "$source_dir"
tar -xzf "$archive" -C "$work"
patch -d "$source_dir" -p1 --forward < "$patch_file"

build_dir="$work/build"
cmake -S "$source_dir" -B "$build_dir" "${cmake_args[@]}"
echo "Build tdjson with $jobs jobs"
cmake --build "$build_dir" --target tdjson -j "$jobs"

if [ "$system" = Darwin ]; then
  # The `.app` contains only the library. A dependency on a file of Homebrew or Nix fails on the computer of a user.
  echo "The dependencies of $library:"
  otool -L "$build_dir/$library"
  if otool -L "$build_dir/$library" | tail -n +2 | grep -E '^[[:space:]]*(/opt/homebrew|/usr/local|/nix)/'; then
    echo "$library needs the files above. Link OpenSSL statically." >&2
    exit 1
  fi
fi

# Install only the files of the JSON interface. `cmake --install` also needs the static libraries. The prefix can
# be a shared folder (for example `/usr/local`), so the script replaces only its own files.
mkdir -p "$prefix/lib/pkgconfig" "$prefix/include/td/telegram"
rm -f "$prefix/lib/$library"
cp "$build_dir/$library" "$prefix/lib/"
for link in "${links[@]}"; do
  ln -sfn "$library" "$prefix/lib/$link"
done
cp "$source_dir/td/telegram/td_json_client.h" "$build_dir/td/telegram/tdjson_export.h" "$prefix/include/td/telegram/"
cp "$build_dir/pkgconfig/tdjson.pc" "$prefix/lib/pkgconfig/"

echo "The TDLib of PANIM is in $prefix."
echo "Tell pkg-config where it is:"
echo "  export PKG_CONFIG_PATH=\"$prefix/lib/pkgconfig\${PKG_CONFIG_PATH:+:\$PKG_CONFIG_PATH}\""
