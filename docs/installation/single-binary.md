# Single binary

You can also run Proksi as a standalone binary using rust's `cargo` or downloading it directly from [https://github.com/luizfonseca/proksi/releases](https://github.com/luizfonseca/proksi/releases) for your system.

## Cargo

Proksi is a Rust-based proxy service and can be installed as a binary through the published version [on crates.io](https://crates.io/search?q=proksi).

To install (and compile) Proksi for your system, first ensure you have the latest Rust version:



### 1. Rust is not installed

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Rust is already installed

```bash
rustup update
```

### 3. Install the binary

```bash
cargo install proksi
```

You can now run Proksi as a user binary:

```bash
touch proksi.hcl
# add routing configuration to your proksi.hcl file

proksi -c ./
```



## Downloading the binary

Download the archive for your system from the [Releases page on GitHub](https://github.com/luizfonseca/proksi/releases): `proksi-linux-musl-x86_64`, `proksi-linux-musl-aarch64` (static, any Linux), `proksi-linux-gnu-*`, `proksi-macos-aarch64` or `proksi-macos-x86_64`. For example:

```bash
# Replace the version and platform with the ones you want
curl -L -O https://github.com/luizfonseca/proksi/releases/download/proksi-v0.8.0/proksi-linux-musl-x86_64.tar.gz
tar -xzf proksi-linux-musl-x86_64.tar.gz
chmod +x ./proksi
```

Once that is done you can check if the binary is functional:

```
proksi --help
```
