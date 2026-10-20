# MyLongboxDB

An offline Linux desktop app for cataloging and searching a collection of
comic books, magazines, and collected editions. Everything, including
cover photos, lives in one local SQLite file, and the app never touches
the network.

**Status:** 0.1.4 is the latest release. It is not on Flathub yet;
install it from the Releases page as described below.

## Installing

Each release on the repository's Releases page has a Flatpak bundle and
its SHA-256 checksum. Check the download, then install it:

```sh
sha256sum -c MyLongboxDB-0.1.4.flatpak.sha256
flatpak install --user MyLongboxDB-0.1.4.flatpak
```

From 0.1.5 on, GitHub also keeps a signed statement, made by the
repository's CI, that each bundle was built from the tagged source.
With the GitHub CLI installed, check it with:

```sh
gh attestation verify MyLongboxDB-0.1.5.flatpak --repo gdfekaris/MyLongboxDB
```

A newer release installs over an older one the same way. A collection
opened by a newer release cannot be opened by an older one, so keep a
copy (Export a Copy) before upgrading if you may want to go back.

## Building

MyLongboxDB is written in Rust with GTK4 and libadwaita and ships only as
a Flatpak.

### Flatpak

```sh
flatpak install --user flathub org.gnome.Sdk//51 \
    org.freedesktop.Sdk.Extension.rust-stable//26.08 org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install --force-clean \
    _build build-aux/io.github.gdfekaris.MyLongboxDB.yml
flatpak run io.github.gdfekaris.MyLongboxDB
```

The build is fully offline: every crate is listed with its checksum in
`build-aux/cargo-sources.json`, which is generated from `Cargo.lock` by
[flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo)
and must be regenerated whenever the lockfile changes.

### Development

The Rust version is pinned in `rust-toolchain.toml`. The crates without
GTK build with a standard toolchain:

```sh
cargo test -p mylongbox-core -p mylongbox-media -p mylongbox-sandbox
```

The app links against GTK4, libadwaita, and libheif from the GNOME 51
SDK rather than the host system. To build and test everything, run Cargo
inside the SDK with the pinned toolchain:

```sh
# Download crates on the host; the SDK shell below has no network.
cargo fetch

sdk() {
    flatpak run --socket=wayland --socket=fallback-x11 --device=dri \
        --filesystem="$PWD" --filesystem=~/.cargo --filesystem=~/.rustup \
        --env=PATH="$HOME/.cargo/bin:/usr/bin:/bin" \
        --env=CARGO_HOME="$HOME/.cargo" --env=RUSTUP_HOME="$HOME/.rustup" \
        --env=CARGO_TARGET_DIR="$PWD/target/sdk" --env=CARGO_NET_OFFLINE=true \
        --cwd="$PWD" --command=sh org.gnome.Sdk//51 -c "$*"
}

sdk cargo clippy --workspace --all-targets --all-features -- -D warnings
sdk cargo test --workspace
```

### Translations

Text shown in the app is marked in the Rust code with the functions in
`crates/mylongbox-app/src/i18n.rs` and in the Blueprint files with
`_()`. The template for translators, `po/mylongboxdb.pot`, is made from
the files listed in `po/POTFILES`:

```sh
sdk "grep '\.rs\$' po/POTFILES | xgettext --files-from=- --language=Rust \
    --from-code=UTF-8 --add-comments=Translators --keyword=gettext \
    --keyword=ngettext:1,2 --keyword=pgettext:1c,2 --keyword=gettext_f \
    --keyword=ngettext_f:1,2 --keyword=gettext_noop --package-name=mylongboxdb \
    --msgid-bugs-address=https://github.com/gdfekaris/MyLongboxDB/issues \
    --sort-by-file -o po/mylongboxdb.pot &&
  grep '\.blp\$' po/POTFILES | xgettext --files-from=- --language=C \
    --from-code=UTF-8 --add-comments=Translators --keyword=_ --keyword=C_:1c,2 \
    --sort-by-file -j -o po/mylongboxdb.pot"
```

