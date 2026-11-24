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
| 12 | macOS: the app, the browser profiles, TDLib and the package on macOS | Started on `next-0.2`. Release 0.2.0 (§10). This is a large milestone | A Mac for the tests. An Apple Developer account for the signature of the app (optional) |
| 13 | Bluesky: the timeline, the notifications, posts and answers, the direct messages. Proof format version 2 and the automatic check of a proof post | Not started. Release 0.3.0 (§11) | — |
| 14 | Email services through IMAP/SMTP: mail in the timeline, mail out, IMAP IDLE. Check 3: IMAP/SMTP through the proxy | Not started. Release 0.3.0 (§11) | — |
| 15 | Six repairs: the automatic lock in all the windows, stronger passcode settings, no change in memory after a failed write, limits for a key file, the unread GitHub notifications, the order of the timeline answers | Complete. Release 0.1.2 is public (2026-11-18, §8) | — |
| 16 | The seal of the open folders: no freeze of the window, fewer Telegram writes, no seal when nothing changed, a correct copy of the SQLite files | Complete. Release 0.1.3 is public (2026-11-19, §9) | — |

Release 0.1.0 has GitHub and Telegram in the shared timeline, and X, Discord and Proton Mail in browser views. Release 0.1.1 has milestone 11. Release 0.1.2 has milestone 15 (`DISCUSSION.md` §44, §45). Release 0.1.3 has milestone 16 (`DISCUSSION.md` §45, §46). Release 0.2.0 has milestone 12 (`DISCUSSION.md` §33, §34). Release 0.3.0 has milestones 13 and 14 (`DISCUSSION.md` §42). Milestones 8 and 9 come later.

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
10. Make the AppImage with the values: `nix build --impure .#appimage-release`. Copy it to a test system that is not NixOS, for example a virtual machine with Fedora or Debian. Start it. Make sure that the window opens, that the fonts show, that x.com loads, and that a Telegram login connects. If it does not start, try `--appimage-extract-and-run`, then `WEBKIT_DISABLE_DMABUF_RENDERER=1`, and record the result. (Release 0.2.0 removed this target, `DISCUSSION.md` §50.)
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

**Complete (2026-11-18).** The release steps:

1. `main` got the tag `v0.1.2` (commit `7d708f8`). The tag is also in `SakuraRainn/panim-dev`.
2. The AppImage was built from the tag with the values of PANIM. The person signed `SHA256SUMS` with the key `7B5C5C4A551F160B`.
3. `nix run .#publish` made the public commit `05f3d92` on top of the commit of 0.1.1. The script examined the commit, and the person pushed it with the tag `v0.1.2`.
4. The release v0.1.2 of `SakuraRainn/panim` has the AppImage, the source archive, `SHA256SUMS` and `SHA256SUMS.minisig`.
5. The public commit also has the draft `base/panim-proof-v2.md` and the plan of release 0.3.0. The draft tells that it is not published.
6. The `CHANGELOG.md` of the tag tells that repair 6 stops a problem that a person can see. This is not correct, because the problem cannot occur in release 0.1.2. The release notes on GitHub and the `CHANGELOG.md` of `main` have the correct text.

## 9. Release 0.1.3

Release 0.1.3 has milestone 16 (`DISCUSSION.md` §45, §46). It has no new feature. It changes how PANIM seals the open folders, and which thread waits for the lock of the session.

### Milestone 16: the seal of the open folders

The examination of the code and of the open folders (2026-11-18) gave these results (`DISCUSSION.md` §46):

- **The cause of the freeze.** The commands with a passcode are already async. But the commands `status`, `accounts`, `timeline`, `unread`, `page_notice` and `sync_state` are not async, so Tauri runs them on the main thread. The title handler of an account window also runs on the main thread. They all wait for the lock of the session. During the periodic seal or Argon2id, the main thread waits, and the windows stop.
- **The browser profile.** The files of local storage and of the service workers are SQLite files in WAL mode. `hsts-storage.sqlite` is a SQLite file with a rollback journal. IndexedDB, when a site uses it, also has SQLite files in WAL mode. `cookies` is a text file, not a SQLite file.
- **The TDLib folder.** The folder `db` has only `td.binlog`, because PANIM sets the three databases of TDLib to false. TDLib adds entries to the end of the binlog, and each entry has a checksum. When TDLib writes the binlog again, it writes a new file and renames it. Thus, a copy during a write loses no more than the last entry.

The work has four parts. Each part has its own subagent, worktree and branch (`AGENTS.md` §7). Parts A and D can occur at the same time. Part B starts after part A. Part C starts after part B, because parts B and C change the same code.

