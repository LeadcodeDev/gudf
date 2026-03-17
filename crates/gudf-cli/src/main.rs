use std::fs;
use std::io::{self, Read};
use std::path::Path;

use clap::{Parser, Subcommand};
use colored::Colorize;

use gudf::output::inline::InlineFormatter;
use gudf::output::json_patch::JsonPatchFormatter;
use gudf::output::unified::UnifiedFormatter;
use gudf::output::OutputFormatter;
use gudf::formats::code::CodeFormat;
use gudf::{
    CrossFormatKind, DiffEngine, FormatKind, GudfError, MergeStrategy, MutationChain,
    SemanticAnalyzer, SemanticOptions, SensitiveFieldAnnotator,
};

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

    /// Build a mutation chain from file versions and query with expressions
    ///
    /// Expressions: HEAD, HEAD~N, HEAD^, ORIG, @N, <sha-prefix>
    /// Range syntax: FROM..TO (e.g. ORIG..HEAD, HEAD~2..HEAD)
    Chain {
        /// Base file followed by successive version files
        #[arg(required = true)]
        files: Vec<String>,

        /// Expression range to diff (e.g. "ORIG..HEAD", "HEAD~2..HEAD")
        #[arg(long, default_value = "ORIG..HEAD")]
        diff: String,

        /// Show document at an expression instead of diffing
        #[arg(long)]
        show: Option<String>,

        /// Show mutation log with SHAs and stats
        #[arg(long)]
        log: bool,

        /// Save expression state to a file: --save EXPR:PATH
        #[arg(long)]
        save: Option<String>,

        /// Output format (unified, inline, json-patch)
        #[arg(short, long, default_value = "unified")]
        output: String,

        /// Number of context lines for unified output
        #[arg(short = 'U', long, default_value = "3")]
        context: usize,

        /// Force format (text, json, toml, yaml)
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

fn parse_format(s: &str) -> Option<FormatKind> {
    match s {
        "text" => Some(FormatKind::Text),
        "json" => Some(FormatKind::Json),
        "toml" => Some(FormatKind::Toml),
        "yaml" => Some(FormatKind::Yaml),
        _ => None,
    }
}

fn main() {
    let cli = Cli::parse();

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
            Commands::Chain {
                files,
                diff,
                show,
                log,
                save,
                output,
                context,
                format,
            } => {
                run_chain(&files, &diff, show.as_deref(), log, save.as_deref(), &output, context, format.as_deref());
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

    let mut engine = DiffEngine::new();

    if let Some(lang) = &cli.lang {
        engine.register(Box::new(CodeFormat::new(lang.as_str())));
    }

    if cli.annotate {
        engine.add_annotator(Box::new(SensitiveFieldAnnotator));
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
        let kind = match parse_format(fmt) {
            Some(k) => k,
            None => {
                eprintln!("{}: unknown format '{fmt}'", "error".red().bold());
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

    let result = if cli.semantic {
        SemanticAnalyzer::new(SemanticOptions::default()).analyze(result)
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

// ── chain subcommand ───────────────────────────────────────────────────

fn run_chain(
    files: &[String],
    diff_range: &str,
    show: Option<&str>,
    log: bool,
    save: Option<&str>,
    output_format: &str,
    context: usize,
    format: Option<&str>,
) {
    if files.is_empty() {
        eprintln!("{}: at least one file required", "error".red().bold());
        std::process::exit(1);
    }

    // Build the chain: first file is the base, rest are mutations
    let base_path = &files[0];
    let mut chain = if let Some(fmt) = format.and_then(parse_format) {
        match MutationChain::from_file_as(base_path, fmt) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: {base_path}: {e}", "error".red().bold());
                std::process::exit(1);
            }
        }
    } else {
        match MutationChain::from_file(base_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}: {base_path}: {e}", "error".red().bold());
                std::process::exit(1);
            }
        }
    };

    // Apply successive files as mutations
    for path in &files[1..] {
        if let Err(e) = chain.mutate_file(path) {
            eprintln!("{}: {path}: {e}", "error".red().bold());
            std::process::exit(1);
        }
    }

    // --log: print mutation log
    if log {
        run_chain_log(&chain, files);
        return;
    }

    // --show EXPR: print document at expression
    if let Some(expr) = show {
        run_chain_show(&chain, expr);
        return;
    }

    // --save EXPR:PATH: save expression to file
    if let Some(save_spec) = save {
        run_chain_save(&chain, save_spec);
        return;
    }

    // Default: diff between two expressions
    run_chain_diff(&chain, diff_range, output_format, context);
}

fn run_chain_log(chain: &MutationChain, files: &[String]) {
    for entry in chain.log() {
        let label = if entry.step < files.len() {
            files[entry.step].as_str()
        } else {
            "?"
        };

        match &entry.stats {
            None => {
                println!(
                    "{} {} @{} (origin)",
                    entry.sha.short().yellow(),
                    label,
                    entry.step,
                );
            }
            Some(stats) => {
                let mut parts = Vec::new();
                if stats.additions > 0 {
                    parts.push(format!("+{}", stats.additions).green().to_string());
                }
                if stats.deletions > 0 {
                    parts.push(format!("-{}", stats.deletions).red().to_string());
                }
                if stats.modifications > 0 {
                    parts.push(format!("~{}", stats.modifications).cyan().to_string());
                }
                if stats.moves > 0 {
                    parts.push(format!("m{}", stats.moves).blue().to_string());
                }
                if stats.renames > 0 {
                    parts.push(format!("r{}", stats.renames).magenta().to_string());
                }
                let stats_str = if parts.is_empty() {
                    "(no changes)".to_string()
                } else {
                    parts.join(" ")
                };
                println!(
                    "{} {} @{} {}",
                    entry.sha.short().yellow(),
                    label,
                    entry.step,
                    stats_str,
                );
            }
        }
    }
}

fn run_chain_show(chain: &MutationChain, expr: &str) {
    match chain.resolve(expr) {
        Some((step, doc)) => {
            let sha = chain.sha_at(step).unwrap();
            eprintln!(
                "{}: @{} {}",
                expr.bold(),
                step,
                sha.short().yellow(),
            );
            print!("{doc}");
        }
        None => {
            eprintln!(
                "{}: cannot resolve expression '{expr}'",
                "error".red().bold()
            );
            std::process::exit(1);
        }
    }
}

fn run_chain_save(chain: &MutationChain, spec: &str) {
    // Format: EXPR:PATH
    let (expr, path) = match spec.rsplit_once(':') {
        Some((e, p)) if !p.is_empty() => (e, p),
        _ => {
            eprintln!(
                "{}: --save format is EXPR:PATH (e.g. HEAD~1:rollback.json)",
                "error".red().bold()
            );
            std::process::exit(1);
        }
    };

    match chain.save_expr(expr, path) {
        Ok(()) => {
            let (step, _) = chain.resolve(expr).unwrap();
            let sha = chain.sha_at(step).unwrap();
            eprintln!(
                "{}: saved {} ({}) to {}",
                "ok".green().bold(),
                expr,
                sha.short().yellow(),
                path,
            );
        }
        Err(e) => {
            eprintln!("{}: {e}", "error".red().bold());
            std::process::exit(1);
        }
    }
}

fn run_chain_diff(chain: &MutationChain, range: &str, output_format: &str, context: usize) {
    // Parse range: FROM..TO
    let (from, to) = match range.split_once("..") {
        Some((f, t)) => (f.trim(), t.trim()),
        None => {
            // Single expression: diff ORIG..EXPR
            ("ORIG", range.trim())
        }
    };

    let output = match output_format {
        "unified" => chain.unified(from, to).context(context).render(),
        "inline" => chain
            .unified(from, to)
            .render_with(&InlineFormatter),
        "json-patch" => chain
            .unified(from, to)
            .render_with(&JsonPatchFormatter),
        other => {
            eprintln!(
                "{}: unknown output format '{other}'",
                "error".red().bold()
            );
            std::process::exit(1);
        }
    };

    match output {
        Ok(text) => print!("{text}"),
        Err(e) => {
            eprintln!("{}: {e}", "error".red().bold());
            std::process::exit(1);
        }
    }
}

// ── merge subcommand ───────────────────────────────────────────────────

fn run_merge(
    base_path: &str,
    left_path: &str,
    right_path: &str,
    strategy: &str,
    format: Option<&str>,
) {
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

    let fmt = match format
        .or_else(|| get_extension(base_path).as_deref().map(|_| "auto"))
        .unwrap_or("auto")
    {
        "json" => FormatKind::Json,
        "toml" => FormatKind::Toml,
        "yaml" => FormatKind::Yaml,
        _ => match get_extension(base_path).as_deref() {
            Some("json") => FormatKind::Json,
            Some("toml") => FormatKind::Toml,
            Some("yaml") | Some("yml") => FormatKind::Yaml,
            _ => FormatKind::Json,
        },
    };

    let result = match gudf::merge(&base, &left, &right, fmt, merge_strategy) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    };

    if result.is_clean() {
        println!(
            "{}",
            serde_json::to_string_pretty(&result.merged).unwrap()
        );
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

// ── dir diff ───────────────────────────────────────────────────────────

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
