{
  description = "PANIM";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # The Rust version of `rust-toolchain.toml`, for the package and the dev shell.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" ];
      pkgsFor = system: import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      forAll = f: lib.genAttrs systems (system: f system (pkgsFor system));
      # TDLib with the patch of PANIM: TDLib makes no connection that does not use the proxy of the persona.
      # The patch file gives the details. The package keeps only the shared library, the headers and `tdjson.pc`.
      tdlibFor = pkgs: pkgs.tdlib.overrideAttrs (old: {
        pname = "tdlib-panim";
        patches = (old.patches or [ ]) ++ [ ./nix/tdlib-no-direct.patch ];
        postInstall = (old.postInstall or "") + ''
          rm $out/lib/*.a
          rm -r $out/lib/cmake
          find $out/lib/pkgconfig -type f ! -name tdjson.pc -delete
        '';
      });
      rustVersion = (lib.importTOML ./rust-toolchain.toml).toolchain.channel;
      # The Rust toolchain of the package: only rustc, cargo and the standard library.
      rustFor = pkgs: pkgs.rust-bin.stable.${rustVersion}.minimal;
      # The Rust toolchain of the dev shell: the same version, also with Clippy and rustfmt.
      rustDevFor = pkgs: (rustFor pkgs).override { extensions = [ "clippy" "rustfmt" ]; };

      # PANIM without the Telegram app values. A pure build cannot read the values, and the source never has them.
      panimFor = pkgs: pkgs.callPackage ./nix/package.nix {
        rustToolchain = rustFor pkgs;
        tdlib-panim = tdlibFor pkgs;
      };

      # PANIM with the Telegram app values of PANIM, for the hand checks with Nix (`docs/ROADMAP.md` §6). The release
      # packages do not come from it (`CONTRIBUTING.md`). Only an impure build can read the environment:
      # `nix build --impure .#panim-release`. The values then are in the Nix store and in the binary.
      releaseFor = pkgs:
        let
          id = builtins.getEnv "PANIM_TELEGRAM_API_ID";
          hash = builtins.getEnv "PANIM_TELEGRAM_API_HASH";
        in
        if id == "" || hash == "" then
          throw "panim-release needs PANIM_TELEGRAM_API_ID and PANIM_TELEGRAM_API_HASH in the environment, and `nix build --impure`."
        else if builtins.match "[1-9][0-9]{0,9}" id == null || builtins.match "[0-9a-fA-F]{32}" hash == null then
          throw "PANIM_TELEGRAM_API_ID must be a number, and PANIM_TELEGRAM_API_HASH must have 32 hexadecimal digits."
        else
          (panimFor pkgs).override {
            telegramApiId = id;
            telegramApiHash = lib.toLower hash;
          };
    in {
      packages = forAll (system: pkgs: rec {
        # `nix build .#tdlib` builds the library alone. The build is long.
        tdlib = tdlibFor pkgs;
        # `nix build` gives `result/bin/panim`, without the Telegram app values.
        panim = panimFor pkgs;
        default = panim;
        # `nix build --impure .#panim-release`, with the values in the environment.
        panim-release = releaseFor pkgs;
      });

      apps = forAll (system: pkgs: {
        # `nix run` starts PANIM.
        default = {
          type = "app";
          program = lib.getExe self.packages.${system}.panim;
          meta.description = "Start PANIM";
        };
        # `nix run .#release -- <arguments>` writes `SHA256SUMS` for the release files and signs it (`CONTRIBUTING.md`).
        release = {
          type = "app";
          program = lib.getExe (pkgs.writeShellApplication {
            name = "panim-release";
            runtimeInputs = with pkgs; [ coreutils git gnutar gzip minisign ];
            text = builtins.readFile ./nix/release.sh;
          });
          meta.description = "Write and sign the checksums of a release of PANIM";
        };
        # `nix run .#publish -- <arguments>` makes the public commit and the tag of a release from its signed source
        # archive, in a clone of the public repository. It does not push (`CONTRIBUTING.md`).
        publish = {
          type = "app";
          program = lib.getExe (pkgs.writeShellApplication {
            name = "panim-publish";
            runtimeInputs = with pkgs; [ coreutils findutils gawk git gnugrep gnused gnutar gzip minisign ];
            text = builtins.readFile ./nix/publish.sh;
          });
          meta.description = "Make the public commit of a release of PANIM from its signed source archive";
        };
      });

      # The libraries that the app and its web views need on Linux, Node.js for the user interface, and the Rust
      # toolchain of `rust-toolchain.toml`. The Rust of the dev shell comes before a rustup of the system in `PATH`.
      devShells = forAll (system: pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = [ (rustDevFor pkgs) pkgs.pkg-config pkgs.nodejs_24 pkgs.pnpm pkgs.minisign ];
          buildInputs = with pkgs; [
            webkitgtk_4_1
            gtk3
            libsoup_3
            glib-networking
            openssl
            librsvg
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
            gst_all_1.gst-plugins-good
            gst_all_1.gst-plugins-bad
            # TDLib is the Telegram library. It supplies `libtdjson.so` and the pkg-config file `tdjson`.
            (tdlibFor pkgs)
            # The seal copies the SQLite files of WebKit with this SQLite. It is the SQLite that WebKitGTK loads.
            sqlite
          ];
          # WebKitGTK needs glib-networking for TLS. Without it, no HTTPS page loads.
          GIO_EXTRA_MODULES = "${pkgs.glib-networking}/lib/gio/modules";
          # The GTK file dialogs need the GSettings schemas. Without them, the app stops when a dialog opens.
          shellHook = ''
            export XDG_DATA_DIRS=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}
          '';
        };
      });
    };
}
