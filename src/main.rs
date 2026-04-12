use clap::{Parser, Subcommand, ValueEnum};
use clapfig::{Clapfig, ConfigAction, ConfigArgs, ConfigSubcommand, SearchPath};
use serde::Serialize;
use simple_gal::config::SiteConfig;
use simple_gal::json_output::{
    self, BuildPayload, CacheStatsPayload, CheckPayload, ConfigOpPayload, Counts, ErrorEnvelope,
    ErrorKind, GeneratePayload, OkEnvelope, ProcessPayload, ScanPayload,
};
use simple_gal::{config, generate, output, process, scan};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// Shared flags for commands that process images.
#[derive(clap::Args, Clone)]
struct CacheArgs {
    /// Disable the processing cache — force re-encoding of all images
    #[arg(long)]
    no_cache: bool,
}

/// Output format for all commands.
///
/// `text` is the human-readable default; `json` is the machine-readable
/// envelope format documented in [`json_output`] and used by scripts and
/// GUIs. In JSON mode every command emits exactly one JSON document — to
/// stdout on success, to stderr on error — so callers can always `jq` it.
#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    /// Newline-delimited JSON: one JSON object per line. Progress events
    /// stream as they happen; the final line is the result envelope.
    Ndjson,
}

/// Arguments for the scan command.
#[derive(clap::Args, Clone)]
struct ScanArgs {
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

Run 'simple-gal config gen' to generate a documented config.toml,
or 'simple-gal config schema' to emit a JSON Schema for tooling.")]
#[command(version = version_string())]
struct Cli {
    /// Content directory
    #[arg(long, default_value = "content", global = true)]
    source: PathBuf,

    /// Output directory.
    ///
    /// Not marked `global = true` because the `config gen` and `config
    /// schema` subcommands have their own `--output` flag (the file to
    /// write the template / schema to), and clap's global-flag inheritance
    /// would otherwise collide the two.
    #[arg(long, default_value = "dist")]
    output: PathBuf,

    /// Directory for intermediate files (manifest, processed images)
    #[arg(long, default_value = ".simple-gal-temp", global = true)]
    temp_dir: PathBuf,

    /// Output format: `text` (human-readable, default for most commands)
    /// or `json` (machine-readable envelope, one document on stdout or stderr).
    /// The `scan` command defaults to `json` for backwards compatibility.
    #[arg(long, global = true, value_enum)]
    format: Option<OutputFormat>,

