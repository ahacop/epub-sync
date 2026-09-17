# Changelog

## 0.1.8 (2026-09-17)

A sync from macOS no longer leaves a `._` file next to each book it sends.
The copy to the Kobo now writes only the book's bytes, so macOS has no
extended attributes to store on the FAT volume. Sync also deletes the `._`
files that earlier syncs left in the `EpubSync` folder.

## 0.1.7 (2026-09-14)

A library made by 0.1.6 does not open with this release. The schema is now
one migration, and a database from 0.1.6 has the tables but no version
stamp, so the migration fails on the first table. Run `epubsync init` on a
new folder and import the books again.

The viewer shows each book's word count and Flesch reading ease. The table
has a Words column and an Ease column between Series and Progress, both
sortable, and the sidebar shows a Length line and an Ease line with the
Flesch band name. Import reads both numbers from a file that carries them,
as Standard Ebooks files do, and measures them from the text of a file that
does not. The measured numbers are written into the library file as the
same `schema:wordCount` and `schema:educationalLevel` elements. A book in a
language the scorer has no coefficients for gets a word count and no
reading ease.

The command and the viewer now open a book whose OPF puts a prefix such as
`ns0:` on a creator's attributes without declaring it. A write of such a
file declares the prefix, so the file becomes well-formed XML. A chapter
file whose name holds a space or another percent-escaped character is found
in the zip, so the spine walk and the cover lookup no longer stop at it. A
chapter that starts with an XML declaration parses.

A build from the working tree prints the git description as its version,
such as `0.1.6-16-g7a09b62-dirty`, so a dev build is told apart from the
release.

`just cli <args>` runs the command from the working tree, and a bare `just`
lists the recipes.

Full changelog: https://github.com/ahacop/epub-sync/compare/v0.1.6...v0.1.7

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
