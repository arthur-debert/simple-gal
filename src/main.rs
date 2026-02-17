use clap::{Parser, Subcommand};
use simple_gal::{config, generate, output, process, scan};
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
#[command(long_about = "\
Static site generator for photo portfolios

Your filesystem is the data source. Directories become albums, images are
ordered by numeric prefix, and markdown files become pages.

Content structure:

  content/
  ├── config.toml                  # Site config (optional, cascades to children)
  ├── assets/                      # Static assets (favicon, fonts) → copied to output root
  ├── 040-about.md                 # Page (numbered = shown in nav)
  ├── 050-github.md                # Link page (URL-only .md → external nav link)
  ├── 010-Landscapes/              # Album (numbered = shown in nav)
  │   ├── config.toml              # Per-gallery config (overrides parent)
  │   ├── description.txt          # Album description
  │   ├── 001-dawn.jpg             # Preview image (lowest number)
  │   ├── 001-dawn.txt             # Image sidecar description
  │   └── 010-mountains.jpg        # Non-contiguous numbering OK
  ├── 020-Travel/                  # Container (has subdirs, not images)
  │   ├── 010-Japan/               # Nested album
  │   │   ├── description.md       # Markdown description (priority over .txt)
  │   │   └── 001-tokyo.jpg
  │   └── 020-Italy/
  │       └── 001-rome.jpg
  └── wip-experiments/             # No number prefix = hidden from nav

Metadata resolution (first available wins):
  Title:       IPTC tag → filename (001-Dusk.jpg → \"Dusk\")
  Description: sidecar .txt → IPTC caption
  Gallery:     directory name; description from description.md or .txt

Run 'simple-gal gen-config' to generate a documented config.toml.")]
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
    /// Scan content directory into a manifest
    Scan,
    /// Generate responsive image sizes and thumbnails
    Process,
    /// Produce the final HTML site from processed images
    Generate,
    /// Run the full pipeline: scan → process → generate
    Build,
    /// Validate content directory without building
    Check,
    /// Print a stock config.toml with all options documented
    GenConfig,
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
            output::print_scan_output(&manifest, &cli.source);
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
            output::print_process_output(&result);
        }
        Command::Generate => {
            let processed_dir = cli.temp_dir.join("processed");
            let processed_manifest_path = processed_dir.join("manifest.json");
            generate::generate(
                &processed_manifest_path,
                &processed_dir,
                &cli.output,
                &cli.source,
            )?;
            let manifest_content = std::fs::read_to_string(&processed_manifest_path)?;
            let manifest: generate::Manifest = serde_json::from_str(&manifest_content)?;
            output::print_generate_output(&manifest);
        }
        Command::Build => {
            // Resolve content root: check config.toml in source dir for content_root override
            let source = resolve_build_source(&cli.source);

            std::fs::create_dir_all(&cli.temp_dir)?;

            println!("==> Stage 1: Scanning {}", source.display());
            let manifest = scan::scan(&source)?;
            let scan_manifest_path = cli.temp_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&scan_manifest_path, json)?;
            output::print_scan_output(&manifest, &source);

            println!("==> Stage 2: Processing images");
            init_thread_pool(&manifest.config.processing);
            let processed_dir = cli.temp_dir.join("processed");
            let processed_manifest =
                process::process(&scan_manifest_path, &source, &processed_dir)?;
            let processed_manifest_path = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&processed_manifest)?;
            std::fs::write(&processed_manifest_path, &json)?;
            output::print_process_output(&processed_manifest);

            println!("==> Stage 3: Generating HTML → {}", cli.output.display());
            generate::generate(
                &processed_manifest_path,
                &processed_dir,
                &cli.output,
                &source,
            )?;
            let gen_manifest_content = std::fs::read_to_string(&processed_manifest_path)?;
            let gen_manifest: generate::Manifest = serde_json::from_str(&gen_manifest_content)?;
            output::print_generate_output(&gen_manifest);

            println!("==> Build complete: {}", cli.output.display());
        }
        Command::Check => {
            let source = resolve_build_source(&cli.source);
            println!("==> Checking {}", source.display());
            let manifest = scan::scan(&source)?;
            output::print_scan_output(&manifest, &source);
            println!("==> Content is valid");
        }
        Command::GenConfig => {
            print!("{}", config::stock_config_toml());
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
