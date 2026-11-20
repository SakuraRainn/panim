# PANIM: architecture

This document tells how we build PANIM. It shows the current agreement. `DESIGN.md` tells what PANIM does.

## 1. Overview

```
┌──────────────────────── PANIM (Tauri) ────────────────────────┐
│                                                                   │
│  Main window (web UI)              Browser windows                │
│  personas · timeline · compose     in the persona's profile:      │
│           │                        its data folder, its proxy     │
│           │ Tauri commands                                        │
│  ┌────────▼──────────────── Rust core ─────────────────────────┐  │
│  │ personas · keys · account proofs · accounts                 │  │
│  │ local store · sealed secrets · gate                         │  │
│  │                                                             │  │
│  │ connectors:  GitHub   Telegram   Email   X                  │  │
│  │              REST     TDLib      IMAP/   API v2             │  │
│  │                                  SMTP                       │  │
│  │ routes: one network route per persona                       │  │
│  └──────────────────────────────┬──────────────────────────────┘  │
└─────────────────────────────────┼─────────────────────────────────┘
                                  │ child process, JSON (step 3, not step 1)
                                  ▼
                         zikaron CLI / MCP
```

## 2. Shell: Tauri

- We use **Tauri**. It has a Rust backend and a web user interface. The web user interface uses the web engine of the system: WebKitGTK on Linux, WebView2 on Windows and WKWebView on macOS.
- The web user interface uses **Svelte** with TypeScript.
- The first release is for Linux. macOS comes next.
- PANIM does not use the `ui-kit` (egui) of ZIKARON. PANIM uses only the command line or the MCP server of ZIKARON (§7).
- **Fallback: Electron.** We use Electron if the check in §9 fails.

## 2a. The app (milestone 3)

- `crates/app/src-tauri/src/session.rs` holds the state of the app: the first start, the gate, the recovery, the accounts, the settings and the browser profile. It has no Tauri code, so the tests examine it without a window.
- `crates/app/src-tauri/src/main.rs` gives the session to the user interface as Tauri commands.
  - Tauri runs a command that is not `async` on the main thread. The main thread draws all the windows.
  - Each command that locks the session is `async` and runs on a blocking thread. Thus, the main thread does not wait for the lock of the session, and the windows do not stop during a seal or Argon2id.
  - A command that opens a window gets its data from the session and releases the lock. Then it makes the window. After the build, it makes sure that the vault is still open. If not, it closes the window.
  - The changes of the unread state from the windows (the title, the focus, the close and `page_notice`) go into one queue. One thread does them in their order. It examines the focus of a window before it locks the session.
  - Async commands can give their answers in a different order. The user interface ignores an answer that is not from its latest call. The settings page keeps its own copy of the settings.
  - Only the exit of the app locks the session on the main thread, because the exit must wait for the last seal.
- The app has an app ACL manifest (`build.rs`). Thus, Tauri examines each command against the capabilities, also for the main window. Add each new command to the list in `build.rs` and to a capability.
- The main window can call all commands except `page_notice` (`capabilities/main.json`). This capability has no remote URLs.
- The windows of browser-view accounts load sites of other parties. They can call only one command, `page_notice`, from an `https` page (`capabilities/account.json`). This command has no arguments. The app finds the account from the label of the window, not from data of the page. The account windows get no core permissions.
- `crates/app/src-tauri/src/github.rs` does the work of the GitHub connector for the app (§4a). It has no Tauri code. Its tests use a fake HTTP client.
- `crates/app/src-tauri/src/telegram.rs` does the work of the Telegram connector for the app (§4b). `src-tauri/src/session/telegram.rs` keeps the Telegram part of the session. They have no Tauri code. Their tests use a fake TDLib.
- A command that uses the network is `async` and runs on a blocking thread. It does not hold the lock of the session while a request runs.
- One thread does the sync of the joined accounts. When the timeline or the state of a joined account changes, the app sends the event `timeline-changed` to the main window. The main window then calls the commands `timeline` and `sync_state`.
- The command `timeline` does not send the raw record or the link of an item to the user interface. The command `open_link` gets the link from the store. It opens only a page of GitHub.
- For a received event of a known type, the command `timeline` also sends the kind of the event and the repository. The user interface shows the phrase of the kind in the language of the person.
- The device page of GitHub, the page of a timeline item and the home page of a GitHub account open in profile windows:
  - A profile window uses the profile folder, the route and the user agent of the persona, as an account window does.
  - The label of a profile window starts with `profile-`. No capability names this label, so the page can call no command.
  - A profile window gets no script of PANIM.
  - The app closes the profile windows with the account windows, before it seals the profile.
  - The command `open_account` opens `https://github.com/` for a joined GitHub account. The label of this window is `profile-account-<id>`. A second click shows the same window. The removal of the account closes the window.
- The command `check_github_proof` examines a gist (§4a). The page "Proofs" uses it (§6a).
- The proof commands are in §6a. Only the main window can call them. The page `https://gist.github.com/` opens in a profile window with the label `profile-gist`.
- The unread state is in `crates/app/src-tauri/src/unread.rs`, without Tauri. When the state changes, the app sends the event `unread-changed` to the main window. The main window then calls the command `unread`.
- A browser window uses the profile folder of the persona, the route of the persona and a current Safari user agent.
- The app seals the open browser profile every five minutes, if a file changed after the last seal (§5). When the main window closes, or when the person locks the app, the app closes the browser windows and seals the profile.
- The settings that the gate needs (the language and the time for the automatic lock) are in `settings.json` in the data folder. They are not secret.
- The automatic lock counts the input in all the windows:
  - Key, pointer and wheel events in the main window reset the time of the lock (`Main.svelte`).
  - On Linux, the Rust side connects to the GTK signal `event` of the WebKit view of each account window and each profile window. GTK sends each event to this signal before WebKit gets it.
  - The Rust side counts only key presses, button presses and scroll events. It does not stop the event, so the page gets it.
  - The Rust side sends the event `input-elsewhere` to the main window, no more than one time in 10 seconds (`activity.rs`, without Tauri). This event also resets the time of the lock.
  - The script of a page cannot make a GTK event. Thus, a page cannot stop the automatic lock.
  - On macOS, the events come from a different source. Milestone 12 must add this part. Until then, only the input in the main window counts on macOS.
- The text of the user interface is in `crates/app/src/lib/i18n.svelte.ts`, in English and Chinese. `pnpm check` fails if a text is missing in one language.
- Known limits:
  - libsoup can write a warning about the HSTS table of the profile. The pages load correctly.
  - The app needs the GSettings schemas. Without them, the app stops when a file dialog opens. The Nix package supplies them with `wrapGAppsHook3` (§11).

## 3. Rust core

- **Personas.** A persona is the unit of separation. Each persona has a name, its accounts, one browser profile and one route.
  - A new person has one persona.
  - In step 1, there is only one persona. PANIM makes it when it starts for the first time. The user interface does not show it.
  - The store, the profile and the route use the persona as their key. Thus, step 2 can add personas, and the stored data does not change.
  - PANIM records a link between personas only when the person shows that link.
  - From step 3, each persona is a ZIKARON identity.
- **Persona keys.** Each persona has one signing key. PANIM makes the key when it makes the persona. PANIM seals the key with the other secrets (§6).
- **Accounts.** Each account belongs to one persona. It has a platform, a display name and its credentials. PANIM seals the credentials. The account uses the route of its persona.
- **Local store.** PANIM keeps the updates from joined accounts as timeline items. All items have the same fields: account, source id, author, time, text, link and read state. Each item also keeps the raw record from the platform.
- **Secrets.** PANIM seals passwords, tokens and keys. It uses the design of the ZIKARON local data standard (`crates/app/local-data-standard.md` in the ZIKARON repository), but not its code:
  - A random master key seals all the local data.
  - Each file has its own key, which comes from the master key. The cipher is XChaCha20-Poly1305. Each file is bound to its owner and its place, so a file cannot replace a different file.
  - The passcode opens the master key every day, through a slow key derivation function.
  - The persona key is the recovery. Its key file opens the master key a second way, and then the person sets a new passcode. The person keeps the key file outside PANIM.
  - Later, the family components `local-seal` and `passcode-gate` can replace this code (§10).
- **Durable writes and one writer.** These protect the local store.
- **The backup.** One archive file keeps the sealed vault (§3a).

