# MyLongboxDB

An offline Linux desktop app for cataloging and searching a collection of
comic books, magazines, and collected editions. Everything, including
cover photos, lives in one local SQLite file, and the app never touches
the network.

**Status:** early development. Nothing is released yet.

## Building

MyLongboxDB is written in Rust with GTK4 and libadwaita and ships only as
a Flatpak. The core crates build with a standard Rust toolchain; the
version is pinned in `rust-toolchain.toml`.

```sh
cargo build --workspace
cargo test --workspace
```

The GUI and image crates link against GTK4, libadwaita, libheif, and
lcms2 from the GNOME 49 SDK rather than the host system. To build and test
them, run Cargo inside the SDK with the pinned toolchain:

```sh
flatpak install --user flathub org.gnome.Sdk//49

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
- **No running other programs.** `std::process` is banned through
  `clippy.toml`. Verify with
  `cargo clippy --workspace --all-targets -- -D warnings`.
- **No `unsafe` code** outside the small `mylongbox-sandbox` crate.
  Every other crate declares `#![forbid(unsafe_code)]`.

The Flatpak sandbox permissions and runtime self-restrictions will be
documented here, with verification steps, as they land.

## Reporting vulnerabilities

Please report security issues privately through GitHub's
"Report a vulnerability" form on the repository's Security tab rather
than in a public issue.

## License

MIT. See [LICENSE](LICENSE).
