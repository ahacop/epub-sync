# Cut a release: bump the version, tag, push, and update the Homebrew tap.
#
#     just release 0.1.2
#
# The tap checkout is expected next to this repo. Override with
#     just TAP=~/src/homebrew-tap release 0.1.2

TAP := "../homebrew-tap"

release version:
    #!/usr/bin/env bash
    set -euo pipefail
    version="{{version}}"
    tap="{{TAP}}"

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

    # 3. Hash the tag tarball. GitHub can take a moment to serve a new tag.
    url="https://github.com/ahacop/epub-sync/archive/refs/tags/v$version.tar.gz"
    sha=""
    for _ in $(seq 1 10); do
        sha=$(curl -sfL "$url" | sha256sum | cut -d' ' -f1) && [ ${#sha} -eq 64 ] && break
        sleep 3
    done
    [ ${#sha} -eq 64 ] || { echo "could not fetch $url"; exit 1; }

    # 4. Point the formula at the new tarball and push the tap.
    git -C "$tap" pull -q --ff-only
    sed -i "s|tags/v.*\.tar\.gz|tags/v$version.tar.gz|; s|sha256 \"[0-9a-f]*\"|sha256 \"$sha\"|" "$tap/Formula/epubsync.rb"
    git -C "$tap" add Formula/epubsync.rb
    git -C "$tap" commit -q -m "epubsync $version"
    git -C "$tap" push -q
    echo "tap updated to $version"
    echo "CI: https://github.com/ahacop/epub-sync/actions"
