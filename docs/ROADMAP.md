# PANIM: roadmap

This document shows the plan for step 1 and the current state of the work. Change it when a milestone starts or is complete. `DESIGN.md` tells what PANIM does. `ARCHITECTURE.md` tells how we build it. `DISCUSSION.md` is the record of the decisions.

The work is on the branch `main`. Until 2026-11-18, its name was `step-1` (`DISCUSSION.md` §41). The author of the commits is SakuraRainn.

## 1. Milestones of step 1

| # | Milestone | State | Waits for |
|---|---|---|---|
| 1 | Proof code: make and examine blocks of `base/panim-proof-v1.md` | Complete | — |
| 2 | Storage and sealing: the vault, the sealed store, the sealed secrets, the sealed profiles, the key file | Complete | — |
| 3 | The app: the first start, the 12 recovery words, the gate, the accounts, the timeline, the settings, the browser windows | Complete | — |
| 4 | Browser-view accounts: X, Discord and Proton Mail (free plans), and the script for unread counts | Complete | — |
| 5 | GitHub: the device login, notifications and the events of watched repositories in the timeline, answers, the check of a proof in a gist | Complete | — |
| 6 | Proof screens: make a proof, show the text to post and its location, examine the proof of a different persona | Complete | — |
| 7 | Telegram: chats in the timeline, messages, a proxy for each persona. Check 4: the Rust bindings of TDLib and how to package TDLib | Complete | — |
| 8 | Proton Mail through Bridge, as one more service of the email connector (milestone 14) | After milestone 14 (`DISCUSSION.md` §28, §42) | A paid Proton account with Proton Mail Bridge |
| 9 | The X API: the user adds their own developer key. PANIM shows the cost of each action | After release 0.1.0 (`DISCUSSION.md` §28) | An X developer key with credit |
| 10 | Release 0.1.0 for Linux: the package (Nix, with `wrapGAppsHook`), the backup, `CHANGELOG.md`, the merge of `step-1` | Complete. Release 0.1.0 is public (2026-11-17, §6) | — |
| 11 | The documents and the release steps: `README.md` for the users, `CONTRIBUTING.md`, `SECURITY.md`, issue templates, the script for the public commit, the AppImage message on Ubuntu | Complete. Release 0.1.1 is public (2026-11-18, §7) | — |
| 12 | macOS: the app, the browser profiles, TDLib and the package on macOS | Not started. Release 0.2.0 (§10). This is a large milestone | A Mac for the tests. An Apple Developer account for the signature of the app (optional) |
| 13 | Bluesky: the timeline, the notifications, posts and answers, the direct messages. Proof format version 2 and the automatic check of a proof post | Not started. Release 0.3.0 (§11) | — |
| 14 | Email services through IMAP/SMTP: mail in the timeline, mail out, IMAP IDLE. Check 3: IMAP/SMTP through the proxy | Not started. Release 0.3.0 (§11) | — |
| 15 | Six repairs: the automatic lock in all the windows, stronger passcode settings, no change in memory after a failed write, limits for a key file, the unread GitHub notifications, the order of the timeline answers | The six repairs are complete. Release 0.1.2 (§8) waits for the hand checks | — |
| 16 | The seal of the open folders: no freeze of the window, no seal when nothing changed, a correct copy of the SQLite files | Not started. Release 0.1.3 (§9) | — |

Release 0.1.0 has GitHub and Telegram in the shared timeline, and X, Discord and Proton Mail in browser views. Release 0.1.1 has milestone 11. Release 0.1.2 has milestone 15 (`DISCUSSION.md` §44, §45). Release 0.1.3 has milestone 16 (`DISCUSSION.md` §45). Release 0.2.0 has milestone 12 (`DISCUSSION.md` §33, §34). Release 0.3.0 has milestones 13 and 14 (`DISCUSSION.md` §42). Milestones 8 and 9 come later.

## 2. Milestone 4 in detail

1. **Complete (2026-11-13).** The person made sure that a real login stays after a restart. This was the last part of check 1 (`ARCHITECTURE.md` §9).
2. **Complete.** Read the unread counts (`DISCUSSION.md` §18):
   - The Rust side reads the count from the title of the page. It does not keep the title.
   - A small script replaces the notifications of the page. It sends a signal with no content.
   - The account windows can call only `page_notice`, which has no arguments (`capabilities/account.json`).
3. **Complete.** Show the unread count and the notice mark of each browser-view account in the list of accounts.
4. **Complete (2026-11-13).** The person made sure that the count and the notice mark work on X, Discord and Proton Mail, with test accounts:
   - Get an unread item. Make sure that the count or the dot shows. Then read the item, and make sure that the count goes away.
   - Get a notification from the site while the account window does not have the focus. Make sure that the notice mark shows, and that it goes away when you go to the window.

## 3. Milestone 5 in detail

`ARCHITECTURE.md` §4a tells how the GitHub connector operates. `DISCUSSION.md` §19 gives the decisions.

