{
  description = "PANIM";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # The Rust version of `rust-toolchain.toml`. The package uses the same compiler as the builds with rustup.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # The AppImage of a release: one file with the closure of PANIM (`docs/ARCHITECTURE.md` §11).
    nix-appimage = {
      url = "github:ralismark/nix-appimage";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, nix-appimage }:
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

      # PANIM without the Telegram app values. A pure build cannot read the values, and the source never has them.
      panimFor = pkgs: pkgs.callPackage ./nix/package.nix {
        rustToolchain = pkgs.rust-bin.stable.${rustVersion}.minimal;
        tdlib-panim = tdlibFor pkgs;
      };

      # PANIM with the Telegram app values of PANIM, for a release (`README.md`). Only an impure build can read the
      # environment: `nix build --impure .#panim-release`. The values then are in the Nix store and in the binary.
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

      # The AppRun of nix-appimage, with the patch `nix/appimage-apprun-store.patch`. Without the patch, the AppImage
      # does not start on a system without `/nix/store`. The AppRun runs before the store of the image is visible, so
      # it must be static, as the original.
      apprunFor = pkgs:
        let
          src = pkgs.applyPatches {
            name = "appimage-apprun-source";
            src = "${nix-appimage}/appruns/userns-chroot";
            patches = [ ./nix/appimage-apprun-store.patch ];
          };
        in
        pkgs.pkgsStatic.runCommandCC "AppRun-panim" { } ''
          mkdir $out
          mkdir $out/mountroot
          $CC ${src}/main.c -o $out/AppRun
        '';

      # The AppImage of a package. A small launcher starts PANIM. On a system that is not NixOS, it tells libglvnd
      # to use the Mesa drivers in the AppImage, because the drivers of the system do not load with the libraries of
      # Nix. The desktop file and the icon come from the package.
      appimageFor = system: pkgs: panim:
        let
          mesa = pkgs.mesa;
          launcher = pkgs.runCommand "panim-launcher-${panim.version}" { } ''
            mkdir -p $out/bin
            ln -s ${panim}/share $out/share
            cat > $out/bin/panim <<'EOF'
            #!${pkgs.runtimeShell}
            if [ ! -e /run/opengl-driver ]; then
              export __EGL_VENDOR_LIBRARY_FILENAMES="''${__EGL_VENDOR_LIBRARY_FILENAMES:-${mesa}/share/glvnd/egl_vendor.d/50_mesa.json}"
              export LIBGL_DRIVERS_PATH="''${LIBGL_DRIVERS_PATH:-${mesa}/lib/dri}"
              export GBM_BACKENDS_PATH="''${GBM_BACKENDS_PATH:-${mesa}/lib/gbm}"
              # The fontconfig of Nix knows only one font. The configuration of the system names its font folders,
              # for example the Chinese fonts.
              if [ -z "''${FONTCONFIG_FILE:-}" ] && [ -f /etc/fonts/fonts.conf ]; then
                export FONTCONFIG_FILE=/etc/fonts/fonts.conf
              fi
            fi
            exec ${panim}/bin/panim "$@"
            EOF
            chmod 755 $out/bin/panim
          '';
        in
        nix-appimage.lib.${system}.mkAppImage {
          program = "${launcher}/bin/panim";
          apprun = apprunFor pkgs;
          name = "PANIM-${panim.version}-${pkgs.stdenv.hostPlatform.parsed.cpu.name}.AppImage";
          squashfsArgs = [ "-comp" "zstd" "-Xcompression-level" "19" ];
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
        # The AppImage without the values (for a test), and with the values (for a release, `--impure`).
        appimage = appimageFor system pkgs panim;
        appimage-release = appimageFor system pkgs panim-release;
        # The patched AppRun of the AppImage alone, for a test.
        appimage-apprun = apprunFor pkgs;
      });

      apps = forAll (system: pkgs: {
        # `nix run` starts PANIM.
        default = {
          type = "app";
          program = lib.getExe self.packages.${system}.panim;
          meta.description = "Start PANIM";
        };
        # `nix run .#release -- <arguments>` writes `SHA256SUMS` for the release files and signs it (`README.md`).
        release = {
          type = "app";
          program = lib.getExe (pkgs.writeShellApplication {
            name = "panim-release";
            runtimeInputs = with pkgs; [ coreutils git gnutar gzip minisign ];
            text = builtins.readFile ./nix/release.sh;
          });
          meta.description = "Write and sign the checksums of a release of PANIM";
        };
      });

      # The libraries that the app and its web views need on Linux, and Node.js for the user interface.
      # rustup supplies the Rust toolchain (rust-toolchain.toml).
      devShells = forAll (system: pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.pkg-config pkgs.nodejs_24 pkgs.pnpm pkgs.minisign ];
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
