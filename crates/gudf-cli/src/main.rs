use std::fs;
use std::io::{self, Read};

use clap::Parser;
use colored::Colorize;

use gudf::output::inline::InlineFormatter;
use gudf::output::json_patch::JsonPatchFormatter;
use gudf::output::unified::UnifiedFormatter;
use gudf::output::OutputFormatter;
use gudf::{DiffEngine, FormatKind, GudfError};
use gudf::formats::code::CodeFormat;

#[derive(Parser)]
#[command(
    name = "gudf",
    about = "Git Unified Diff Format — universal diff tool for text, JSON, TOML, YAML, and code"
)]
struct Cli {
    /// First file (use '-' for stdin)
    file1: String,

    /// Second file
    file2: String,

    /// Force a specific format (text, json, toml, yaml)
    #[arg(short, long)]
    format: Option<String>,

    /// Output format (unified, inline, json-patch)
    #[arg(short, long, default_value = "unified")]
    output: String,

    /// Language for code diff (rust, javascript, python)
    #[arg(short, long)]
    lang: Option<String>,
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

fn main() {
    let cli = Cli::parse();

    let old = match read_input(&cli.file1) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    let new = match read_input(&cli.file2) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    let mut engine = DiffEngine::new();

    if let Some(lang) = &cli.lang {
        engine.register(Box::new(CodeFormat::new(lang.as_str())));
    }

    let result = if let Some(lang) = &cli.lang {
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

    let output: Box<dyn OutputFormatter> = match cli.output.as_str() {
        "unified" => Box::new(UnifiedFormatter::new(&cli.file1, &cli.file2)),
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
