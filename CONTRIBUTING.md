# Contribute to PANIM

English | [简体中文](CONTRIBUTING.zh-CN.md)

This file is for the people who change PANIM: it tells how to build and test PANIM, the rules for a change, and how a change comes into PANIM. `README.md` is for the people who use PANIM.

Before you change PANIM, read these documents:

- `docs/DESIGN.md` tells what PANIM does.
- `docs/ARCHITECTURE.md` tells how we build PANIM.
- `docs/ROADMAP.md` shows the milestones, the current state and the next work.

Some documents name sections of `DISCUSSION.md`. This file is the record of the decisions. It is only in the development repository.

## The two repositories

- **The public repository** `SakuraRainn/panim` has only the releases. Each release is one commit on `main` and one tag. The commit is the same as the signed source archive of the release.
- **The development repository** is private. The maintainer does the work there. The history of the work is only there.

Thus, the public repository cannot merge a pull request (the section "How a change comes into PANIM").

## Build and test

You can build PANIM on Linux in two ways:

- **With Nix.** The flake supplies all the tools and libraries. The maintainer uses this build for the development. Users of Nix can install PANIM from the flake.
- **Without Nix.** The libraries come from your system, Rust comes from rustup, and a script builds the TDLib of PANIM. The release packages come from this build, in containers (`docker/check.sh`).

The two ways use the same TDLib patch, `nix/tdlib-no-direct.patch`. Without the patch, TDLib can make connections that do not use the proxy of the persona.

### Build with Nix

The flake supplies the Rust toolchain, WebKitGTK, Node.js, pnpm, the TDLib of PANIM and the other libraries. You do not need rustup. Do all the steps in the shell of `nix develop`.

1. At the root of the repository, run `nix develop`. The first time, Nix builds the TDLib of PANIM with its patch. This takes approximately 10 minutes or more.
2. In `crates/app`, run `pnpm install`.
3. In `crates/app`, run `pnpm build`. The Rust crate `panim` needs `crates/app/dist` before it can compile.
4. At the root, run `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check`.
5. In `crates/app`, run `pnpm check`. It fails if a text is missing in one language.
6. To start the app for development, run `pnpm tauri dev` in `crates/app`.
7. To build the package, run `nix build` at the root. The result is `result/bin/panim`.

### Build without Nix

These steps are for Ubuntu 24.04. On a different system, install the same libraries with its package manager. Do the steps outside the shell of `nix develop`.

1. Install the system packages:

   ```
   sudo apt install build-essential cmake gperf curl file pkg-config libssl-dev zlib1g-dev libsqlite3-dev \
     libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libxdo-dev libayatana-appindicator3-dev
   ```