These parts are complete in code, with tests that use no network:

1. The crate `panim-github`: the HTTP client on the route of the persona, the device login, the sync, the answers and the check of a gist.
2. The store rule: an item that comes again with a later time becomes unread again.
3. The app: the commands, the sync thread, the event `timeline-changed`, the profile windows and the state of each joined account.
4. The user interface: GitHub in the dialog "Add an account", the timeline with the read state, the answers, the filter by account and "Sync now".
5. The phrases of the events in English and in Chinese (`DISCUSSION.md` §20).
6. Tests of the real HTTP client with a fake proxy on 127.0.0.1: the HTTP proxy, the SOCKS5 proxy and the refusal of `http`.
7. Only the events of watched repositories, and the read state of the GitHub inbox (`DISCUSSION.md` §22).

On 2026-11-14, GitHub gave a user code for the client ID of "PANIM Desktop". Thus, the client ID is correct, and the device flow is on. No person entered the code, so no login was done.

**Complete (2026-11-15).** The person did the hand checks with a real GitHub account, and the milestone passed. These are the checks:

1. Add a GitHub account. Make sure that the device page opens in a window of PANIM, and that the account shows in the list after the login.
2. Get a notification on GitHub. Make sure that it shows in the timeline as an unread item.
3. Click the item. Make sure that it becomes read in PANIM and on GitHub.
4. Get a new comment in the same thread. Make sure that the item becomes unread again.
5. Write an answer to an issue in a public repository. Make sure that the comment shows on GitHub.
6. Let a different account star a repository of the account. Make sure that the event shows in the timeline.
7. Set a proxy route. Make sure that the proxy carries the requests and the device page.
8. Revoke "PANIM Desktop" in the settings of GitHub. Make sure that the token does not work, and that the account then shows the mark.
9. Remove the account. Make sure that its items go away.
10. Select Chinese in the settings. Make sure that the events show their phrases in Chinese.
11. Click the GitHub account. Make sure that the timeline shows only its items, and that a GitHub window opens with the login. Click it again. Make sure that the same window comes to the front.
12. Make sure that the count of the GitHub account in PANIM is equal to the unread count of the GitHub inbox.
13. Read a thread on github.com. Click "Sync now". Make sure that the item becomes read and that the count goes down.
14. Make sure that an event does not change the count, and that an event has no read action.
15. Watch a repository. Make sure that an event in that repository shows in the timeline.
16. Make sure that an action of a followed account in a repository that you do not watch does not show.
17. Click "Sync now". Make sure that the old events of repositories that you do not watch go away.
18. Stop to watch a repository. Make sure that its events go away at the next sync.

The check of a proof in a gist has a screen from milestone 6. The hand checks of milestone 6 (§4) include it.

## 4. Milestone 6 in detail

`ARCHITECTURE.md` §6a tells how the proof screens operate. `DISCUSSION.md` §23 gives the decisions.

These parts are complete in code, with tests that use no network:

1. `panim-core`: the statement for an account, the handle and the id in the form of `base/panim-proof-v1.md` §5, the date from the clock, and the marks of the current state (§8.2).
2. `panim-vault`: the kept proofs in the new sealed place `store/proofs`. A vault from before milestone 6 opens, and it can then keep a proof. A test writes the old files with a copy of the old structs.
3. The app: the commands `proofs`, `make_proof`, `set_identity`, `set_proof_location`, `check_own_proof`, `check_pasted_proof` and `open_gist_page`. Only the main window can call them.
4. The page "Proofs" in the user interface, in English and in Chinese: the persona address, a proof for each account, the text to post and its location, the check of the gist of the person, and the check of a different persona.

**Complete (2026-11-15).** The person did the hand checks with test accounts only (`DISCUSSION.md` §12), and the milestone passed. These are the checks:

1. Open the page "Proofs". Make sure that the address is the address in the settings, in lowercase. Copy it.
2. Select the GitHub test account. Click "Make proof". Make sure that the block has six lines, and that it shows the login and the numeric id of the account.
3. Click "Make proof" again on the same day. Make sure that the block does not change.
4. Click "Open gist.github.com". Make sure that the page opens with the login of the persona. Paste the block into a public gist and save it.
5. Paste the URL of the gist on the page "Proofs". Click "Check my gist". Make sure that the check passes, and that the gist shows as the location.
6. Under "Examine", paste the same gist URL. Make sure that the block is "Correct" and shows "This is your persona".
7. On a second computer or in a second data folder (`PANIM_DATA`), examine the same gist. Make sure that the block is "Correct", and that it does not show "This is your persona".
8. Make a secret gist with the block. Check it with "Check my gist" and with "Examine". Make sure that both say that the gist is secret.
9. Post the block in a gist of a different GitHub account. Check it with "Check my gist". Make sure that the check fails with "a different account".
10. Change one character of the date in a gist. Make sure that "Examine" shows "Wrong signature".
11. Paste the block, with text before and after it, under "Pasted text". Make sure that PANIM shows the persona, the platform, the id, the handle, the kind and the date. Make sure that it says what PANIM did not check.
12. Select the X test account or the Discord test account. Make sure that the page asks for the numeric user id. Enter the id from the help text. Make a proof. Click "Open the account window", and post the block.
13. Enter the URL of the post as the location. Close PANIM and open it again. Make sure that the kept proof, its date and its location are there.
14. Select the Proton Mail account. Make sure that the block has the id `-` and the address in lowercase.
15. Remove an account with a kept proof, and add it again. Make sure that it has no proof.
16. Select Chinese in the settings. Make sure that the page "Proofs" shows all its texts in Chinese.

