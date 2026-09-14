{
  description = "EpubSync: a KEPUB library manager and Kobo sync tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # The libraries an Iced window loads at run time on Linux. The
      # binary opens them with dlopen, so they are not linked at build
      # time and must be on its rpath or on LD_LIBRARY_PATH.
      windowLibs = pkgs: with pkgs; [ libxkbcommon vulkan-loader wayland libX11 libXcursor libXi libxcb ];
      windowLibPath = pkgs: pkgs.lib.makeLibraryPath (with pkgs; [ wayland vulkan-loader libxkbcommon ]);
    in
    {
      packages = forAll (pkgs:
        let
          fs = pkgs.lib.fileset;
          # One workspace crate as a package. `extra` is merged into the
          # buildRustPackage attributes.
          crate = name: extra: pkgs.rustPlatform.buildRustPackage ({
            version = "0.1.5";
            src = fs.toSource {
              root = ./.;
              fileset = fs.unions [ ./Cargo.toml ./Cargo.lock ./crates ./kepub-shim ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            # build.rs compiles the kepubify shim with Go. The vendor folder
            # makes that build offline, so no Go vendor hash is needed.
            nativeBuildInputs = [ pkgs.go ];
            preBuild = ''
              export GOCACHE="$TMPDIR/go-cache"
              export GOPATH="$TMPDIR/go"
              export GOFLAGS=-mod=vendor
              export GOPROXY=off
              export CGO_ENABLED=1
            '';
            cargoBuildFlags = [ "-p" name ];
          } // extra);
        in
        {
          default = crate "epubsync-cli" {
            pname = "epubsync";
            meta = {
              description = "Manage a KEPUB library and sync it to a Kobo";
              homepage = "https://github.com/ahacop/epub-sync";
              license = pkgs.lib.licenses.gpl3Plus;
              mainProgram = "epubsync";
            };
          };

          app = crate "epubsync-app" ({
            pname = "epubsync-app";
            meta = {
              description = "View an EpubSync library in a window";
              homepage = "https://github.com/ahacop/epub-sync";
              license = pkgs.lib.licenses.gpl3Plus;
              mainProgram = "epubsync-app";
            };
          } // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            buildInputs = windowLibs pkgs;
            postFixup = ''
              patchelf --add-rpath ${windowLibPath pkgs} $out/bin/epubsync-app
            '';
          });
        });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ rustc cargo rust-analyzer clippy rustfmt go sqlite just pkg-config ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux (windowLibs pkgs);
          shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            export LD_LIBRARY_PATH="${windowLibPath pkgs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
          '';
        };
      });
    };
}
