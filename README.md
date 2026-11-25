# PANIM

English | [简体中文](README.zh-CN.md)

PANIM (פנים, "face", a word that is always plural in Hebrew) is a desktop persona manager for Linux and macOS. You connect your accounts to it. Then you read and answer them in one location, on your computer. There is no PANIM server.

Later, PANIM will let one person split into several personas. Each persona will keep its accounts, its browser profile and its network route apart. Today, PANIM has one persona, and the app does not show it.

`CHANGELOG.md` tells what each release does. `docs/DESIGN.md` tells the full plan.

## The accounts

| Account | How PANIM shows it |
|---|---|
| GitHub | In the timeline: the notifications and the events of the repositories that you watch. You can answer an issue or a pull request in a public repository. |
| Telegram | In the timeline: one item for each chat of your main chat list, with the last message and the unread count. A chat panel shows the text messages and sends text. |
| X | In a browser view |
| Discord | In a browser view |
| Proton Mail | In a browser view |

- **The timeline** shows the updates of GitHub and Telegram together. PANIM keeps them on your computer.
- **A browser view** opens the web page of the service in the private browser profile of PANIM. The profile keeps your login. PANIM reads only the unread count from the page, not your messages.

PANIM obeys the rules of each platform. It does not scrape pages, and it does not control your account through an API that is for bots. PANIM does not support Facebook, Instagram, Threads and WhatsApp.

## Only the official releases are PANIM

A different app can use the name and the look of PANIM. Only the releases of the official repository are PANIM. The official repository is https://github.com/SakuraRainn/panim. Each release has a `SHA256SUMS` file and its minisign signature `SHA256SUMS.minisig`. Before you start a download, check it (the next section).

The minisign public key of PANIM (key id `7B5C5C4A551F160B`):

```
RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa
```

Do not trust a key from a different location. A key in a message, in an issue or on a different site is not the key of PANIM.

## Check a download

1. From the release page, download the files that you need, `SHA256SUMS` and `SHA256SUMS.minisig` into one folder.
2. Install minisign, for example with `sudo apt install minisign`, `nix shell nixpkgs#minisign` or, on macOS, `brew install minisign`.
3. Examine the signature: `minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`. The result must be "Signature and comment signature verified". The trusted comment gives the version and the commit.
4. Examine the files: `sha256sum -c SHA256SUMS --ignore-missing`. On macOS, use `shasum -a 256 -c SHA256SUMS --ignore-missing`. Each file that you downloaded must be "OK".
5. If a step fails, do not start the file. Download it again from the official repository.

## Install

PANIM runs on Linux, on x86_64, and on macOS 14 or later, on Apple silicon.

- **Linux.** The packages need glibc 2.39 or later, for example Ubuntu 24.04, Debian 13 or Fedora 40 and later. Select one of these methods: the `.deb`, the `.rpm`, the AppImage or Nix.
- **macOS.** Use the `.dmg`. A Mac with an Intel processor cannot start it.

The `.deb`, the `.rpm`, the AppImage and the `.dmg` have the Telegram app values of PANIM (the section "Telegram").

### The .deb (Ubuntu and Debian)

1. Download `PANIM_<version>_amd64.deb` and check it (the section "Check a download").
2. Run `sudo apt install ./PANIM_<version>_amd64.deb`. Apt also installs the libraries that PANIM needs.
3. Start PANIM from the menu of the applications, or run `panim`.

### The .rpm (Fedora)

1. Download `PANIM-<version>-1.x86_64.rpm` and check it (the section "Check a download").
2. Run `sudo dnf install ./PANIM-<version>-1.x86_64.rpm`. Dnf also installs the libraries that PANIM needs.
3. Start PANIM from the menu of the applications, or run `panim`.

### The AppImage

The AppImage is one file. It has WebKitGTK, GTK and the other libraries of PANIM. It uses the graphics libraries and the fonts of your system.

1. Download `PANIM_<version>_amd64.AppImage` and check it (the section "Check a download").
2. Put it in a folder that only you can write to, for example `~/Applications`.
3. Run `chmod +x PANIM_<version>_amd64.AppImage`.
4. Start the file.

If the AppImage does not start correctly:

- **"No suitable fusermount binary found"**: install FUSE 3 (on Ubuntu, Debian and Fedora, the package `fuse3`). Without FUSE 3, the AppImage unpacks itself into a temporary folder each time, and it starts more slowly.
- **The window stays empty**: start the AppImage with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. If that does not help, also set `LIBGL_ALWAYS_SOFTWARE=1`.

