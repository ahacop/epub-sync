# Changelog

## Unreleased

The command and the viewer now open a book whose OPF puts a prefix such as
`ns0:` on a creator's attributes without declaring it. A write of such a
file declares the prefix, so the file becomes well-formed XML.

## 0.1.6 (2026-09-14)

The Homebrew package now includes the viewer. `brew install ahacop/tap/epubsync`
installs two programs: the `epubsync` command and the `epubsync-app` viewer.
Before this release the package held only the command, and a Mac user had to
build the viewer from source with Rust and Go.

The viewer is a window that shows the library as a sortable table with a
filter. A click on a row opens the book's details in a sidebar. It is
read-only. Start it from a terminal with `epubsync-app`. It ships as a plain
binary, not an app bundle, so it does not appear in Launchpad or Spotlight.

The release tarball for Apple Silicon now contains both binaries. Each one
needs no other program on `PATH`.

Full changelog: https://github.com/ahacop/epub-sync/compare/v0.1.5...v0.1.6
