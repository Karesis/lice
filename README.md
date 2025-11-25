<p align="center">
  <img src="assets/logo.png" alt="Lice Logo" width="200">
</p>

<h1 align="center">lice</h1>

<p align="center">
  <strong>A lightweight, ultra-fast CLI tool to automate license headers.</strong>
</p>

<p align="center">
  </p>

---

`lice` recursively scans your project and applies (or updates) license headers to source files. It is built with [**fluf**](https://github.com/Karesis/fluf), showcasing the power of a modern, opinionated C toolkit.

## 🚀 Features

* **Automated**: Recursively walks directories and applies headers to `.c` and `.h` files.
* **Smart Updates**: Detects existing headers. If a file already has a license block, `lice` updates it instead of duplicating it.
* **Exclusions**: Supports ignoring specific files or directories (e.g., `vendor`, `build`) via `-e`.
* **Zero Dependency**: Statically linked, single-binary distribution. No Python/Node.js runtime required.

## 📦 Installation

### From Source

```bash
# 1. Clone the repository
git clone https://github.com/Karesis/lice.git
cd lice

# 2. Initialize dependencies (fluf)
make update

# 3. Build
make

# 4. Install (Optional, requires sudo)
sudo make install
````

## 📖 Usage

### Basic Usage

Create a file named `LICENSE_HEADER` containing your desired comment block:

```c
/*
 * Copyright 2025 MyName
 * Licensed under Apache 2.0
 */
```

Then run `lice` pointing to that file:

```bash
lice -f LICENSE_HEADER
```

### Advanced Usage

Apply to specific folders (`src`, `include`) while excluding `vendor` and `tests`:

```bash
lice -f LICENSE_HEADER -e vendor -e tests src include
```

### Options

| Option | Description |
| :--- | :--- |
| `-f, --file <path>` | **Required.** Path to the file containing the license header text. |
| `-e, --exclude <pattern>` | Exclude paths matching the pattern (substring match). Can be used multiple times. |
| `-h, --help` | Show help message. |

## 🛠️ Built With

`lice` is a first-party application demonstrating the capabilities of the **fluf** C23 toolkit:

  * **Memory Safety**: Uses RAII (`defer`) for automatic resource management.
  * **Error Handling**: Uses `Result` types for robust error propagation.
  * **Performance**: Zero-copy string parsing and bump-allocated storage.

## 📄 License

This project is licensed under the **Apache-2.0 License**. See the [LICENSE](./LICENSE) file for details.

