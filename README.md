<p align="center">
  <img src="assets/logo.png" alt="Lice Logo" width="200">
</p>

<h1 align="center">lice</h1>

<p align="center">
  <strong>A lightweight, blazing fast CLI tool to automate license headers.</strong>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange.svg" alt="Language">
  <img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License">
</p>

---

`lice` recursively scans your project and applies (or updates) license headers to source files. 

Originally written in C, **lice** has been rewritten in **Rust** to provide memory safety, safe concurrency, and multi-language support out of the box.

## 🚀 Features

* **Multi-Language Support**: Automatically detects and applies comment styles for:
    * C/C++ (`.c`, `.h`, `.cpp`, `.hpp`)
    * Rust (`.rs`)
    * Python/Shell (`.py`, `.sh`, `.rb`, `.yaml`, `.toml`)
    * Haskell/Lua/SQL (`.hs`, `.lua`, `.sql`)
* **Parallel Processing**: Built-in multi-threading (Producer-Consumer model) to handle large codebases instantly.
* **Smart Updates**: Detects existing headers (including Shebangs `#!`). If a file already has a license block, `lice` updates it gracefully.
* **Zero Dependency**: Built using **only the Rust Standard Library (`std`)**. No external crates, no bloat.
* **Exclusions**: Supports ignoring specific files or directories (e.g., `vendor`, `target`) via `-e`.

## 📦 Installation

### From Source

Ensure you have Rust installed (via `rustup`).

```bash
# 1. Clone the repository
git clone https://github.com/Karesis/lice.git
cd lice

# 2. Build via Cargo
cargo build --release

# 3. Install (Copy binary to path)
cp target/release/lice ~/.local/bin/
# OR using cargo install
cargo install --path .
```

## 📖 Usage

### Basic Usage

Create a file named `HEADER` containing your desired license text:

```bash
lice -f HEADER
```

### Advanced Usage

Apply to specific folders, exclude build artifacts, and use **8 threads** for speed:

```bash
lice -f HEADER -e vendor -e target -j 8 src include
```

### Options

| Option | Description |
| :--- | :--- |
| `-f, --file <path>` | **Required.** Path to the file containing the license header text. |
| `-e, --exclude <pattern>` | Exclude paths matching the pattern. Can be used multiple times. |
| `-j, --jobs <n>` | Number of worker threads. Default: Auto-detect CPU cores. |
| `-h, --help` | Show help message. |

## 🛠️ Built With

  * **Rust**: For performance and safety.
  * **std::thread & std::sync**: Hand-crafted thread pool using pure standard library primitives.
  * **std::fs**: Efficient recursive directory traversal.

## 📄 License

This project is licensed under the **Apache-2.0 License**. See the [LICENSE](./LICENSE) file for details.