In step 1, these parts are code of PANIM or usual crates (§10):

- `panim-core` has the data model, the persona key and the proofs. It does not write to the disk.
- `panim-vault` has the sealed local data:
  - The cipher is XChaCha20-Poly1305. The passcode key comes from Argon2id (64 MiB, three passes). The file keys come from HKDF-SHA256.
  - The header `vault.json` keeps the settings of Argon2id. `Vault::open` gets the settings of the app (`DISCUSSION.md` §44):
    - If one or more settings in the header are below the settings of the app, PANIM seals the master key again after the passcode opens it. The new seal has a new salt and the larger value of each setting. The passcode does not change.
    - PANIM writes the new header while it has the lock of the one writer. If this write fails, the open still succeeds. The old header stays complete, and the next open tries again.
    - A change to the header cannot make the seal weaker, because the old seal does not open with different settings. Thus, this step is for old vaults and test vaults.
    - The maximum is 4 GiB of memory, 64 passes and 16 lanes (`KdfParams::MAX`). Settings above it give `VaultError::Damaged` before Argon2id runs, and do not count as a wrong try. Thus, a changed header cannot use all the memory of the computer.
    - The persona key does not use these settings. Thus, the recovery opens a vault with a damaged header, and sets the settings of the app.
  - The store writes the personas and the accounts to one sealed file, and the items of each account to a different sealed file.
  - The store writes the kept proofs to the sealed place `store/proofs` (§6a).
  - The store writes its files with `postcard`. This form has no field names, so a new field in a stored struct makes the old files unreadable. Put new data in a new place, and give the new file a version number.
  - A failed write does not change the data in memory. Each change of the store or the secrets keeps a copy of the part that it changes: the meta, the items of one account, the proofs or the secret table. If the write fails, the change puts the copy back and gives `Error::Storage`.
  - `Vault::write` writes a temporary file and then renames it. Thus, each file on the disk is complete.
  - The removal of an account writes the meta first. If a later step fails, an items file or a proof without its account stays on the disk. The store ignores this data when it opens.
  - The key file of the persona key is a V3 keystore (Web3 Secret Storage). Other wallets can read it.
  - The import of a key file refuses scrypt settings above n = 2^20, r = 16 or p = 16, and pbkdf2 settings above c = 10,000,000, before the KDF starts. It compares the MAC in constant time.
  - A passcode must have eight characters or more.
  - The app keeps the Telegram app values of the person in the sealed place `app/telegram` (§4b). The file is JSON with a version number. A vault from before milestone 10 has no such file, and PANIM then uses its own values.

## 3a. The backup (milestone 10)

`DISCUSSION.md` §29 gives the decisions.

- **What the archive keeps.** The backup is one `tar` file with these entries, in this sequence:
  1. `panim-backup`: the mark of the format, `panim-backup/1`.
  2. `vault.json`: the header with the two seals of the master key.
  3. `data/<32 hexadecimal digits>`: each sealed file. These are the store, the secrets, the settings of the vault, the browser profile and the TDLib folders.
- **No new cipher.** All the files in `data/` are sealed with keys from the master key, and the header holds only sealed copies of the master key. Thus, the archive needs no new cipher. Only the passcode, the 12 recovery words or the key file open it.
- **What the archive does not keep.**
  - `vault.lock` is the lock of one process on one computer.
  - `gate.json` is the count of wrong passcode tries. A restored vault starts with no wrong tries. This count is not a protection against a copy of the files, because Argon2id is that protection. The count includes only the passcodes that a person types in PANIM. A person who can change the files can remove `gate.json`.
  - `settings.json` has the language and the time of the automatic lock. These are not secret, and the first start sets the language again.
  - A temporary file of an interrupted write (`data/.<name>.<random>.tmp`).
- **The steps of a backup** (the command `backup_vault`, only from the main window):
  1. PANIM closes the browser windows.
  2. PANIM stops a Telegram login that is in progress, and closes the Telegram clients, as at a lock.
  3. The session seals each TDLib folder and removes it from memory. It also seals the browser profile.
  4. While the session stays locked, PANIM writes the archive to a temporary file in the selected folder. Then it does `fsync` and renames the file. The target cannot be in the vault folder.
  5. PANIM opens the TDLib folders and starts the Telegram clients again.
- **The restore** (the command `restore_vault`) is possible only on the first start, when the data folder has no vault. PANIM takes the lock of the one writer, and then examines the archive:
  - The first entry must be the mark, with a known format.
  - Each other entry must be a usual file with the name `vault.json` or `data/<32 lowercase hexadecimal digits>`. PANIM refuses a link, a folder, a different name, a path with `..`, an absolute path and a second copy of a name.
  - Each sealed file must start with the magic bytes of a sealed file (`PNL1`).
  - The limits are 16 GiB for the archive, 8 GiB for one file, 64 KiB for the header and 100,000 files.
  - The header must be a header that the vault can read, and the archive must have one or more sealed files.
- PANIM writes the files into a new folder in the data folder first (`.restore-<random>`). When all the checks pass, it moves `data/` into the data folder. The header comes last, so a failed restore leaves no vault. PANIM then removes the new folder, also after an error.
- After the restore, the gate shows. The person opens the vault with the passcode of the backup, or recovers it with the 12 recovery words or the key file.
- Known limits:
  - The archive has the logins of the browser profile and the Telegram sessions. If PANIM still runs on the old computer, the same sessions are then in two places. The restore screen tells the person to close PANIM on the old computer first, or to remove the Telegram accounts there.
  - The backup is not incremental. Each backup writes all the sealed files again.
  - PANIM does not compress the archive. The sealed files do not become smaller.

## 4. Connectors

| Connector | Protocol | Sign-in | Notes |
|---|---|---|---|
| GitHub | REST on HTTPS | OAuth device flow | Notifications and received events (§4a). The limit is 5,000 requests each hour for each user. |
| Telegram | MTProto through TDLib | Phone number, code and the optional two-step password | One timeline item for each chat (§4b). PANIM has its own `api_id`, and the person can give their own values. TDLib does its own network connections, so PANIM sets its proxy for each client. |
| Email | IMAP/SMTP | Address and password | For Proton Mail, PANIM connects to Proton Mail Bridge at 127.0.0.1. Later, it connects directly to other services. |
| X | API v2 on HTTPS | OAuth 2.0 with PKCE | Optional. PANIM uses it only when the user adds their own developer key. X bills each request to that key. The browser view operates without the key. |
| Bluesky (release 0.3.0) | AT Protocol (XRPC on HTTPS) | To be decided in milestone 13: OAuth of the AT Protocol, or an app password | The crate `panim-bluesky`. A proof uses version 2 of the proof format (`base/panim-proof-v2.md`). |

In release 0.3.0, the email connector also connects directly to usual services with a password or an app password (`DISCUSSION.md` §42).

Each connector polls the platform or listens to it. It writes timeline items to the local store. It sends posts and answers as its account.

## 4a. The GitHub connector (milestone 5)

- **The crate.** `crates/panim-github` has the network code of the connector. `panim-core` stays without network code.
  - All functions use the interface `Http`. The real client is the crate `ureq` with rustls. The tests use a fake client and no network.
  - The client sends each request on the route of the persona. With a SOCKS5 proxy, the proxy gets the host name, so DNS does not leak.
  - The client accepts only `https` URLs, and it does not follow a redirect. Thus, a token does not go to a different host.
  - The time limits are 15 seconds to connect and 30 seconds for one request.
  - Tests with a fake proxy on 127.0.0.1 examine the real client. An HTTP proxy gets `CONNECT` and no token. A SOCKS5 proxy gets the host name, not an address. The client refuses an `http` URL before it connects.
- **The sign-in.** PANIM is the OAuth App "PANIM Desktop" on GitHub. It uses the device flow. The device flow has no client secret, so the client ID is a constant in the source.
  1. PANIM asks GitHub for a device code and a user code.
  2. PANIM shows the user code. The person opens the device page of GitHub in a profile window (§2a) and enters the code there.
  3. PANIM polls GitHub at the interval that GitHub gives. If GitHub answers `slow_down`, the interval becomes longer.
  4. GitHub gives a token. PANIM gets the id and the login of the account. Then it seals the token and adds the account.
