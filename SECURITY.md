# Security

English | [简体中文](SECURITY.zh-CN.md)

PANIM keeps your sessions, your keys and the link between your accounts. If you find a vulnerability in PANIM, tell the maintainer in private first. Then the maintainer can fix it before other people know about it.

## Report a vulnerability

Do not open a public issue for a vulnerability.

1. Open https://github.com/SakuraRainn/panim/security.
2. Click "Report a vulnerability". GitHub then opens a private report. Only you and the maintainer can see it.
3. Write the report (the next section), and send it.

If the button "Report a vulnerability" is not there, open an issue with the form "Question" and only this text: "I want to report a vulnerability in private." Do not write details of the vulnerability in this issue. The maintainer then tells you a private way to send the report.

## What to put in the report

- The version of PANIM, and the method of installation (AppImage or Nix).
- Your system, for example "Fedora 43, x86_64".
- What an attacker can do, and what the attacker must have first. For example: access to the files, a page in a browser view, or a position on the network.
- The steps to show the problem. A small test is better than a long text.
- Your name or your GitHub account, if you want the maintainer to name you when the fix is published.

Do not put your own passcode, recovery words, key file, tokens or Telegram values into the report. Use a test vault (`PANIM_DATA`) and test accounts.

## What occurs after the report

1. The maintainer tells you that the report arrived, and asks questions if necessary.
2. The maintainer makes the fix in a new release. Only the latest release gets fixes.
3. After the release, the maintainer publishes a security advisory in the public repository. The advisory names you, if you agree.

PANIM has one maintainer, so the time for a fix can change. The maintainer tells you the state of the work.

## What PANIM protects

- **The data on the disk.** PANIM seals the store, the secrets, the browser profile and the Telegram folders with XChaCha20-Poly1305. Each file is bound to its place, so a file cannot replace a different file.
- **The passcode.** The key that opens the vault comes from the passcode through Argon2id. After five wrong passcodes, only the 12 recovery words or the key file open the vault.
- **The persona key.** The 12 recovery words make the key. PANIM keeps it sealed. The key file has its own password.
- **The browser views.** A page in a browser view can send only one signal to PANIM, the notice of new messages. It cannot call the other functions of PANIM.
- **The network route.** GitHub, Telegram and the browser profile use the route of the persona. The TDLib of PANIM has a patch, so that Telegram makes no connection past the proxy.
- **The release files.** `SHA256SUMS` and its minisign signature show that a download is from the maintainer (`README.md`, "Check a download").

A way to go past one of these protections is a vulnerability. Report it.

## What PANIM does not protect

These limits are known. `docs/DESIGN.md` and `docs/ARCHITECTURE.md` tell more.

- **A computer that an attacker controls.** While PANIM is open, the data is open in memory and in `$XDG_RUNTIME_DIR`. Malware on the computer can read it.
- **The swap.** If the computer has swap, the system can write the open browser profile to the swap. Use encrypted swap or no swap.
- **The fingerprint.** All the browser views have the fingerprint of the computer. A tracker that tries hard can link personas with it.
- **The platforms.** Telegram knows the phone number of each account. An own `api_id` of Telegram links the personas that use it. A platform can see what you do on it.
- **The recovery words and the key file.** A person who has your 12 recovery words, or your key file and its password, can open your vault and sign as your persona.
- **A backup.** A backup has the browser logins and the Telegram sessions. It is sealed, but it opens with your passcode, your words or your key file.
- **The Telegram app values of PANIM.** The AppImage has them, so any person who has the AppImage can read them. This is true for all the apps that have an `api_id`.

A vulnerability in a platform (GitHub, Telegram, X, Discord, Proton) goes to that platform. A vulnerability in a library (for example WebKitGTK or TDLib) goes to its project. If PANIM uses a library in a way that is not safe, report it to PANIM.
