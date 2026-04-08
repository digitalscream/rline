# rline

A native Linux GUI text editor with AI-assisted coding features (coming soon), built with Rust and GTK4.

## System Dependencies

Install the required development libraries before building:

### Ubuntu / Debian

```bash
sudo apt-get install -y \
    libgtk-4-dev \
    libgtksourceview-5-dev \
    libvte-2.91-gtk4-dev \
    libgraphene-1.0-dev
```

### Fedora

```bash
sudo dnf install -y \
    gtk4-devel \
    gtksourceview5-devel \
    vte291-gtk4-devel \
    graphene-devel
```

### Arch Linux

```bash
sudo pacman -S gtk4 gtksourceview5 vte4 graphene
```

## Building

Requires Rust 1.85 or later.

```bash
cargo build              # Debug build
cargo build --release    # Release build
```

## Binary Pre-Requisites

```sudo apt install libvte-2.91-gtk4-0 libgtksourceview-5-0 libgraphene-1.0-0
```

## Running

```bash
cargo run
```

## Development

```bash
cargo fmt --check                              # Check formatting
cargo clippy -- -D warnings                    # Lint
cargo test                                     # Run tests
cargo fmt && cargo clippy -- -D warnings && cargo test  # Pre-commit checklist
```