    /// Suppress non-error output in text mode. No effect on JSON mode.
    #[arg(long, global = true)]
    quiet: bool,

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
    /// Manage site configuration: gen, schema, list, get, set, unset
    Config(ConfigArgs),
}

/// Wrapper around any command error tagged with an [`ErrorKind`] so the
/// CLI can render it appropriately and pick a matching exit code.
struct CliError {
    kind: ErrorKind,
    source: Box<dyn std::error::Error + 'static>,
}

impl CliError {
    fn new(kind: ErrorKind, source: Box<dyn std::error::Error + 'static>) -> Self {
        // If the error chain contains a ConfigError, reclassify — a config
        // parse failure inside the scan stage is still a config error from
        // the user's point of view.
        let kind = if find_config_error(source.as_ref()).is_some() {
            ErrorKind::Config
        } else if is_io_error(source.as_ref()) {
            // Promote raw IO errors out of Internal classification when
            // nothing more specific is available.
            match kind {
                ErrorKind::Internal => ErrorKind::Io,
                other => other,
            }
        } else {
            kind
        };
        CliError { kind, source }
    }
}

fn is_io_error(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = cur {
        if e.downcast_ref::<std::io::Error>().is_some() {
            return true;
        }
        cur = e.source();
    }
    false
}

/// Join the progress-printer thread and convert any panic from that
/// thread into a tagged [`CliError`]. The printer only formats events
/// and writes to stdout, so a panic here is very unusual — but in JSON
/// mode we've promised exactly one envelope, and an unwrap would both
/// skip the envelope and bypass the exit-code mapping.
fn join_printer(handle: std::thread::JoinHandle<()>) -> Result<(), CliError> {
    handle.join().map_err(|_| {
        let msg: Box<dyn std::error::Error + 'static> = "progress printer thread panicked".into();
        CliError::new(ErrorKind::Internal, msg)
    })
}

trait TagError<T> {
    fn tag(self, kind: ErrorKind) -> Result<T, CliError>;
}

impl<T, E> TagError<T> for Result<T, E>
where
    E: Into<Box<dyn std::error::Error + 'static>>,
{
    fn tag(self, kind: ErrorKind) -> Result<T, CliError> {
        self.map_err(|e| CliError::new(kind, e.into()))
    }
}

fn main() {
    let cli = Cli::parse();
    let format = resolve_format(&cli);
    match run(&cli, format) {
        Ok(()) => {}
        Err(err) => {
            report_error(err.source.as_ref(), err.kind, format);
            std::process::exit(err.kind.exit_code());
        }
    }
}

/// Resolve the effective output format: explicit `--format`, else the
/// command's default. `scan` defaults to JSON (historical behavior from
/// v0.12); everything else defaults to text.
fn resolve_format(cli: &Cli) -> OutputFormat {
    if let Some(fmt) = cli.format {
        return fmt;
    }
    match &cli.command {
        Command::Scan(_) => OutputFormat::Json,
        _ => OutputFormat::Text,
    }
}

/// Walk the error `source()` chain looking for a [`config::ConfigError`].
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

/// Print an error using the best renderer available:
///   - JSON mode: serialize [`ErrorEnvelope`] to stderr
///   - text mode with a config parse failure: clapfig rich/plain
///   - text mode fallback: plain `Error:` + cause chain
fn report_error(err: &(dyn std::error::Error + 'static), kind: ErrorKind, format: OutputFormat) {
    if matches!(format, OutputFormat::Json | OutputFormat::Ndjson) {
        let envelope = ErrorEnvelope::new(kind, err);
        let emit = if matches!(format, OutputFormat::Ndjson) {
            json_output::emit_stderr_compact(&envelope)
        } else {
            json_output::emit_stderr(&envelope)
        };
        if let Err(ser_err) = emit {
            eprintln!("Error: failed to render error envelope: {ser_err}");
        }
        return;
    }

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

fn run(cli: &Cli, format: OutputFormat) -> Result<(), CliError> {
    let json_mode = matches!(format, OutputFormat::Json | OutputFormat::Ndjson);
    let ndjson = matches!(format, OutputFormat::Ndjson);
    let quiet = cli.quiet;

    match &cli.command {
        Command::Scan(args) => run_scan(cli, args, format),
        Command::Process(cache_args) => run_process(cli, cache_args, json_mode, ndjson, quiet),
        Command::Generate => run_generate(cli, json_mode, ndjson, quiet),
        Command::Build(cache_args) => run_build(cli, cache_args, json_mode, ndjson, quiet),
        Command::Check => run_check(cli, json_mode, ndjson, quiet),
        Command::Config(args) => run_config(cli, args, json_mode, ndjson),
    }
}

/// Emit a JSON result envelope: pretty-printed for `--format json`,
/// compact single-line for `--format ndjson`.
fn emit_json_result<T: Serialize>(ndjson: bool, value: &T) -> Result<(), CliError> {
    if ndjson {
        json_output::emit_ndjson_result(value).tag(ErrorKind::Internal)
    } else {
        json_output::emit_stdout(value).tag(ErrorKind::Internal)
    }
}

fn run_scan(cli: &Cli, args: &ScanArgs, format: OutputFormat) -> Result<(), CliError> {
    let manifest = scan::scan(&cli.source).tag(ErrorKind::Scan)?;

    let saved_path = if let Some(path) = &args.save_manifest {
        let manifest_path = if path.as_os_str() == "__default__" {
            cli.temp_dir.join("manifest.json")
        } else {
            path.clone()
        };
        if let Some(parent) = manifest_path.parent() {
            std::fs::create_dir_all(parent).tag(ErrorKind::Io)?;
        }
        let json = serde_json::to_string_pretty(&manifest).tag(ErrorKind::Internal)?;
        std::fs::write(&manifest_path, json).tag(ErrorKind::Io)?;
        Some(manifest_path)
    } else {
        None
    };

    match format {
        OutputFormat::Json | OutputFormat::Ndjson => {
            let payload = ScanPayload::new(&manifest, &cli.source, saved_path);
            emit_json_result(
                matches!(format, OutputFormat::Ndjson),
                &OkEnvelope::new("scan", payload),
            )?;
        }
        OutputFormat::Text => {
            if !cli.quiet {
                output::print_scan_output(&manifest, &cli.source);
            }
        }
    }
    Ok(())
}

fn run_process(
    cli: &Cli,
    cache_args: &CacheArgs,
    json_mode: bool,
    ndjson: bool,
    quiet: bool,
) -> Result<(), CliError> {
    let scan_manifest_path = cli.temp_dir.join("manifest.json");
    let manifest_content = std::fs::read_to_string(&scan_manifest_path).tag(ErrorKind::Io)?;
    let input_manifest: serde_json::Value =
        serde_json::from_str(&manifest_content).tag(ErrorKind::Internal)?;
    let site_config: config::SiteConfig =
        serde_json::from_value(input_manifest.get("config").cloned().unwrap_or_default())
            .tag(ErrorKind::Config)?;
    init_thread_pool(&site_config.processing);
    let processed_dir = cli.temp_dir.join("processed");
    let (tx, rx) = std::sync::mpsc::channel();
    let suppress = (json_mode && !ndjson) || quiet;
    let printer = std::thread::spawn(move || {
        for event in rx {
            if ndjson {
                json_output::emit_ndjson_progress(&event).ok();
            } else if !suppress {
                for line in output::format_process_event(&event) {
                    println!("{}", line);
                }
            }
        }
    });
    let result = process::process(
        &scan_manifest_path,
        &cli.source,
        &processed_dir,
        !cache_args.no_cache,
        Some(tx),
    )
    .tag(ErrorKind::Process)?;
    join_printer(printer)?;
    let output_manifest_path = processed_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&result.manifest).tag(ErrorKind::Internal)?;
    std::fs::write(&output_manifest_path, &json).tag(ErrorKind::Io)?;

    if json_mode {
        let payload = ProcessPayload {
            processed_dir: processed_dir.clone(),
            manifest_path: output_manifest_path,
            cache: (&result.cache_stats).into(),
        };
        emit_json_result(ndjson, &OkEnvelope::new("process", payload))?;
    } else if !quiet {
        println!("Cache: {}", result.cache_stats);
    }
    Ok(())
}

fn run_generate(cli: &Cli, json_mode: bool, ndjson: bool, quiet: bool) -> Result<(), CliError> {
    let processed_dir = cli.temp_dir.join("processed");
    let processed_manifest_path = processed_dir.join("manifest.json");
    generate::generate(
        &processed_manifest_path,
        &processed_dir,
        &cli.output,
        &cli.source,
    )
    .tag(ErrorKind::Generate)?;
    let manifest_content = std::fs::read_to_string(&processed_manifest_path).tag(ErrorKind::Io)?;
    let manifest: generate::Manifest =
        serde_json::from_str(&manifest_content).tag(ErrorKind::Internal)?;

    if json_mode {
        let payload = GeneratePayload::new(&manifest, &cli.output);
        emit_json_result(ndjson, &OkEnvelope::new("generate", payload))?;
    } else if !quiet {
        output::print_generate_output(&manifest);
    }
    Ok(())
}

fn run_build(
    cli: &Cli,
    cache_args: &CacheArgs,
    json_mode: bool,
    ndjson: bool,
    quiet: bool,
) -> Result<(), CliError> {
    let source = resolve_build_source(&cli.source);
    let stage_text = !json_mode && !quiet;

    std::fs::create_dir_all(&cli.temp_dir).tag(ErrorKind::Io)?;

    if stage_text {
        println!("==> Stage 1: Scanning {}", source.display());
    }
    let manifest = scan::scan(&source).tag(ErrorKind::Scan)?;
    let scan_manifest_path = cli.temp_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&manifest).tag(ErrorKind::Internal)?;
    std::fs::write(&scan_manifest_path, &json).tag(ErrorKind::Io)?;
    if stage_text {
        output::print_scan_output(&manifest, &source);
        println!("==> Stage 2: Processing images");
    }

    init_thread_pool(&manifest.config.processing);
    let processed_dir = cli.temp_dir.join("processed");
    let (tx, rx) = std::sync::mpsc::channel();
    let suppress = !stage_text && !ndjson;
    let printer = std::thread::spawn(move || {
        for event in rx {
            if ndjson {
                json_output::emit_ndjson_progress(&event).ok();
            } else if !suppress {
                for line in output::format_process_event(&event) {
                    println!("{}", line);
                }
            }
        }
    });
    let result = process::process(
        &scan_manifest_path,
        &source,
        &processed_dir,
        !cache_args.no_cache,
        Some(tx),
    )
    .tag(ErrorKind::Process)?;
    join_printer(printer)?;
    let processed_manifest_path = processed_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&result.manifest).tag(ErrorKind::Internal)?;
    std::fs::write(&processed_manifest_path, &json).tag(ErrorKind::Io)?;
    if stage_text {
        println!("Cache: {}", result.cache_stats);
        println!("==> Stage 3: Generating HTML → {}", cli.output.display());
    }