## 5. Milestone 7 in detail

`ARCHITECTURE.md` §4b tells how the Telegram connector operates. `DISCUSSION.md` §24, §25 and §26 give the decisions.

These parts are complete in code, with tests that use no network and no Telegram account:

1. The TDLib of PANIM: the package `tdlib-panim` in `flake.nix`, with the patch `nix/tdlib-no-direct.patch`. Runs with `strace` showed only connections to the proxy (`spikes/tdlib/README.md` §9).
2. The crate `panim-telegram`: the thin layer over `tdjson`, the interface `Td` with a fake TDLib, the start order, the guard, the login states and errors, the model of the chats, the chat panel and the receive loop.
3. `panim-vault`: a sealed folder can leave out entries (`files/`), and a folder can be deleted with its archive.
4. The app: the commands, the start of the clients at the unlock, the close before the lock, the seal, the removal with `logOut`, and a new start of the clients after a new route.
5. The user interface, in English and in Chinese: Telegram in the dialog "Add an account", the count and the marks in the list of accounts, the chat items in the timeline, and the chat panel.

Nobody did a login, because the test accounts of Telegram are off. The person does these hand checks on production, with their own Telegram account. Use a vault for tests (`PANIM_DATA`), and build PANIM with `~/.config/panim/telegram.env`.

1. Add a Telegram account with the phone number and the code. Make sure that the account shows in the list, with the name and the username.
2. Add a Telegram account that has a two-step password. Make sure that the dialog asks for the password and shows the hint. Enter a wrong password first, and make sure that the error shows.
3. Make sure that the chats of the main chat list show in the timeline. Make sure that muted chats and archived chats do not show. Mute a chat and archive a different chat in the Telegram app. Make sure that their items go away.
4. Make sure that the count of the account is equal to the number of unread chats in the Telegram app. Make sure that each item shows the unread count of its chat.
5. Click an unread item in PANIM. Make sure that the chat becomes read in the Telegram app.
6. Read a chat in the Telegram app. Make sure that the item becomes read in PANIM.
7. Mark a chat as unread in the Telegram app. Make sure that it shows as unread in PANIM. Click it, and make sure that the mark goes away in the Telegram app.
8. Write an answer from the timeline. Make sure that the message shows in the Telegram app.
9. Open the chat panel. Make sure that the last messages show, with the sender and the time, and that a photo shows as a label. Send a message from the panel. Let a different account send a message, and make sure that it shows in the open panel.
10. Open the chat panel of a channel where you cannot write. Make sure that the panel says this, and that the timeline has no "Answer".
11. Lock PANIM and unlock it. Make sure that the account needs no new login and the chats come back.
12. Close PANIM and open it again. Make sure that the account needs no new login.
13. Remove the account. Make sure that the session of PANIM goes away from the list of devices in the Telegram app (Settings → Devices).
14. End the session of PANIM from the Telegram app. Make sure that PANIM shows the warning mark on the account.
15. Set a proxy route with an IP address, for example Tor on 127.0.0.1:9050. Make sure that Telegram connects. Watch the connections of PANIM with `strace -f -e trace=%network` or with a firewall rule. Make sure that PANIM makes no connection past the proxy and no DNS lookup for Telegram.
16. Set a proxy route with a host name that is not `localhost`. Make sure that the account shows the error for a proxy name, and that `strace` shows no lookup of the name.
17. Make a proof for the Telegram account on the page "Proofs". Make sure that the block has the numeric id and the username. Post it in a public channel or link it from the bio, and examine it with "Pasted text".
18. Select Chinese in the settings. Make sure that the Telegram texts show in Chinese.
19. In the code step, make sure that the screen shows the number that you typed and the route of the code. If the route is "app", look in the chat "Telegram" of your Telegram apps, also in the archive.
20. In the code step, wait until the countdown ends. Click "Send the code again". Make sure that the screen shows the new route (for example an SMS) and a new countdown. If the screen says that there is no other route, make sure that it has no resend button.
21. In the code step, click "Use a different number". Enter your number again, and send it. Make sure that the code step shows the number and its route, with a new countdown.