- **The device code** stays in memory on the Rust side. It does not go to the user interface. A lock, a cancel, a success and the expiry remove it.
- **The scopes** are `notifications` and `public_repo`. PANIM does not ask for access to private repositories. If the person removes a scope on the page of GitHub, PANIM refuses the login and does not keep the token.
- **One account one time.** PANIM refuses a second account with the same numeric id.
- **What a sync reads.** A sync runs every 60 seconds, or less frequently if GitHub asks for that (`X-Poll-Interval`). It does nothing while the vault is locked. A sync sends three requests:
  1. `GET /user/subscriptions` gives the repositories that the account watches. The scope `public_repo` is sufficient for public repositories. The sync reads no more than 10 pages of 100 repositories. If the list has one page, the sync sends `If-None-Match` with the `ETag` of the last answer. The `ETag` of the first page does not show a change in a different page, so a longer list has no validator.
  2. `GET /notifications` gives the unread notification threads. GitHub gives no more than 50 threads in one page. The sync reads no more than 10 pages. It sends `If-Modified-Since`, so GitHub answers 304 when there is no change.
  3. `GET /users/{login}/received_events` gives the events that the account received. The sync reads the first page of 50 events. It sends `If-None-Match` with the `ETag` of the last answer.
  - A read on github.com possibly does not change `Last-Modified`. Then a 304 hides the read. Thus, the request for notifications has no `If-Modified-Since` when the person clicks "Sync now", and also when the last full list is five minutes old or older.
  - The validators, the list of watched repositories, the interval and the result of the last sync are in memory only. A lock removes them.
  - The cost when nothing changes: a 304 does not count against the rate limit. Thus, a sync costs 12 requests each hour, for each page of notifications. A list of more than 100 watched repositories adds one request for each page at each sync.
- **Only the events of watched repositories.** The received events also contain the public actions of the accounts that the person follows, in all repositories. PANIM keeps an event only if its repository is in the list of watched repositories. The names compare without case.
  - If the request for the list fails, the sync uses the last list. If the session has no list, the sync adds no event, and the account shows the error.
  - After a sync with a good list, PANIM removes the stored events of the repositories that are not in the list. Thus, when the person stops to watch a repository, its events go away. This rule never removes a notification.
- **One item for each thread.** A notification thread becomes one timeline item with the source id `n/<thread id>`. An event becomes one item with the source id `e/<event id>`.
  - The text of an event item is an English phrase, for example "starred owner/repo". The user interface does not show this text for a known type. It shows the phrase of the kind in the language of the person (`DISCUSSION.md` §20).
  - When GitHub changes a thread, the thread comes again with the same source id and a later time. The store then makes the item unread again.
- **The read state is the state of the GitHub inbox.**
  - Only a notification has a read state. An event has no read state on GitHub. The user interface shows it as read, and it never counts as unread.
  - The unread count of a GitHub account is the number of unread notifications. If the page limit cut the list, the count shows a "+".
  - When the person marks a notification as read, PANIM also marks the thread as read on GitHub. PANIM changes the local state first.
  - After a complete list of the unread threads, a stored thread in the list becomes unread, and a stored thread that is not in the list becomes read. After a cut list, a 304 or an error, the read state does not change.
  - A sync that started before the request of a local read ended does not make that thread unread. If the request failed, the next sync makes the thread unread again, because it is unread on GitHub.
  - GitHub cannot mark a thread as unread. Thus, PANIM does not let the person mark a notification as unread.
- **Answers.** An answer is a comment on an issue or on a pull request. PANIM finds the target from the raw record of the item.
  - The item can be a notification about an issue or a pull request, or one of the events `IssuesEvent`, `IssueCommentEvent` and `PullRequestEvent`.
  - The scope `public_repo` permits comments only in public repositories. If GitHub answers 403 or 404, PANIM tells the person that the account cannot write there.
- **A click on the account.** A click on a GitHub account in the list of accounts does two things:
  1. The timeline shows only the items of the account. The menu "All accounts" in the timeline removes the filter.
  2. PANIM opens `https://github.com/` in a profile window (§2a), or shows the window if it is open.
- **The check of a proof in a gist** (§6). The input is a gist id or a `https://gist.github.com` URL. PANIM refuses all other hosts.
  - The request has no token. Thus, GitHub does not know which account examines the proof. Without a token, GitHub permits 60 requests each hour for each IP address.
  - The gist must be public. PANIM does not read the files of a secret gist.
  - PANIM examines each block in each file. The verdict is one of these: correct, the signature is wrong, the account is different, or the text is not a block.
  - The id of the owner of the gist binds the block (`base/panim-proof-v1.md` §7).
- **Errors.** If GitHub answers 401, PANIM puts a mark on the account. The account and the token stay, and the usual sync does not try the account again. After a rate limit answer, the sync waits until the limit ends.
- **The lock of the session.** No function holds the lock of the session while a request runs. It locks the session and copies the token, the route and the validators. Then it unlocks, sends the request, locks again and writes the result. The session ignores a result that comes after a lock.
- Known limits:
  - PANIM cannot revoke a token, because a revocation needs the client secret. The person revokes "PANIM Desktop" in the settings of GitHub.
  - An account with the mark needs a new login. The person removes the account and adds it again. The removal deletes the timeline items of the account.
  - If the person reads a notification on GitHub, PANIM sees it at the next full list, after no more than five minutes.
  - If the account watches more than 1,000 repositories, the list is cut. Then PANIM drops the events of the other repositories.
  - A watched repository with "All Activity" gives a notification and an event for the same issue. Both show in the timeline.
  - If the person changes the login on GitHub, the request for events fails. The person then removes the account and adds it again.
  - The client does not follow a redirect. Thus, a request for a repository that moved gives an error.
  - While a request runs, the token is in the memory of the HTTP client as plain text.

## 4b. The Telegram connector (milestone 7)

`DISCUSSION.md` §24, §25 and §26 give the decisions.

- **The crate.** `crates/panim-telegram` has the code that speaks to TDLib. `panim-core` stays without network code.
  - `tdjson.rs` is our own thin layer over the JSON interface of TDLib. It has four functions: `td_create_client_id`, `td_send`, `td_receive` and `td_execute`. The requests are `serde_json` values.
  - All the logic uses the interface `Td`. The real implementation is `RealTd`. The tests use a fake TDLib (the feature `fake`), with scripted updates in and the requests out. No test uses the network or a Telegram account.
  - `client.rs` keeps the state of one client. It has no network code and no clock: it reads one value of TDLib and gives the requests to send and the events for the app.
  - `runtime.rs` has one receive loop for all the clients. The loop starts with the first client and stops when no client is open. It does not hold its lock while it gives an event to the app.
- **The build.** `build.rs` finds `libtdjson.so` through pkg-config (`tdjson`, version 1.8.68 or later) and puts its folder into the run path of the binary.
  - `build.rs` also reads the `api_id` and the `api_hash` of PANIM. The environment has priority (`PANIM_TELEGRAM_API_ID`, `PANIM_TELEGRAM_API_HASH`). Then it reads `~/.config/panim/telegram.env`. `PANIM_TELEGRAM_ENV` names a different file, and an empty value means "no file".
  - The script writes the values into `OUT_DIR`. It does not print them and does not use `cargo:rustc-env`.
  - Without the values, the build passes with one warning. Then only the own values of the person can start a login (the next item).