1. **Part A: the main thread does not wait for the lock of the session.**
   - Make the commands `status`, `accounts`, `timeline`, `unread`, `page_notice` and `sync_state` async. They do their work with `blocking`, as the other async commands do. Examine the other commands that are not async, and move each one that locks the session.
   - The title handler of an account window does not lock the session on the main thread. It sends the title to a thread, and that thread changes the session.
   - Then the main thread never waits for the lock. During a seal or Argon2id, a list in the window can show its data later, but the window does not stop.
   - Thus, the move of Argon2id and scrypt outside the lock is not necessary.
   - Async commands can give their answers in a different order. The number of each `load()` call (milestone 15, repair 6) then becomes necessary. Do the filter check of repair 6 in the hand checks.
   - Tests: the tests of `Session` stay the same. Run `pnpm check`.
2. **Part D: fewer writes and events from Telegram.**
   - Today, each Telegram update writes the items file of the account three times, and it always sends `timeline-changed`. Then the main window makes three calls.
   - Add one method of the store that adds items, marks items as read and removes items in one write. It tells if the items changed.
   - Send `timeline-changed` only when the items changed.
   - Tests: an update with no change writes no file and sends no event. An update with all three changes writes the file one time. After a failed write, the data in memory is the same as before (milestone 15, repair 3).
3. **Part B: no seal when nothing changed, and a slow seal for the hand checks.** Parts A and D are complete (2026-11-18). Parts B and C are complete (2026-11-19). The seal stays under the lock of the session (`DISCUSSION.md` §47).
   - Before a periodic seal of a browser profile or a TDLib folder, compare the paths, sizes and modification times of the files with the last seal. If they are the same, do not seal.
   - The seal at the lock of the vault, at the backup and at the stop of Telegram always occurs.
   - A test setting `PANIM_TEST_SLOW` makes each seal and each unlock wait for the given number of seconds under the lock of the session. It also makes the commands `timeline`, `accounts` and `sync_state` wait. Only a debug build has this setting. A release build does not compile it.
   - Tests: a seal with no change writes no file. A change to one file makes a seal. A new file and a removed file make a seal.
4. **Part C: a correct copy of the SQLite files of WebKit.**
   - The seal reads the first 16 bytes of each file. For a SQLite file, it makes a copy with the backup interface of SQLite, through the crate `rusqlite` with the SQLite of the system (`DISCUSSION.md` §48). The copy stays in memory. The archive holds the copy. It does not hold the `-wal`, `-shm` or `-journal` file of that database.
   - The seal copies the other files as before, for example `cookies` and `td.binlog`.
   - If the backup of a database fails, the seal copies the file as before and logs the failure. One bad database must not stop the seal of the profile.
   - The format of the archive does not change. An archive from release 0.1.2 opens with release 0.1.3.
   - Tests: a test profile has a SQLite database in WAL mode. A second thread writes to it during the seal. Open the archive, and run `PRAGMA integrity_check` on the copy. The copy has all the rows of a complete write.
5. **Not in release 0.1.3:** a stream into the cipher. It needs a new format of the sealed file, with the data in parts. A change of the format is not for a patch release. Record it for a later release.
6. **Then:**
   1. Change `ARCHITECTURE.md` (the seal of the open folders, the threads of the commands) and §12 of this document.
   2. Version 0.1.3 and `CHANGELOG.md`.
   3. Hand checks:
      - Start a debug build with `PANIM_TEST_SLOW=10`. Unlock, and move a browser window during the unlock. The windows do not stop.
      - In the same build, use an X window during a periodic seal. The window does not stop.
      - In the same build, do the filter check of milestone 15, repair 6 (§8).
      - Stop PANIM with an open X window, a short time after a periodic seal. Start it again. The login and the local storage of X are there.
      - A vault from release 0.1.2 opens with release 0.1.3, and the accounts, the timeline and the Telegram sessions are there.
   4. The release, as for release 0.1.2 (§8).

The hand check of parts A to D (2026-11-19): the person started a debug build with `PANIM_TEST_SLOW=10`. The window did not stop during the unlock and during a periodic seal. The filter check of milestone 15, repair 6 passed. The logins and Telegram were there after a lock. A review by a second agent examines parts A to D.

The review found seven problems. Five repairs are in `main`, and `DISCUSSION.md` §49 records the other limits (2026-11-19). `ARCHITECTURE.md`, the version 0.1.3 and `CHANGELOG.md` are complete. The results of the last hand checks on 2026-11-19, all passed:

- Two fast changes of the settings stayed.
- The notice of a page went away when the window got the focus, and no notice stayed after the window closed.
- PANIM stopped with `pkill -9` after a periodic seal. After a new start, the login to X was there.
- The vault of release 0.1.2 opened, and the accounts, the timeline and the Telegram sessions were there.

**Complete (2026-11-19).** The release steps:

1. `main` got the tag `v0.1.3` (commit `d4f52ec`). The tag is also in `SakuraRainn/panim-dev`.
2. The AppImage was built from the tag with the values of PANIM. The person signed `SHA256SUMS` with the key `7B5C5C4A551F160B`.
3. `nix run .#publish` made the public commit `44d700c` on top of the commit of 0.1.2. The person pushed it with the tag `v0.1.3`.
4. The release v0.1.3 of `SakuraRainn/panim` has the AppImage, the source archive, `SHA256SUMS` and `SHA256SUMS.minisig`.

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