A translation is a `po/<language>.po` file made from the template, with
its language code added to `po/LINGUAS`; the Flatpak build checks and
compiles each one.

## Security model

The app is designed so that each claim can be checked without trusting
its authors:

- **No networking code.** `deny.toml` bans HTTP, socket, and TLS crates
  and any dependency from outside crates.io. Verify with
  `cargo deny check`.
- **Every dependency is reviewed or deliberately accepted.** cargo-vet
  records, in `build-aux/supply-chain/`, which crates were audited by
  organisations whose audits are imported (Mozilla, Google, the Bytecode
  Alliance, and others) and which were accepted at a given version; a new
  crate or a new version fails the check until someone looks at it.
  Verify with `cargo vet --locked`.
- **No running other programs.** `std::process`, GLib's spawn
  functions, GIO subprocesses, and GTK's URI and file launchers are
  banned through `clippy.toml`. Verify with
  `sdk cargo clippy --workspace --all-targets -- -D warnings`.
- **Collection files are treated as untrusted.** SQLite runs in
  defensive mode with untrusted schema disabled, URI file names, ATTACH,
  and extension loading off, and size limits on every value; a file that
  belongs to another application is refused rather than modified, and so
  is a collection whose tables, indexes, or triggers differ in any way
  from what the app creates. An imported collection's search index is
  rebuilt from its own tables rather than trusted. All SQL uses bound
  parameters. Verify with `cargo test -p mylongbox-core`, whose
  tests check each of these settings.
- **No `unsafe` code** outside the small `mylongbox-sandbox` crate.
  Every other crate declares `#![forbid(unsafe_code)]`.
- **Photos are decoded within limits and saved as pixels only.** A
  photo over 50 MB, 100 megapixels, or 40,000 pixels on a side is
  refused before its pixels are decoded. HEIC photos are read by a copy
  of libheif built into the Flatpak with only the libde265 HEVC decoder
  and with plugin loading off, so no other code is loaded. Picked
  photos are decoded on a thread that first locks itself down: it
  cannot open files or make, use, or pass on sockets, and can write
  only to standard output and error. That stops the simple ways a
  photo crafted to take over a decoder would reach your files, but not
  a determined one: the thread shares the app's memory, so code that
  takes it over can reach the rest of the app, and a few ways of using
  descriptors the app already holds are not blocked. What holds then
  are the limits on the whole app below. Each photo is
  re-encoded from its pixels, so no EXIF, GPS, XMP, camera, or comment
  data from the original file is kept, and the original is never
  changed. Photos stored in a collection are shown through the app's
  own decoder, never through GTK's image loaders, and only if they are
  JPEGs no larger than the app saves (2,400 pixels a side, 320 for
  thumbnails), so a collection file from elsewhere cannot feed hostile
  images to any other decoder. Verify with
  `cargo test -p mylongbox-sandbox`,
  `sdk cargo test -p mylongbox-media --features heic -- --include-ignored`,
  and the `libheif` module in the Flatpak manifest.
- **Imported spreadsheets and archives are treated as untrusted.** A
  CSV file over 100 MB is refused without being read whole. A full
  export is never unpacked to disk: only the files its CSV names are
  read, each up to 50 MB, so names inside the archive cannot reach
  outside it, and its photos go through the same limited decoder and
  re-encoding as any other. Every row is added through the same checks
  as the add form, in one transaction that is undone if the import is
  cancelled. Verify with `cargo test -p mylongbox-core import`.

- **No network or file access from the sandbox.** The Flatpak manifest
  grants exactly `--socket=wayland`, `--socket=fallback-x11`,
  `--share=ipc`, and `--device=dri`: no network, and no access to your
  home folder or the host filesystem. Verify with
  `flatpak info --show-permissions io.github.gdfekaris.MyLongboxDB`.
  Collections live in the app's own folder,
  `~/.var/app/io.github.gdfekaris.MyLongboxDB/data/collections/`. Files
  elsewhere are reached only through the system file chooser: importing
  copies a collection in after checking it, and Export a Copy, the way
  to back up, checks the copy in full before writing it out.
  Safety copies, made before imports, upgrades, changes to several
  books at once, and deleting from Trash, stay in
  `collections/safety/` beside your collections, one per collection; a
  switch in Preferences turns them off. GTK's recently used files list
  is turned off, so the app leaves no record of which files you picked.

