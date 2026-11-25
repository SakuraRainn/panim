# Makes the public commit of a release of PANIM from its signed source archive (`CONTRIBUTING.md`, "Make a release").
#
# Run it from the development repository, after `nix run .#release`:
#
#   nix run .#publish -- --from /path/to/release --clone /path/to/panim --version 0.1.1 \
#     --name NAME --email ID+NAME@users.noreply.github.com [--pubkey /path/to/minisign.pub]
#
# 1. It examines `SHA256SUMS` of the folder for `panim-<version>-source.tar.gz`. With `--pubkey`, it also examines
#    `SHA256SUMS.minisig` and the version in its trusted comment.
# 2. It replaces the tracked files of the clone with the files of the archive. A file that the archive does not have
#    goes out of the clone.
# 3. It makes one commit on `main` with the given name and email as the author and the committer. Then it makes the
#    annotated tag `v<version>`.
# 4. It examines the result: the tree of the tag is the same as the archive, the files that `export-ignore` removes
#    are not there, and the Telegram `api_hash` of `PANIM_TELEGRAM_API_HASH` is not there. If a check fails, it
#    removes the commit and the tag again.
#
# The version has the form X.Y.Z for a release, or X.Y.Z-rc.N for a release candidate (for example 0.2.0-rc.1).
# For a release candidate, the script does the same steps, but the push command pushes only the tag. Thus, the branch
# `main` of the public repository stays at the last release. The local `main` of the clone gets the commit of the
# release candidate, so do not use this clone for the next release. Make a new clone.
#
# It never pushes. It shows the push command. It accepts only an email of the form `<name>@users.noreply.github.com`,
# so that the email of the development repository does not become public. `--allow-any-email` stops this check.

usage() {
  echo "usage: panim-publish --from FOLDER --clone PUBLIC_CLONE --version X.Y.Z[-rc.N] --name NAME --email EMAIL [--pubkey PUBLIC_KEY] [--allow-any-email]" >&2
  exit 2
}

fail() {
  echo "panim-publish: $*" >&2
  exit 1
}

from=""
clone=""
version=""
name=""
email=""
pubkey=""
any_email=0
while [ $# -gt 0 ]; do
  case "$1" in
    --from) from="${2:-}"; shift 2 ;;
    --clone) clone="${2:-}"; shift 2 ;;
    --version) version="${2:-}"; shift 2 ;;
    --name) name="${2:-}"; shift 2 ;;
    --email) email="${2:-}"; shift 2 ;;
    --pubkey) pubkey="${2:-}"; shift 2 ;;
    --allow-any-email) any_email=1; shift ;;
    *) usage ;;
  esac
done
[ -n "$from" ] && [ -n "$clone" ] && [ -n "$version" ] && [ -n "$name" ] && [ -n "$email" ] || usage

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$ ]]; then
  fail "the version must have the form X.Y.Z or X.Y.Z-rc.N"
fi
candidate=0
if [[ "$version" == *-rc.* ]]; then
  candidate=1
fi
if [[ "$name" == *$'\n'* ]] || [[ "$email" == *[[:space:]\<\>]* ]]; then
  fail "the name or the email has a character that is not permitted"
fi
if [ "$any_email" -eq 0 ] && ! [[ "$email" =~ ^([0-9]+\+)?[A-Za-z0-9-]+@users\.noreply\.github\.com$ ]]; then
  fail "the email must be the private email of GitHub (ID+NAME@users.noreply.github.com). Use --allow-any-email to accept a different email."
fi

tag="v$version"
archive_name="panim-$version-source.tar.gz"
from="$(realpath "$from")"
[ -f "$from/$archive_name" ] || fail "$from has no $archive_name"
[ -f "$from/SHA256SUMS" ] || fail "$from has no SHA256SUMS"

# The paths that `export-ignore` removes. The script knows the paths of today. If the current folder is in a Git
# repository with a `.gitattributes`, the script also reads its `export-ignore` paths.
private_paths=(AGENTS.md CLAUDE.md .githooks .gitattributes docs/DISCUSSION.md)
if top="$(git rev-parse --show-toplevel 2>/dev/null)" && [ -f "$top/.gitattributes" ]; then
  while read -r path rest; do
    case " $rest " in
      *" export-ignore "*) private_paths+=("$path") ;;
    esac
  done < <(grep -v '^[[:space:]]*#' "$top/.gitattributes")