#### The inventory of the code (2026-11-19)

The coordinator read the code of `next-0.2` with Tauri 2.12.5 and `wry` 0.57.2. No Mac was used. These parts depend on Linux.

Problems that stop PANIM or make it unsafe on macOS:

1. **The browser profile** (`main.rs`, `.data_directory(profile)` in the two account windows).
   - WKWebView does not support `data_directory`. Tauri gives `data_store_identifier([u8; 16])` for macOS 14 and later.
   - macOS then keeps the data in its own folder, not in the profile area of PANIM. Thus, the seal of Linux does not apply.
   - `wry` can list and delete the data stores (`fetch_data_store_identifiers`, `remove_data_store`). A seal that keeps the logins needs check 1 first.
   - This is the largest risk of the milestone.
   - **Done in the code (2026-11-24). Not tested on a Mac.** On macOS, the two windows use `data_store_identifier`. When the profile opens, the session makes the folder of the store a symbolic link to the open profile folder, and it deletes the link at the close (`DISCUSSION.md` §54). The tests on Linux examine the link with a temporary folder.
2. **The memory area** (`panim-vault/src/profile.rs`, `ProfileArea::runtime`; `main.rs`, the start).
   - The area needs `XDG_RUNTIME_DIR`. macOS does not set it, so PANIM stops at the start.
   - The TDLib folders also use this area. Make a macOS area after milestone 16, because milestone 16 changes this code.
   - **Done in the code (2026-11-24). Not tested on a Mac.** On macOS, `src-tauri/src/ramdisk.rs` makes a RAM disk of 2 GiB at the start and mounts it in `$TMPDIR` with `nobrowse` (`DISCUSSION.md` §53). The exit detaches it after the lock. The tests on Linux examine only the parse of the output of `hdiutil` and `diskutil`.
3. **The proxy of a profile** (`main.rs`, `.proxy_url` in the two account windows).
   - On macOS, it compiles only with the feature `macos-proxy` of Tauri, and it operates only on macOS 14 and later.
   - Without the feature, the profiles do not use the route of the persona. Thus, the minimum is macOS 14.
   - **Done in the code (2026-11-19). Not tested on a Mac.** `Cargo.toml` sets `macos-proxy` only for macOS. `bundle/macos.json` sets macOS 14.0 as the minimum.
   - **Done in the code (2026-11-24). Not tested on a Mac.** `src-tauri/Info.plist` sets `NSAllowsArbitraryLoadsInWebContent`, so that plain `http://` pages load (`DISCUSSION.md` §55, "The plain HTTP failure").
4. **The OpenSSL of TDLib** (`scripts/build-tdlib.sh`).
   - On macOS, the script links the OpenSSL of Homebrew as a shared library. The `libtdjson` in the `.app` then needs a Homebrew folder that users do not have.
   - Link OpenSSL statically on macOS. Make sure with `otool -L`. macOS supplies zlib.
   - **Done in the script (2026-11-19). Not tested on a Mac.** The script sets `OPENSSL_USE_STATIC_LIBS`. After the build, it stops if `otool -L` shows a library in `/opt/homebrew`, `/usr/local` or `/nix`.
   - **Checked on the macOS runner (2026-11-20).** `libtdjson` has no path of Homebrew. The objects of OpenSSL are for macOS 15.0 (see "The first run on the macOS runner").
   - **Done in the script (2026-11-20). Not tested on a Mac.** The script builds OpenSSL 3.5.9 from source for macOS 14, because the objects of Homebrew were for macOS 15.0 (`DISCUSSION.md` §52). It stops if an object of OpenSSL has a minimum above 14.0. The workflow does not install openssl@3.

Parts that operate on macOS, but differently from Linux:

5. **The input for the automatic lock** (`main.rs`, `watch_input`). The handler uses GTK. On macOS, the function does nothing, so the input in a browser window does not reset the lock. Use an `NSEvent` local monitor.
   - **Done in the code (2026-11-19). Not tested on a Mac.** `watch_app_input` installs one local monitor in `setup`. It counts the input of all the windows, but not of the main window.
6. **The vault folder** (`main.rs`, `data_dir`). It uses `~/.local/share/panim`. The usual folder of a macOS app is `~/Library/Application Support/panim`.
   - **Done in the code (2026-11-19). Not tested on a Mac.** On macOS, `data_dir` gives `~/Library/Application Support/panim`. `PANIM_DATA` has priority, as on Linux (`DISCUSSION.md` §51).
7. **The notification script** (`unread.js`). It replaces `window.Notification`. WKWebView supports this interface differently. Check 5 examines it.
8. **The unread count from the title** (`main.rs`, `on_document_title_changed`). Tauri gives no platform limit for it. Check 5 examines it.

