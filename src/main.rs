//    Copyright 2025 Karesis
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::{fs, io, process};

// ============================================================================
// 1. Configuration
// ============================================================================
const USAGE_INFO: &'static str = r#"lice - Automate source code license headers
    
USAGE:
  lice [OPTIONS] [PATHS...]

ARGUMENTS:
  [PATHS...]               Directories or files to process.
                           If omitted, the current directory is used (.).

OPTIONS:
  -f, --file <PATH>        Path to the license header file. (Required)
  -e, --exclude <PATTERN>  Exclude file/directory matching this pattern.
                           Can be specified multiple times.
  -h, --help               Show this help message and exit.

EXAMPLES:
  # Apply license to the current directory
  lice -f HEADER.txt .

  # Apply to 'src' and 'include', excluding 'vendor' and 'build'
  lice -f HEADER.txt -e vendor -e build src include"#;

struct Config {
    license_file: Option<String>,
    excludes: Vec<String>,
    targets: Vec<PathBuf>,
    jobs: Option<usize>,
}

impl Config {
    fn from_env() -> Result<Self, String> {
        let raw_args: Vec<String> = std::env::args().skip(1).collect();

        // check if args is empty
        if raw_args.is_empty() {
            eprintln!("{}", USAGE_INFO);
            process::exit(0);
        }

        let mut args = raw_args.into_iter();

        let mut config = Config {
            license_file: None,
            excludes: Vec::new(),
            targets: Vec::new(),
            jobs: None,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-f" | "--file" => {
                    let val = args.next().ok_or("-f/--file requires an argument")?;
                    config.license_file = Some(val);
                }
                "-e" | "--exclude" => {
                    let val = args.next().ok_or("-e/--exclude requires an argument")?;
                    config.excludes.push(val);
                }
                "-h" | "--help" => {
                    eprintln!("{}", USAGE_INFO);
                    process::exit(0);
                }
                "-j" | "--jobs" => {
                    let val = args.next().ok_or("-j requires an argument")?;
                    // parse str to usize
                    let num = val.parse::<usize>().map_err(|_| "Invalid number for -j")?;
                    config.jobs = Some(num);
                }
                s if s.starts_with('-') => return Err(format!("Unknown option: {}", s)),
                _ => config.targets.push(PathBuf::from(arg)),
            }
        }

        config.validate()
    }

    fn validate(self) -> Result<Self, String> {
        if self.license_file.is_none() {
            return Err("Missing required argument: -f/--file".to_string());
        }
        if self.targets.is_empty() {
            return Err("No target paths specified. Use '.' for current directory.".to_string());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy)]
struct LanguageProfile {
    /// comment start
    /// e.g., "/*" or "" (line comment)
    start: &'static str,

    /// comment prefix
    /// e.g., " * " or "// "
    prefix: &'static str,

    /// comment end
    /// e.g., " */" or ""
    end: &'static str,
}

/// comment styles
const STYLE_C_LIKE: LanguageProfile = LanguageProfile {
    start: "/*\n",
    prefix: " * ",
    end: " */\n\n",
};
const STYLE_HASH: LanguageProfile = LanguageProfile {
    start: "",
    prefix: "# ",
    end: "\n",
}; // Python, Shell, Ruby
const STYLE_DOUBLE_SLASH: LanguageProfile = LanguageProfile {
    start: "",
    prefix: "// ",
    end: "\n",
}; // Rust, Go, Java (line mode)
const STYLE_DASH: LanguageProfile = LanguageProfile {
    start: "",
    prefix: "-- ",
    end: "\n",
}; // Lua, Haskell, SQL

fn get_language_style(ext: &str) -> Option<LanguageProfile> {
    match ext {
        "c" | "h" | "cpp" | "hpp" | "css" => Some(STYLE_C_LIKE),
        "rs" | "go" | "java" | "js" | "ts" => Some(STYLE_DOUBLE_SLASH),
        "py" | "sh" | "rb" | "yaml" | "toml" => Some(STYLE_HASH),
        "lua" | "hs" | "sql" => Some(STYLE_DASH),
        _ => None, // not supported
    }
}

// ============================================================================
// 2. Core Engine
// ============================================================================

struct LiceEngine {
    config: Config,
    raw_license_text: String,
}

impl LiceEngine {
    /// read license file and preprocess header
    fn new(config: Config) -> Result<Self, io::Error> {
        let path = config.license_file.as_ref().unwrap(); // validate ensured 
        let raw = fs::read_to_string(path)?;

        Ok(Self {
            config,
            raw_license_text: raw,
        })
    }

    /// entry
    fn run(self) -> io::Result<()> {
        let num_threads = self.config.jobs.unwrap_or_else(|| {
            thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        });

        // ============================
        // Mode A: Single-thread
        // ============================
        if num_threads == 1 {
            println!("Running in single-threaded mode.");
            self.traverse(|path| {
                self.process_file(&path);
            });
            return Ok(());
        }

        // ============================
        // Mode B: Multi-thread
        // ============================
        let shared_engine = Arc::new(self);
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let shared_rx = Arc::new(Mutex::new(rx));

        println!("Starting {} worker threads...", num_threads);

        let mut handles = Vec::with_capacity(num_threads);
        for _ in 0..num_threads {
            let thread_rx = Arc::clone(&shared_rx);
            let thread_engine = Arc::clone(&shared_engine);

            handles.push(thread::spawn(move || {
                loop {
                    // get the job
                    let path = match thread_rx.lock().unwrap().recv() {
                        Ok(p) => p,
                        Err(_) => break, // exit
                    };
                    // process
                    thread_engine.process_file(&path);
                }
            }));
        }

        // main thread
        shared_engine.traverse(|path| {
            if let Err(e) = tx.send(path) {
                eprintln!("Failed to send task: {}", e);
            }
        });

        drop(tx);

        for h in handles {
            h.join().unwrap();
        }
        Ok(())
    }

