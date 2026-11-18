# PANIM

PANIM (פנים, "face", a word that is always plural in Hebrew) is a desktop persona manager. One person uses it to read and answer their accounts in one location. Later, the person can split into several personas, and each persona keeps its accounts apart.

PANIM is in step 1. Refer to these documents:

- `CHANGELOG.md` tells what each release does, and its known limits.
- `docs/DESIGN.md` tells what PANIM does.
- `docs/ARCHITECTURE.md` tells how we build PANIM.
- `docs/DISCUSSION.md` is the record of the decisions. It is only in the development repository, not in the public repository.

## Only the official releases are PANIM

A different app can use the name and the look of PANIM. Only the releases of the official repository are PANIM. The official repository is https://github.com/SakuraRainn/panim. Each release has a `SHA256SUMS` file and its minisign signature `SHA256SUMS.minisig`. Before you start a download, check it (the next section).

The minisign public key of PANIM (key id `7B5C5C4A551F160B`):

```
RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa
```

Do not trust a key from a different location. A key in a message, in an issue or on a different site is not the key of PANIM.

## Check a download

1. Download the release files and `SHA256SUMS` and `SHA256SUMS.minisig` into one folder.
2. Install minisign, for example with `nix shell nixpkgs#minisign`.
3. Examine the signature: `minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`. The result must be "Signature and comment signature verified". The trusted comment gives the version and the commit.
4. Examine the files: `sha256sum -c SHA256SUMS --ignore-missing`. Each file that you downloaded must be "OK".
5. If a step fails, do not start the file. Download it again from the official repository.

## Install

### With Nix

1. Run `nix run github:SakuraRainn/panim` to start PANIM, or `nix build github:SakuraRainn/panim` to get `result/bin/panim`.
2. A build from the source has no Telegram app values of PANIM (the section "Telegram"). To add a Telegram account, give your own values in the settings.

### The AppImage

1. Download `PANIM-<version>-x86_64.AppImage` and check it (the section "Check a download").
2. Run `chmod +x PANIM-<version>-x86_64.AppImage`, then start the file.
3. The AppImage needs FUSE. Without FUSE, start it with the option `--appimage-extract-and-run`.
4. The AppImage needs unprivileged user namespaces. Some systems turn them off, for example Ubuntu 24.04 and later. Then use the Nix package.
5. If the window stays empty, start the AppImage with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. If that does not help, also set `LIBGL_ALWAYS_SOFTWARE=1`.

The AppImage has the Telegram app values of PANIM.

## Telegram

Telegram knows each app by its app values: an `api_id` and an `api_hash`.

- **PANIM's values.** The release files have them. All the people who use PANIM share them, so they do not link your personas. They are the default.
- **Your own values.** Get them at my.telegram.org → API development tools. In PANIM, open Settings → Telegram app values, select "My own app values" and enter them. PANIM keeps them sealed and does not show the `api_hash` again.
- **Warning:** an `api_id` belongs to one Telegram account. If the logins of several personas use your own `api_id`, Telegram can see that they have the same owner.
- A change of this setting is only for new logins. Each account keeps the values of its login. If those values are not available, for example in a build without PANIM's values, the account shows a warning and needs a new login.

## Backup

1. In PANIM, open Settings → Backup, and click "Back up…". Select the location of the file.
2. Keep the file safe. It stays sealed. Only your passcode, your 12 recovery words or your key file open it.
3. To restore it, start PANIM on a computer with no vault. On the first screen, click "Restore from a backup", and select the file.
4. Close PANIM on the old computer first, or remove the Telegram accounts there. Otherwise, the same sessions are in two places.

## Build

1. Run `nix develop` to get WebKitGTK, Node.js and the other Linux libraries.
2. In `crates/app`, run `pnpm install`, then `pnpm build`.
3. At the root, run `cargo test` and `cargo clippy --all-targets`.
4. In `crates/app`, run `pnpm check`.
5. To start the app for development, run `pnpm tauri dev` in `crates/app`.
6. To get the package, run `nix build` at the root.

Set `PANIM_DATA` to a test folder, so that a test does not use your vault in `~/.local/share/panim`.

## Make a release

These are steps of the maintainer.

1. Put the values of PANIM into the environment. Do not put them into the repository.
2. Build the AppImage with the values:

   ```
   PANIM_TELEGRAM_API_ID=… PANIM_TELEGRAM_API_HASH=… nix build --impure .#appimage-release
   ```

   The values then are in the Nix store of this computer and in the binary.
3. Copy the AppImage out of the Nix store: `cp result PANIM-<version>-x86_64.AppImage`.
4. Make the tag of the release. Then write and sign the checksums with the secret key, which stays outside the repository:

   ```
   nix run .#release -- --version <version> --ref v<version> --key ~/.minisign/panim.key \
     --out /path/to/release --artifact PANIM-<version>-x86_64.AppImage --pubkey /path/to/panim.pub
   ```

5. Publish all the files of the output folder in the release of the official repository.