- **The app values** (milestone 10, `DISCUSSION.md` §29). A login uses PANIM's values from the build (the built-in values) or the own values of the person.
  - In the settings, the person selects "PANIM's app values" or "My own app values". The own values are an `api_id` (a number) and an `api_hash` (32 hexadecimal digits). The Rust side examines the form (`AppConfig::parse`).
  - The setting is in the sealed place `app/telegram` (§3). The user interface never gets the `api_hash` back. It gets only the `api_id` and the state of the setting.
  - A new login uses the own values if the person selected them, or if the build has no values. Otherwise, it uses PANIM's values. If PANIM has no values of either type, the dialog says "Telegram is not configured" and points to the settings.
  - **Each account keeps the values of its login.** The secret of the account records them. For PANIM's values, the secret has no record. This is also the form from before milestone 10, so an old account uses PANIM's values. For the own values, the secret keeps a copy of them after a line feed: `panim-telegram-app/1 own <api_id> <api_hash>`.
  - Thus, a change of the setting changes only new logins. The removal of the own values does not change an account that logged in with them.
  - If the values of an account are not available, PANIM does not start its client and does not open its folder. The account shows the warning `telegram_values_missing`, which asks for a new login. This occurs for an account with PANIM's values in a build without values, and for a record that this version does not know. PANIM never starts a client with different values.
  - A login with PANIM's values writes no record, so that an earlier version of PANIM can still read the secret.
  - The help texts tell where to get the own values (my.telegram.org, "API development tools"). They also tell that an `api_id` belongs to one Telegram account. If the logins of several personas use the same own `api_id`, Telegram can see that they have the same owner. PANIM's values are the same for all the people who use PANIM.
  - What TDLib does when the `api_id` changes. These facts come from the source of TDLib 1.8.68:
    - An account with a complete login (`auth` is `ok` in the binlog) does not examine the `api_id`. It goes to the state "Ready" with the new values (`AuthManager.cpp`, the constructor).
    - A login that is not complete (`auth_state` in the binlog) is dropped if the `api_id` or the `api_hash` changed (`AuthManager::load_state`).
    - Each connection sends `initConnection` with the current `api_id` (`MtprotoHeader.cpp`). Thus, the server sees the new `api_id` on the old authorization key.
    - The source does not show if the server accepts a different `api_id` on an old authorization key. This is not known. The design does not need it, because each client starts with the values of its login.
- **The TDLib of PANIM.** `flake.nix` makes the package `tdlib-panim` from the package `tdlib` of nixpkgs (1.8.68), with the patch `nix/tdlib-no-direct.patch`. The dev shell supplies it. The patch removes each network path that does not use the proxy:
  1. The config recovery (`ConfigManager.cpp`) does not start. Without the patch, it sends HTTPS requests to Google DNS, Mozilla DNS, Azure and Firebase after 5 seconds of "Connecting", directly.
  2. A connection to an address without the proxy (`request_raw_connection_by_ip`) gives an error.
  3. The HTTP client of TDLib (`Wget`) gives an error. It has no proxy support.
  4. The resolver (`GetHostByNameActor.cpp`) accepts only an IP address. It does not call `getaddrinfo`.
  - The package keeps only the shared library, the headers and `tdjson.pc` (52 MiB).
  - PANIM does not send `pingProxy` or `testProxy`. These requests can connect past the proxy, and the patch does not change them.
  - The report of the TDLib spike (§9 of `spikes/tdlib/README.md`) gives the proof with `strace`. The spike was deleted after the check. Commit `3672d42` of the development repository has it.
- **The start of a client.** The order is fixed, and the tests examine it:
  1. Before the first client: `setLogVerbosityLevel` 0 and `setLogStream` with `logStreamEmpty`. The default log goes to stderr and contains the `api_hash`.
  2. The first request of each client: `addProxy` with `enable: true` for a proxy route, or `disableProxy` for a direct route. TDLib keeps the proxy in its database, so each start sends the request again.
  3. `setTdlibParameters`, when the client asks for it.
  4. After the login: `getMe`, the three scope notification settings, `getProxies` (to remove the proxies of earlier routes) and `loadChats`.
- **Only an IP address for the proxy.** TDLib gets the proxy as an IP address. If the route has a host name, the client does not start, and the account shows the error `telegram_proxy_name`. PANIM does not look up the name. The names `localhost` and `localhost.` become `127.0.0.1` in PANIM, without a lookup.
- **The parameters.** The three databases are off (`use_file_database`, `use_chat_info_database`, `use_message_database`), so TDLib writes only `td.binlog`.
  - `use_secret_chats` is false, and `use_test_dc` is false.
  - Each account has its own database key: 32 random bytes in the sealed secrets. The key is never empty, because an empty key means the fixed key of TDLib.
  - The device model is "PANIM", the system version is "Linux", and the application version is the version of the crate. The language is the language of the app. TDLib does not get the name of the computer.
- **The guard.** The guard is a second protection, for a build of TDLib without the patch. It runs only for a proxy route.
  - When the state is `connectionStateConnecting` for 4 seconds, the guard sends `setNetworkType` with `networkTypeNone`. After 2 seconds, it sends `networkTypeOther`. Without the patch, TDLib starts the config recovery after 5 seconds.
  - The guard is a small state machine with the time as a parameter. Its tests use no clock.
- **The login.** The person gives the phone number with the country code, then the code, then the two-step password if the account has one.
  - Each login has a new folder and a new database key. A cancel, a lock or an error that stops the login deletes the folder.
  - The code step shows the route of the code, the route of the next code and the timeout, from `authenticationCodeInfo` (`td_api.tl` of TDLib 1.8.68). The routes are: a message in the Telegram apps, an SMS (a code, a word or a phrase), a call, a flash call, a missed call, fragment.com, and Firebase. An unknown type is "other".
    - The route "app" is a message in the chat "Telegram" of each Telegram app with a login. The screen says where to look.
    - Firebase is a device check that only the official apps on Android and iOS can do. PANIM cannot do it. A resend can change the route.
  - **The resend.** The command `telegram_login_resend` sends `resendAuthenticationCode` with the reason `resendCodeReasonUserRequest`. PANIM permits it only in the code step, only when a next route exists, and only after the timeout of Telegram. These are also the conditions of TDLib.
    - The client keeps the time of each code state. A resend before the timeout gives `telegram_resend_early` with the seconds, and PANIM sends no request. The screen shows a countdown.
    - Without a next route, the screen says that Telegram offers no other route for this login. The person can start again later.
    - The new `updateAuthorizationState` gives the new route, the next route and a new timeout.
  - **A different number.** In the code step, TDLib accepts `setAuthenticationPhoneNumber` again, if no other login request is in progress. The action "Use a different number" sends the new number in the same login. The screen shows the number as the person typed it, so that the person can see a wrong digit. PANIM does not keep the number.
  - Each login request waits for a new `updateAuthorizationState`, not for a different state. A resend on the same route gives the same state again.
  - PANIM cannot serve the states `WaitEmailAddress`, `WaitEmailCode`, `WaitOtherDeviceConfirmation`, `WaitPremiumPurchase` and `WaitRegistration`. The dialog tells the person why the login cannot continue.
  - The errors of TDLib become error codes with texts: a wrong code, an expired code, a wrong password, a flood wait with the seconds, a phone number that is not correct, and a banned number.
  - The phone number, the code and the password go only into the request. They never go into an error, a log or the user interface. An error text of Telegram with five or more digits in sequence is not shown.
  - After the login, the account gets the numeric user id as `remote_id` and the first active username as the handle. An account can have no username. The proof screen (§6a) then makes a proof for Telegram.
  - If PANIM has the same Telegram account, it refuses the new login with `account_exists`, and it sends `logOut` for the new session.
  - When the username changes in Telegram, PANIM changes the handle and removes the kept proof.
- **Storage.** The TDLib folder of each account is in the profile area in memory (`$XDG_RUNTIME_DIR`, §5). The vault keeps it as one sealed archive, with the code of the browser profile (`panim-vault/src/profile.rs`).
  - The folder has a random name (`telegram-` and 16 hexadecimal digits). The secret of the account has the database key and then the name of the folder. Thus, the stored structs of the vault do not change.
  - The archive does not keep `files/`. With the databases off, TDLib downloads nothing that PANIM needs.
  - PANIM seals the folder when the login is complete, before it adds the account. It also seals the folders every five minutes, with the browser profile, if a file changed (§5). It always seals them at a lock or an exit.
  - At a lock or an exit, PANIM sends `close` to each client and waits for `authorizationStateClosed` (5 seconds for all the clients). Then it seals the folders and removes them from memory. At the unlock, PANIM opens the folders and starts new clients.
  - A vault from before milestone 7 has no Telegram data, and it opens as before. A test makes sure that the app adds no file to a vault without a Telegram account.