    generate::generate(
        &processed_manifest_path,
        &processed_dir,
        &cli.output,
        &source,
    )
    .tag(ErrorKind::Generate)?;
    let gen_manifest_content =
        std::fs::read_to_string(&processed_manifest_path).tag(ErrorKind::Io)?;
    let gen_manifest: generate::Manifest =
        serde_json::from_str(&gen_manifest_content).tag(ErrorKind::Internal)?;

    if stage_text {
        output::print_generate_output(&gen_manifest);
        println!("==> Build complete: {}", cli.output.display());
    }

    if json_mode {
        let image_pages: usize = gen_manifest.albums.iter().map(|a| a.images.len()).sum();
        let pages_count = gen_manifest.pages.iter().filter(|p| !p.is_link).count();
        let payload = BuildPayload {
            source: &source,
            output: &cli.output,
            counts: simple_gal::json_output::GenerateCounts {
                albums: gen_manifest.albums.len(),
                image_pages,
                pages: pages_count,
            },
            cache: CacheStatsPayload::from(&result.cache_stats),
        };
        emit_json_result(ndjson, &OkEnvelope::new("build", payload))?;
    }
    Ok(())
}

fn run_check(cli: &Cli, json_mode: bool, ndjson: bool, quiet: bool) -> Result<(), CliError> {
    let source = resolve_build_source(&cli.source);
    if !json_mode && !quiet {
        println!("==> Checking {}", source.display());
    }
    let manifest = scan::scan(&source).tag(ErrorKind::Scan)?;
    if !json_mode && !quiet {
        output::print_scan_output(&manifest, &source);
        println!("==> Content is valid");
    }
    if json_mode {
        let images = manifest.albums.iter().map(|a| a.images.len()).sum();
        let payload = CheckPayload {
            valid: true,
            source: &source,
            counts: Counts {
                albums: manifest.albums.len(),
                images,
                pages: manifest.pages.len(),
            },
        };
        emit_json_result(ndjson, &OkEnvelope::new("check", payload))?;
    }
    Ok(())
}