**Complete (2026-11-17).** The person did the hand checks with their own Telegram account, and the milestone passed. On 2026-11-16, Telegram did not send the login code. On 2026-11-17, the code came with no change to PANIM (issue 1 of the repository). After the removal of the account, a new login also operated.

## 6. Milestone 10 in detail

`ARCHITECTURE.md` §3a and §11 tell how the backup and the package operate. `ARCHITECTURE.md` §4b tells how the Telegram app values operate. `DISCUSSION.md` §29 gives the decisions.

These parts are complete in code, with tests that use no network and no Telegram account:

1. The Telegram app values: PANIM's values from the build, the own values of the person in the settings (the sealed place `app/telegram`), and the record of the values in the secret of each account. A test opens a vault from milestone 7 and starts its account with PANIM's values.
2. The backup in the settings and the restore on the first start (`panim-vault/src/backup.rs`). The tests make a backup with an open TDLib folder, restore it, and refuse damaged and hostile archives.
3. The Nix package (`nix/package.nix`): `nix build`, `nix run`, `packages.panim-release` with the values from the environment, and the AppImage.
4. The script for the checksums and the signature (`nix/release.sh`, `nix run .#release`). A test used a key for tests only.
5. Version 0.1.0, `CHANGELOG.md` and `README.md`.

The agent built the AppImage with dummy values only (`api_id` 1). These are the hand checks of the person. Use a test vault (`PANIM_DATA`) for each check, except check 12.

1. In the repository, run `nix build`. Make sure that `result/bin/panim` exists. Run `ldd result/bin/.panim-wrapped | grep tdjson`, and make sure that it shows `libtdjson.so.1.8.68` in the store path of `tdlib-panim`.
2. Start `result/bin/panim` (or `nix run`). Make sure that the window opens. Do the first start. Save a key file, and make sure that the file dialog opens.
3. In this build, open "Add an account" → Telegram. Make sure that the dialog says "Telegram is not configured" and points to the settings. In the settings, make sure that "PANIM's app values" cannot be selected and that the text tells why.
4. In the settings, enter an `api_id` with a letter, then an `api_hash` with 31 digits. Make sure that PANIM refuses each. Enter correct own values and save. Make sure that only the `api_id` shows after the save. Make sure that the persona-link warning shows.
5. Add a Telegram account with the own values. Make sure that the dialog says "This login uses your own app values".
6. Make the release build: `PANIM_TELEGRAM_API_ID=… PANIM_TELEGRAM_API_HASH=… nix build --impure .#panim-release`. Start it with the same test vault. Make sure that the account of check 5 still connects. Select "PANIM's app values", and add a second Telegram account. In a Telegram app, open Settings → Devices, and make sure that the two sessions show different app names.
7. Start the build of check 1 (no values) with the same test vault. Make sure that the account with PANIM's values shows the warning about missing app values and does not connect. Make sure that the account with the own values connects.
8. In the settings, click "Back up…". Save the file. Make sure that the browser windows close, that the size shows, and that the Telegram accounts connect again.
9. Close PANIM. Start PANIM with a new empty `PANIM_DATA`. On the first screen, click "Restore from a backup". Make sure that the warning about the old computer shows. Select the file of check 8. Open the vault with the passcode. Make sure that the accounts, the timeline, a browser login and the Telegram sessions are there. Also try a key file as the backup, and make sure that PANIM refuses it.
10. Make the AppImage with the values: `nix build --impure .#appimage-release`. Copy it to a test system that is not NixOS, for example a virtual machine with Fedora or Debian. Start it. Make sure that the window opens, that the fonts show, that x.com loads, and that a Telegram login connects. If it does not start, try `--appimage-extract-and-run`, then `WEBKIT_DISABLE_DMABUF_RENDERER=1`, and record the result.
11. Select Chinese in the settings. Make sure that the Telegram app values, the backup and the restore show their texts in Chinese.
12. Before the release: register the app "PANIM" on my.telegram.org with a Telegram account that is only for this purpose (`DISCUSSION.md` §29). Put the values into `~/.config/panim/telegram.env`, not into the repository.
13. The person made the minisign key pair on 2026-11-17 (key id `7B5C5C4A551F160B`). The secret key is in `~/.minisign/panim.key`, outside the repository. `README.md` has the public key. After the tag, run `nix run .#release` with `--key ~/.minisign/panim.key` (`README.md`). On a different computer, check the download with `sha256sum -c SHA256SUMS` and `minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`.

The results of the hand checks on 2026-11-17:

- **Passed:** checks 1 to 9 and check 11. The coordinator did check 1. The person did the other checks.
- **Check 12 is complete.** The person registered the `api_id` of PANIM with a dedicated Telegram account. The values are in `~/.config/panim/telegram.env`.
- **Check 13:** the key pair exists. The signature waits for the tag.
- **Check 10 failed.** On Fedora, the AppImage stopped with "No such file or directory" for a path in `/nix/store`. The cause was in the `AppRun` of `nix-appimage`: on a system without `/nix/store`, it bound the wrong folder of the image (`DISCUSSION.md` §30). The patch `nix/appimage-apprun-store.patch` fixes it. A test on NixOS with `/nix` hidden passed. The person built the AppImage with the values again. On Fedora, it then started and operated (2026-11-17), so check 10 passed.

