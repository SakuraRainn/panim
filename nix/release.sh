# Writes the checksums of a release of PANIM and signs them with minisign (`README.md`, "Check a download").
#
# Run it from the repository: nix run .#release -- --version 0.2.0 --ref v0.2.0 --key ~/.minisign/panim.key \
#   --out /path/to/folder [--artifact /path/to/PANIM_0.2.0_amd64.AppImage]... [--pubkey /path/to/minisign.pub]
#
# 1. It makes the source snapshot `panim-<version>-source.tar.gz` from the Git ref, with `git archive`.
# 2. It copies each artifact into the output folder.
# 3. It writes `SHA256SUMS` for all these files.
# 4. It signs `SHA256SUMS` with the secret key. The signature is `SHA256SUMS.minisig`.
# 5. It examines the checksums, and with `--pubkey` also the signature.
#
# The secret key must be outside the repository. The script never makes or copies a key.

usage() {
  echo "usage: panim-release --version X.Y.Z --ref REF --key SECRET_KEY --out FOLDER [--artifact FILE]... [--pubkey PUBLIC_KEY]" >&2
  exit 2
}

version=""
ref=""
key=""
out=""
pubkey=""
artifacts=()
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2:-}"; shift 2 ;;
    --ref) ref="${2:-}"; shift 2 ;;
    --key) key="${2:-}"; shift 2 ;;
    --out) out="${2:-}"; shift 2 ;;
    --artifact) artifacts+=("${2:-}"); shift 2 ;;
    --pubkey) pubkey="${2:-}"; shift 2 ;;
    *) usage ;;
  esac
done
[ -n "$version" ] && [ -n "$ref" ] && [ -n "$key" ] && [ -n "$out" ] || usage

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "the version must have the form X.Y.Z" >&2
  exit 2
fi

repo="$(git rev-parse --show-toplevel)"
if [ ! -f "$key" ]; then
  echo "the secret key $key does not exist" >&2
  exit 1
fi
key_real="$(realpath "$key")"
case "$key_real" in
  "$repo"/*)
    echo "the secret key must be outside the repository" >&2
    exit 1
    ;;
esac
commit="$(git -C "$repo" rev-parse --verify "$ref^{commit}")"

mkdir -p "$out"
if [ -e "$out/SHA256SUMS" ] || [ -e "$out/SHA256SUMS.minisig" ]; then
  echo "$out has a SHA256SUMS already" >&2
  exit 1
fi

names=()
source_name="panim-$version-source.tar.gz"
# `git archive` gives the same bytes for the same commit. `gzip -n` writes no name and no time.
git -C "$repo" archive --format=tar --prefix="panim-$version/" "$commit" | gzip -n -9 > "$out/$source_name"
names+=("$source_name")

for artifact in "${artifacts[@]}"; do
  if [ ! -f "$artifact" ]; then
    echo "the artifact $artifact does not exist" >&2
    exit 1
  fi
  name="$(basename "$artifact")"
  if [ "$(realpath "$artifact")" != "$(realpath -m "$out/$name")" ]; then
    cp --dereference "$artifact" "$out/$name"
    chmod 644 "$out/$name"
  fi
  names+=("$name")
done

cd "$out"
sha256sum -- "${names[@]}" > SHA256SUMS
minisign -S -s "$key_real" -m SHA256SUMS -t "PANIM $version, commit $commit"
sha256sum -c SHA256SUMS
if [ -n "$pubkey" ]; then
  minisign -V -p "$pubkey" -m SHA256SUMS
fi
echo "The release files are in $out:"
ls -l -- "${names[@]}" SHA256SUMS SHA256SUMS.minisig
