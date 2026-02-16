use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use lantai::{LantaiConfig, SearchQuery};

#[derive(Parser)]
#[command(
    name = "lantai",
    about = "File-system persistent memory with semantic search"
)]
struct Cli {
    /// Embedding API base URL (env: OPENAI_BASE_URL)
    #[arg(
        long,
        env = "OPENAI_BASE_URL",
        default_value = "https://api.openai.com/v1"
    )]
    base_url: String,

    /// Embedding API key (env: OPENAI_API_KEY)
    #[arg(
        long,
        env = "OPENAI_API_KEY",
        hide_env_values = true,
        default_value = ""
    )]
    api_key: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index markdown files in specified directories
    Index {
        /// Directories to index (defaults to config memory_dir)
        dirs: Vec<String>,
    },
    /// Search indexed memories
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short = 'k', long, default_value = "5")]
        limit: usize,
    },
    /// Watch directories for changes and auto-index
    Watch {
        /// Directories to watch (defaults to config memory_dir)
        dirs: Vec<String>,
    },
    /// Show index statistics
    Stats,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config = LantaiConfig::load()?;

    match &cli.command {
        Commands::Index { dirs } => cmd_index(&cli, &config, dirs).await?,
        Commands::Search { query, limit } => cmd_search(&cli, &config, query, *limit).await?,
        Commands::Watch { dirs } => cmd_watch(&cli, &config, dirs).await?,
        Commands::Stats => cmd_stats(&cli, &config)?,
    }

    Ok(())
}

/// Build a Lantai instance with the configured embedding provider.
fn build_lantai(cli: &Cli, config: &LantaiConfig) -> anyhow::Result<lantai::Lantai> {
    // Ensure database parent directory exists
    let db_path = std::path::Path::new(&config.database_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let embedding = lantai::embedding::openai::OpenAIEmbedding::new(
        &cli.base_url,
        &cli.api_key,
        &config.embedding.model,
        config.embedding.dimensions,
    );
    let lantai = lantai::Lantai::new(config.clone(), Box::new(embedding))?;
    Ok(lantai)
}

/// Resolve directories: use provided dirs, or fall back to config memory_dir.
fn resolve_dirs(config: &LantaiConfig, dirs: &[String]) -> Vec<String> {
    if dirs.is_empty() {
        vec![config.memory_dir.clone()]
    } else {
        dirs.to_vec()
    }
}

// ─── Subcommand implementations ──────────────────────────────────────────────

async fn cmd_index(cli: &Cli, config: &LantaiConfig, dirs: &[String]) -> anyhow::Result<()> {
    let dirs = resolve_dirs(config, dirs);
    let lantai = build_lantai(cli, config)?;

    let dir_refs: Vec<&str> = dirs.iter().map(|s| s.as_str()).collect();
    println!("Indexing {} director(ies)...", dir_refs.len());

    let report = lantai.index(&dir_refs).await?;

    println!(
        "Done. +{} ~{} -{} files, +{} -{} chunks",
        report.files_added,
        report.files_updated,
        report.files_deleted,
        report.chunks_added,
        report.chunks_deleted,
    );
    Ok(())
}

async fn cmd_search(
    cli: &Cli,
    config: &LantaiConfig,
    query: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let lantai = build_lantai(cli, config)?;

    let q = SearchQuery::new(query, limit);
    let results = lantai.search_with_options(&q).await?;

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    for (i, r) in results.iter().enumerate() {
        println!("─── Result {} (score: {:.4}) ───", i + 1, r.score);
        println!(
            "  Source: {}:{}-{}",
            r.source_path, r.start_line, r.end_line
        );
        if !r.heading_path.is_empty() {
            println!("  Path:   {}", r.heading_path);
        }
        println!();
        // Print content with indentation
        for line in r.content.lines() {
            println!("  {line}");
        }
        println!();
    }
    Ok(())
}

async fn cmd_watch(cli: &Cli, config: &LantaiConfig, dirs: &[String]) -> anyhow::Result<()> {
    let dirs = resolve_dirs(config, dirs);
    #[allow(clippy::arc_with_non_send_sync)] // LantaiWatcher requires Arc; single-task usage
    let lantai = Arc::new(build_lantai(cli, config)?);

    let debounce_ms = config.watch.as_ref().map(|w| w.debounce_ms).unwrap_or(1500);

    let watch_dirs: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();

    let watcher = lantai::watcher::LantaiWatcher::new(lantai, watch_dirs, debounce_ms);
    let cancel = tokio_util::sync::CancellationToken::new();
    let cancel_clone = cancel.clone();

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        println!("\nReceived Ctrl+C, shutting down...");
        cancel_clone.cancel();
    });

    println!(
        "Watching {} director(ies). Press Ctrl+C to stop.",
        dirs.len()
    );
    watcher.watch(cancel).await?;

    println!("Watcher stopped.");
    Ok(())
}

fn cmd_stats(cli: &Cli, config: &LantaiConfig) -> anyhow::Result<()> {
    let lantai = build_lantai(cli, config)?;
    let stats = lantai.stats()?;

    println!("=== Lantai Index Statistics ===");
    println!("  Files:             {}", stats.total_files);
    println!("  Chunks:            {}", stats.total_chunks);
    println!("  Cached embeddings: {}", stats.total_cached_embeddings);
    Ok(())
}