**Complete (2026-11-17).** The release steps:

1. `step-1` got the tag `v0.1.0` (commit `2092b42`). The first publication used commit `e2943bd`. It was published again without `docs/DISCUSSION.md` and the spikes (`DISCUSSION.md` §32). The private repository is now `SakuraRainn/panim-dev`. It has `step-1` and the tag.
2. The AppImage was built from the tag with the values of PANIM. The person signed `SHA256SUMS` with the key `7B5C5C4A551F160B`.
3. The public repository `SakuraRainn/panim` has one commit on `main` and the tag `v0.1.0`. The commit is the same as the source archive of the release. It has no history of `panim-dev`.
4. The release v0.1.0 of `SakuraRainn/panim` has the AppImage, the source archive, `SHA256SUMS` and `SHA256SUMS.minisig`.

For the next release, make the public commit from the source archive of the release (`git archive` of the tag). `.gitattributes` leaves out `AGENTS.md`, `CLAUDE.md`, `.githooks` and `docs/DISCUSSION.md`. Then add the commit on `main` of `SakuraRainn/panim`, with the private email of GitHub as the author.

## 7. Release 0.1.1

Release 0.1.1 has milestone 11 (`DISCUSSION.md` §34, §35). It has no new feature. It changes the documents, the release steps and one error message.

### Milestone 11: the documents and the release steps

Today, `README.md` has the steps of the maintainer next to the steps of the users. The public repository has no `AGENTS.md`, so a contributor has no rules.

1. `README.md` is for the users:
   - What PANIM does, and for whom. The accounts that PANIM supports, and how each one shows (timeline or browser view).
   - The download, the check of the download, and the start of the AppImage. The installation with Nix.
   - The first start: the passcode, the 12 recovery words and the key file.
   - The Telegram app values, the network route and the backup, in short.
   - The known limits, and where to ask for help (the issues of the public repository).
   - No build steps and no release steps. A link to `CONTRIBUTING.md`.
2. `CONTRIBUTING.md` is for the people who change PANIM:
   - The build and the tests (`nix develop`, `pnpm`, `cargo`), and `PANIM_DATA` for a test vault.
   - The rules from `AGENTS.md` that apply to all people: STE for the documents and the comments, each text of the user interface in English and Chinese, the rules of each platform (no scraping, no bot API for a user account), no change to a published file in `base/`.
   - The layout of the repository.
   - The Telegram app values for a build, and the rule that they never go into the repository.
   - How to report a problem.
   - How a change comes into PANIM. The public repository has only the releases, so it cannot merge a pull request. The maintainer applies the change in the development repository, with the name of the contributor (`Co-authored-by`). The change then comes with the next release.
   - The release steps of the maintainer (now in `README.md`).
3. `SECURITY.md`: how to report a vulnerability in private, not in a public issue. Use the private vulnerability reports of GitHub, if the person turns them on. Tell what PANIM protects (the vault, the sessions, the keys, the network route) and what it does not protect (`DESIGN.md`, the known limits).
4. Issue templates for the public repository (`.github/ISSUE_TEMPLATE/`): a problem report (the version, the system, the AppImage or Nix, the steps, the result) and a question. A link to `SECURITY.md` for a vulnerability.
5. `CONTRIBUTING.md`, `SECURITY.md` and `.github/` must be public. Do not mark them `export-ignore`. `README.md`, `CONTRIBUTING.md` and `SECURITY.md` also have a Chinese version (`README.zh-CN.md` and the others). The English file is the reference (`DISCUSSION.md` §38).
6. The public commit in the release script. Today, the maintainer makes it by hand (§6). A new step of `nix/release.sh` (or a second script) does it:
   - It takes the signed output folder and a clone of the public repository.
   - It replaces the files of the clone with the files of the source archive. It makes one commit and the tag on `main`.
   - The author is the private email of GitHub of the maintainer, which the script gets as an argument. It never uses the email of the development repository.
   - It examines the commit before the push: the files that `export-ignore` removes are not there, and the Telegram values are not there.
   - It does not push. It shows the push command.
7. The AppImage on Ubuntu 24.04 and later. Ubuntu stops unprivileged user namespaces with AppArmor, so the `AppRun` stops with "cannot unshare".
   - In the patch of the `AppRun`, when `unshare` fails, show a clear message: the cause, and a link to the section of `README.md`.
   - In `README.md`, give the safe workaround (an AppArmor profile for the AppImage), and the alternatives (Nix, `--appimage-extract-and-run` does not help, because it also needs the namespaces; verify this).
   - Test the message with a test that makes `unshare` fail, if possible.
