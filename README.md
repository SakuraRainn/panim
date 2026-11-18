# PANIM

English | [简体中文](README.zh-CN.md)

PANIM (פנים, "face", a word that is always plural in Hebrew) is a desktop persona manager for Linux. You connect your accounts to it. Then you read and answer them in one location, on your computer. There is no PANIM server.

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
2. Install minisign, for example with `sudo apt install minisign` or `nix shell nixpkgs#minisign`.
3. Examine the signature: `minisign -Vm SHA256SUMS -P RWQLFh9VSlxce6HvAgce8fOpN5K41RRR5zeh8KsFVMPIDtdmRGD1MkXa`. The result must be "Signature and comment signature verified". The trusted comment gives the version and the commit.
4. Examine the files: `sha256sum -c SHA256SUMS --ignore-missing`. Each file that you downloaded must be "OK".
5. If a step fails, do not start the file. Download it again from the official repository.

## Install

PANIM runs on Linux, on x86_64. Select one of these two methods: the AppImage or Nix.

### The AppImage

The AppImage is one file. It has all the libraries that PANIM needs, and the Telegram app values of PANIM (the section "Telegram").

1. Download `PANIM-<version>-x86_64.AppImage` and check it (the section "Check a download").
2. Put it in a folder that only you can write to, for example `~/Applications`.
3. Run `chmod +x PANIM-<version>-x86_64.AppImage`.
4. Start the file.

If the AppImage does not start:

- **"cannot unshare"**: your system blocks unprivileged user namespaces. Refer to the next section.
- **"Cannot mount AppImage"**: install FUSE 3 (on Ubuntu, Debian and Fedora, the package `fuse3`). Or start the AppImage with the option `--appimage-extract-and-run`.
- **The window stays empty**: start the AppImage with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. If that does not help, also set `LIBGL_ALWAYS_SOFTWARE=1`.

### Ubuntu 24.04 and later

The AppImage must make a user namespace. Ubuntu 24.04 and later block unprivileged user namespaces with AppArmor (`kernel.apparmor_restrict_unprivileged_userns = 1`). Then the AppImage stops with "cannot unshare: Operation not permitted", and it shows these steps with its own path.

The safe fix is an AppArmor profile that lets only the AppImage of PANIM make user namespaces. Ubuntu uses this method for the apps that need them. The other programs on your computer keep the protection.

1. Find the full path of the AppImage, for example `/home/you/Applications/PANIM-0.1.1-x86_64.AppImage`.
2. Write the profile. Put your path between the quotes:

   ```
   sudo tee /etc/apparmor.d/panim <<'EOF'
   abi <abi/4.0>,
   include <tunables/global>
   profile panim "/home/you/Applications/PANIM-0.1.1-x86_64.AppImage" flags=(unconfined) {
     userns,
     include if exists <local/panim>
   }
   EOF
   ```

3. Load the profile: `sudo apparmor_parser -r /etc/apparmor.d/panim`.
4. Start the AppImage again.

Know these points:

- The profile is for one path. If you move or rename the AppImage, change the path in the profile, and do step 3 again.
- For the next releases, you can write the pattern `PANIM-*-x86_64.AppImage` in the path. Use a pattern only in a folder that only you can write to.
- The profile does not limit PANIM in a different way. It only lets PANIM make user namespaces.
- To remove the profile, run `sudo apparmor_parser -R /etc/apparmor.d/panim`, then `sudo rm /etc/apparmor.d/panim`.
- The option `--appimage-extract-and-run` does not help. It starts the same first step, and this step also needs a user namespace.
- Do not turn off the protection for all programs (`sysctl kernel.apparmor_restrict_unprivileged_userns=0`). The profile is sufficient.
- You can also install PANIM with Nix (the next section). The Nix package does not need a user namespace.

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

PANIM keeps all its data sealed in `~/.local/share/panim`. Without the passcode, the 12 words or the key file, nobody can open it. Also the maintainer of PANIM cannot open it.

## Telegram

Telegram knows each app by its app values: an `api_id` and an `api_hash`.

- **PANIM's values.** The AppImage has them. All the people who use PANIM share them, so they do not link your personas. They are the default.
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

- PANIM runs only on Linux. A version for macOS comes in a later release.
- All the browser views have the same fingerprint as your computer. The profile and the route of PANIM stop only easy links between personas.
- The automatic lock counts only the input in the main window.
- If your computer has swap, the system can write the open browser profile to the swap. Use encrypted swap or no swap.
- The unread count of a browser view exists only while its window is open.
- The Telegram chat panel shows text only. On a network that blocks the addresses of Telegram, use a proxy.
- The AppImage has the graphics drivers of Mesa. Nobody tested it with the proprietary NVIDIA driver.

`CHANGELOG.md` has the full list for each release.

## Help

- **A problem or a question:** open an issue at https://github.com/SakuraRainn/panim/issues. Select the form for a problem or for a question.
- **A vulnerability:** do not open a public issue. Read `SECURITY.md`.
- **A change to PANIM:** read `CONTRIBUTING.md`. It tells how to build PANIM, the rules, and how a change comes into PANIM.

## License

PANIM has the MIT License. Refer to `LICENSE`.