fi

# Step 1: the checksum and the signature.
(
  cd "$from" || exit 1
  line="$(awk -v n="$archive_name" '$2 == n || $2 == "*" n' SHA256SUMS)"
  [ -n "$line" ] || fail "SHA256SUMS has no line for $archive_name"
  printf '%s\n' "$line" | sha256sum -c --quiet - || fail "the checksum of $archive_name is not correct"
)
if [ -n "$pubkey" ]; then
  [ -f "$from/SHA256SUMS.minisig" ] || fail "$from has no SHA256SUMS.minisig"
  if [ -f "$pubkey" ]; then
    key_args=(-p "$pubkey")
  else
    key_args=(-P "$pubkey")
  fi
  verified="$(minisign -V "${key_args[@]}" -m "$from/SHA256SUMS")" || fail "the signature of SHA256SUMS is not correct"
  printf '%s\n' "$verified"
  # release.sh writes the trusted comment "PANIM <version>, commit <commit>". The script compares the version as
  # fixed text, because "." is a special character in a regex.
  trusted="$(sed -n 's/^Trusted comment: //p' <<< "$verified")"
  signed_commit="${trusted#"PANIM $version, commit "}"
  if [ "$signed_commit" = "$trusted" ] || ! [[ "$signed_commit" =~ ^[0-9a-f]{40}$ ]]; then
    fail "the trusted comment of the signature is not for PANIM $version"
  fi
else
  echo "panim-publish: no --pubkey, so the signature of SHA256SUMS was not examined" >&2
fi

# Step 2: the clone. These settings make the result independent of the configuration of the person: no filters, no
# hooks and no signature with a key that can show a different identity.
clone="$(realpath "$clone")"
g() {
  git -C "$clone" -c core.autocrlf=false -c core.safecrlf=false -c core.attributesFile=/dev/null \
    -c core.hooksPath=/dev/null -c core.symlinks=true -c core.fileMode=true \
    -c commit.gpgSign=false -c tag.gpgSign=false -c tag.forceSignAnnotated=false "$@"
}
[ "$(g rev-parse --show-toplevel 2>/dev/null)" = "$clone" ] || fail "$clone is not the top folder of a Git repository"
case "$(g config --get remote.origin.url || true)" in
  *panim-dev*) fail "the origin of $clone is the development repository" ;;
esac
[ -z "$(g status --porcelain --ignored --untracked-files=all)" ] ||
  fail "$clone has changes, or files that are not tracked. Use a clean clone."
for path in "${private_paths[@]}"; do
  if [ -n "$(g ls-files -- "$path")" ]; then
    fail "$clone tracks $path. It looks like the development repository."
  fi
done
if g rev-parse --verify --quiet HEAD >/dev/null; then
  old_head="$(g rev-parse HEAD)"
  [ "$(g symbolic-ref --quiet HEAD || true)" = "refs/heads/main" ] || fail "$clone is not on the branch main"
else
  # The first release: the repository has no commit yet.
  old_head=""
  [ -z "$(g for-each-ref refs/heads)" ] || fail "$clone has branches, but HEAD has no commit"
  g symbolic-ref HEAD refs/heads/main
fi
if g rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
  fail "$clone has the tag $tag already"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$from/$archive_name" -C "$work" --no-same-owner --no-same-permissions
src="$work/panim-$version"
[ -d "$src" ] || fail "$archive_name has no folder panim-$version"
[ "$(find "$work" -mindepth 1 -maxdepth 1 | wc -l)" -eq 1 ] || fail "$archive_name has more than the folder panim-$version"

# Replace the files. `git rm` removes each tracked file, also the files that the new release does not have.
g rm -r -q --ignore-unmatch -- .
cp -a "$src/." "$clone/"
g add -A -f -- .

# Step 3: the commit and the tag.
export GIT_AUTHOR_NAME="$name" GIT_AUTHOR_EMAIL="$email" GIT_COMMITTER_NAME="$name" GIT_COMMITTER_EMAIL="$email"
g commit -q --no-verify -m "PANIM $version" \
  -m "This commit is the same as $archive_name of the release $tag. SHA256SUMS and its minisign signature in the release cover the archive."