8. Automatic tests: a GitHub Actions workflow in `panim-dev` runs `pnpm check`, `cargo fmt`, `cargo clippy` and `cargo test` for each push to `step-1` and each pull request (`DISCUSSION.md` §36). All the tools come from Nix, also Rust (`DISCUSSION.md` §40).
9. Version 0.1.1 and `CHANGELOG.md`.
10. Hand checks: the person reads `README.md`, `CONTRIBUTING.md` and `SECURITY.md`. A person who does not know PANIM must be able to install it from `README.md` alone. The person turns on the private vulnerability reports in the settings of the public repository.
11. The release: the tag, the AppImage, the signature and the public commit (with the new script), as for release 0.1.0 (§6).

**Complete (2026-11-18).** The person read the three documents and accepted them. The person turned on the private vulnerability reports. The automatic tests passed on the pull request and on `step-1`. The release steps:

1. `step-1` got the tag `v0.1.1` (commit `3525cb5`). The tag is also in `SakuraRainn/panim-dev`.
2. The AppImage was built from the tag with the values of PANIM. The person signed `SHA256SUMS` with the key `7B5C5C4A551F160B`.
3. `nix run .#publish` made the public commit `a465b7f` on top of the commit of 0.1.0. The script examined the commit, and the coordinator pushed it with the tag `v0.1.1`.
4. The release v0.1.1 of `SakuraRainn/panim` has the AppImage, the source archive, `SHA256SUMS` and `SHA256SUMS.minisig`.

## 8. Release 0.1.2

Release 0.1.2 has milestone 15 (`DISCUSSION.md` §44, §45). It has no new feature. It repairs three known limits of release 0.1.0 and three problems that a review of the code found. Thus, it is a patch release, and it comes before release 0.2.0.

### Milestone 15: six repairs

Each repair has its own subagent, worktree and branch (`AGENTS.md` §7). The repairs change different files, so the work can occur at the same time.

The six repairs are complete (2026-11-18). The automatic tests passed on the merge of each group of three branches. Repairs 1 and 6 wait for their hand checks.

1. **The automatic lock counts the input in all the windows.**
   - Today, only the input in the main window resets the time of the automatic lock (`ARCHITECTURE.md` §2). A person who works in a browser window gets a lock, and the browser windows close.
   - The Rust side gets the key, button and scroll events of each account window from GTK, before WebKit gets them.
   - The script of the page does not send these events. Thus, a page cannot stop the automatic lock.
   - The Rust side sends the event `input-elsewhere` to the main window, no more than one time in 10 seconds. The timer in `Main.svelte` resets on this event.
   - Tests: the limit of one event in 10 seconds, without Tauri.
   - Hand check: work in an X window for more time than the lock time. Make sure that PANIM does not lock. Then stop the input, and make sure that PANIM locks.
   - On macOS, the events come from a different source. Milestone 12 must do this part again.
2. **PANIM makes weak passcode settings stronger.**
   - The header of the vault keeps the settings of Argon2id. Today, PANIM opens a vault with all settings, also with settings below `KdfParams::STANDARD`.
   - `Vault::open` gets the settings of the app. When the passcode opens a vault with weaker settings, PANIM seals the master key again with the settings of the app. The passcode does not change.
   - A change to the header cannot make the seal weaker, because the old seal does not open with different settings. Thus, the repair is for old or test vaults.
   - PANIM refuses settings above a maximum (for example 4 GiB of memory or 64 passes). Thus, a changed header cannot use all the memory of the computer.
   - Tests: a vault with weak settings opens and gets the settings of the app. The passcode still opens it. A header with too large settings gives `VaultError::Damaged`.
3. **A failed write does not change the data in memory.**
   - Today, `SealedStore` and `SealedSecrets` change the data in memory, then write it. If the write fails, the data in memory is different from the data on the disk.
   - Each change keeps a copy of the part that it changes: the meta, the items of one account, the proofs or the table of the secrets. If the write fails, the change puts the copy back and gives `Error::Storage`.
   - `Vault::write` writes a temporary file and then renames it. Thus, each file on the disk is complete.
   - `remove_account` writes more than one file. It writes the meta first. An items file or a proof without its account stays on the disk, but `SealedStore::open` ignores it.
   - Tests: a test hook in `Vault` makes the next write fail. After the failure, the data in memory is the same as before the change, and the store opens again with the same data.

4. **A key file has limits for its settings.**
   - `keyfile::import` reads the settings of scrypt (`n`, `r`, `p`) and of pbkdf2 (`c`) from the file. Today, they have no maximum, and `as u32` can cut a large value.
   - Refuse a file above these limits: `log_n` 20, `r` 16, `p` 16, `c` 10 000 000. Use `u32::try_from`, not `as`. The error is the usual error of a bad key file.
   - Compare the MAC in constant time.
   - Tests: a file above each limit gives the error quickly. The test vectors of the key file still pass.
