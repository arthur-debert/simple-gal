use clap::{Parser, Subcommand, ValueEnum};
use simple_gal::{config, generate, output, process, scan};
use std::io::IsTerminal;
use std::path::PathBuf;

/// Shared flags for commands that process images.
#[derive(clap::Args, Clone)]
struct CacheArgs {
    /// Disable the processing cache — force re-encoding of all images
    #[arg(long)]
    no_cache: bool,
}

/// Output format for the scan command.
#[derive(Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    /// Pretty-printed JSON manifest
    #[default]
    Json,
    /// Human-readable tree display
    Text,
}

/// Arguments for the scan command.
#[derive(clap::Args, Clone)]
struct ScanArgs {
    /// Output format
    #[arg(long, default_value = "json")]
    format: OutputFormat,

    /// Save the JSON manifest to a file.
    /// When passed without a value, uses <temp-dir>/manifest.json.
    #[arg(long, num_args = 0..=1, default_missing_value = "__default__")]
    save_manifest: Option<PathBuf>,
}

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
    Scan(ScanArgs),
    /// Generate responsive image sizes and thumbnails
    Process(CacheArgs),
    /// Produce the final HTML site from processed images
    Generate,
    /// Run the full pipeline: scan → process → generate
    Build(CacheArgs),
    /// Validate content directory without building
    Check,
    /// Print a stock config.toml with all options documented
    GenConfig,
}

fn main() {
    if let Err(err) = run() {
        report_error(err.as_ref());
        std::process::exit(1);
    }
}

/// Walk the error `source()` chain looking for a [`config::ConfigError`].
/// Returns the matching error if one is found so the CLI can render it
/// through clapfig's plain/rich renderers; returns `None` for every other
/// error kind (IO, process, generate, …).
fn find_config_error<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> Option<&'a config::ConfigError> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = current {
        if let Some(cfg) = e.downcast_ref::<config::ConfigError>() {
            return Some(cfg);
        }
        current = e.source();
    }
    None
}

/// Print an error to stderr with the best renderer available for the
/// current environment. Config parse failures get clapfig's rich/plain
/// treatment (source snippet + caret); everything else falls through to a
/// plain `Error: {message}` line.
fn report_error(err: &(dyn std::error::Error + 'static)) {
    if let Some(cfg_err) = find_config_error(err)
        && let Some(clap_err) = cfg_err.to_clapfig_error()
    {
        let msg = if std::io::stderr().is_terminal() {
            clapfig::render::render_rich(&clap_err)
        } else {
            clapfig::render::render_plain(&clap_err)
        };
        eprintln!("{msg}");
        return;
    }
    eprintln!("Error: {err}");
    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Scan(args) => {
            let manifest = scan::scan(&cli.source)?;

            if let Some(path) = args.save_manifest {
                let manifest_path = if path.as_os_str() == "__default__" {
                    cli.temp_dir.join("manifest.json")
                } else {
                    path
                };
                if let Some(parent) = manifest_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let json = serde_json::to_string_pretty(&manifest)?;
                std::fs::write(&manifest_path, json)?;
            }

            match args.format {
                OutputFormat::Json => {
                    let json = serde_json::to_string_pretty(&manifest)?;
                    println!("{}", json);
                }
                OutputFormat::Text => {
                    output::print_scan_output(&manifest, &cli.source);
                }
            }
        }
        Command::Process(cache_args) => {
            let scan_manifest_path = cli.temp_dir.join("manifest.json");
            let manifest_content = std::fs::read_to_string(&scan_manifest_path)?;
            let input_manifest: serde_json::Value = serde_json::from_str(&manifest_content)?;
            let site_config: config::SiteConfig =
                serde_json::from_value(input_manifest.get("config").cloned().unwrap_or_default())?;
            init_thread_pool(&site_config.processing);
            let processed_dir = cli.temp_dir.join("processed");
            let (tx, rx) = std::sync::mpsc::channel();
            let printer = std::thread::spawn(move || {
                for event in rx {
                    for line in output::format_process_event(&event) {
                        println!("{}", line);
                    }
                }
            });
            let result = process::process(
                &scan_manifest_path,
                &cli.source,
                &processed_dir,
                !cache_args.no_cache,
                Some(tx),
            )?;
            printer.join().unwrap();
            let output_manifest = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&result.manifest)?;
            std::fs::write(&output_manifest, &json)?;
            println!("Cache: {}", result.cache_stats);
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
        Command::Build(cache_args) => {
            let source = resolve_build_source(&cli.source);

            std::fs::create_dir_all(&cli.temp_dir)?;

            println!("==> Stage 1: Scanning {}", source.display());
            let manifest = scan::scan(&source)?;
            let scan_manifest_path = cli.temp_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&manifest)?;
            std::fs::write(&scan_manifest_path, &json)?;
            output::print_scan_output(&manifest, &source);

            println!("==> Stage 2: Processing images");
            init_thread_pool(&manifest.config.processing);
            let processed_dir = cli.temp_dir.join("processed");
            let (tx, rx) = std::sync::mpsc::channel();
            let printer = std::thread::spawn(move || {
                for event in rx {
                    for line in output::format_process_event(&event) {
                        println!("{}", line);
                    }
                }
            });
            let result = process::process(
                &scan_manifest_path,
                &source,
                &processed_dir,
                !cache_args.no_cache,
                Some(tx),
            )?;
            printer.join().unwrap();
            let processed_manifest_path = processed_dir.join("manifest.json");
            let json = serde_json::to_string_pretty(&result.manifest)?;
            std::fs::write(&processed_manifest_path, &json)?;
            println!("Cache: {}", result.cache_stats);

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
fn resolve_build_source(cli_source: &std::path::Path) -> PathBuf {
    cli_source.to_path_buf()
}
