# A bare `just` lists the recipes.
[private]
default:
    @just --list

# Outside the dev shell the recipe runs itself inside `nix develop`, which
# has cargo and puts the window libraries on LD_LIBRARY_PATH.

# Run the library viewer
app:
    #!/usr/bin/env bash
    set -euo pipefail
    if ! command -v cargo >/dev/null; then
        exec nix develop --command just app
    fi
    cargo run -p epubsync-app

# The tap checkout is expected next to this repo. Override with
#     just TAP=~/src/homebrew-tap release 0.1.2

TAP := "../homebrew-tap"

# Cut a release: bump the version, tag, push, and update the Homebrew tap
release version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    tap="{{TAP}}"

    # cargo comes from the Nix dev shell. Outside it, run the recipe inside.
    if ! command -v cargo >/dev/null; then
        exec nix develop --command just TAP="$tap" release "$version"
    fi

    [ "$(git branch --show-current)" = main ] || { echo "not on main"; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "the tree is not clean"; exit 1; }
    [ -d "$tap/Formula" ] || { echo "no tap checkout at $tap"; exit 1; }
    git fetch -q origin
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main is not pushed"; exit 1; }

    # 1. Bump the version in the workspace, the flake, and the lock file.
    sed -i "s/^version = \".*\"$/version = \"$version\"/" Cargo.toml
    sed -i "s/version = \".*\";/version = \"$version\";/" flake.nix
    cargo build -q -p epubsync-cli
    git add Cargo.toml Cargo.lock flake.nix
    git commit -q -m "Release $version"

    # 2. Tag and push.
    git tag -a "v$version" -m "EpubSync $version"
    git push -q origin main "v$version"
    echo "pushed v$version"

    # 3. Wait for the release workflow to publish the macOS tarball.
    #    The build takes several minutes.
    echo "waiting for the release workflow: https://github.com/ahacop/epub-sync/actions"
    sha=""
    for _ in $(seq 1 90); do
        sha=$(gh release download "v$version" --repo ahacop/epub-sync --pattern SHA256SUMS --output - 2>/dev/null | cut -d' ' -f1) && [ ${#sha} -eq 64 ] && break
        sleep 20
    done
    [ ${#sha} -eq 64 ] || { echo "no SHA256SUMS on release v$version"; exit 1; }

    # 4. Point the formula at the new tarball and push the tap.
    formula="$tap/Formula/epubsync.rb"
    git -C "$tap" pull -q --ff-only
    sed -i "s|^  version \".*\"|  version \"$version\"|; s|sha256 \"[0-9a-f]*\"|sha256 \"$sha\"|" "$formula"
    git -C "$tap" add Formula/epubsync.rb
    git -C "$tap" commit -q -m "epubsync $version"
    git -C "$tap" push -q
    echo "tap updated to $version"