g tag -a "$tag" -m "PANIM $version"
commit="$(g rev-parse HEAD)"

# Step 4: the checks. If one fails, remove the commit and the tag. Nothing was pushed.
undo() {
  echo "panim-publish: CHECK FAILED: $*" >&2
  g tag -d "$tag" >/dev/null
  if [ -n "$old_head" ]; then
    g reset -q --hard "$old_head"
  else
    g update-ref -d refs/heads/main
    g read-tree --empty
  fi
  g clean -q -f -d -x
  echo "panim-publish: the commit and the tag were removed. The clone is as before." >&2
  exit 1
}

# The list of the archive: the mode, the Git blob id and the path of each file. `--no-filters` reads the bytes as they
# are. It does not use the commit, so it is a separate check of the tree.
list_archive() {
  (
    cd "$src" || exit 1
    find . \( -type f -o -type l \) -printf '%P\n' | LC_ALL=C sort | while IFS= read -r f; do
      if [ -L "$f" ]; then
        printf '120000 %s\t%s\n' "$(printf '%s' "$(readlink "$f")" | git hash-object --stdin)" "$f"
      elif [ -x "$f" ]; then
        printf '100755 %s\t%s\n' "$(git hash-object --no-filters -- "$f")" "$f"
      else
        printf '100644 %s\t%s\n' "$(git hash-object --no-filters -- "$f")" "$f"
      fi
    done
  )
}
[ "$(g rev-parse "$tag^{commit}")" = "$commit" ] || undo "the tag $tag does not point to the new commit"
if [ "$(list_archive)" != "$(g ls-tree -r --format='%(objectmode) %(objectname)%x09%(path)' "$tag" | LC_ALL=C sort -t "$(printf '\t')" -k2)" ]; then
  undo "the tree of $tag is not the same as $archive_name"
fi
for path in "${private_paths[@]}"; do
  [ -z "$(g ls-tree -r --name-only "$tag" -- "$path")" ] || undo "$tag has $path, which export-ignore removes"
done
[ "$(g log -1 --format='%an%n%ae%n%cn%n%ce' "$commit")" = "$(printf '%s\n%s\n%s\n%s' "$name" "$email" "$name" "$email")" ] ||
  undo "the author or the committer of the commit is not $name <$email>"
[ "$(g cat-file tag "$tag" | sed -n 's/^tagger \(.*\) [0-9]* [-+][0-9]*$/\1/p')" = "$name <$email>" ] ||
  undo "the tagger of $tag is not $name <$email>"
if [ -n "$old_head" ]; then
  [ "$(g rev-parse "$commit^")" = "$old_head" ] || undo "the parent of the commit is not the old main"
fi
if [ -n "${PANIM_TELEGRAM_API_HASH:-}" ]; then
  # `-q` prints nothing, so the value does not show.
  if g grep -q -i -F -e "$PANIM_TELEGRAM_API_HASH" "$tag" --; then
    undo "the tree of $tag has the Telegram api_hash"
  fi
  echo "The Telegram api_hash is not in the tree."
else
  echo "panim-publish: PANIM_TELEGRAM_API_HASH is not set, so the check for the Telegram api_hash did not run" >&2
fi

echo
echo "The commit $commit and the tag $tag are ready in $clone."
echo "Author and committer: $name <$email>"
g log -1 --format='%B' "$commit"
g diff --shortstat "${old_head:-$(g hash-object -t tree /dev/null)}" "$commit"
echo
if [ "$candidate" -eq 1 ]; then
  echo "Nothing was pushed. $version is a release candidate. Examine the commit, then push only the tag:"
  echo
  echo "  git -C '$clone' push origin 'refs/tags/$tag'"
  echo
  echo "The branch main of the public repository does not change. It stays at the last release."
  echo "The local main of this clone now has the commit of the release candidate."
  echo "Do not use this clone for the next release. Make a new clone."
else
  echo "Nothing was pushed. Examine the commit, then push it:"
  echo
  echo "  git -C '$clone' push origin main 'refs/tags/$tag'"
fi