- **Chats to items.** The client keeps a small model of the chats in memory. It reads `updateNewChat`, `updateChatLastMessage`, `updateChatPosition`, `updateChatReadInbox`, `updateChatIsMarkedAsUnread`, `updateChatNotificationSettings`, `updateScopeNotificationSettings`, `updateChatTitle`, `updateChatDraftMessage`, `updateChatPermissions`, `updateUser`, `updateSupergroup` and `updateBasicGroup`.
  - A chat gives one timeline item (source id `c/<chat id>`) only if it has a position in the main chat list, has a last message and is not muted. An archived chat has no position in the main chat list.
  - If the chat uses the default mute setting, the setting of its scope applies (private chats, groups or channels). Until PANIM knows that setting, the chat gives no item.
  - The author of the item is the title of the chat. The time and the text come from the last message. The text has no more than 300 characters.
  - A message that is not text has a kind, for example `photo` or `voice`. The user interface shows the label of the kind in the language of the person, and also the caption. TDLib has more than 100 content types. PANIM has 30 kinds, and an unknown type gets the kind `other`.
  - The raw record of the item is a small JSON object with a version: the kind, the unread count, the "marked as unread" flag, the direction of the last message and the right to write.
  - When a chat becomes muted, goes to the archive or leaves the main list, PANIM removes its item.
  - The client loads no more than 10 pages of 100 chats. After a complete list, PANIM removes the stored items of the chats that give no item now. If the list is cut, the count shows a "+".
- **Unread.** An item is unread if Telegram counts unread messages in the chat (`unread_count > 0`), or if the person marked the chat as unread in a Telegram app.
  - A chat that has only an unread reaction is read. A mention is a message, so Telegram counts it in `unread_count`.
  - A muted chat gives no item, also when it has an unread mention.
  - The item shows the unread count of Telegram for the chat. A chat that is only marked as unread shows a dot.
  - The count of the account is the number of shown chats that are unread, not the number of messages. The user interface says this.
- **Read state in the two directions.**
  - A click on an item sends `viewMessages` with `force_read: true` for the last message. If the chat is marked as unread, PANIM also sends `toggleChatIsMarkedAsUnread` with false. `force_read` marks the messages as read without `openChat`, so a click in the timeline does not open the chat.
  - PANIM changes the local state first. An `updateChatReadInbox` from before the read does not make the item unread again, while Telegram did not confirm the read. A new message from a different person ends this rule. If Telegram refuses the read, the item becomes unread again. This is the in-flight rule of the GitHub connector (§4a).
  - A read in a Telegram app comes as `updateChatReadInbox`, and the item becomes read in PANIM.
  - PANIM does not let the person mark a chat as unread, as for GitHub.
- **The chat panel.** The button "Open chat" opens a panel with the last messages of the chat.
  - The panel sends `openChat` when it opens and `closeChat` when it closes. Thus, TDLib gets all the updates of the chat while the person looks at it.
  - It asks `getChatHistory` for 50 messages. TDLib can give fewer, so the panel asks again, with no more than 5 requests. The panel keeps no more than 200 messages.
  - Each message shows the sender, the time, the direction and the text. A message that is not text shows the label of its kind.
  - When the panel shows a new message from a different person, PANIM sends `viewMessages` for it. Thus, the chat becomes read.
  - New messages, sent messages, changed messages and deleted messages show in the open panel. The app sends the event `telegram-chat-changed` to the main window.
  - `sendMessage` sends plain text only: `inputMessageText` with no entities and no link preview. The limit is 4096 UTF-16 code units. The answer box of the timeline uses the same command.
  - PANIM does not let the person write in a chat where the account cannot write: a channel of a different person, a group that does not permit text, a restricted member, or a secret chat. The panel tells the person.
- **The removal of an account.** PANIM sends `logOut`, so the session ends on Telegram. It waits 15 seconds for `authorizationStateClosed`. Then it deletes the folder, the archive, the secret and the items.
  - If Telegram does not confirm the logout, PANIM closes the client and deletes the local data. It tells the person to end the session in Telegram (Settings → Devices).
  - If Telegram ended the session before (a different device ended it), PANIM does not tell the person this.
- **A new route.** TDLib gets the proxy only at the start of a client. Thus, a new route closes all the clients and starts new clients on the new route.
- **The commands.** `telegram_status`, `telegram_login_start`, `telegram_login_phone`, `telegram_login_code`, `telegram_login_password`, `telegram_login_resend`, `telegram_login_cancel`, `telegram_chat_open`, `telegram_chat` and `telegram_chat_close`. The commands `set_read`, `answer` and `remove_account` also serve Telegram. The settings use `telegram_app`, `set_telegram_app` and `forget_telegram_app` (milestone 10). Only the main window can call these commands. The ACL tests examine this for account windows and profile windows.
- **The state of a client.** The list of accounts shows a warning mark when the client is not connected, when the client cannot start, and when Telegram ended the session.
- Known limits:
  - Nobody did a login with a test. Telegram turned off the test accounts in 2025 (§6 of the report of the TDLib spike). The person does the login checks on production by hand (`ROADMAP.md` §5).
  - No `strace` run examined a session with a login. The proof of the patch is for the cases without a login.
  - Without the config recovery, TDLib cannot find new addresses of Telegram on a network that blocks the built-in addresses. Use a proxy on such a network.
  - If a connection through the proxy stays in "Connecting" for 4 seconds, for example on a slow Tor circuit, the guard stops it, and TDLib tries again. On a very slow proxy, this can repeat.
  - A fatal error in TDLib stops the process (`td_log.h`: verbosity level 0 is for fatal errors). TDLib can call a function before the stop (`td_set_log_message_callback`), but the app cannot prevent the stop. PANIM does not set that function. After a crash, the TDLib folder has the state of the last seal.
  - A crash loses the changes of TDLib after the last seal (no more than five minutes). The login stays, because PANIM seals the folder when the login is complete.
  - PANIM seals the folder every five minutes while TDLib can write. If the last record of the binlog is not complete, TDLib drops it when it opens the folder. The archive of a lock is exact, because the clients are closed.
  - The panel shows text only. It shows no photo, file, voice message, reaction or reply.
  - A chat that is not in the first 1,000 chats of the main list gives no item.
  - While the client runs, the database key and the messages of the open panel are in memory as plain data.
  - The binary needs the same version of TDLib that it was built with (`libtdjson.so.1.8.68`). The Nix package links to `tdlib-panim` (§11).

## 5. Browser views

- Each persona has one browser profile. The profile is a data folder of the Tauri webview (cookies and storage) and the proxy of the persona. All browser windows of the persona use this profile. Thus, the logins stay after a restart.
- Browser-view accounts open in this profile. These are X (with or without a key), Discord and Proton Mail free plans. Any other site can also open in the profile.
- The device page of GitHub, the pages of timeline items and the home page of a GitHub account also open in this profile, in profile windows (§2a). Thus, the login to GitHub stays with the persona and uses its route.
- In one profile, each site sees one login. Thus, two accounts on one service belong to two personas.
- PANIM seals the profile folders with the master key (§3):
  - When the app opens, it unseals each profile into memory. On Linux, this is `$XDG_RUNTIME_DIR`, which is not on the disk.
  - Each vault has its own folder there (`$XDG_RUNTIME_DIR/panim/<16 hexadecimal digits>`). The name comes from the master key. Thus, two copies of the app with different data folders keep their open profiles apart.
  - The app seals the profile again when it closes, and also every five minutes. Thus, a crash loses only a few minutes of browser state.
  - The periodic seal occurs only if a file changed. PANIM compares the path, the size and the modification time of each file with the last seal. The `-shm` file of a SQLite database is not in this comparison.
  - Before Linux 6.13, the times of the files in tmpfs are not exact. Thus, if a file changed in the last two seconds, the next periodic seal always occurs.
  - The seal at a lock, at an exit and at a backup always occurs.
  - The seal makes a copy of each SQLite database with the backup interface of SQLite (`rusqlite`, with the SQLite of the system). The copy is one state of the database. The archive holds the copy without its `-wal`, `-shm` and `-journal` files. If the copy fails, the seal copies the files as before.
  - A database in WAL mode without a `-wal` file is closed. The seal copies it as a usual file.
  - `DISCUSSION.md` §46 to §49 give the reasons and the known limits.
- PANIM does not read the content of the page.
- The unread count comes from the title of the page. The Rust side reads the title when it changes. It keeps only the count, not the title:
  - `(N)` at the start of the title gives the count N, for example on X and Proton Mail.
  - `•` or `●` at the start gives "unread items exist, but the count is not known". Discord uses this.
  - All other titles give no unread items.