Parts that need no change:

- The Unix file functions of `panim-vault` (the modes of the folders and files). macOS is a Unix system.
- The link to TDLib (`#[link(name = "tdjson")]`), and the run path `@executable_path/../Frameworks` in `bundle/macos.json` and `scripts/build-packages.sh`.
- The user interface. It has no text or key that is only for Linux.
- The patch of TDLib. It changes code that all the systems use.

Other notes:

- The flake builds only for Linux. The macOS build uses the build without Nix (`DISCUSSION.md` §40).
- `tauri.conf.json` gives only PNG icons. The bundler made `PANIM.icns` from them in the first run on the macOS runner.
- The architecture is not decided yet. The macOS runners of GitHub use Apple silicon, so start with `aarch64`.
- Release 0.1.3 added `rusqlite` with the SQLite of the system (`DISCUSSION.md` §48). On macOS, it links the `libsqlite3` of the system. Examine which SQLite WKWebView uses, and whether the seal of item 1 needs the backup interface of SQLite.

The sequence of the work:

1. Now, on `next-0.2`: items 3, 4, 5 and 6. They do not change the seal.
   - **Done in the code (2026-11-19).** Nobody ran this code on a Mac. On Linux, `cargo test` passed (262 passed, 0 failed), and the behavior did not change.
   - A check from Linux compiled the code of the crate `panim` for `aarch64-apple-darwin` with `cargo clippy`. This check does not link, and it did not compile the C part of `objc2`.
2. On the macOS runners of GitHub (the person agrees to the minutes first): the five checks, and items 1, 3, 7 and 8.
   - **Done (2026-11-24)** in runs 44660826017 and 45249682616. All the checks gave a result. Check 4 has no check of the direct connections yet.
3. After milestone 16 (release 0.1.3): items 1 and 2, with the decisions from the checks in `DISCUSSION.md`.
   - Release 0.1.3 is merged into `next-0.2` (2026-11-23). The decisions are in `DISCUSSION.md` §53 to §55.
4. On a Mac: the hand checks, then the signature of Apple.

#### The checks on the macOS runners

The workflow `.github/workflows/macos-check.yml` does step 2 of the sequence above. Only a push to the branch `macos-check` starts it.

- To start a run, push `next-0.2` to `macos-check`: `git push origin next-0.2:macos-check`.
- Each run uses the minutes of a macOS runner, and a macOS minute costs approximately 10 times a Linux minute. Start a run only when the person agrees.
- The first run builds TDLib and takes approximately 45 to 60 minutes. A run with the caches takes approximately 20 to 25 minutes. The time limit is 75 minutes.

The run does these steps on `macos-15` (Apple silicon):

1. The spike `spikes/macos-webview` (checks 1, 2 and 5, items 1, 3, 7 and 8). Its `README.md` tells each check.
2. A RAM disk with `hdiutil` (check 3, item 2).
3. The TDLib of PANIM with `scripts/build-tdlib.sh`, and an examination with `otool` (part of check 4). The check of the direct connections is not in this run. The `README.md` of the spike gives a method.
4. `pnpm check`, `cargo fmt`, `cargo clippy` and `cargo test`, without Nix.
5. The `.app` and the `.dmg` with `scripts/build-packages.sh`. The run examines the libraries, the run paths, the minimum of macOS and the signature. Then it starts PANIM for 15 seconds, and a second time for 10 seconds with the same data folder. The two starts examine the RAM disk of PANIM (`DISCUSSION.md` §53): its folder, its mount with `nobrowse`, its device file and its use again after a kill.

The third run also adds two spike checks: `data_store.link_after_close` (`DISCUSSION.md` §54) and `ramdisk.memory` (`DISCUSSION.md` §53).

The results are in the artifacts of the run. `macos-check-report` has the logs and the spike results, and `macos-dmg` has the `.dmg`. GitHub keeps them for 7 days. The page of the run also shows each `CHECK` line.

#### The first run on the macOS runner (2026-11-20)

Run 44660826017 used commit `e4d5e0b` on macOS 15.8.2 (arm64, 3 cores, 7 GB), with Xcode 16.4 and Rust 1.97.1. It took 30 minutes. The result was "failure", because two steps failed. The other steps passed.

The time of the long steps:

- The spike: 6 minutes. The build of TDLib: 15 minutes, without a cache.
- `cargo clippy`: 70 s. `cargo test`: 76 s. The `.app` and the `.dmg`: 3.5 minutes.

The steps that failed:

1. **`cargo test`.** The test binary of the crate `panim` did not compile: "symbol `_EMBED_INFO_PLIST` is already defined". The test module `acl` calls `generate_context!` a second time, and on macOS each call adds this symbol. No test ran on macOS.
   - Repaired (2026-11-20): the test module uses `generate_context!(test = true)`. Not tested on a Mac.