/// Dispatch the `simple-gal config <action>` subcommand group through
/// clapfig. clapfig owns gen / schema / list / get / set / unset; we wrap
/// the typed `ConfigResult` it returns in our JSON envelope when
/// `--format json` is in effect, otherwise print it via `Display`.
fn run_config(cli: &Cli, args: &ConfigArgs, json_mode: bool, ndjson: bool) -> Result<(), CliError> {
    // ConfigArgs::into_action takes self, but we only have a &ConfigArgs
    // (we never own the Cli value). Mirror its dispatch by hand so we can
    // build a fresh ConfigAction without consuming the args.
    let action = config_action_from_args(args);

    let builder = Clapfig::builder::<SiteConfig>()
        .app_name("simple-gal")
        .file_name("config.toml")
        .search_paths(vec![SearchPath::Path(cli.source.clone())])
        .no_env()
        .post_validate(|c: &SiteConfig| c.validate().map_err(|e| e.to_string()));

    let result = builder.handle(&action).tag(ErrorKind::Config)?;

    if json_mode {
        let payload = ConfigOpPayload::from_result(&result);
        emit_json_result(ndjson, &OkEnvelope::new("config", payload))?;
    } else {
        println!("{result}");
    }
    Ok(())
}

/// Borrow-friendly mirror of [`ConfigArgs::into_action`].
///
/// `ConfigArgs` doesn't implement `Clone`, so we can't use the upstream
/// helper from a `&ConfigArgs`. Re-implementing the match by hand is
/// trivial and lets the rest of `run` keep its `&Cli` shape.
fn config_action_from_args(args: &ConfigArgs) -> ConfigAction {
    match args.action.as_ref() {
        None | Some(ConfigSubcommand::List) => ConfigAction::List {
            scope: args.scope.clone(),
        },
        Some(ConfigSubcommand::Gen { output }) => ConfigAction::Gen {
            output: output.clone(),
        },
        Some(ConfigSubcommand::Schema { output }) => ConfigAction::Schema {
            output: output.clone(),
        },
        Some(ConfigSubcommand::Get { key }) => ConfigAction::Get {
            key: key.clone(),
            scope: args.scope.clone(),
        },
        Some(ConfigSubcommand::Set { key, value }) => ConfigAction::Set {
            key: key.clone(),
            value: value.clone(),
            scope: args.scope.clone(),
        },
        Some(ConfigSubcommand::Unset { key }) => ConfigAction::Unset {
            key: key.clone(),
            scope: args.scope.clone(),
        },
    }
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
fn resolve_build_source(cli_source: &Path) -> PathBuf {
    cli_source.to_path_buf()
}