- **The app locks itself out of the internet at startup.** Before
  anything else runs, a seccomp filter is applied to every thread: any
  socket other than a local Unix socket (which the display server,
  D-Bus, and the file chooser use) is refused, and so is setting up
  io_uring, which could open sockets another way. The kernel keeps the
  filter until the app exits, so nothing can lift it. This holds even
  outside the Flatpak, which already gives the app no network, with
  one gap there: on a kernel that runs x32 programs, the filter does
  not check x32 system calls, which the Flatpak's own filter refuses.
  Verify
  with `cargo test -p mylongbox-sandbox`, whose tests apply the filter
  and show internet sockets failing while Unix sockets work, and, with
  the app open, `grep Seccomp_filters /proc/$(pgrep -x mylongboxdb)/status`,
  which shows one filter more than the Flatpak sandbox's own (2 rather
  than 1).

- **The app can change files only in its own folders.** At startup,
  before any other thread runs, Landlock limits every way of changing
  files (writing, creating, deleting, renaming, emptying) to the app's
  data, cache, and settings folders, the files you pick in the file
  chooser (which the Flatpak hands over through its document portal;
  a build run outside the Flatpak cannot save exports elsewhere), and
  the graphics devices under `/dev`, and refuses TCP
  connections as a second line behind the socket filter. Reading is left
  to the Flatpak, which shows the app only its own folders and the
  system files GTK needs. Verify with `cargo test -p mylongbox-sandbox`,
  whose tests apply the limits and show changes failing outside the
  allowed folder while reading still works. On a kernel without
  Landlock the app prints a note and runs with the Flatpak's limits
  alone.

### What the sandbox cannot stop

These limits come from the desktop rather than the app, so they hold
for every Flatpak app. Each one matters only if hostile code is
already running inside the app, for instance through a flaw in a photo
decoder.

- **Files you picked stay reachable.** Each file picked in the file
  chooser stays shared with the app, for reading and writing, until
  you take it back; the app cannot give it up itself. List what the app
  can reach with
  `flatpak permission-show io.github.gdfekaris.MyLongboxDB` (the
  `documents` lines), and take it all back with
  `flatpak permission-reset io.github.gdfekaris.MyLongboxDB`. Ticking
  "Open files read-only" in the file chooser shares a file for reading
  only. Keep backups somewhere the app has never been shown.
- **Links open in your browser.** Any Flatpak app can ask the desktop
  to open a web or mail link, and when a default browser or mail app is
  set it opens without asking, so a hijacked app could send data out
  inside a link. To be asked first, set the app to always ask for each
  kind of link:

  ```sh
  for kind in http https mailto; do
      flatpak permission-set desktop-used-apps x-scheme-handler/$kind \
          io.github.gdfekaris.MyLongboxDB "" 0 2147483647
  done
  ```

  Undo with `flatpak permission-remove desktop-used-apps
  x-scheme-handler/<kind> io.github.gdfekaris.MyLongboxDB`.
- **X11 sessions do not separate windows.** On an X11 desktop session
  the app can read other windows and send them keystrokes, as every X11
  program can. A Wayland session does not allow this, and there the app
  uses Wayland only. Check which you run with `echo $XDG_SESSION_TYPE`.

## Reporting vulnerabilities

Please report security issues privately through GitHub's
"Report a vulnerability" form on the repository's Security tab rather
than in a public issue.

## License

MIT. See [LICENSE](LICENSE).

The Flatpak also includes libheif and libde265, which are licensed under
the GNU LGPL and linked dynamically. Their license files are installed
in `/app/share/licenses/`.