2. **The examination of the `.app`.** The workflow called `lipo -archs` with two files, and `lipo` accepts only one. Thus, the step stopped before the `codesign` lines and the `package.*` checks.
   - Repaired (2026-11-20) in the workflow. The lines before the error give the facts below.

TDLib (check 4, item 4):

- PASS: `libtdjson` needs only `libz`, `libc++` and `libSystem` of the system. It has no path of Homebrew or Nix. Thus, the static OpenSSL operates.
- PASS: `minos` is 14.0 (SDK 15.5). The size is 27 MB.
- A risk: `ld` gave warnings that the objects of `libcrypto.a` of Homebrew (OpenSSL 3.5.9) are for macOS 15.0. The library says 14.0, but this OpenSSL can use functions that macOS 14 does not have. Nobody started it on macOS 14.
  - Repaired (2026-11-20): the script builds OpenSSL 3.5.9 from source for macOS 14 (item 4). Not tested on a Mac.

The `.app` and the `.dmg`:

- The bundler made `PANIM.app` (39 MB) and `PANIM_0.1.2_aarch64.dmg` (15 MB). It also made `PANIM.icns` from the PNG icons.
- `Contents/Frameworks` has `libtdjson`. The binary has one run path, `@executable_path/../Frameworks`, and it needs only frameworks of the system and `@rpath/libtdjson`.
- `LSMinimumSystemVersion` is 14.0. `minos` of the binary and of `libtdjson` is 14.0.
- `ld` said "duplicate -rpath '@executable_path/../Frameworks' ignored". The bundler and `PANIM_TDJSON_RPATH` both add this run path. The result is correct.
- Not examined: the signature (`codesign`), because the step stopped before it.

The start of 15 s:

- PASS: PANIM ran for 15 s. The screenshot shows the main window with the first step of the welcome page.
- `smoke.log` is empty. The data folder was empty after 15 s.

The spike (checks 1, 2, 3 and 5):

- PASS: the title, the notification script, and the write, persistence and isolation of the data stores.
- The data store of an identifier is in `~/Library/WebKit/<bundle id>/WebsiteDataStore/<uuid in lowercase>`. The app also has `~/Library/Caches/<bundle id>`.
- PASS: a seal of the folder only. After a delete of the folder, the data is gone. After a copy back, the data is there.
- FAIL: `fetch_data_store_identifiers` and `remove_data_store`. The child process stopped with SIGSEGV each time (4 of 4). The spike summary says "None" and "no answer", but the cause is the crash. `remove_data_store` deleted the folder before the crash.
- PASS: the proxy with HTTPS, through an HTTP proxy and a SOCKS5 proxy. SOCKS5 got the host name (`atyp=domain`).
- FAIL: the proxy with plain HTTP. See "The plain HTTP failure" below.
- Not valid: the DNS check. See "The DNS check" below.
- INFO: a RAM disk with `hdiutil` operates. It has its own device, and it can mount with `nobrowse`.

#### The plain HTTP failure

The logs show these facts:

- Each load of an `http://` URL with a host name failed. This is true with the HTTP proxy, with the SOCKS5 proxy and without a proxy (`dns-control`).
- For these loads, the child printed no `LOAD started` line. The proxy logs have no line, and `tcpdump` saw no DNS lookup of the name.
- The loads of `http://127.0.0.1:<port>` operated. The log of the test server has each request.
- The `Info.plist` of the spike has no key `NSAppTransportSecurity`.

Thus, WebKit did not send the request to the network. App Transport Security (ATS) is a possible cause, because ATS does not apply to an IP address. The logs do not show that ATS is the cause. The spike does not record the error of a failed navigation.

A check for the next run:

1. Collect the system log of the time of the HTTP loads: `log show --last 10m --predicate 'eventMessage CONTAINS[c] "App Transport Security"'`.
2. Load the two `http://` pages again in a second `.app` with `NSAllowsArbitraryLoadsInWebContent` set to true.
3. Add a navigation handler to the child that prints each URL, so that the log shows if `wry` got the request.

If the pages load in step 2, the cause is ATS. Then PANIM must decide if the browser windows can open plain HTTP pages.

The second run showed that ATS is the cause. `src-tauri/Info.plist` now sets `NSAllowsArbitraryLoadsInWebContent` (2026-11-24, `DISCUSSION.md` §55). Not tested on a Mac.

#### The DNS check

The control load used `http://`, and this load did not go to the network (see above). Thus, the control was not valid. The method of `tcpdump` operated: it recorded 68 lines of DNS traffic of the system.

A better method for the next run:

1. Use an `https://` URL for the control load without a proxy. Then the system must do a lookup of the control name.
2. Keep `tcpdump` on port 53.
3. Add a second record: a small DNS server of the spike on 127.0.0.1 that logs each name. Set it as the DNS server with `networksetup -setdnsservers`. Set the old value again at the end.

