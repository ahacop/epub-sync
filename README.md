# EpubSync

EpubSync manages a library of KEPUB files, edits their metadata, and syncs
them to a Kobo. Import converts every EPUB to KEPUB with kepubify, which is
compiled into the binary. The app does not read books and does not fetch
metadata from the internet.

## Install

With Nix:

```sh
nix run github:ahacop/epub-sync -- --help
```

With Homebrew:

```sh
brew install ahacop/tap/epubsync
```

Both build the binary from source with Rust and Go. The installed program is
one file and needs no other program on `PATH`.

## Commands

```sh
epubsync init ~/Books/epubsync      # create the library folder and point the config at it
epubsync import book.epub           # convert to KEPUB and add it; a folder imports every EPUB in it
epubsync list                       # every book: id, title, authors, series, progress per device
epubsync edit 3                     # open the metadata as TOML in $EDITOR
epubsync edit 3 --title "New Title" # set one field without the editor
epubsync remove 3                   # delete the file and its rows
epubsync sync                       # make the Kobo's EpubSync folder match the library
epubsync sync --dry-run             # print the plan and change nothing
epubsync eject                      # unmount the Kobo and end the USB session
epubsync words                      # words looked up on the Kobo, newest first
```

The library is one folder. It holds every book as `<id>.kepub.epub` and the
database `library.sqlite`. Copy the folder to back it up. The config file
holds the folder path and lives in the XDG config directory.

`edit` flags: `--title`, `--publisher`, `--description`, `--author "Name|Sort"`
(repeat for several authors), `--series`, and `--series-number`.

## Sync

`sync` finds the Kobo under `/run/media/$USER`, `/media`, `/media/$USER`, or
`/Volumes`, or takes `--device <path>`. It copies books the device lacks,
replaces books edited since the last send, and deletes device files whose
book left the library, after you confirm. Then it reads reading progress and
looked-up words back from the Kobo database.

A book deleted on the Kobo is sent again on the next sync. To take a book
off the device, remove it from the library.

Series and the author string show on the Kobo on the sync after the one that
sent the book, because the firmware creates the book's row when it imports
the file.

Every write to the Kobo database is gated on its `dbversion`. On a version
the app has not been run against, sync skips replacements and row updates
and says so. `--allow-newer-firmware` runs them anyway.

## Eject

`sync` ends by ejecting the Kobo, and `epubsync eject` does it on its own.
An eject is an unmount plus the SCSI eject that ends the USB session; after
a plain unmount the Kobo keeps showing "connected". Pull the cable only
after the eject, or the Kobo database can be left corrupt.

On Linux the eject goes through udisks2 over D-Bus, so udisks2 must be
running, and polkit decides whether your session may unmount and eject. A
logged-in local user may by default. On macOS it runs `diskutil eject`.

## Build

The dev shell has rustc, cargo, Go, and SQLite:

```sh
nix develop
cargo test
```

Go is a build-time dependency only. `crates/epubsync-core/build.rs` compiles
the shim in `kepub-shim/` with `go build -buildmode=c-archive` and links it
statically. The shim's dependencies are vendored, so no build needs the
network.

## Release

`just release 0.1.2` bumps the version, commits, tags, pushes, and points
the Homebrew formula at the new tag. It expects the tap checkout at
`../homebrew-tap`, or pass `TAP=<path>`.
