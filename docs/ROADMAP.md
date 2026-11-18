# PANIM: roadmap

This document shows the plan for step 1 and the current state of the work. Change it when a milestone starts or is complete. `DESIGN.md` tells what PANIM does. `ARCHITECTURE.md` tells how we build it. `DISCUSSION.md` is the record of the decisions.

The work is on the branch `step-1`. The author of the commits is SakuraRainn.

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
| 8 | Proton Mail through Bridge: mail in the timeline, mail out, IMAP IDLE. Check 3: IMAP/SMTP through the proxy | After release 0.1.0 (`DISCUSSION.md` §28) | A paid Proton account with Proton Mail Bridge |
| 9 | The X API: the user adds their own developer key. PANIM shows the cost of each action | After release 0.1.0 (`DISCUSSION.md` §28) | An X developer key with credit |
| 10 | Release 0.1.0 for Linux: the package (Nix, with `wrapGAppsHook`), the backup, `CHANGELOG.md`, the merge of `step-1` | Complete. Release 0.1.0 is public (2026-11-17, §6) | — |

Release 0.1.0 has GitHub and Telegram in the shared timeline, and X, Discord and Proton Mail in browser views. Milestones 8 and 9 come after release 0.1.0. After release 0.1.0, the next system is macOS (`DISCUSSION.md` §13).

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

1. `step-1` got the tag `v0.1.0` (commit `e2943bd`). The private repository is now `SakuraRainn/panim-dev`. It has `step-1` and the tag.
2. The AppImage was built from the tag with the values of PANIM. The person signed `SHA256SUMS` with the key `7B5C5C4A551F160B`.
3. The public repository `SakuraRainn/panim` has one commit on `main` and the tag `v0.1.0`. The commit is the same as the source archive of the release. It has no history of `panim-dev`.
4. The release v0.1.0 of `SakuraRainn/panim` has the AppImage, the source archive, `SHA256SUMS` and `SHA256SUMS.minisig`.

For the next release, make the public commit from the source archive of the release (`git archive` of the tag). `.gitattributes` leaves out `AGENTS.md`, `CLAUDE.md` and `.githooks`. Then add the commit on `main` of `SakuraRainn/panim`, with the private email of GitHub as the author.

## 7. Open tasks outside the milestones

- Add the row of PANIM (project 1) to the family derivation table of ZIKARON (`crates/app/src/family.rs` in the ZIKARON repository). In step 3, ZIKARON must read the persona key at project 1, or take it from the key file (`DISCUSSION.md` §16).
- Delete the old binary `target/release/partzufim`. The binary is now `target/release/panim`.
- Known limits from milestones 2 and 3 (`ARCHITECTURE.md` §2a, §9):
  - The automatic lock counts only input in the main window.
  - If a write to the disk fails, the data in memory can be different from the data on the disk until the store opens again.
  - There is no check yet for passcode settings that are below the standard.
  - If the computer has swap, the kernel can write the profiles in `$XDG_RUNTIME_DIR` to the swap.

## 8. How to start work in a new session

1. Read `AGENTS.md`, then this document.
2. Read `DESIGN.md` and `ARCHITECTURE.md` for the current agreement.
3. Read the end of `DISCUSSION.md` for the latest decisions.
4. Run `git log --oneline` on the branch `step-1`.
5. Start the next milestone, or the open task that the person asks for.
