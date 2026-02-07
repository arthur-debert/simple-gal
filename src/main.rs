//! # Simple Gal
//!
//! A minimal static site generator for fine art photography portfolios.
//!
//! ## Build Pipeline
//!
//! Simple Gal processes images through a three-stage pipeline:
//!
//! ```text
//! 1. Scan      →  manifest.json    (filesystem → structured data)
//! 2. Process   →  processed/       (responsive sizes + thumbnails)
//! 3. Generate  →  dist/            (final HTML site)
//! ```
//!
//! Each stage is independent and produces a manifest file that the next stage consumes.
//! This allows incremental builds and easy debugging.
//!
//! ## Usage
//!
//! ```bash
//! # Full build (defaults: --source content --output dist)
//! simple-gal build
//!
//! # Or run stages individually
//! simple-gal scan
//! simple-gal process
//! simple-gal generate
//!
//! # Generate a stock config.toml
//! simple-gal gen-config config.toml
//!
//! # Override paths
//! simple-gal --source photos --output public build
//! ```
//!
//! ## Modules
//!
//! - [`config`] - Site configuration loaded from `config.toml`
//! - [`naming`] - Centralized filename parsing (`NNN-name` convention)
//! - [`types`] - Shared types used across pipeline stages
//! - [`scan`] - Stage 1: Filesystem scanning and manifest generation
//! - [`process`] - Stage 2: Image processing (responsive sizes, thumbnails)
//! - [`generate`] - Stage 3: HTML site generation

mod config;
mod generate;
mod imaging;
mod metadata;
mod naming;
mod process;
mod scan;
mod types;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

fn version_string() -> &'static str {
    let on_tag = env!("ON_RELEASE_TAG");
    if on_tag == "true" {
        env!("CARGO_PKG_VERSION")
    } else {
        let hash = env!("GIT_HASH");
        if hash.is_empty() {
            "dev@unknown"
        } else {
            // Leaked once at startup — trivial, called exactly once
            Box::leak(format!("dev@{hash}").into_boxed_str())
        }
    }
}

#[derive(Parser)]
#[command(name = "simple-gal")]
#[command(about = "Static site generator for photo portfolios")]
#[command(version = version_string())]
struct Cli {
    /// Content directory
    #[arg(long, default_value = "content", global = true)]
    source: PathBuf,

    /// Output directory
    #[arg(long, default_value = "dist", global = true)]
    output: PathBuf,

    /// Directory for intermediate files (manifest, processed images)
    #[arg(long, default_value = ".simple-gal-temp", global = true)]
    temp_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan filesystem and generate manifest
    Scan,
    /// Process images (generate responsive sizes and thumbnails)
    Process,
    /// Generate final HTML site
    Generate,
    /// Run full build pipeline (scan + process + generate)
    Build,
    /// Output a stock config.toml with all options and documentation
    GenConfig {
        /// Path to write the config file (use - for stdout)
        #[arg(default_value = "config.toml")]
        path: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan => {
            let manifest = scan::scan(&cli.source)?;
            std::fs::create_dir_all(&cli.temp_dir)?;
            let manifest_path = cli.temp_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&manifest_path, json)?;
            println!("Wrote manifest to {}", manifest_path.display());
        }
        Command::Process => {
            let scan_manifest_path = cli.temp_dir.join("manifest.json");
            let manifest_content = std::fs::read_to_string(&scan_manifest_path)?;
            let input_manifest: serde_json::Value = serde_json::from_str(&manifest_content)?;
            let site_config: config::SiteConfig =
                serde_json::from_value(input_manifest.get("config").cloned().unwrap_or_default())?;
            init_thread_pool(&site_config.processing);
            let processed_dir = cli.temp_dir.join("processed");
            let result = process::process(&scan_manifest_path, &cli.source, &processed_dir)?;
            let output_manifest = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&result)?;
            std::fs::write(&output_manifest, &json)?;
            println!("Processed {} albums", result.albums.len());
            println!("Wrote manifest to {}", output_manifest.display());
        }
        Command::Generate => {
            let processed_dir = cli.temp_dir.join("processed");
            let processed_manifest_path = processed_dir.join("manifest.json");
            generate::generate(&processed_manifest_path, &processed_dir, &cli.output)?;
        }
        Command::Build => {
            // Resolve content root: check config.toml in source dir for content_root override
            let source = resolve_build_source(&cli.source);

            std::fs::create_dir_all(&cli.temp_dir)?;

            println!("==> Stage 1: Scanning filesystem");
            let manifest = scan::scan(&source)?;
            let scan_manifest_path = cli.temp_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&scan_manifest_path, json)?;

            println!("==> Stage 2: Processing images");
            init_thread_pool(&manifest.config.processing);
            let processed_dir = cli.temp_dir.join("processed");
            let processed_manifest =
                process::process(&scan_manifest_path, &source, &processed_dir)?;
            let processed_manifest_path = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&processed_manifest)?;
            std::fs::write(&processed_manifest_path, &json)?;

            println!("==> Stage 3: Generating HTML");
            generate::generate(&processed_manifest_path, &processed_dir, &cli.output)?;

            println!("==> Build complete: {}", cli.output.display());
        }
        Command::GenConfig { path } => {
            let content = config::stock_config_toml();
            if path == "-" {
                print!("{}", content);
            } else {
                let p = std::path::Path::new(&path);
                if p.exists() {
                    eprintln!(
                        "Error: {} already exists. Remove it first or choose a different path.",
                        path
                    );
                    std::process::exit(1);
                }
                std::fs::write(p, content)?;
                println!("Wrote stock config to {}", path);
            }
        }
    }

    Ok(())
}

/// Initialize the rayon thread pool based on processing config.
///
/// Caps at the number of available CPU cores — user can constrain down, not up.
fn init_thread_pool(processing: &config::ProcessingConfig) {
    let threads = config::effective_threads(processing);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .ok();
    println!("Using {} worker threads", threads);
}

/// Resolve the content source directory for the build command.
///
/// Loads `config.toml` from the given source directory and uses its `content_root`
/// if it specifies a different path. Otherwise returns the source path as-is.
fn resolve_build_source(cli_source: &std::path::Path) -> PathBuf {
    config::load_config(cli_source)
        .map(|c| PathBuf::from(c.content_root))
        .unwrap_or_else(|_| cli_source.to_path_buf())
}