- A small script in each account window replaces `window.Notification` of the page (`src-tauri/src/unread.js`). When the page makes a notification, the script calls `page_notice`. This signal has no content. The script does not read the page, and it does not send the title or the body of the notification.
- The app ignores a second notice from the same account in one second. When the person looks at the account window, the notice mark goes away.
- The unread state is in memory only. It exists only while the account window is open. A closed window has no count, and the app does not keep windows open in the background.
- Known limits:
  - A notification from a service worker (`showNotification`) does not go through the script, so PANIM does not see it.
  - The first call of the script can go to the `ipc:` custom protocol. A page with a strict CSP blocks it, and Tauri then uses `postMessage`. The site can see a CSP report about the blocked request.
- Known limit: All profiles have the same browser fingerprint as the computer (`DESIGN.md` §2).

## 6. Persona keys and account proofs

`DESIGN.md` §6 tells the purpose of the keys and the proofs.

- **The key is a raw secp256k1 key.** ZIKARON uses this type of key for an author (`base/zikaron-v1.md` §5 in the ZIKARON repository). Thus, in step 3, the same key becomes the ZIKARON identity of the persona, and no data moves. The address of the key is the name of the persona. In step 1, a usual crate (`k256`) signs. PANIM does not use ZIKARON code.
- **The key comes from 12 recovery words** (`crates/panim-core/src/words.rs`), with the method of ZIKARON:
  - BIP-39 (English list): 16 random bytes give 12 words. The seed is `PBKDF2-HMAC-SHA512(words, "mnemonic", 2048)`, with no passphrase.
  - BIP-32 at `m/44'/60'/1'/0/0`. In the family derivation table, PANIM is project 1. ZIKARON is project 0.
  - Each persona has its own words, so the group and the index are always 0.
  - The tests use the BIP-32 test vector 1, the BIP-39 vector of 16 zero bytes, and the published address of the Hardhat phrase.
  - The same words in ZIKARON give a different key, because ZIKARON uses project 0. In step 3, ZIKARON must read the persona key at project 1, or take it from the key file.
- **The key obeys the rules of ZIKARON from the first day.** If it does not, it cannot become the key of an author later:
  - The key signs only messages that have the ZIKARON shape: a domain literal, a line feed and a 32-byte hash.
  - The domain literals are the literals of PANIM, for example `panim/1-proof` and `panim/1-revoke`. No ZIKARON domain uses these literals. `zikaron-v1.md` §5.7 permits these documents as "the client's own documents".
  - The signatures are EIP-191, with RFC 6979 deterministic nonces and low-s. `zikaron-v1.md` §5.7 sets these rules for producers.
  - The key does not answer a `personal_sign` request from a different application. It does not sign an EIP-7702 authorization. It does not hold money.
- **A proof statement** is a short text in a fixed form. It gives the address of the persona, the platform, the account and the date. For the account, it gives the handle and also the stable numeric id, if the platform has one. The key signs the 32-byte hash of the text. The person posts the text and the signature together.
- **A revocation** has the same form, with the literal `panim/1-revoke`. It gives the account and the date when the account stopped being part of the persona. The persona publishes it on its other proven accounts, because a thief can have the revoked account.
- **To make a proof:** PANIM signs the statement. Then it shows the text to post, and where to post it (`DESIGN.md` §6). PANIM does not post the proof through a connector. An account belongs to one persona, so the key of that persona always signs the statement.
- **To examine a proof**, PANIM does two checks:
  1. The signature. PANIM compares the signature of the statement with the address of the persona. The form is fixed and public, so any person can do this check without PANIM.
  2. The source of the post. If the connector has an API for it, PANIM gets the post through the API (a GitHub gist, or X with the key of the user). If not, the person looks at the post in a browser view and pastes the statement into PANIM. PANIM does not read the page.
- **The exact text form** is in `base/panim-proof-v1.md`. Version 2 (`base/panim-proof-v2.md`, release 0.3.0) adds Bluesky, with a DID as the id. Other tools can use it to make and examine proofs without PANIM. The value that the key signs is SHA-256 of the statement, as ZIKARON uses SHA-256 for `presig`.

## 6a. The proof screens (milestone 6)

`DISCUSSION.md` §23 gives the decisions.

- **The logic.** `panim-core` (`proof.rs`) makes the statement for an account, without disk or network:
  - `account_handle` gives the handle in the form of `base/panim-proof-v1.md` §5. It removes one `@` at the start. For email and Discord, it changes the handle to lowercase. Only a Telegram account can have no handle.
  - `account_id` gives the id. Email has the id `-`. All other platforms must have a numeric id. PANIM never makes a statement with the id `-` for a platform that has an id.
  - `proof_statement` makes the statement with the date of the clock in UTC. The session gets the time as a parameter, so a test can set it.
  - `current_flags` marks the current block of each persona and account with the rule of §8.2.
- **The id of an account.**
  - A joined GitHub account gets its numeric id from the login (§4a).
  - A browser-view account has no id, because PANIM does not read the page. The person enters the handle and the numeric id on the page "Proofs" (`set_identity`). The id becomes the `remote_id` of the account.
  - A Proton Mail browser-view account is an email account. It has the id `-`, and its address is the handle.
- **The signature.** `make_proof` reads the persona key from the sealed secrets, signs the statement and drops the key. It does not ask for the passcode, because a proof is a public signature and does not show the key. The signature is deterministic, so a second proof on the same date gives the same block. A different kept block changes only after the person agrees.
- **The kept proofs.** For each account, the store keeps the latest block and the URL where the person posted it.
  - The sealed place is `store/proofs`. The file has a version number and a list of pairs (account, proof).
  - A vault from before milestone 6 has no `store/proofs`. The store reads that as "no proofs". The other files do not change, so an old vault opens. A test writes the old files with a copy of the old structs and opens them.
  - The removal of an account removes its kept proof. A new handle or a new id removes the kept proof too, because the block no longer names the account.
- **The check of the gist of the person** (`check_own_proof`). It uses the gist check of §4a. It passes only if a block in the gist is correct, names this persona and this account, and is the current state. Then the gist becomes the location of the proof. A failure gives one reason: the gist is secret, the gist has no block, the block is for a different account or persona, the signature is wrong, or a later revocation exists.
- **The check of a different persona.**
  - A gist: `check_github_proof` gives the account that published the gist, and a verdict for each block. Each correct block also has its current state. The check gives no more than 50 blocks. It cuts each text from the gist to 120 characters.
  - A pasted text: `check_pasted_proof` finds the block, and examines its form and its signature (§7, steps 2 to 6). It uses no network. The text has no more than 16 KiB. PANIM did not examine steps 7 and 8, so the page shows the account and the platform that the person must compare.
  - PANIM does not look for revocations on the other accounts of the persona (`DESIGN.md` §8). The page says this.
  - The user interface shows all remote text as text, never as HTML.

## 7. ZIKARON

- PANIM starts ZIKARON as a child process: `zikaron <verb> …`. The stdout is one canonical JSON value. The exit codes and the refusal reasons come from fixed tables (`CLI-SCHEMA.md` in the ZIKARON repository). If stdout has no bytes, the call was incorrect.
- Alternatively, PANIM uses the MCP server of ZIKARON.
- Step 1 does not use ZIKARON in any form. From step 3, ZIKARON anchors the history of each persona.

## 8. Network separation

- Each persona has a route: direct, a proxy or Tor. The HTTPS connectors use an HTTP client with the proxy of the persona.
- TDLib and the browser profile set the proxy of the persona in their own settings.
- TDLib gets the proxy as an IP address, before its network starts. The TDLib of PANIM has a patch that removes the direct requests, and the connector has a guard as a second protection (§4b). A host name in the route stops Telegram, because PANIM does not look up the name outside the proxy.
- **Proton Mail Bridge is one process for all its accounts.** Thus, Proton sees all the accounts on one route. To keep them apart, use one Bridge for each persona. If not, the link is a known limit.

## 9. Checks before we build

- Done (2026-11-12, the webview spike, deleted after the check): A webview on Linux (WebKitGTK 2.54, `wry` 0.57) uses its own proxy and data folder. Results:
  - x.com, discord.com and mail.proton.me load and show their sign-in pages.
  - A cookie that lasts and the local storage stay after a restart.
  - A second profile does not see them.
  - The HTTP proxy and the SOCKS5 proxy carry the traffic. With SOCKS5, the proxy gets the host name, so DNS does not leak.
  - A person did not sign in to a real account at that time. The person did this check on 2026-11-13 with the app: a real login stays after a restart.
  - Known problems: WebKitGTK sends an old user agent (`Version/60.5 Safari`). Set a current user agent. The cookies file in the profile folder is plain text.
