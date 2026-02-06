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
//! # Full build (uses content_root from config.toml, output defaults to dist/)
//! simple-gal build
//!
//! # Or run stages individually
//! simple-gal scan ./content
//! simple-gal process manifest.json --output processed
//! simple-gal generate processed/manifest.json --output dist
//! ```
//!
//! ## Modules
//!
//! - [`config`] - Site configuration loaded from `config.toml`
//! - [`scan`] - Stage 1: Filesystem scanning and manifest generation
//! - [`process`] - Stage 2: Image processing (responsive sizes, thumbnails)
//! - [`generate`] - Stage 3: HTML site generation

mod config;
mod generate;
mod imaging;
mod process;
mod scan;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "simple-gal")]
#[command(about = "Static site generator for photo portfolios")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan filesystem and generate manifest.json
    Scan {
        /// Root directory containing albums
        #[arg(default_value = ".")]
        root: PathBuf,

        /// Output manifest path
        #[arg(short, long, default_value = "manifest.json")]
        output: PathBuf,
    },
    /// Process images (generate responsive sizes)
    Process {
        /// Manifest file from scan stage
        #[arg(default_value = "manifest.json")]
        manifest: PathBuf,

        /// Source root (where original images are)
        #[arg(short, long)]
        source: Option<PathBuf>,

        /// Output directory for processed images
        #[arg(short, long, default_value = "processed")]
        output: PathBuf,
    },
    /// Generate final HTML site
    Generate {
        /// Manifest file (from process stage)
        #[arg(default_value = "processed/manifest.json")]
        manifest: PathBuf,

        /// Processed images directory
        #[arg(short, long, default_value = "processed")]
        processed: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "dist")]
        output: PathBuf,
    },
    /// Run full build pipeline
    Build {
        /// Root directory containing albums
        root: Option<PathBuf>,

        /// Output directory
        #[arg(default_value = "dist")]
        output: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan { root, output } => {
            let manifest = scan::scan(&root)?;
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&output, json)?;
            println!("Wrote manifest to {}", output.display());
        }
        Command::Process {
            manifest,
            source,
            output,
        } => {
            let source_root = source.unwrap_or_else(|| PathBuf::from("."));
            // Read site config from the manifest to get image processing settings
            let manifest_content = std::fs::read_to_string(&manifest)?;
            let input_manifest: serde_json::Value = serde_json::from_str(&manifest_content)?;
            let site_config: config::SiteConfig =
                serde_json::from_value(input_manifest.get("config").cloned().unwrap_or_default())?;
            let config = process::ProcessConfig::from_site_config(&site_config);
            let result = process::process(&manifest, &source_root, &output, &config)?;
            let output_manifest = output.join("manifest.json");
            let json = serde_json::to_string_pretty(&result)?;
            std::fs::write(&output_manifest, &json)?;
            println!("Processed {} albums", result.albums.len());
            println!("Wrote manifest to {}", output_manifest.display());
        }
        Command::Generate {
            manifest,
            processed,
            output,
        } => {
            generate::generate(&manifest, &processed, &output)?;
        }
        Command::Build { root, output } => {
            // Resolve content root: CLI arg > config.toml content_root > "content"
            let root = root.unwrap_or_else(|| {
                let default = PathBuf::from("content");
                config::load_config(&default)
                    .map(|c| PathBuf::from(c.content_root))
                    .unwrap_or(default)
            });

            // Use a temp directory for all intermediate files - never touch source
            let temp_dir = std::env::temp_dir().join(format!("simple-gal-{}", std::process::id()));
            std::fs::create_dir_all(&temp_dir)?;

            println!("==> Stage 1: Scanning filesystem");
            let manifest = scan::scan(&root)?;
            let scan_manifest_path = temp_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&scan_manifest_path, json)?;

            println!("==> Stage 2: Processing images");
            let processed_dir = temp_dir.join("processed");
            let config = process::ProcessConfig::from_site_config(&manifest.config);
            let processed_manifest =
                process::process(&scan_manifest_path, &root, &processed_dir, &config)?;
            let processed_manifest_path = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&processed_manifest)?;
            std::fs::write(&processed_manifest_path, &json)?;

            println!("==> Stage 3: Generating HTML");
            generate::generate(&processed_manifest_path, &processed_dir, &output)?;

            println!("==> Cleaning up temp files");
            std::fs::remove_dir_all(&temp_dir)?;

            println!("==> Build complete: {}", output.display());
        }
    }

    Ok(())
}