5. **The unread GitHub notifications show.**
   - The command `timeline` asks the store for 500 items. Then `Session::timeline` removes the GitHub events, because they have no read state.
   - With the filter "unread", many events can fill the 500 places. Then older unread notifications do not show.
   - Remove the items without a read state before the limit, not after it.
   - `sync_state` still copies the unread items to count them. A count without a copy needs a new method of the store, so it waits (`DISCUSSION.md` §45).
   - Tests: an account with more than 500 events and one older unread notification. The filter "unread" shows the notification.
6. **An old answer does not replace a new timeline.**
   - `load()` in `Main.svelte` runs on each change of the filter and on each event `timeline-changed`. It makes three calls, and nothing stops an older call.
   - A slow older answer can replace a newer answer, for example after a change of the account filter.
   - Give each call of `load()` a number. Ignore an answer that is not from the latest call.
   - Hand check: change the account filter fast while an account syncs. The timeline shows the selected account.
   - Known limit: each new call stops the older calls. During many Telegram updates, the timeline can stay the same until the updates stop. Repair 5 of milestone 16 sends fewer events.

Then:

1. Change `ARCHITECTURE.md`, the known limits in `README.md` and `README.zh-CN.md`, and §12 of this document. In `ARCHITECTURE.md`, tell that `gate.json` counts only the passcodes that a person types in PANIM. A person who can change the files can remove it.
2. Version 0.1.2 and `CHANGELOG.md`.
3. Hand checks: the lock check of repair 1 and the filter check of repair 6. A vault from release 0.1.1 opens with release 0.1.2, and the timeline, the accounts and the Telegram sessions are there.
4. The release: the tag, the AppImage, the signature and the public commit, as for release 0.1.1 (§7).

The results of the hand checks on 2026-11-18:

- **Passed:** the lock check of repair 1, with an X window. The vault of release 0.1.1 opened, and the accounts, the timeline, a browser login and the Telegram sessions were there. It opened again after a restart.
- **Not done:** the filter check of repair 6. The person could not make the problem occur. The commands `accounts`, `timeline` and `sync_state` are not async, so Tauri runs them on the main thread in their order. Thus, a newer call always gets the last answer, and the problem cannot occur today. Repair 1 of milestone 16 can make these commands async. Then the problem can occur, so milestone 16 does this hand check.
- The automatic tests passed on `main` (2026-11-18).

## 9. Release 0.1.3

Release 0.1.3 has milestone 16 (`DISCUSSION.md` §45). It has no new feature. It changes how PANIM seals the open folders.

### Milestone 16: the seal of the open folders

1. **No freeze of the window.**
   - Every five minutes, a thread locks the session and seals the browser profile and the TDLib folders. The window waits for the same lock, so it stops during the seal.
   - Make the archive and write it outside the lock of the session. Make sure that two seals of one folder cannot occur at the same time, and that the lock of the vault waits for a seal.
   - Argon2id and scrypt also run under the lock (unlock, recovery, a change of the passcode). Move them outside the lock, or make these commands async.
2. **No seal when nothing changed.** Compare the paths, sizes and modification times with the last seal. If they are the same, do not seal.
3. **A correct copy of the SQLite files.**
   - WebKit and TDLib write SQLite files while the folder is open. The seal copies the file and its WAL file at different times, so the copy can be damaged.
   - The seal at the lock and at the close is correct, because the windows close first. The problem is the periodic seal, which PANIM restores after a crash.
   - Examine the files of WebKit and TDLib. Then select a solution, for example the backup interface of SQLite. Record the choice in `DISCUSSION.md`.
   - Test: stop PANIM with an open window during a seal. Start it again. The logins are there.
4. **Less memory.** Today, the seal keeps the archive and its sealed copy in memory together. Examine a stream into the cipher. This part is optional.
5. **Fewer events from Telegram.**
   - Each Telegram update writes the items file of the account three times, and it always sends `timeline-changed`. Then the main window makes three calls.
   - Write the file one time for each update. Send the event only when the timeline changed.
6. Version 0.1.3, `CHANGELOG.md`, the hand checks and the release, as for release 0.1.2. The hand checks include the filter check of milestone 15, repair 6 (§8).

## 10. Release 0.2.0

Release 0.2.0 has milestone 12. A new system is a new feature, so the version goes to 0.2.0 (`DISCUSSION.md` §34).

### Milestone 12: macOS

This is a large milestone. Tauri uses WKWebView on macOS, not WebKitGTK. Some parts of PANIM depend on Linux. Do the checks first, as for Linux (`ARCHITECTURE.md` §9).

Checks before we build:

1. **The browser profile.** Can each persona have its own data store in WKWebView, through Tauri and `wry`? Can PANIM put the data store in a folder that it seals, as on Linux? If WKWebView keeps the data in a folder of the system, the seal of the profile must change.
2. **The proxy of a profile.** Does WKWebView send all the traffic of a profile through an HTTP or a SOCKS5 proxy (macOS 14 or later)? Does the DNS lookup stay in the proxy?
3. **The memory area.** macOS has no `$XDG_RUNTIME_DIR` on `tmpfs`. Find a place where the open profiles and the TDLib folders stay in memory, or record the limit.
4. **TDLib.** Build the TDLib of PANIM with its patch for macOS (`aarch64-darwin` and `x86_64-darwin`). Do the checks with `strace` again with the tools of macOS.
5. **The unread counts.** Make sure that the title and the notification script operate in WKWebView.

### A build without Nix

Release 0.2.0 also gets a usual build, and the Nix package stays on top of it (`DISCUSSION.md` §40):

1. Linux: the libraries of the system (for example the WebKitGTK packages of Ubuntu), `rustup` and `pnpm`. A script builds the TDLib of PANIM from the source with its patch.
2. The packages from the bundler of Tauri (`.deb`, `.rpm`, AppImage on Linux; `.app` and `.dmg` on macOS), with the patched TDLib in them. Do the checks of the release 0.1.0 AppImage again (Fedora, the signature).
3. The Nix package (`nix/package.nix`) stays. It uses the same TDLib patch.
4. `CONTRIBUTING.md` gives the two ways to build.

Then build:

1. The app on macOS, with the same features as on Linux.
2. The package: an `.app` and a `.dmg`, from Nix if possible. The checksums and the signature of the release, as on Linux.
3. The signature and the notarization of Apple, if the person has an Apple Developer account. If not, record how a user starts an app that has no signature.
4. The hand checks of milestones 3 to 7 and 10 on a Mac.

## 11. Release 0.3.0

Release 0.3.0 comes after release 0.2.0. It adds two platforms (`DISCUSSION.md` §42). Do milestone 13 first, because it is smaller.

### Milestone 13: Bluesky

`DISCUSSION.md` §42 and §43 give the decisions.

1. Proof format version 2 (`base/panim-proof-v2.md`):
   - Add the domains `panim/2-proof` and `panim/2-revoke` and the platform `bluesky` to `panim-core`.
   - Add the worked example of version 2 §6. Calculate it with the code, and keep it as a test vector.
   - A checker of version 2 also examines version 1 blocks.
2. The crate `panim-bluesky`. Its tests use a fake HTTP client and no network, as in `panim-github`.
3. The sign-in. Examine the OAuth of the AT Protocol and the app passwords. Record the choice in `DISCUSSION.md`.
4. The notifications and the posts of the followed accounts in the timeline. Answers and new posts.
5. The direct messages (`chat.bsky.*`), if the sign-in gives access to them.
6. The proof screens: show the block, and tell the person to post it and pin it. The check gets the post from its URL through the public API and compares the DIDs.
7. Hand checks with a test account: the login, the timeline, an answer, a proof post and its check.

### Milestone 14: Email services

Before the work starts, decide on these two parts (`DISCUSSION.md` §45):

- **The storage of the items.** Today, each change writes the full file of the items of an account. A mailbox can have thousands of items, so this does not scale. Select a new layout of the sealed store.
- **The lock of the session.** One lock holds all of the session. Each connector adds more work under it.

1. Check 3: IMAP and SMTP on TLS through an HTTP proxy, a SOCKS5 proxy and Tor. The DNS lookup must stay in the proxy.
2. The email connector: the login with an address and a password or an app password, IMAP IDLE, mail in the timeline and mail out.
3. The settings of the usual services (the host names and the ports). The person can also enter other values.
4. Hand checks with a Fastmail or mailbox.org test account, and with a Gmail account and an app password.

## 12. Open tasks outside the milestones

- Add the row of PANIM (project 1) to the family derivation table of ZIKARON (`crates/app/src/family.rs` in the ZIKARON repository). In step 3, ZIKARON must read the persona key at project 1, or take it from the key file (`DISCUSSION.md` §16).
- Delete the old binary `target/release/partzufim`. The binary is now `target/release/panim`.
- Known limits from milestones 2 and 3 (`ARCHITECTURE.md` §2a, §9):
  - The automatic lock counts only input in the main window. Release 0.1.2 repairs it (§8).
  - If a write to the disk fails, the data in memory can be different from the data on the disk until the store opens again. Release 0.1.2 repairs it (§8).
  - There is no check yet for passcode settings that are below the standard. Release 0.1.2 repairs it (§8).
  - If the computer has swap, the kernel can write the profiles in `$XDG_RUNTIME_DIR` to the swap.

## 13. How to start work in a new session

1. Read `AGENTS.md`, then this document.
2. Read `DESIGN.md` and `ARCHITECTURE.md` for the current agreement.
3. Read the end of `DISCUSSION.md` for the latest decisions.
4. Run `git log --oneline` on the branch `main`.
5. Start the next milestone, or the open task that the person asks for.