- Done in code (2026-11-13, `crates/panim-vault/src/profile.rs`): PANIM seals each profile folder as one archive. It opens the profile into `$XDG_RUNTIME_DIR` (on this computer, a `tmpfs` of 3 GB). The tests use usual files. A test with a live WebKit profile comes with the app (milestone 3).
  - Known limit: if the computer has swap, the kernel can write `tmpfs` pages to the swap. Use encrypted swap, or no swap.
- IMAP/SMTP on TLS operates through the proxy of the persona. This is for Proton Mail Bridge and, later, for other email services.
- Done (2026-11-16, the TDLib spike, deleted after the check): TDLib 1.8.68 operates through our own thin layer over its JSON interface. The test accounts of Telegram are off, so the login part did not run in the spike. The person did the login checks on production by hand on 2026-11-17 (`ROADMAP.md` §5). Results:
  - The layer has four functions (`td_create_client_id`, `td_send`, `td_receive`, `td_execute`) and uses `serde_json`. `pkg-config` finds the library (`tdjson`). We do not use the crate `tdlib-rs`: it has types for TDLib 1.8.61 only, and it adds approximately 100 packages.
  - `addProxy` with `enable: true` before `setTdlibParameters` puts all the connections on the proxy from the start. `addProxy` after the answer to `setTdlibParameters` is too late: TDLib made one direct connection first.
  - TDLib keeps the proxy in its database. For a direct route, send `disableProxy` before `setTdlibParameters`.
  - With SOCKS5, the proxy gets IPv4 addresses. TDLib has the addresses of Telegram in its code, so it does no DNS lookup with a proxy that operates.
  - When the proxy is down or refuses, TDLib tries the proxy again. It makes no direct connection.
  - The database key protects `td.binlog` and `db.sqlite` (on the test servers: `td_test.binlog` and `db_test.sqlite`). A wrong key gives the error 401, and the file does not change. With the three database flags off, TDLib writes only the binlog.
  - `setLogVerbosityLevel` 0 and `logStreamEmpty`, before the first client, stop all the log output. TDLib writes no log file by default.
  - Each client has 4 threads. One `td_receive` loop serves all the clients. `close` ends a client in less than 30 ms, and the same process can open the database again.
  - The build script can read `api_id` and `api_hash` from the environment or from `~/.config/panim/telegram.env`. The build passes without them. The values do not show in the build output.
  - Known problems:
    - Without a patch, TDLib can go past the proxy. When the proxy accepts a connection and Telegram does not answer for 5 seconds, TDLib looks up `dns.google` with the system resolver and connects to it directly. It can also ask Mozilla, Azure and Firebase hosts. No option stops this. The request does not contain the account, but it shows the real address of the computer.
    - Without a patch, if the proxy has a host name, TDLib asks `dns.google` directly for its address.
    - Solved (2026-11-16): the TDLib of PANIM has the patch `nix/tdlib-no-direct.patch` (§4b). In 28 runs with `strace`, the cases above made connections only to the proxy, with no DNS lookup. A direct route and a proxy that operates still reach the state "Ready" (`spikes/tdlib/README.md` §9).
    - The default log of TDLib goes to stderr and contains the `api_id` and the `api_hash`.
    - An empty database key means the fixed key `cucumber`. Always give a key.
    - The binary needs the same version of the library that it was built with (`libtdjson.so.1.8.68`). The package `tdlib` of nixpkgs has 203 MiB, because it contains the static libraries. `libtdjson.so` has 52 MB. The package `tdlib-panim` keeps only the shared library.
    - TDLib writes `db.sqlite` with the mode 0644 and its folders with the mode 0750.
  - Not done: the login on the test servers. The servers refused the test phone numbers (`99966XYYYY`) with `PHONE_CODE_INVALID`. Since June 2025, a test account needs a real phone number and the official iOS app. Thus, no run examined the chat list, the unread counts, the messages, `logOut` with a login, or the files of a session with a login.
- Done (2026-11-12, `crates/panim-core/src/key.rs`): The signatures from `k256` obey the producer rules of ZIKARON (`zikaron-v1.md` §5.7). The test uses the 42 signing vectors of ZIKARON (`base/zikaron-conformance/corpus/sign`) as data. For each vector, PANIM gives the same digest, the same signature and the same signer.

## 10. ZIKARON family components

Step 1 does not use ZIKARON. It has no ZIKARON crates and no family components. The persona key obeys the key rules of ZIKARON (§6), but the code of PANIM makes and uses the key. PANIM operates alone, with its own code or usual crates.

We prepare for a later step. The secrets, the local store and the network routes are behind interfaces of PANIM. When PANIM becomes part of the family, a family component can replace the code behind each interface. We copy the components in, as the family does, and record their versions in a `FAMILY.md` of PANIM. These are the possible components:

| Component | Function in PANIM |
|---|---|
| `transport` | Network routes for each persona |
| `local-seal`, `sealed-layer` | Sealed secrets |
| `passcode-gate` | The passcode that opens the app |
| `key-file` | Persona keys |
| `durable-write`, `single-writer` | The local store |
| `machine-backup` | Backup, sealed as the store is |
| `os-platform` | OS paths and the platform layer |

At that time, make sure that each call to the ZIKARON command line can name the identity that does the action.

## 11. The package and the release (milestones 10 and 11)

`DISCUSSION.md` §29, §30, §31 and §35 give the decisions. `README.md` gives the steps for the users. `CONTRIBUTING.md` gives the release steps of the maintainer.

- **The Nix package.** `flake.nix` gives `packages.<system>.panim` (also `default`) and `apps.<system>.default`. Thus, `nix build` and `nix run` operate. `nix/package.nix` has the derivation:
  - `fetchPnpmDeps` gets the packages of `crates/app/pnpm-lock.yaml` as a fixed-output derivation (`fetcherVersion` 5). When the lock file changes, the hash in `nix/package.nix` must change too.
  - `pnpm --dir crates/app build` makes `crates/app/dist` before Cargo starts.
  - `rustPlatform.buildRustPackage` builds the crate `panim` with the Rust version of `rust-toolchain.toml`. The input `rust-overlay` supplies this version. The Cargo packages come from `Cargo.lock` (`importCargoLock`), with no hash.
  - The build adds the feature `tauri/custom-protocol`. Without it, the binary loads the user interface from the dev server. `pnpm tauri build` adds this feature itself.
  - `wrapGAppsHook3` makes the wrapper `bin/panim`. It supplies the GSettings schemas, `glib-networking` (TLS for WebKitGTK) and the GStreamer plugins.
  - The binary links to `tdlib-panim`. The build script puts the folder of `libtdjson.so.1.8.68` into the run path.
  - The package also has a desktop file and two icons.
  - The tests do not run in the Nix build. They run in the dev shell (`AGENTS.md` §6).
  - The dev shell has the same Rust toolchain as the package, and also Clippy and rustfmt. Thus, a build in the dev shell does not need rustup.
- **The automatic tests.** `.github/workflows/ci.yml` runs on GitHub Actions for a push to `main`, a pull request against `main`, and a start by hand.
  - The job `test` runs on `ubuntu-latest` in `nix develop`: `pnpm install --frozen-lockfile`, `pnpm build`, `pnpm check`, `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
  - It sets `PANIM_TELEGRAM_ENV` to an empty value, and `PANIM_DATA` and `XDG_RUNTIME_DIR` to temporary folders. The tests need no window and no display.
  - The dev shell supplies the Rust version of `rust-toolchain.toml` from `rust-overlay`, with Clippy and rustfmt. The job does not use the rustup of the runner.
  - The reason: the Rust of the upstream rustup links with its own `rust-lld`, and not with the ld wrapper of Nix. Then a test binary has no run path to the Nix libraries, and it cannot start. The Rust of `rust-overlay` puts the ld wrapper of Nix around `rust-lld`.
  - `Swatinem/rust-cache` runs `rustc` and `cargo` in the dev shell (`cmd-format`).
  - The job also gets the pnpm packages of `nix/package.nix` again. Thus, a wrong `fetchPnpmDeps` hash makes the run fail.
  - The job `package` does `nix build .#panim`. It runs only when a person starts the workflow by hand and selects `package`. Do this before a release.
  - Each action has a full commit SHA, not a tag, because the owner of an action can move a tag.
  - Two caches of GitHub Actions keep a run short. `nix-community/cache-nix-action` keeps the Nix store with the dev shell and `tdlib-panim`. `Swatinem/rust-cache` keeps the Cargo packages and `target/`. A pull request reads the caches of `main`.
    - The job saves the Nix store immediately after it builds the dev shell. Thus, a failed test does not stop the save. Before the save, the garbage collector removes all paths that have no root. The dev shell and the flake inputs have a root.
    - A run on `main` saves the Nix store if no cache has its key. A pull request also saves it if no cache has its key, for example when it changes `flake.lock`. Only this pull request can use that cache.
    - Only a run on `main` saves the Rust cache. It also saves it after a failed test.
  - The key of the Nix cache has the hash of `flake.nix`, `flake.lock` and the TDLib patch. A new key builds TDLib again only if TDLib changed.
  - A private repository has 2,000 minutes each month (free plan). The first run builds TDLib and takes approximately 40 to 70 minutes. A run with the caches takes approximately 8 to 12 minutes.
  - The workflow is not in the public repository (`.gitattributes`). The public repository gets only the snapshots of the releases.