`log stream` of `mDNSResponder` is not a good method. macOS shows the host names in this log as `<private>`.

Other changes for the next run:

- Upload the crash reports of the spike from `~/Library/Logs/DiagnosticReports`. They show where `fetch_data_store_identifiers` and `remove_data_store` stop.
- Mark the spike checks as FAIL with the text "crash" when a child stops with a signal.

Done in the spike for the second run (2026-11-20, not run yet): the checks above, the ATS check with `PanimCheckAts.app`, the DNS server with `/etc/resolver` instead of `networksetup`, the link to a RAM disk, the data store functions with a webview, and the list of `~/Library/Caches`. Refer to `spikes/macos-webview/README.md`.

#### The second run on the macOS runner (2026-11-24)

Run 45249682616 used commit `9e4dbf8` (before the merge of release 0.1.3) on macOS 15.8.2. The first two attempts got no runner, and GitHub cancelled them after 15 minutes. The third attempt took 24 minutes, and all the steps passed.

The build and the package:

- PASS: `cargo test` on macOS (262 passed, 0 failed). The repair with `generate_context!(test = true)` operates.
- PASS: the OpenSSL from source (item 4). The objects have the minimum 14.0, and `ld` gave no warning about a newer macOS. The build of OpenSSL and TDLib took 839 s.
- PASS: `libtdjson` and the `.app` have no path of Homebrew or Nix. `minos` and `LSMinimumSystemVersion` are 14.0. The `.app` is 39 MB, and the `.dmg` is 15 MB.
- PASS: PANIM ran for 15 s.

Plain HTTP (item 3):

- The cause is App Transport Security (ATS). With the key `NSAllowsArbitraryLoadsInWebContent` in `Info.plist`, `http://neverssl.com/` loaded with no proxy, through the HTTP proxy and through the SOCKS5 proxy. Without the key, it did not load in the three cases.
- With the key, the HTTP proxy got `CONNECT neverssl.com:80`, and the SOCKS5 proxy got the host name (`atyp=domain`).

The DNS lookup (check 2):

- PASS: the method is valid. The local DNS server got the lookup of the control name, which loaded with no proxy.
- PASS: with HTTPS, through the HTTP proxy and through the SOCKS5 proxy, the DNS server got no lookup. The proxy got the host name. Thus, the DNS lookup stays in the proxy.
- The same check with plain HTTP did not load, because the app of this check had no ATS key.

The data stores (item 1):

- PASS again: the write, the persistence, the isolation and the seal of the folder only.
- FAIL again: `fetch_data_store_identifiers` and `remove_data_store` in a process with no web view. The process stopped with SIGSEGV. The crash report shows `WTF::RunLoop::dispatch` in the code of WebKit.
- PASS: the same two functions in a process with a web view. `remove_data_store` deleted the folder of the store, and the data was gone.
- PASS: a symbolic link from `WebsiteDataStore/<uuid>` to a folder on a RAM disk. WebKit wrote the cookies, the local storage and IndexedDB on the RAM disk (21 files, 575 KB). A new process read them. The link stayed a link.
- `~/Library/Caches/<bundle id>/WebKit` has the files of the default store only: `AlternativeService.sqlite` and three `salt` files (66 KB). It has no cookies and no local storage.

The RAM disk (check 3, item 2):

- PASS: the user of the runner made a RAM disk without `sudo`, with `hdiutil attach -nomount ram://` and `diskutil erasevolume APFS`. It mounted the volume again with `nobrowse` at a private folder, and detached it.
- The volume has "Owners: Disabled". Thus, the mode of a file does not stop other users. A folder of the user with mode 700 around the mount point stops them.

The unread counts (check 5):

- PASS again: the title and `unread.js`. Without `unread.js`, `Notification` exists in WKWebView, and its permission is "denied".

The decisions from this run are in `DISCUSSION.md` §53 to §55.


#### The third run on the macOS runner (2026-11-24)

Run 45308011980 used commit `593c4fb`, with the code of items 1 and 2 and the merge of release 0.1.3. It took 35 minutes. The result was "failure".

The build:

- PASS: TDLib came from the cache. `cargo clippy` passed, so the code of items 1 and 2 compiles on macOS.
- FAIL: `cargo test` and the `.app` did not link: "Undefined symbols: `_sqlite3_set_clientdata`". `rusqlite` 0.43 needs SQLite 3.44 or later. The SQLite of the macOS SDK is older. Release 0.1.3 added `rusqlite`.
  - Repaired (2026-11-24): macOS uses the bundled SQLite of `rusqlite` (`DISCUSSION.md` §56). Not tested on a Mac.
- Thus, the tests on macOS, the examination of the `.app` and the start of PANIM did not run. The checks of the RAM disk of PANIM did not run.

The spike:

- From the child 17 on, the processes of the spike did not stop after their work, and the watchdog stopped them. In the second run, the same code had no such stop. Thus, the results of the children 17 to 37 are not valid in this run.
- FAIL: `data_store.link_after_close`. The child made its main window, and then its event loop gave no event. This is the child 33, so the same problem is a probable cause.
  - Changed (2026-11-24): the child makes the first window of the store before the event loop starts, and it writes each step. Not tested on a Mac.
- The children 1 to 16 gave the same results as in the second run.

The memory of a RAM disk of 2 GiB (`ramdisk.memory`):

- The attach and the erase did not take 2 GiB. The free memory changed by less than 15 MiB, and the wired memory by less than 40 MiB.
- After the detach, 535 MiB more memory were free than before the attach. Thus, the RAM disk keeps memory for the data that it got, and the detach gives it back.

#### The fourth run on the macOS runner (2026-11-24)

Run 45315372353 used commit `7c25fea`, with the bundled SQLite on macOS. It took approximately 30 minutes, and all the steps passed.

The build and PANIM:

- PASS: `cargo test` on macOS (291 passed, 0 failed). The bundled SQLite links.
- PASS: the `.app` (41 MB) and the `.dmg` (16 MB). `minos` and `LSMinimumSystemVersion` are 14.0, and no path of Homebrew or Nix.
- PASS: PANIM ran for 15 s, with no `XDG_RUNTIME_DIR`.
- PASS: `smoke.ramdisk`. PANIM made its RAM disk: one base folder with mode 700, the name from SHA-256 of the data folder, the `device` file, and one APFS mount with `nobrowse` at `area`.
- PASS: `smoke.ramdisk_reuse`. After a kill, the second start used the same RAM disk.
- The step detached the RAM disk at the end. The exit of PANIM with a detach (Cmd+Q) is not examined. The 15 s start does not open a vault, so the link of item 1 did not run.

The spike:

- The children 1 to 16 gave the same results as in the second run.
- From the child 17 on, the processes did not stop after their work, as in the third run. The first run had the same stop from the child 17 on. Only the second run did not have it. Thus, the cause is in the runner or in the spike, not in PANIM. The results of the children 17 to 37 are not valid in this run.
- `data_store.link_after_close`: the write passed (all the four values). The log shows `LOOP start`, but the event loop gave no event after it. Thus, the seal and the reopen did not occur. The question of §54 is still open.

The memory of a RAM disk of 2 GiB: the same result as in the third run. The attach and the erase did not take 2 GiB. The detach gave back 530 MiB.
### A build without Nix

Release 0.2.0 also gets a usual build, and the Nix package stays on top of it (`DISCUSSION.md` §40):

1. Linux: the libraries of the system (for example the WebKitGTK packages of Ubuntu), `rustup` and `pnpm`. A script builds the TDLib of PANIM from the source with its patch.
2. The packages from the bundler of Tauri (`.deb`, `.rpm`, AppImage on Linux; `.app` and `.dmg` on macOS), with the patched TDLib in them. Do the checks of the release 0.1.0 AppImage again (Fedora, the signature).
3. The Nix package (`nix/package.nix`) stays. It uses the same TDLib patch.
4. `CONTRIBUTING.md` gives the two ways to build.

**The Linux part is complete (2026-11-19).** `docker/check.sh all` passed on the computer of the person, with Docker:

- The build in a clean Ubuntu 24.04 container: `pnpm check`, `cargo fmt`, `cargo clippy` and `cargo test` (256 passed, 0 failed) passed. The binary and `libtdjson` have no `/nix/store` path.
- The `.deb` in a clean Ubuntu 24.04, the AppImage (unpacked) in Ubuntu and the `.rpm` in a clean Fedora 45 found all their libraries. PANIM ran for 15 seconds in Xvfb in each case.
- Not done yet: the signature of the new packages, and a start of the AppImage without `--appimage-extract`.
- **Complete (2026-11-20).** The AppImage has `libtdjson` one time, in `usr/lib` (`ARCHITECTURE.md` §11). `docker/check.sh all` passed again with commit `1b08f07`. The AppImage is now approximately 91.5 MiB. Before, it was approximately 105.7 MiB.

**More checks (2026-11-19), after the merge of release 0.1.2 into `next-0.2` (commit `45806bc`):**

- `docker/check.sh all` passed again: `cargo test` (262 passed, 0 failed), the `.deb`, the AppImage and the `.rpm`.
- **The AppImage through FUSE: complete.** In a clean Ubuntu 24.04 container, it started directly, not unpacked. The container had only `fuse3` and the usual libraries of a desktop (X11, Wayland, GL, fonts), and no WebKitGTK. PANIM ran for 15 seconds in Xvfb.
- **The AppImage without FUSE 3.** It shows "No suitable fusermount binary found", then it unpacks itself and starts PANIM.
- **The signature: complete.** `nix run .#release` with a test key wrote and signed the checksums of the `.deb`, the `.rpm`, the AppImage and the source archive. All the checksums and the signature passed. The script needs no change. The real signature comes at the release.
- **The glibc.** The binary, `libtdjson` and WebKitGTK need glibc 2.39 or later (Ubuntu 24.04, Debian 13, Fedora 40 and later).

