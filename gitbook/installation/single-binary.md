# Single binary

You can also run Proksi as a standalone binary using rust's `cargo` or downloading it directly from [https://github.com/luizfonseca/proksi/releases](https://github.com/luizfonseca/proksi/releases) for you system.

## Cargo

Proksi is a Rust-based proxy service and can be installed as a binary through the published version [on crates.io](https://crates.io/search?q=proksi).

To install (and compile) Proksi for your system, first ensure you have the latest Rust version:



### 1. Rust is not installed&#x20;

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

You can now  run Proksi as a user binary:

```bash
touch proksi.hcl
# add routing configuration to your proksi.hcl file

proksi -c ./
```



## Downloading the binary

Ensure you are download the right one from the [Releases page on Github](https://github.com/luizfonseca/proksi/releases) and once you download it, make sure it has the right permissions to execute, e.g.:

```bash
# Replace {VERSION} with the version you want
# Replace {PLATFORM} with the one for your system
curl -O -L https://github.com/luizfonseca/proksi/releases/download/{VERSION}/{PLATFORM}.tar.gz
tar -czvf {PLATFORM}.tar.gz

chmod +x ./proksi
```

Once that is done you can check if the binary is functional:

```
proksi --help
```
