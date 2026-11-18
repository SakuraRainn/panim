# Changelog

All the changes of PANIM that a user can see are in this file. The versions use semantic versioning.

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
