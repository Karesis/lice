use std::path::{Path, PathBuf};
use std::{fs, io, process};

// ============================================================================
// 1. Configuration
// ============================================================================
const USAGE_INFO: &'static str = 
r#"lice - Automate source code license headers
    
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
    lice -f HEADER.txt -e vendor -e build src include
"#;

struct Config {
    license_file: Option<String>,
    excludes: Vec<String>,
    targets: Vec<PathBuf>,
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

// ============================================================================
// 2. Core Engine
// ============================================================================

struct LiceEngine {
    config: Config,
    golden_header: String,
}

impl LiceEngine {
    /// read license file and preprocess header
    fn new(config: Config) -> Result<Self, io::Error> {
        let path = config.license_file.as_ref().unwrap(); // validate ensured 
        let raw = fs::read_to_string(path)?;
        let header = Self::make_golden_header(&raw);
        
        Ok(Self {
            config,
            golden_header: header,
        })
    }

    /// entry
    fn run(&self) -> io::Result<()> {
        self.process_targets(&self.config.targets)
    }

    /// Iterative DFS
    fn process_targets(&self, initial_targets: &[PathBuf]) -> io::Result<()> {
        let mut stack = initial_targets.to_vec();

        while let Some(path) = stack.pop() {
            if self.is_excluded(&path) {
                println!("[EXCLUDE] {:?}", path);
                continue;
            }

            if path.is_dir() {
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(e) => stack.push(e.path()),
                                Err(e) => eprintln!("Error reading entry: {}", e),
                            }
                        }
                    }
                    Err(e) => eprintln!("Failed to read dir {:?}: {}", path, e),
                }
            } else if let Some(ext) = path.extension() {
                match ext.to_str().unwrap_or("") {
                    "c" | "h" => {
                        if let Err(e) = self.apply_license(&path) {
                            eprintln!("Error processing file {:?}: {}", path, e);
                        }
                    }
                    _ => continue,
                }
            }
        }
        Ok(())
    }

    /// core business
    fn apply_license(&self, path: &Path) -> io::Result<()> {
        let content = fs::read_to_string(path)?;

        // 直接使用 self.golden_header
        if content.starts_with(&self.golden_header) {
            println!(" License OK: {:?}", path);
            return Ok(());
        }

        let new_content = if content.trim_start().starts_with("/*") {
            println!(" Updating license: {:?}", path);
            if let Some(end_idx) = content.find("*/") {
                let body_start = &content[end_idx + 2..];
                format!("{}{}", self.golden_header, body_start.trim_start())
            } else {
                eprintln!(" Skipping {:?} (malformed block comment)", path);
                return Ok(());
            }
        } else {
            println!(" Adding license: {:?}", path);
            format!("{}{}", self.golden_header, content)
        };

        fs::write(path, new_content)?;
        Ok(())
    }

    /// Helper: if a path is excluded
    fn is_excluded(&self, path: &Path) -> bool {
        for component in path.components() {
            match component.as_os_str().to_str() {
                Some(s) => {
                    for pattern in &self.config.excludes {
                        if s == pattern { return true; }
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

    /// Helper: get header to attach
    fn make_golden_header(raw: &str) -> String {
        let mut out = String::from("/*\n");
        for line in raw.lines() {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                out.push_str(" *\n");
            } else {
                out.push_str(&format!(" * {}\n", trimmed));
            }
        }
        out.push_str(" */\n\n");
        out
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