2. Install rustup (https://rustup.rs). In the repository, rustup uses the Rust version of `rust-toolchain.toml`.
3. Run `rustup component add clippy rustfmt` in the repository.
4. Install Node.js 24 and pnpm.
5. At the root, run `scripts/build-tdlib.sh`. This takes approximately 10 minutes or more.
   - The script downloads the source of TDLib and examines its SHA-256.
   - It applies `nix/tdlib-no-direct.patch` and builds `libtdjson` in `target/tdlib`.
6. Tell pkg-config where the TDLib is. Do this in each new shell:

   ```
   export PKG_CONFIG_PATH="$PWD/target/tdlib/lib/pkgconfig"
   ```

7. Do the steps 2 to 6 of "Build with Nix".
8. To build the packages, run `scripts/build-packages.sh` at the root. The result is a `.deb`, an `.rpm` and an AppImage in `target/release/bundle`.

The binary of `cargo build` finds `libtdjson` in `target/tdlib/lib`. The `.deb` and the `.rpm` put `libtdjson` into `/usr/lib/panim`, and the binary finds it there. The AppImage has `libtdjson` in its `usr/lib`.

`docker/check.sh` does all these steps in a clean Ubuntu 24.04 container. Then it installs the `.deb` in Ubuntu and the `.rpm` in Fedora, and starts PANIM in Xvfb. It needs Docker or rootless Podman (`CONTAINER_ENGINE`). `TDLIB_JOBS` sets the number of jobs of the TDLib build. Each job can need approximately 2 GB of memory. The output is in `target/container`.

The two scripts also have the steps for macOS. Only the macOS runners of GitHub ran them (`.github/workflows/macos-check.yml` and `macos-release.yml`). Nobody tested them on a Mac yet.

### Test data and the tests

Set `PANIM_DATA` to a test folder. Then a test or a development start does not use your real data in `~/.local/share/panim`.

The tests use no network and no account:

- The tests of `panim-github` use a fake HTTP client.
- The tests of `panim-telegram` and of the app use a fake TDLib. Never log in to Telegram from a test.

The maintainer runs all the tests before each release. In the development repository, automatic tests also run for each change.

### The Telegram app values in a build

Telegram knows each app by its app values: an `api_id` and an `api_hash`. The source of PANIM never has the values of PANIM.

- The build of `panim-telegram` reads the values from `PANIM_TELEGRAM_API_ID` and `PANIM_TELEGRAM_API_HASH`, or from the file `~/.config/panim/telegram.env`.
- Without the values, the build passes. Then the app asks for the own values of the person in the settings.
- To build without the values, set `PANIM_TELEGRAM_ENV` to an empty text.
- `docker/check.sh --values FILE` makes the release packages with the values of FILE (the section "Make a release"). The build container reads only FILE, read-only. The script never prints the values.
- The workflow `macos-release.yml` makes the `.dmg` of a release with the values from two secrets of the development repository. It never prints them.
- The tests do not need the values.

Never put Telegram values into the repository, into an issue or into a log. Never print them. If you use your own values for a test, keep them in `~/.config/panim/telegram.env` or in the environment.

## The rules

These rules apply to each change.

### The writing rules

Write all documentation in ASD-STE100 Simplified Technical English (STE). This rule applies to the files in `docs/` and `base/`, `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, the doc comments (`///` and `//!`) and the comments in the code.

1. Use only the approved words of the STE dictionary, with their approved meanings. You can use technical names (for example, `TDLib`, `IMAP`, "proxy") and technical verbs (for example, "compile").
2. Use one word for one meaning. Use the same word for the same thing each time.
3. Make sure that a sentence has no more than 20 words in a procedure, and no more than 25 words in a description.
4. Write one instruction in each sentence. Use the imperative for instructions (for example, "Run the test").
5. Use the active voice. Use only the present tense, the simple past tense and the simple future tense.
6. Do not use the "-ing" form of a verb, except in a technical name.
7. Make sure that a noun cluster has no more than three words.
8. Do not leave out articles ("the", "a") to make a sentence shorter.
9. Write one topic in each paragraph. Make sure that a paragraph has no more than six sentences.
10. Use a vertical list for a sequence of steps or a set of items.

### The Chinese documents

`README.md`, `CONTRIBUTING.md` and `SECURITY.md` each have a Chinese version: `README.zh-CN.md`, `CONTRIBUTING.zh-CN.md` and `SECURITY.zh-CN.md`.

- A change to one of the three English files must also change its Chinese file, in the same commit.
- The English file is the reference. If the two files are different, the English file is correct.
- STE does not apply to the Chinese text. Write short and clear Chinese, with the same sections, commands, paths and links as the English file.
- Use the words of the Chinese user interface (`crates/app/src/lib/i18n.svelte.ts`), for example 身份 (persona), 口令 (passcode) and 找回词 (recovery words).

### The user interface

- Each text of the user interface is in English and in Chinese, in `crates/app/src/lib/i18n.svelte.ts`. Add each new text in the two languages.
- `pnpm check` fails if a text is missing in one language.

### The platforms

- Keep the rules of each platform.
- Do not scrape pages.
- Do not control a user account through an API that is for bots.
- If an API does not give access, PANIM shows the account in a browser view.

### The specifications

`base/` contains the specifications, for example `base/panim-proof-v1.md`. Other tools use them, so do not change a published version. Make a new version.

### Step 1

PANIM is in step 1 (`docs/DESIGN.md` §3). Step 1 has one persona for each person, and the user interface does not show it. Use the persona as the key for the accounts, the browser profile and the network route. Step 1 does not use ZIKARON.

## The layout

- `crates/` contains the Rust workspace:
  - `panim-core` has the data model, the persona key and the proofs. It has no user interface, no network code and no disk code.
  - `panim-vault` has the sealed local data: the vault, the sealed store, the sealed secrets, the sealed browser profiles, the key file and the backup.
  - `panim-github` has the network code of the GitHub connector.
  - `panim-telegram` has the code of the Telegram connector: the thin layer over `tdjson`, the login, the chats and the chat panel.
  - `app` is the desktop app. `app/src` is the Svelte user interface. `app/src-tauri` is the Rust side (the crate `panim`).
- `base/` contains the specifications.
- `docs/` contains the design, the architecture and the roadmap.
- `nix/` contains the package, the patch and the release scripts:
  - `package.nix` is the Nix package.
  - `tdlib-no-direct.patch` is the patch of the TDLib of PANIM. The file tells each change.
  - `release.sh` writes and signs the checksums of a release. `publish.sh` makes the public commit of a release.
- `scripts/` contains the scripts of the build without Nix:
  - `build-tdlib.sh` builds the TDLib of PANIM with the patch of `nix/`.
  - `build-packages.sh` builds the packages with the bundler of Tauri. `crates/app/src-tauri/bundle/` has the bundler settings for Linux and macOS.
- `docker/` contains the check of the build without Nix in containers (`check.sh`).
- `.github/ISSUE_TEMPLATE/` contains the forms for the issues of the public repository.
- `README.zh-CN.md`, `CONTRIBUTING.zh-CN.md` and `SECURITY.zh-CN.md` are the Chinese versions of the three public documents.
- `flake.nix` is the Nix flake. `rust-toolchain.toml` sets the Rust version. The dev shell and the Nix package use this version.

## Report a problem

- Open an issue at https://github.com/SakuraRainn/panim/issues, and select the form for a problem or for a question.
- Give the version of PANIM, your system, the method of installation (the `.deb`, the `.rpm`, the AppImage or Nix), the steps and the result.
- Do not put a passcode, recovery words, a key file, a token or Telegram values into an issue.
- For a vulnerability, do not open a public issue. Read `SECURITY.md`.

## How a change comes into PANIM

The public repository has only the releases, so it cannot merge a pull request. A change comes into PANIM in this sequence:

1. Open an issue first. Tell the problem and the change that you want to make. Then the maintainer can tell you if the change fits the plan.
2. Make the change on the latest release, and run the tests (the section "Build and test").
3. Send the change. Use one of these methods:
   - Open a pull request in the public repository.
   - Attach a patch from `git format-patch` to the issue.
4. The maintainer examines the change and applies it in the development repository. The commit has your name and your email in a `Co-authored-by` line.
5. The change comes with the next release. Then the maintainer closes your pull request or your issue.

The public commit of a release is one commit, so it does not show the commits of the development repository. The entry of the release in `CHANGELOG.md` names the contributors of the release.

When you send a change, you agree that PANIM can use it under the MIT License of PANIM (`LICENSE`).

## Make a release

These are the steps of the maintainer. Do them in the development repository. Do the `nix` commands in the shell of `nix develop`.

Do this one time, before the first release with macOS. Give the development repository the two secrets of the workflow `macos-release.yml`:

```
gh secret set PANIM_TELEGRAM_API_ID
gh secret set PANIM_TELEGRAM_API_HASH
```

Each command asks for the value and reads it from the standard input. Do not give a value on the command line (for example with `--body`), because the shell history keeps it.

1. Change the version in all `Cargo.toml` files, `crates/app/package.json`, `crates/app/src-tauri/tauri.conf.json` and `Cargo.lock`. Write the entry of the release in `CHANGELOG.md`.
2. Make sure that all the tests pass. Commit the change, and make the tag `v<version>`.
3. Check out the tag `v<version>` in a clean tree. Remove the folders `target/container` and `target/macos`, so that they have no packages of an older release.
4. Build the packages with the Telegram values of PANIM. This step needs Docker or rootless Podman:

   ```
   docker/check.sh all --values ~/.config/panim/telegram.env
   ```

   - The first run takes approximately 30 minutes.
   - The script refuses to start if the tree has changes, or if `HEAD` does not have the tag `v<version>` of `tauri.conf.json`.
   - The build container reads only the file of the values, read-only. The image, the report and the logs do not get the values.
   - The build, the check of the values and the install checks must pass. The packages are in `target/container/packages`.
   - The values then are in the binary of each package.
5. Build the `.dmg` for macOS on a macOS runner of GitHub. A macOS minute costs approximately 10 Linux minutes, so do one run only. Push the tag, and push its commit to the branch `macos-release`:

   ```
   git push --atomic origin v<version> '+v<version>^{commit}:refs/heads/macos-release'
   gh run list --workflow macos-release.yml --limit 1
   gh run watch <run>
   gh run download <run> -n macos-release-dmg -D target/macos
   sha256sum target/macos/PANIM_<version>_aarch64.dmg
   ```

   - `^{commit}` is necessary, because GitHub does not accept an annotated tag as the value of a branch.
   - The workflow refuses a commit that does not have the tag `v<version>` or `v<version>-rc.<n>` of `tauri.conf.json`.
   - The run takes approximately 30 minutes. The first run on the branch also builds TDLib, and takes approximately 20 minutes more.
   - All the checks of the run must pass. Compare the SHA-256 with the line `SHA256` on the summary page of the run.
6. Write and sign the checksums with the secret key, which stays outside the repository:

   ```
   nix run .#release -- --version <version> --ref v<version> --key ~/.minisign/panim.key \
     --out /path/to/release \
     --artifact target/container/packages/PANIM_<version>_amd64.deb \
     --artifact target/container/packages/PANIM-<version>-1.x86_64.rpm \
     --artifact target/container/packages/PANIM_<version>_amd64.AppImage \
     --artifact target/macos/PANIM_<version>_aarch64.dmg \
     --pubkey ~/.minisign/panim.pub
   ```

7. Clone the public repository into a new folder: `git clone https://github.com/SakuraRainn/panim /path/to/panim-public`.
8. Put the Telegram values of PANIM into the environment, not into the repository. Then the script of the next step can make sure that the `api_hash` is not in the commit:

   ```
   set -a; . ~/.config/panim/telegram.env; set +a
   ```

9. Make the public commit and the tag from the signed source archive:

   ```
   nix run .#publish -- --from /path/to/release --clone /path/to/panim-public --version <version> \
     --name <name> --email <id>+<name>@users.noreply.github.com --pubkey ~/.minisign/panim.pub
   ```

   - The author and the committer are the private email of GitHub. The script refuses a different email.
   - The script replaces all the files of the clone with the files of the archive, and removes the files that the release does not have.
   - It makes sure that the tree is the same as the archive, that the files that `export-ignore` removes are not there, and that the `api_hash` is not there. If a check fails, it removes the commit and the tag again.
   - It does not push.
10. Examine the commit. Then run the push command that the script shows.
11. In the public repository, make the release on the tag `v<version>`. Publish all the files of the output folder: the `.deb`, the `.rpm`, the AppImage, the `.dmg`, `panim-<version>-source.tar.gz`, `SHA256SUMS` and `SHA256SUMS.minisig`.
12. Push the branch of the development repository. Step 5 pushed the tag.

A release candidate is for the hand checks on a Mac (`docs/ROADMAP.md` §10). Make the tag `v<version>-rc.<n>` on the commit. Do only step 5, with `v<version>-rc.<n>` in place of `v<version>` in the `git push` command. Then the `.dmg` has the Telegram app values of PANIM. Do not publish a release candidate.

`.gitattributes` marks the files that are only for the development: `git archive` leaves them out. Thus, the source archive and the public repository do not have them. Do not mark `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, their Chinese files or `.github/` this way.
