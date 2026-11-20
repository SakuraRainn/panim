# Changelog

All the changes of PANIM that a user can see are in this file. The versions use semantic versioning.

## 0.1.3 (2026-11-19)

This release has no new feature. It changes how PANIM seals the open browser profile and the Telegram folders.

### Changes

- **The windows do not stop.** Before, the windows of PANIM could stop for a short time during the unlock and during the seal every five minutes. Now this work does not stop the windows. A list can show its data a short time later.
- **A correct copy of the browser data.** PANIM now copies the databases of a browser view (for example, the local storage of a site) with the backup interface of SQLite. Before, a copy during a write could give a damaged database after a crash.
- **No seal when nothing changed.** Before, PANIM sealed the browser profile and the Telegram folders every five minutes, also when no file changed. Now it seals them only after a change.
- **Fewer timeline updates from Telegram.** Before, each Telegram update wrote the data of the account three times and always updated the timeline. Now it writes one time, and only when something changed.
- **A window that opens during a lock closes.** If PANIM locked while a browser view opened, the window could stay open on a locked vault. Now PANIM closes it.
- **Two fast changes in the settings stay.** Before, a second change of the language or the automatic lock could undo the first change.

### Known limits

The known limits of 0.1.2 do not change. The repair of the timeline order in 0.1.2 now has an effect, because the answers of PANIM can come in a different order.

## 0.1.2 (2026-11-18)

This release has no new feature. It repairs six problems.

### Changes

- **The automatic lock counts the input in all the windows.** Before, only the input in the main window reset the time. If you worked in a browser view, PANIM locked and closed the window. Now a key press, a click or a scroll in a browser view also resets the time. PANIM gets this input from the system, not from the page, so a page cannot stop the lock.
- **Stronger passcode settings.** When your passcode opens a vault with weaker Argon2id settings than the settings of PANIM, PANIM seals the vault again with the stronger settings. Your passcode does not change. PANIM refuses a vault file with settings that are too large.
- **A failed write does not change the data in PANIM.** Before, if a write to the disk failed, PANIM could show data that was not on the disk. Now PANIM puts back the data that it had before the change.
- **Limits for a key file.** PANIM refuses a key file with scrypt or pbkdf2 settings above a limit. Before, such a file could stop PANIM or use all the memory. PANIM also compares the check value of the file in constant time.
- **The unread GitHub notifications show.** Before, with the filter "unread", many GitHub events could hide older unread notifications.
- **The timeline keeps the latest filter.** PANIM now ignores an older answer of the timeline after a change of the filter. In this release, the answers always come in order, so you see no change. This protects a later release, which can answer out of order. (The `CHANGELOG.md` of the tag `v0.1.2` has an incorrect text here. The release notes on GitHub have the correct text.)

### Known limits

The known limits of 0.1.1 do not change, except that the automatic lock now counts the input in all the windows. During many Telegram updates at the same time, the timeline can stay the same until the updates stop. Release 0.1.3 sends fewer updates.

## 0.1.1 (2026-11-18)

This release has no new feature. It changes the documents, the release steps and one error message of the AppImage.

### Changes

- **`README.md` is for the people who use PANIM.** It tells what PANIM does, how to check and install the download, the first start, the Telegram app values, the network route, the backup and the known limits.
- **`CONTRIBUTING.md` is new.** It tells how to build and test PANIM, the rules for a change, and how a change comes into PANIM. It also has the release steps of the maintainer.
- **`SECURITY.md` is new.** It tells how to report a vulnerability in private, and what PANIM protects.
- **Chinese versions of the three documents:** `README.zh-CN.md`, `CONTRIBUTING.zh-CN.md` and `SECURITY.zh-CN.md`. The English files are the reference.
- **Forms for the issues.** The public repository has a form for a problem report and a form for a question.
- **The AppImage on Ubuntu 24.04 and later.** Ubuntu blocks the user namespaces that the AppImage needs. Before, the AppImage stopped with only "cannot unshare". Now it also tells the cause and what to do. On Ubuntu, it shows an AppArmor profile for its own path. `README.md` has the steps in the section "Ubuntu 24.04 and later".
- **The public commit of a release.** The new script `nix run .#publish` makes it from the signed source archive. It examines the checksums, the signature, the email of the author, the files and the Telegram values. It does not push.

### Known limits

The known limits of 0.1.0 do not change. The AppImage still needs unprivileged user namespaces. On Ubuntu 24.04 and later, an AppArmor profile for the AppImage gives them (`README.md`).

## 0.1.0 (2026-11-17)

The first release of PANIM, for Linux. PANIM is in step 1: one persona for one person. The user interface does not show the persona yet. `docs/DESIGN.md` tells what PANIM does.

### What PANIM does

- **One timeline for the joined accounts.**
  - GitHub: the notifications and the events of the repositories that you watch. The read state follows the GitHub inbox. You can write an answer to an issue or a pull request in a public repository.
  - Telegram: one item for each chat in your main chat list, with the last message and the unread count. Muted and archived chats stay out. The read state follows Telegram in the two directions. A chat panel shows the text messages and sends text.
- **Browser views** for X, Discord and Proton Mail (free plans). Each account opens in the browser profile of the persona. PANIM reads only the unread count from the title of the page and the notification signal of the page.
- **The persona key and the account proofs.** PANIM makes the key from 12 recovery words. It signs proofs in the form of `base/panim-proof-v1.md`. It examines a proof in a GitHub gist and a pasted proof.
- **The sealed vault.** A passcode opens the vault. PANIM seals the store, the secrets, the browser profile and the TDLib folders with XChaCha20-Poly1305. The 12 recovery words or a key file reset a lost passcode. After five wrong passcodes, only the words or the key file open the vault.
- **One network route for the persona:** direct, an HTTP proxy or a SOCKS5 proxy. GitHub, Telegram and the browser profile use it. The TDLib of PANIM has a patch, so that Telegram makes no connection past the proxy.
- **The Telegram app values.** A release has the values of PANIM. In the settings, you can give your own `api_id` and `api_hash` for new logins. Each account keeps the values of its login.
- **The backup.** The settings write the vault into one sealed file. The first start can restore a vault from this file.
- **English and Chinese.**

### Install

- `nix build` or `nix run` with the flake of the repository.
- An AppImage for x86_64, with a `SHA256SUMS` file and a minisign signature. `README.md` tells how to check a download. The minisign public key of PANIM is `RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa` (key id `7B5C5C4A551F160B`).

### Known limits

- PANIM runs only on Linux. macOS comes next.
- Proton Mail Bridge (paid plans in the timeline) and the X API come after 0.1.0 (milestones 8 and 9).
- All the browser windows have the same fingerprint as the computer. Different profiles and routes stop only easy links between personas.
- The automatic lock counts only the input in the main window.
- If the computer has swap, the kernel can write the open profiles in `$XDG_RUNTIME_DIR` to the swap. Use encrypted swap or no swap.
- The unread count of a browser-view account exists only while its window is open.
- The Telegram chat panel shows text only. TDLib cannot find new addresses of Telegram on a network that blocks the built-in addresses. Use a proxy on such a network.
- A build from the source has no Telegram values of PANIM. Then you must give your own values, and Telegram can link the personas that use them.
- A backup has the browser logins and the Telegram sessions. If you restore it on a second computer, close PANIM on the first computer.
- The AppImage needs unprivileged user namespaces, and FUSE or `--appimage-extract-and-run`. Nobody tested it with the proprietary NVIDIA driver.
