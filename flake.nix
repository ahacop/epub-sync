{
  description = "EpubSync: a KEPUB library manager and Kobo sync tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs:
        let
          fs = pkgs.lib.fileset;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "epubsync";
            version = "0.1.1";
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
            cargoBuildFlags = [ "-p" "epubsync-cli" ];
            meta = {
              description = "Manage a KEPUB library and sync it to a Kobo";
              homepage = "https://github.com/ahacop/epub-sync";
              license = pkgs.lib.licenses.mit;
              mainProgram = "epubsync";
            };
          };
        });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ rustc cargo rust-analyzer clippy rustfmt go sqlite ];
        };
      });
    };
}