Release 0.2.0 publishes these packages and stops the AppImage of Nix (`DISCUSSION.md` §50). `README.md` tells the new packages. Open tasks:

1. A release build of the packages with the Telegram app values. Then change the release steps in `CONTRIBUTING.md`. Until then, the release steps tell the AppImage of Nix.
   - **Done in the code (2026-11-24).** The first real release build runs on the computer of the maintainer, because this computer has no Docker and no values.
   - `docker/check.sh --values FILE` is the release mode. The build container reads only FILE, read-only. The script refuses the mode if the repository has changes, or if `HEAD` does not have the tag `v<version>`.
   - After the build, it fails if the build gave "Telegram is not configured" or if the binary does not have the `api_hash`. It never prints a value.
   - A test with a stub engine examined the checks on the host and the arguments of the engine. A test with fake files examined the check of the values. Nobody ran the containers in this mode yet.
   - `CONTRIBUTING.md` "Make a release" now tells these steps.
2. The `.dmg` of a release with the Telegram app values (`DISCUSSION.md` §57).
   - **Done in the code (2026-11-24). Not run yet.** `.github/workflows/macos-release.yml` builds it for a push to `macos-release`, from a tag `v<version>` or `v<version>-rc.<n>`. `bundle/macos.json` signs the `.app` ad hoc, and the two macOS workflows examine the signature. `README.md` and `CONTRIBUTING.md` tell the new steps.
3. The removal of `packages.appimage-release`, `packages.appimage` and the two patches of the `AppRun`.
   - **Done (2026-11-24).** The flake has no AppImage, no `appimage-apprun` and no input `nix-appimage`. We deleted the two patches. `flake.lock` lost only the nodes of `nix-appimage`. `packages.panim-release` stays for the hand checks with Nix.

Then build:

1. The app on macOS, with the same features as on Linux.
2. The package: an `.app` and a `.dmg`, from Nix if possible. The checksums and the signature of the release, as on Linux.
3. The signature and the notarization of Apple, if the person has an Apple Developer account. If not, record how a user starts an app that has no signature.
4. The hand checks of milestones 3 to 7 and 10 on a Mac.

Release 0.2.0 has macOS (`DISCUSSION.md` §57): only Apple silicon, an ad hoc signature, and a `.dmg` from the workflow `macos-release.yml`. Item 2 does not use Nix. Item 3: no signature of Apple in release 0.2.0.

#### The hand checks on a Mac

Use a Mac with Apple silicon and macOS 14 or later. Use the `.dmg` of a release candidate (`v0.2.0-rc.<n>`), so that it has the Telegram app values. Use test accounts only (`DISCUSSION.md` §12).

1. **The install.** Download the `.dmg` and its `SHA256SUMS`, and check the checksum. Open the `.dmg`, and move PANIM to Applications.
   - Open PANIM. macOS blocks it. In System Settings > Privacy & Security, select "Open Anyway". Record the exact text of each message.
2. **The first start.** Make a passcode. Write down the 12 recovery words, and check them. Save a key file.
   - Make sure that the vault is in `~/Library/Application Support/panim`.
   - Run `mount | grep panim`. One volume with `nobrowse` is at `$TMPDIR/panim-<16 hex>/area`.
3. **The browser profile.** Add an X or Discord account. Log in in its window, and close the window.
   - Run `ls -l ~/Library/WebKit/io.github.sakurarainn.panim/WebsiteDataStore/`. The folder of the store is a link to the RAM disk.
   - Lock PANIM. The link is gone. Wait one minute, and run the `ls` again. Record if WebKit made a new folder (`DISCUSSION.md` §54).
   - Unlock. Open the account again. The login is still there.
4. **The automatic lock.** Set a short time. Type in an account window. The lock does not occur while you type. Stop. The lock occurs.
5. **The proxy.** Set a proxy for the persona. In an account window, open a page that shows your address. It shows the address of the proxy.
6. **The unread counts.** Make a new message for a test account. The count shows in the main window.
7. **GitHub.** Do the device login. The notifications show in the timeline. Answer one.
8. **Telegram.** Log in with a test number. The chats show in the timeline. Send one message. Lock and unlock. The account is still logged in.
9. **The proofs.** Make a proof for an account, and examine it.
10. **The backup.** Make a backup. Remove the data folder (with `PANIM_DATA`, use a test folder). Restore the backup. The accounts and the logins are there.
11. **The exit.** Quit PANIM with Cmd+Q. `mount | grep panim` shows nothing. Start PANIM again, and unlock. The logins are there.
12. **After a crash.** Stop PANIM with `kill -9`. Start it again. It uses the same RAM disk, and it unlocks.

Record the results in this section, with the date, the macOS version and the model of the Mac.

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