    // Helper: accepts a closure
    fn traverse<F>(&self, mut callback: F)
    where
        F: FnMut(PathBuf), // this closure accepts a PathBuf and ret ()
    {
        let mut stack = self.config.targets.to_vec();

        while let Some(path) = stack.pop() {
            if self.is_excluded(&path) {
                continue;
            }

            if path.is_dir() {
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            stack.push(entry.path());
                        }
                    }
                    Err(e) => eprintln!("Failed to read dir {:?}: {}", path, e),
                }
            } else {
                callback(path);
            }
        }
    }

    /// Iterative DFS
    fn process_file(&self, path: &PathBuf) {
        // check ext
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e,
            None => return, // no ext, ignore
        };

        match get_language_style(ext) {
            Some(style) => {
                if let Err(e) = self.apply_license(path, style) {
                    eprintln!("Error processing {:?}: {}", path, e);
                }
            }
            None => {
                eprintln!("[WARN] Ignoring unsupported file type: {:?}", path)
            }
        }
    }

    /// core business
    fn apply_license(&self, path: &Path, style: LanguageProfile) -> io::Result<()> {
        let content = fs::read_to_string(path)?;

        // generate header
        let header = self.make_header_for_style(&self.raw_license_text, style);

        // handle with shebang
        let offset = if content.starts_with("#!") {
            content.find('\n').map(|i| i + 1).unwrap_or(0)
        } else {
            0
        };
        let body_to_check = &content[offset..];

        // check if exists
        if body_to_check.trim_start().starts_with(header.trim()) {
            println!(" License OK: {:?}", path);
            return Ok(());
        }

        let new_content = if !style.start.is_empty() {
            // block comments
            if content.trim_start().starts_with(style.start) {
                if let Some(end_idx) = content.find(style.end) {
                    let body = &content[end_idx + style.end.len()..];
                    format!("{}{}", header, body.trim_start())
                } else {
                    // malformed file
                    eprintln!(
                        "[WARN] Skipping {:?}: Unclosed block comment detected.",
                        path
                    );
                    return Ok(()); // continue
                }
            } else {
                format!("{}{}", header, content)
            }
        } else {
            // line comments
            self.replace_line_comment_header(&content, &header, style)
        };

        fs::write(path, new_content)?;
        Ok(())
    }

    /// handle line comment header replacement
    fn replace_line_comment_header(
        &self,
        content: &str,
        header: &str,
        style: LanguageProfile,
    ) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut keep_start_idx = 0;
        let mut shebang_line = None;

        // check shebang
        if let Some(first_line) = lines.first() {
            if first_line.starts_with("#!") {
                shebang_line = Some(*first_line);
                keep_start_idx = 1;
            }
        }

        // scan for "old"
        while keep_start_idx < lines.len() {
            let line = lines[keep_start_idx];
            let trimmed = line.trim();

            if trimmed.starts_with(style.prefix.trim()) {
                keep_start_idx += 1;
            } else if trimmed.is_empty() {
                keep_start_idx += 1;
                break;
            } else {
                // reach the code
                break;
            }
        }

        let body = lines[keep_start_idx..].join("\n");

        let mut out = String::new();

        if let Some(sb) = shebang_line {
            out.push_str(sb);
            out.push('\n');
        }

        out.push_str(header);
        out.push_str(&body);

        // add \n to eof
        if !out.ends_with('\n') {
            out.push('\n');
        }

        out
    }

    /// Helper: generate header from style and raw str
    fn make_header_for_style(&self, raw: &str, style: LanguageProfile) -> String {
        let mut out = String::new();
        if !style.start.is_empty() {
            out.push_str(style.start);
        }

        for line in raw.lines() {
            let trimmed = line.trim_end();
            out.push_str(style.prefix);
            out.push_str(trimmed);
            out.push('\n');
        }

        if !style.end.is_empty() {
            out.push_str(style.end);
        } else {
            // line comment just add \n
            out.push('\n');
        }
        out
    }

    /// Helper: if a path is excluded
    fn is_excluded(&self, path: &Path) -> bool {
        for component in path.components() {
            match component.as_os_str().to_str() {
                Some(s) => {
                    for pattern in &self.config.excludes {
                        if s == pattern {
                            return true;
                        }
                    }
                }
                None => {
                    eprintln!("[WARN] Skipping non-UTF8 path: {:?}", path);
                    return true;
                }
            }
        }
        false
    }
}

// ============================================================================
// 3. 程序入口 (Main)
// ============================================================================

fn main() {
    // 1. config
    let config = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        eprintln!("{}", USAGE_INFO);
        process::exit(1);
    });

    // 2. init engine
    let engine = LiceEngine::new(config).unwrap_or_else(|e| {
        eprintln!("Failed to initialize engine: {}", e);
        process::exit(1);
    });

    // 3. execute
    if let Err(e) = engine.run() {
        eprintln!("Fatal error: {}", e);
        process::exit(1);
    }
}