- **The Telegram app values.** The source never has them.
  - `packages.panim` has no values. A pure build cannot read the environment or `~/.config/panim/telegram.env`, and the derivation sets `PANIM_TELEGRAM_ENV` to an empty value. Then the app asks for the own values of the person.
  - `packages.panim-release` reads `PANIM_TELEGRAM_API_ID` and `PANIM_TELEGRAM_API_HASH` from the environment. This is possible only with `nix build --impure`. The flake examines the form of the values, and its errors do not show the values.
  - The values of a release build go into the derivation, the Nix store of that computer and the binary. Any person who has the binary can read them. This is also true for all the other apps that have an `api_id`.
- **The AppImage.** `packages.appimage-release` (with the values) and `packages.appimage` (without them) are one file with the full closure of PANIM. The input `nix-appimage` makes it:
  - The file is a type 2 AppImage: the runtime of the AppImage project and a SquashFS image (zstd) with `/nix/store`.
  - The `AppRun` makes a user namespace and shows the store of the image at `/nix/store`. Then it starts a small launcher.
  - The `AppRun` comes from `appruns/userns-chroot/main.c` of `nix-appimage`, with our patch `nix/appimage-apprun-store.patch` (`DISCUSSION.md` §30). The flake compiles it as a static binary, as the original, because it runs before the store of the image is visible.
    - On NixOS, the `AppRun` makes an overlay of the store of the image and the store of the system.
    - On a system without `/nix/store`, the overlay fails. Then the `AppRun` binds a folder of the image onto `/nix/store`. Without the patch, it bound `<appdir>/nix`, so each path became `/nix/store/store/<hash>-...`, and the start failed with "No such file or directory". The patch binds `<appdir>/nix/store`.
    - The rest of `main.c` does not need a `/nix` on the system. It does not copy the root entry `nix` of the system into the new root, and the new `/nix` comes from the image.
    - The second patch `nix/appimage-apprun-userns-help.patch` (milestone 11) adds a message for a system that blocks unprivileged user namespaces, for example Ubuntu 24.04 and later with AppArmor. The patch file tells the details:
      - The message shows when `unshare` fails, when a write to `uid_map`, `setgroups` or `gid_map` fails, and when the mount of the tmpfs fails with `EPERM` or `EACCES`. The first error line of the `AppRun` stays.
      - If `/proc/sys/kernel/apparmor_restrict_unprivileged_userns` is 1, the message shows an AppArmor profile with `userns` for the path in `$APPIMAGE`. The runtime of the AppImage sets this variable. If the path has a quote or a pattern character, the message points only to `README.md`.
      - If not, the message tells the person to ask the administrator. In each case, it names the Nix package and the section "Ubuntu 24.04 and later" of `README.md`.
      - `--appimage-extract-and-run` does not help. The runtime extracts the image and starts the same `AppRun` (`src/runtime/runtime.c` of `appimage/type2-runtime`).
      - The patch is separate from the store patch, so that PANIM can remove the store patch when `nix-appimage` fixes the bind.
    - `packages.appimage-apprun` is the patched `AppRun` alone, for a test.
  - On a system that is not NixOS (no `/run/opengl-driver`), the launcher tells libglvnd and Mesa to use the Mesa drivers in the image. The drivers of the system do not load with the libraries of Nix. It also sets `FONTCONFIG_FILE` to `/etc/fonts/fonts.conf` of the system, because the fontconfig of Nix knows only one font and no Chinese font.
- **The checksums and the signature.** `nix run .#release -- --version X.Y.Z --ref REF --key KEY --out FOLDER [--artifact FILE]... [--pubkey PUB]` runs `nix/release.sh`:
  1. It makes the source snapshot `panim-<version>-source.tar.gz` with `git archive` and `gzip -n`. The same commit gives the same bytes.
  2. It copies each artifact into the folder.
  3. It writes `SHA256SUMS` and signs it with minisign. The trusted comment has the version and the commit.
  4. It examines the checksums, and with `--pubkey` also the signature.
  - The secret key must be outside the repository (the person keeps it in `~/.minisign/panim.key`). The script never makes a key. `README.md` has the public key (key id `7B5C5C4A551F160B`).
- **The public commit** (milestone 11, `DISCUSSION.md` §31, §35). The public repository `SakuraRainn/panim` has one commit for each release. `nix run .#publish -- --from FOLDER --clone CLONE --version X.Y.Z --name NAME --email EMAIL [--pubkey PUB] [--allow-any-email]` runs `nix/publish.sh`. It is a separate script from `release.sh`, because it has different inputs: the signed output folder and a clone of the public repository, not the development repository and the secret key.
  1. It examines the line of `panim-<version>-source.tar.gz` in `SHA256SUMS`. With `--pubkey`, it also examines `SHA256SUMS.minisig` and the trusted comment "PANIM <version>, commit <commit>".
  2. It refuses a clone that is not clean, that is not on `main`, that has the tag already, or that tracks a file that `export-ignore` removes. An empty clone gets the branch `main`.
  3. It removes all the tracked files with `git rm`, copies the files of the archive into the clone, and adds them. Thus, a file that the release does not have goes out.
  4. It makes one commit with the message "PANIM <version>" and a sentence that the commit is the same as the signed archive. The given name and email are the author, the committer and the tagger. Then it makes the annotated tag `v<version>`.
  5. It examines the result. The mode and the blob of each file in the tree of the tag must be the same as in the archive (`git hash-object --no-filters`). The `export-ignore` paths must not be there. The script knows the paths of today and also reads `.gitattributes` of the current repository. If `PANIM_TELEGRAM_API_HASH` is set, `git grep -q` must not find it. The author, the committer and the tagger must be the given identity.
  6. If a check fails, it removes the commit and the tag, and the clone is as before. It never pushes. It shows the push command.
  - The email must have the form `<name>@users.noreply.github.com`, so that the email of the development repository does not become public. `--allow-any-email` stops this check.
  - The script uses `git` with settings that do not depend on the configuration of the person: no filters, no hooks and no signature.
- Known limits:
  - The AppImage needs unprivileged user namespaces. Some systems turn them off, for example Ubuntu 24.04 and later with its AppArmor rule. Then the `AppRun` tells what to do (the second patch above). On Ubuntu, an AppArmor profile with `userns` for the path of the AppImage gives the namespaces to the AppImage only (`README.md`). Nobody tested this profile on Ubuntu yet. The AppImage also needs FUSE 3, or the option `--appimage-extract-and-run`.
  - The AppImage has the Mesa drivers of nixpkgs. They do not support the proprietary NVIDIA driver. Then WebKitGTK can need `WEBKIT_DISABLE_DMABUF_RENDERER=1` or `LIBGL_ALWAYS_SOFTWARE=1`.
  - The AppImage does not supply the locales of glibc. GTK then uses the C locale for the format of dates and numbers. The texts of PANIM do not change.
  - Fixed: the first AppImage did not start on Fedora, because the `AppRun` bound the wrong folder (the patch above). A test with `bwrap --tmpfs /nix` on NixOS shows the fix. The new AppImage waits for a new test by the person on a system that is not NixOS (`ROADMAP.md` §6).
  - The current directory of the process must not be in `/nix`. The `AppRun` goes back to it after the change of the root.