The AppImage of release 0.1.x needed a user namespace and, on Ubuntu 24.04, an AppArmor profile. The AppImage of release 0.2.0 does not need them. To remove the old profile, run `sudo apparmor_parser -R /etc/apparmor.d/panim`, then `sudo rm /etc/apparmor.d/panim`.

### The .dmg (macOS)

1. Download `PANIM_<version>_aarch64.dmg` and check it (the section "Check a download").
2. Open the `.dmg`, and move PANIM to the folder Applications.
3. Start PANIM. The first time, macOS blocks it, because PANIM has no signature of Apple. Close the message. Do not move PANIM to the Trash.
4. Open System Settings > Privacy & Security. Below "Security", click "Open Anyway" for PANIM, and enter your password.

macOS asks one time only. On macOS 15 and later, a Control-click on the app does not open it this way. Use System Settings.

PANIM keeps its data in `~/Library/Application Support/panim`. While PANIM runs, it keeps the open browser profiles on a RAM disk. Finder does not show this disk. Quit PANIM with Cmd+Q: PANIM then seals the data and removes the RAM disk.

### With Nix

If you have Nix with flakes:

1. Run `nix run github:SakuraRainn/panim` to start PANIM.
2. Or run `nix build github:SakuraRainn/panim` to get `result/bin/panim`.

A build from the source has no Telegram app values of PANIM. To add a Telegram account, give your own values (the section "Telegram").

## The first start

1. Select the language: English or Chinese.
2. Set a passcode with eight characters or more. The passcode opens PANIM each day.
3. PANIM shows your 12 recovery words. Write them on paper, in the correct order. Keep the paper safe and outside the computer. Anyone with these words can be you.
4. PANIM asks for four of the words. The first start is complete after this check.
5. If you want an extra copy of your key, save a key file. It has its own password. Keep it outside PANIM.

Your passcode, the 12 recovery words and the key file each open PANIM:

- If you forget the passcode, click "Forgot your passcode?". Use the 12 words or the key file, and set a new passcode.
- After five wrong passcodes, only the 12 words or the key file open PANIM.
- PANIM locks after 15 minutes with no input. You can change this time in the settings.

PANIM keeps all its data sealed in `~/.local/share/panim` (on macOS, in `~/Library/Application Support/panim`). Without the passcode, the 12 words or the key file, nobody can open it. Also the maintainer of PANIM cannot open it.

## Telegram

Telegram knows each app by its app values: an `api_id` and an `api_hash`.

- **PANIM's values.** The `.deb`, the `.rpm` and the AppImage have them. All the people who use PANIM share them, so they do not link your personas. They are the default.
- **Your own values.** Get them at my.telegram.org → API development tools. In PANIM, open Settings → Telegram app values, select "My own app values" and enter them. PANIM keeps them sealed and does not show the `api_hash` again.
- **Warning:** an `api_id` belongs to one Telegram account. If the logins of several personas use your own `api_id`, Telegram can see that they have the same owner.
- A change of this setting is only for new logins. Each account keeps the values of its login.

## The network route

In Settings → Network route, select how PANIM connects: direct, an HTTP proxy or a SOCKS5 proxy. GitHub, Telegram and the browser views all use this route. For Tor, use SOCKS5 with `127.0.0.1` and the port `9050`.

The Telegram library of PANIM has a change, so that Telegram makes no connection past the proxy.

## Backup

1. In PANIM, open Settings → Backup, and click "Back up…". Select the location of the file.
2. Keep the file safe. It stays sealed. Only your passcode, your 12 recovery words or your key file open it.
3. To restore it, start PANIM on a computer with no PANIM data. On the first screen, click "Restore from a backup", and select the file.
4. Close PANIM on the old computer first, or remove the Telegram accounts there. If not, the same sessions are in two places.

## Known limits

- The `.dmg` is only for Apple silicon, and it has no signature of Apple (the section "The .dmg (macOS)").
- All the browser views have the same fingerprint as your computer. The profile and the route of PANIM stop only easy links between personas.
- If your computer has swap, the system can write the open browser profile to the swap. Use encrypted swap or no swap.
- The unread count of a browser view exists only while its window is open.
- The Telegram chat panel shows text only. On a network that blocks the addresses of Telegram, use a proxy.
- The packages use the graphics drivers of your system. Nobody tested PANIM with the proprietary NVIDIA driver.

`CHANGELOG.md` has the full list for each release.

## Help

- **A problem or a question:** open an issue at https://github.com/SakuraRainn/panim/issues. Select the form for a problem or for a question.
- **A vulnerability:** do not open a public issue. Read `SECURITY.md`.
- **A change to PANIM:** read `CONTRIBUTING.md`. It tells how to build PANIM, the rules, and how a change comes into PANIM.

## License

PANIM has the MIT License. Refer to `LICENSE`.
