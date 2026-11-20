# The package of PANIM (milestone 10, `docs/ARCHITECTURE.md` §11).
#
# - pnpm builds the user interface in `crates/app/dist`. `fetchPnpmDeps` gets the packages of `pnpm-lock.yaml` as a
#   fixed-output derivation, so the build itself has no network.
# - Cargo builds the crate `panim` with the Rust version of `rust-toolchain.toml` and the feature
#   `tauri/custom-protocol`. Without this feature, the binary loads the user interface from the dev server.
# - The binary links to `tdlib-panim`, the TDLib with the patch of PANIM. The build script puts its folder into the
#   run path, so the binary finds `libtdjson.so.1.8.68`.
# - `wrapGAppsHook3` makes a wrapper that supplies the GSettings schemas, `glib-networking` (TLS for WebKitGTK) and
#   the GStreamer plugins.
#
# The Telegram app values: by default, the package has none (`telegramApiId = null`). Then the app asks for the own
# values of the person. `flake.nix` gives the values only to `panim-release`, from the environment of an impure
# build. The values then go into the derivation, the Nix store and the binary of that computer.
{
  lib,
  makeRustPlatform,
  rustToolchain,
  fetchPnpmDeps,
  pnpmConfigHook,
  pnpm,
  nodejs,
  pkg-config,
  wrapGAppsHook3,
  makeDesktopItem,
  copyDesktopItems,
  webkitgtk_4_1,
  gtk3,
  libsoup_3,
  glib-networking,
  gsettings-desktop-schemas,
  openssl,
  librsvg,
  gst_all_1,
  tdlib-panim,
  sqlite,
  telegramApiId ? null,
  telegramApiHash ? null,
}:

let
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
  root = ../.;
  # Only the files that the build needs. A change in `docs/` or `nix/` does not build PANIM again.
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../rust-toolchain.toml
      (lib.fileset.difference ../crates (
        lib.fileset.unions [
          (lib.fileset.maybeMissing ../crates/app/node_modules)
          (lib.fileset.maybeMissing ../crates/app/dist)
          (lib.fileset.maybeMissing ../crates/app/src-tauri/gen)
        ]
      ))
    ];
  };
  version = (lib.importTOML ../crates/app/src-tauri/Cargo.toml).package.version;
  hasValues = telegramApiId != null && telegramApiHash != null;
in
rustPlatform.buildRustPackage {
  pname = "panim";
  inherit version src;

  cargoLock.lockFile = ../Cargo.lock;
  buildAndTestSubdir = "crates/app/src-tauri";
  cargoBuildFlags = [
    "--features"
    "tauri/custom-protocol"
  ];

  pnpmDeps = fetchPnpmDeps {
    pname = "panim-ui";
    inherit version pnpm;
    src = lib.fileset.toSource {
      root = ../crates/app;
      fileset = lib.fileset.unions [
        ../crates/app/package.json
        ../crates/app/pnpm-lock.yaml
      ];
    };
    fetcherVersion = 5;
    hash = "sha256-WPhRwXWzjknKhMXMUv3MFghFJazFX2xMxFbGFbin8EY=";
  };
  pnpmRoot = "crates/app";

  nativeBuildInputs = [
    pkg-config
    nodejs
    pnpm
    pnpmConfigHook
    wrapGAppsHook3
    copyDesktopItems
  ];

  buildInputs = [
    webkitgtk_4_1
    gtk3
    libsoup_3
    glib-networking
    gsettings-desktop-schemas
    openssl
    librsvg
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-plugins-bad
    tdlib-panim
    # The seal copies the SQLite files of WebKit with this SQLite. It is the SQLite that WebKitGTK loads.
    sqlite
  ];

  env = {
    # The build script must not read `~/.config/panim/telegram.env`. An empty name means "no file".
    PANIM_TELEGRAM_ENV = "";
  }
  // lib.optionalAttrs hasValues {
    PANIM_TELEGRAM_API_ID = toString telegramApiId;
    PANIM_TELEGRAM_API_HASH = telegramApiHash;
  };

  # The crate `panim` needs `crates/app/dist` before it compiles.
  preBuild = ''
    pnpm --dir crates/app build
  '';

  # The tests run in the dev shell (`AGENTS.md` §6). Some of them need a runtime folder and a mock window.
  doCheck = false;

  desktopItems = [
    (makeDesktopItem {
      name = "panim";
      desktopName = "PANIM";
      comment = "A persona manager: your accounts in one place";
      exec = "panim";
      icon = "panim";
      categories = [ "Network" "InstantMessaging" ];
      startupWMClass = "panim";
    })
  ];

  postInstall = ''
    install -Dm644 crates/app/src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/panim.png
    install -Dm644 crates/app/src-tauri/icons/32x32.png $out/share/icons/hicolor/32x32/apps/panim.png
  '';

  passthru = {
    inherit hasValues;
  };

  meta = {
    description = "A desktop persona manager";
    license = lib.licenses.mit;
    mainProgram = "panim";
    platforms = lib.platforms.linux;
  };
}
