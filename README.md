# MyLongboxDB

An offline Linux desktop app for cataloging and searching a collection of
comic books, magazines, and collected editions. Everything, including
cover photos, lives in one local SQLite file, and the app never touches
the network.

**Status:** early development. Nothing is released yet.

## Building

MyLongboxDB is written in Rust with GTK4 and libadwaita and ships only as
a Flatpak.

### Flatpak

```sh
flatpak install --user flathub org.gnome.Sdk//49 \
    org.freedesktop.Sdk.Extension.rust-stable//25.08 org.flatpak.Builder
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
GUI or image code build with a standard toolchain:

```sh
cargo test -p mylongbox-core -p mylongbox-sandbox
```

The app links against GTK4, libadwaita, libheif, and lcms2 from the
GNOME 49 SDK rather than the host system. To build and test everything,
run Cargo inside the SDK with the pinned toolchain:

```sh
# Download crates on the host; the SDK shell below has no network.
cargo fetch

sdk() {
    flatpak run --socket=wayland --socket=fallback-x11 --device=dri \
        --filesystem="$PWD" --filesystem=~/.cargo --filesystem=~/.rustup \
        --env=PATH="$HOME/.cargo/bin:/usr/bin:/bin" \
        --env=CARGO_HOME="$HOME/.cargo" --env=RUSTUP_HOME="$HOME/.rustup" \
        --env=CARGO_TARGET_DIR="$PWD/target/sdk" --env=CARGO_NET_OFFLINE=true \
        --cwd="$PWD" --command=sh org.gnome.Sdk//49 -c "$*"
}

sdk cargo clippy --workspace --all-targets --all-features -- -D warnings
sdk cargo test --workspace
```

## Security model

The app is designed so that each claim can be checked without trusting
its authors:

- **No networking code.** `deny.toml` bans HTTP, socket, and TLS crates
  and any dependency from outside crates.io. Verify with
  `cargo deny check`.
- **No running other programs.** `std::process`, GLib's spawn
  functions, GIO subprocesses, and GTK's URI and file launchers are
  banned through `clippy.toml`. Verify with
  `sdk cargo clippy --workspace --all-targets -- -D warnings`.
- **Collection files are treated as untrusted.** SQLite runs in
  defensive mode with untrusted schema disabled, URI file names, ATTACH,
  and extension loading off, and size limits on every value; a file that
  belongs to another application is refused rather than modified, and so
  is a collection whose tables, indexes, or triggers differ in any way
  from what the app creates. All SQL uses bound parameters. Verify with `cargo test -p mylongbox-core`, whose
  tests check each of these settings.
- **No `unsafe` code** outside the small `mylongbox-sandbox` crate.
  Every other crate declares `#![forbid(unsafe_code)]`.

- **No network or file access from the sandbox.** The Flatpak manifest
  grants exactly `--socket=wayland`, `--socket=fallback-x11`,
  `--share=ipc`, and `--device=dri`: no network, and no access to your
  home folder or the host filesystem. Verify with
  `flatpak info --show-permissions io.github.gdfekaris.MyLongboxDB`.
  Collections live in the app's own folder,
  `~/.var/app/io.github.gdfekaris.MyLongboxDB/data/collections/`. Files
  elsewhere are reached only through the system file chooser: importing
  copies a collection in after checking it, and exporting writes a
  copy out. GTK's recently used files list is turned off, so the app
  leaves no record of which files you picked.

The runtime self-restrictions will be documented here, with verification
steps, as they land.

## Reporting vulnerabilities

Please report security issues privately through GitHub's
"Report a vulnerability" form on the repository's Security tab rather
than in a public issue.

## License

MIT. See [LICENSE](LICENSE).
