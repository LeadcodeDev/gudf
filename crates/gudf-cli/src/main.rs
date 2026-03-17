use std::fs;
use std::io::{self, Read};
use std::path::Path;

use clap::{Parser, Subcommand};
use colored::Colorize;

use gudf::output::inline::InlineFormatter;
use gudf::output::json_patch::JsonPatchFormatter;
use gudf::output::unified::UnifiedFormatter;
use gudf::output::OutputFormatter;
use gudf::{
    CrossFormatKind, DiffEngine, FormatKind, GudfError, MergeStrategy, SemanticAnalyzer,
    SemanticOptions, SensitiveFieldAnnotator,
};
use gudf::formats::code::CodeFormat;

#[derive(Parser)]
#[command(
    name = "gudf",
    about = "Git Unified Diff Format — universal diff tool for text, JSON, TOML, YAML, and code"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// First file or directory (use '-' for stdin)
    file1: Option<String>,

    /// Second file or directory
    file2: Option<String>,

    /// Force a specific format (text, json, toml, yaml)
    #[arg(short, long)]
    format: Option<String>,

    /// Output format (unified, inline, json-patch)
    #[arg(short, long, default_value = "unified")]
    output: String,

    /// Language for code diff (rust, javascript, python)
    #[arg(short, long)]
    lang: Option<String>,

    /// Number of context lines for unified output
    #[arg(short = 'U', long, default_value = "3")]
    context: usize,

    /// Enable cross-format diff (auto when extensions differ)
    #[arg(long)]
    cross_format: bool,

    /// Enable semantic move/rename detection
    #[arg(long)]
    semantic: bool,

    /// Enable sensitive field annotations
    #[arg(long)]
    annotate: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Three-way merge of structured documents
    Merge {
        /// Base file
        base: String,
        /// Left file
        left: String,
        /// Right file
        right: String,
        /// Merge strategy (manual, ours, theirs)
        #[arg(short, long, default_value = "manual")]
        strategy: String,
        /// Force format (json, toml, yaml)
        #[arg(short, long)]
        format: Option<String>,
    },
}

fn read_input(path: &str) -> Result<String, GudfError> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(GudfError::Io)?;
        Ok(buf)
    } else {
        fs::read_to_string(path).map_err(GudfError::Io)
    }
}

fn get_extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
}

