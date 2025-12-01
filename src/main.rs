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
use std::{fs, io, process};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

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
                    // 解析字符串为数字
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
    start: &'static str,   // 块注释开始，例如 "/*" 或 "" (如果是行注释)
    prefix: &'static str,  // 每一行的前缀，例如 " * " 或 "// "
    end: &'static str,     // 块注释结束，例如 " */" 或 ""
}

// 预定义几种常见风格
const STYLE_C_LIKE: LanguageProfile = LanguageProfile { start: "/*\n", prefix: " * ", end: " */\n\n" };
const STYLE_HASH: LanguageProfile   = LanguageProfile { start: "", prefix: "# ", end: "\n" }; // Python, Shell, Ruby
const STYLE_DOUBLE_SLASH: LanguageProfile = LanguageProfile { start: "", prefix: "// ", end: "\n" }; // Rust, Go, Java (line mode)
const STYLE_DASH: LanguageProfile   = LanguageProfile { start: "", prefix: "-- ", end: "\n" }; // Lua, Haskell, SQL

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
            thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        });

        // ============================
        // 模式 A: 单线程 (直接干活)
        // ============================
        if num_threads == 1 {
            println!("Running in single-threaded mode.");
            // 直接调用 traverse，传入一个闭包让它去 process_file
            self.traverse(|path| {
                self.process_file(&path);
            });
            return Ok(());
        }

        // ============================
        // 模式 B: 多线程 (生产者-消费者)
        // ============================
        let shared_engine = Arc::new(self); // 这里的 self 被移动进了 Arc
        let (tx, rx) = mpsc::channel::<PathBuf>();
        let shared_rx = Arc::new(Mutex::new(rx));

        println!("Starting {} worker threads...", num_threads);

        let mut handles = Vec::with_capacity(num_threads);
        for _ in 0..num_threads {
            let thread_rx = Arc::clone(&shared_rx);
            let thread_engine = Arc::clone(&shared_engine);

            handles.push(thread::spawn(move || {
                loop {
                    // 取任务
                    let path = match thread_rx.lock().unwrap().recv() {
                        Ok(p) => p,
                        Err(_) => break, // 通道关闭，退出
                    };
                    // 干活
                    thread_engine.process_file(&path);
                }
            }));
        }

        // 主线程：只负责遍历和分发
        // 注意：这里我们调用 shared_engine.traverse
        shared_engine.traverse(|path| {
            // 回调函数：把路径通过 channel 发送出去
            if let Err(e) = tx.send(path) {
                eprintln!("Failed to send task: {}", e);
            }
        });
        
        // 记得销毁发送端
        drop(tx); 

        for h in handles { h.join().unwrap(); }
        Ok(())
    }

    // 这是一个高阶函数，accepts a closure
    fn traverse<F>(&self, mut callback: F) 
    where 
        F: FnMut(PathBuf) // 这个闭包接受一个 PathBuf，不返回任何值
    {
        let mut stack = self.config.targets.to_vec();

        while let Some(path) = stack.pop() {
            if self.is_excluded(&path) { continue; }

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
                // *** 核心改变 ***
                // 以前是 tx.send(path) 或者 self.process_file(path)
                // 现在我们不知道要做什么，直接调用传入的回调函数
                callback(path);
            }
        }
    }

    /// Iterative DFS
    fn process_file(&self, path: &PathBuf) {
        // 先做扩展名检查
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e,
            None => return, // 没有扩展名，直接忽略
        };

        // 获取风格配置
        match get_language_style(ext) {
            Some(style) => {
                // 执行具体的 IO 操作
                if let Err(e) = self.apply_license(path, style) {
                    eprintln!("Error processing {:?}: {}", path, e);
                }
            },
            None => {
                eprintln!("[WARN] Ignoring unsupported file type: {:?}", path)
            }
        }
    }

    /// core business
    fn apply_license(&self, path: &Path, style: LanguageProfile) -> io::Result<()> {
        let content = fs::read_to_string(path)?;

        // 1. generate header
        let header = self.make_header_for_style(&self.raw_license_text, style);

        // 2. check if exists
        // 1. 计算我们要检查的“起始位置”
        // 如果有 Shebang，我们就跳过第一行，从第二行开始看
        let offset = if content.starts_with("#!") {
            // 找到第一行结束的位置（换行符的位置 + 1）
            content.find('\n').map(|i| i + 1).unwrap_or(0)
        } else {
            0
        };

        // 2. 取出“正文视口” (View)
        let body_to_check = &content[offset..];

        // 3. 检查：去掉开头的空白后，是否是以我们的 Header 开头？
        // trim_start() 很重要，防止 Header 前面有几个不必要的空行导致匹配失败
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
                    // 块注释没闭合，直接追加在最前
                    format!("{}{}", header, content)
                    // 要是没有闭合，不应该是删除然后再附加?还是报错？还是什么？
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
    fn replace_line_comment_header(&self, content: &str, header: &str, style: LanguageProfile) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut keep_start_idx = 0;
        let mut shebang_line = None;

        // 1. 检查 Shebang (针对 # 风格)
        if let Some(first_line) = lines.first() {
            if first_line.starts_with("#!") {
                shebang_line = Some(*first_line);
                keep_start_idx = 1; // 跳过第一行，从第二行开始检查 License
            }
        }

        // 2. 向下扫描，跳过所有被认为是“旧 Header”的行
        // 定义：连续的、以 prefix 开头的行
        while keep_start_idx < lines.len() {
            let line = lines[keep_start_idx];
            let trimmed = line.trim();

            if trimmed.starts_with(style.prefix.trim()) {
                // 这是一个注释行，认为是旧 Header 的一部分 -> 跳过
                keep_start_idx += 1;
            } else if trimmed.is_empty() {
                // 这是一个空行。
                // 策略：通常 License 和代码之间会有空行。
                // 如果我们剥离了 License，最好也把紧接着的一个空行剥离掉，
                // 因为 new_header 里通常自带了结尾的空行。
                keep_start_idx += 1;
                // 遇到空行后，通常意味着 Header 结束了，停止扫描
                // 避免误删下面的代码块注释
                break; 
            } else {
                // 遇到了代码（既不是注释前缀，也不是空行） -> 停止
                break;
            }
        }

        // 3. 组装新内容
        let body = lines[keep_start_idx..].join("\n"); // 重新拼接剩余部分
        
        let mut out = String::new();
        
        // 如果有 Shebang，先放回去
        if let Some(sb) = shebang_line {
            out.push_str(sb);
            out.push('\n');
        }
        
        out.push_str(header);
        out.push_str(&body);
        
        // 保持文件末尾有换行符是好习惯
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
            // 对于行注释风格（如 Python），通常 Header 结束后加个空行
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
