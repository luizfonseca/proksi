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

```
/path/to/download/proksi --help
```



## Downloading the binary

Ensure you are download the right one from the [Releases page on Github](https://github.com/luizfonseca/proksi/releases) and once you download it, make sure it has the right permissions to execute, e.g.:

```bash
chmod +x /path/to/downloaded/proksi
```

Once that is done you can simply run the binary:

```
/path/to/download/proksi --help
```