fn main() {
    let cli = Cli::parse();

    // Handle subcommands
    if let Some(command) = cli.command {
        match command {
            Commands::Merge {
                base,
                left,
                right,
                strategy,
                format,
            } => {
                run_merge(&base, &left, &right, &strategy, format.as_deref());
                return;
            }
        }
    }

    let file1 = match &cli.file1 {
        Some(f) => f.clone(),
        None => {
            eprintln!("{}: missing file arguments", "error".red().bold());
            std::process::exit(1);
        }
    };
    let file2 = match &cli.file2 {
        Some(f) => f.clone(),
        None => {
            eprintln!("{}: missing second file argument", "error".red().bold());
            std::process::exit(1);
        }
    };

    // Check if both are directories
    if Path::new(&file1).is_dir() && Path::new(&file2).is_dir() {
        run_dir_diff(&file1, &file2);
        return;
    }

    let old = match read_input(&file1) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    let new = match read_input(&file2) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    // Detect cross-format mode
    let ext1 = get_extension(&file1);
    let ext2 = get_extension(&file2);
    let cross_format_mode = cli.cross_format
        || (ext1.is_some() && ext2.is_some() && ext1 != ext2 && {
            let e1 = ext1.as_deref().unwrap();
            let e2 = ext2.as_deref().unwrap();
            CrossFormatKind::from_extension(e1).is_some()
                && CrossFormatKind::from_extension(e2).is_some()
        });

    // Build engine
    let mut engine = DiffEngine::new();

    if let Some(lang) = &cli.lang {
        engine.register(Box::new(CodeFormat::new(lang.as_str())));
    }

    if cli.annotate {
        engine.add_annotator(Box::new(SensitiveFieldAnnotator));
    }

    if cli.semantic {
        let engine_new = DiffEngine::new()
            .with_semantic(SemanticAnalyzer::new(SemanticOptions::default()));
        // We need to handle semantic separately since it's a post-processing step
        let _ = engine_new; // semantic handled below
    }

    let result = if cross_format_mode {
        let old_kind = ext1
            .as_deref()
            .and_then(CrossFormatKind::from_extension)
            .unwrap_or(CrossFormatKind::Json);
        let new_kind = ext2
            .as_deref()
            .and_then(CrossFormatKind::from_extension)
            .unwrap_or(CrossFormatKind::Json);
        engine.diff_cross(&old, old_kind, &new, new_kind)
    } else if let Some(lang) = &cli.lang {
        engine.diff_as(FormatKind::Code(lang.clone()), &old, &new)
    } else if let Some(fmt) = &cli.format {
        let kind = match fmt.as_str() {
            "text" => FormatKind::Text,
            "json" => FormatKind::Json,
            "toml" => FormatKind::Toml,
            "yaml" => FormatKind::Yaml,
            other => {
                eprintln!("{}: unknown format '{other}'", "error".red().bold());
                std::process::exit(1);
            }
        };
        engine.diff_as(kind, &old, &new)
    } else {
        engine.diff(&old, &new)
    };

    let result = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    // Apply semantic analysis if requested
    let result = if cli.semantic {
        let analyzer = SemanticAnalyzer::new(SemanticOptions::default());
        analyzer.analyze(result)
    } else {
        result
    };

    let output: Box<dyn OutputFormatter> = match cli.output.as_str() {
        "unified" => Box::new(
            UnifiedFormatter::new(&file1, &file2).context(cli.context),
        ),
        "inline" => Box::new(InlineFormatter),
        "json-patch" => Box::new(JsonPatchFormatter),
        other => {
            eprintln!(
                "{}: unknown output format '{other}'",
                "error".red().bold()
            );
            std::process::exit(1);
        }
    };

    print!("{}", output.format(&result));
}

fn run_merge(base_path: &str, left_path: &str, right_path: &str, strategy: &str, format: Option<&str>) {
    let base = match read_input(base_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };
    let left = match read_input(left_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };
    let right = match read_input(right_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    let merge_strategy = match strategy {
        "ours" => MergeStrategy::Ours,
        "theirs" => MergeStrategy::Theirs,
        "manual" => MergeStrategy::Manual,
        other => {
            eprintln!("{}: unknown strategy '{other}'", "error".red().bold());
            std::process::exit(1);
        }
    };

    let fmt = match format.or_else(|| get_extension(base_path).as_deref().map(|_| "auto")).unwrap_or("auto") {
        "json" => FormatKind::Json,
        "toml" => FormatKind::Toml,
        "yaml" => FormatKind::Yaml,
        _ => {
            // Auto-detect from base file extension
            match get_extension(base_path).as_deref() {
                Some("json") => FormatKind::Json,
                Some("toml") => FormatKind::Toml,
                Some("yaml") | Some("yml") => FormatKind::Yaml,
                _ => FormatKind::Json,
            }
        }
    };

    let result = match gudf::merge(&base, &left, &right, fmt, merge_strategy) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    if result.is_clean() {
        println!("{}", serde_json::to_string_pretty(&result.merged).unwrap());
    } else {
        eprintln!(
            "{}: {} conflict(s) detected",
            "warning".yellow().bold(),
            result.conflicts.len()
        );
        for conflict in &result.conflicts {
            eprintln!(
                "  {} at '{}': left={}, right={}",
                "CONFLICT".red(),
                conflict.path,
                conflict.left.as_deref().unwrap_or("(none)"),
                conflict.right.as_deref().unwrap_or("(none)"),
            );
        }
        std::process::exit(1);
    }
}

fn run_dir_diff(old_dir: &str, new_dir: &str) {
    let result = match gudf_dir::diff_dirs(Path::new(old_dir), Path::new(new_dir)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    print!("{}", gudf_dir::format_dir_diff(&result));
}
