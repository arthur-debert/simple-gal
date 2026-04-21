use clap::{Parser, Subcommand, ValueEnum};
use clapfig::{Clapfig, ConfigAction, ConfigArgs, ConfigSubcommand, SearchPath};
use serde::Serialize;
use simple_gal::config::SiteConfig;
use simple_gal::json_output::{
    self, BuildPayload, CacheStatsPayload, CheckPayload, ConfigOpPayload, Counts, ErrorEnvelope,
    ErrorKind, GeneratePayload, OkEnvelope, ProcessPayload, ReindexPayload, ScanPayload,
};
use simple_gal::{config, generate, output, process, reindex, scan};
use std::io::{IsTerminal, Write};
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
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    /// Newline-delimited JSON: one JSON object per line. Raw progress
    /// events stream as they happen; the final line is the result envelope.
    Ndjson,
    /// Structured progress stream: NDJSON lines with pre-computed percent,
    /// stage, and image/variant counters. Designed for GUI progress bars.
    Progress,
}

/// Arguments for the scan command.
#[derive(clap::Args, Clone)]
struct ScanArgs {
    /// Save the JSON manifest to a file.
    /// When passed without a value, uses <temp-dir>/manifest.json.
    #[arg(long, num_args = 0..=1, default_missing_value = "__default__")]
    save_manifest: Option<PathBuf>,
}

/// Arguments for the `reindex` command.
///
/// `spacing` and `padding` default to the `[auto_indexing]` values in the
/// loaded config (default step-of-10, 3-wide). Unset flags fall through
/// cleanly — explicit flags always win.
#[derive(clap::Args, Clone)]
struct ReindexArgs {
    /// Directory to reindex. Defaults to the content source (`--source`).
    path: Option<PathBuf>,
    /// Step exponent: numbers are spaced by 10^spacing (0 → 1,2,3; 1 → 10,20,30).
    #[arg(long)]
    spacing: Option<u32>,
    /// Zero-pad numeric prefix to this width. 0 = no padding.
    #[arg(long)]
    padding: Option<u32>,
    /// Only reindex the target directory. Without this flag the walker
    /// descends into numbered subdirectories.
    #[arg(long)]
    flat: bool,
    /// Print the rename plan without touching the filesystem.
    #[arg(long)]
    dry_run: bool,
    /// Skip the TTY confirmation prompt. Required on non-interactive
    /// stdin when the run is not `--dry-run`.
    #[arg(long, short = 'y')]
    yes: bool,
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
    /// Normalize `NNN-` prefixes on albums, groups, pages, and images
    Reindex(ReindexArgs),
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
    if matches!(
        format,
        OutputFormat::Json | OutputFormat::Ndjson | OutputFormat::Progress
    ) {
        let envelope = ErrorEnvelope::new(kind, err);
        let emit = if matches!(format, OutputFormat::Ndjson | OutputFormat::Progress) {
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
    let json_mode = matches!(
        format,
        OutputFormat::Json | OutputFormat::Ndjson | OutputFormat::Progress
    );
    let ndjson = matches!(format, OutputFormat::Ndjson | OutputFormat::Progress);
    let quiet = cli.quiet;

    match &cli.command {
        Command::Scan(args) => run_scan(cli, args, format),
        Command::Process(cache_args) => run_process(cli, cache_args, json_mode, ndjson, quiet),
        Command::Generate => run_generate(cli, json_mode, ndjson, quiet),
        Command::Build(cache_args) => run_build(cli, cache_args, format),
        Command::Check => run_check(cli, json_mode, ndjson, quiet),
        Command::Config(args) => run_config(cli, args, json_mode, ndjson),
        Command::Reindex(args) => run_reindex(cli, args, json_mode, ndjson, quiet),
    }
}

/// Emit a JSON result envelope: pretty-printed for `--format json`,
/// compact single-line for `--format ndjson` and `--format progress`.
/// The `compact` parameter is true for any NDJSON-like mode.
fn emit_json_result<T: Serialize>(compact: bool, value: &T) -> Result<(), CliError> {
    if compact {
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
        OutputFormat::Json | OutputFormat::Ndjson | OutputFormat::Progress => {
            let payload = ScanPayload::new(&manifest, &cli.source, saved_path);
            emit_json_result(
                format != OutputFormat::Json,
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
    let process_result = process::process(
        &scan_manifest_path,
        &cli.source,
        &processed_dir,
        !cache_args.no_cache,
        Some(tx),
    )
    .tag(ErrorKind::Process);
    join_printer(printer)?;
    let result = process_result?;
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

fn run_build(cli: &Cli, cache_args: &CacheArgs, format: OutputFormat) -> Result<(), CliError> {
    let source = resolve_build_source(&cli.source);
    let json_mode = format != OutputFormat::Text;
    let ndjson = matches!(format, OutputFormat::Ndjson | OutputFormat::Progress);
    let progress_mode = format == OutputFormat::Progress;
    let stage_text = !json_mode && !cli.quiet;

    std::fs::create_dir_all(&cli.temp_dir).tag(ErrorKind::Io)?;

    // === Stage 0: Auto-reindex (opt-in via [auto_indexing].auto) ===
    maybe_auto_reindex(cli, &source, stage_text)?;

    // === Stage 1: Scan ===
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

    // Compute progress totals from scan results (used by --format progress).
    // Sum per-album: each album may have different sizes/full_index config.
    let total_images: usize = manifest.albums.iter().map(|a| a.images.len()).sum();
    let variants_total: usize = manifest
        .albums
        .iter()
        .map(|a| {
            let variants_per = a.config.images.sizes.len()
            + 1 // thumbnail
            + usize::from(a.config.full_index.generates); // optional full-index thumbnail
            a.images.len() * variants_per
        })
        .sum();

    // Emit scan-complete progress event.
    if progress_mode {
        let tracker = json_output::ProgressTracker::with_totals(total_images, variants_total);
        json_output::emit_progress(&tracker.scan_complete()).ok();
    }

    // === Stage 2: Process ===
    init_thread_pool(&manifest.config.processing);
    let processed_dir = cli.temp_dir.join("processed");
    let (tx, rx) = std::sync::mpsc::channel();
    let suppress = !stage_text && !ndjson;
    let printer = std::thread::spawn(move || {
        let mut tracker = if progress_mode {
            Some(json_output::ProgressTracker::with_totals(
                total_images,
                variants_total,
            ))
        } else {
            None
        };
        for event in rx {
            if let Some(ref mut t) = tracker {
                if let process::ProcessEvent::ImageProcessed { ref variants, .. } = event {
                    let ev = t.on_image_processed(variants.len());
                    json_output::emit_progress(&ev).ok();
                }
            } else if ndjson {
                json_output::emit_ndjson_progress(&event).ok();
            } else if !suppress {
                for line in output::format_process_event(&event) {
                    println!("{}", line);
                }
            }
        }
        // Return the tracker so we can use it for the generate stage.
        tracker
    });
    let process_result = process::process(
        &scan_manifest_path,
        &source,
        &processed_dir,
        !cache_args.no_cache,
        Some(tx),
    )
    .tag(ErrorKind::Process);
    // Always join the printer thread before propagating a process error,
    // so output is flushed and the thread doesn't outlive the channel.
    let tracker = printer.join().map_err(|_| {
        let msg: Box<dyn std::error::Error + 'static> = "progress printer thread panicked".into();
        CliError::new(ErrorKind::Internal, msg)
    })?;
    let result = process_result?;
    let processed_manifest_path = processed_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&result.manifest).tag(ErrorKind::Internal)?;
    std::fs::write(&processed_manifest_path, &json).tag(ErrorKind::Io)?;
    if stage_text {
        println!("Cache: {}", result.cache_stats);
        println!("==> Stage 3: Generating HTML → {}", cli.output.display());
    }

    // Emit generate-started progress event.
    if let Some(ref t) = tracker {
        json_output::emit_progress(&t.generate_started()).ok();
    }

    // === Stage 3: Generate ===
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

/// Pre-scan hook: consult `[auto_indexing].auto` and reindex the source
/// tree if the user has opted in.
///
/// Behavior per mode:
///
/// - `off` (default): no-op.
/// - `source_only` / `both`: walk the source tree through `reindex::reindex_tree`
///   with the configured `spacing`/`padding`. Any on-disk rename invalidates
///   the processing cache (by removing `<temp-dir>/processed/`); the
///   subsequent process stage rebuilds from scratch. Cache invalidation
///   here is deliberately coarse — per-file content-addressed caching could
///   preserve more state across renames, but that's a follow-up optimization.
/// - `export_only`: not yet implemented. Returns an error telling the user
///   to use the manual `simple-gal reindex` command or switch modes; the
///   source-untouched manifest-rewrite path lands in a later release.
fn maybe_auto_reindex(cli: &Cli, source: &Path, text_mode: bool) -> Result<(), CliError> {
    let site_config = config::load_config(&cli.source).tag(ErrorKind::Config)?;
    let auto = site_config.auto_indexing.auto.clone();
    let spacing = site_config.auto_indexing.spacing;
    let padding = site_config.auto_indexing.padding;

    use config::AutoIndexingMode;
    match auto {
        AutoIndexingMode::Off => Ok(()),
        AutoIndexingMode::SourceOnly | AutoIndexingMode::Both => {
            if text_mode {
                println!(
                    "==> Stage 0: Auto-reindex (mode=source_only, spacing={spacing}, padding={padding})"
                );
            }
            let opts = reindex::WalkOptions {
                is_root: true,
                assets_dir: Some(site_config.assets_dir.as_str()),
                site_description_file: site_config.site_description_file.as_str(),
            };
            let reports = reindex::reindex_tree(source, spacing, padding, false, false, &opts)
                .tag(ErrorKind::Reindex)?;
            let total_renames: usize = reports.iter().map(|r| r.plan.len()).sum();
            if text_mode {
                if total_renames == 0 {
                    println!("  (already normalized)");
                } else {
                    println!(
                        "  {total_renames} rename(s) across {} director{}.",
                        reports.iter().filter(|r| !r.plan.is_empty()).count(),
                        if reports.iter().filter(|r| !r.plan.is_empty()).count() == 1 {
                            "y"
                        } else {
                            "ies"
                        }
                    );
                }
            }
            // Coarse cache invalidation: if anything on disk moved, throw
            // away the processed dir so the process stage can't hand back
            // stale variants keyed by old paths. See doc-comment above.
            if total_renames > 0 {
                let processed_dir = cli.temp_dir.join("processed");
                if processed_dir.exists() {
                    if text_mode {
                        println!(
                            "  Invalidating processing cache ({})",
                            processed_dir.display()
                        );
                    }
                    std::fs::remove_dir_all(&processed_dir).tag(ErrorKind::Io)?;
                }
            }
            Ok(())
        }
        AutoIndexingMode::ExportOnly => {
            let msg: Box<dyn std::error::Error + 'static> =
                "auto_indexing.auto = \"export_only\" is not yet supported. \
                 Use \"source_only\" (renames source files in place) or run \
                 `simple-gal reindex` manually; export-only manifest rewrite \
                 lands in a follow-up release."
                    .into();
            Err(CliError::new(ErrorKind::Config, msg))
        }
    }
}

/// Run `simple-gal reindex`.
///
/// Flag → config → compiled-default precedence for `spacing`/`padding` so
/// users can set defaults in `config.toml` and override them ad-hoc on the
/// command line. A TTY + non-`--dry-run` + non-`--yes` run pauses for
/// confirmation; piped stdin without `--yes` is rejected to avoid silent
/// renames when a script forgets the flag.
fn run_reindex(
    cli: &Cli,
    args: &ReindexArgs,
    json_mode: bool,
    ndjson: bool,
    quiet: bool,
) -> Result<(), CliError> {
    // Target: positional PATH wins over --source; default to --source.
    let target = args.path.clone().unwrap_or_else(|| cli.source.clone());

    // Load the config cascade at the content root so defaults and
    // assets_dir / site_description_file are honored.
    let site_config = config::load_config(&cli.source).tag(ErrorKind::Config)?;

    // CLI flags override config values which override compiled defaults.
    let spacing = args.spacing.unwrap_or(site_config.auto_indexing.spacing);
    let padding = args.padding.unwrap_or(site_config.auto_indexing.padding);

    let opts = reindex::WalkOptions {
        is_root: target == cli.source,
        assets_dir: Some(site_config.assets_dir.as_str()),
        site_description_file: site_config.site_description_file.as_str(),
    };

    // Plan first (dry pass) so we can show the user what's about to happen.
    let planned = reindex::reindex_tree(&target, spacing, padding, args.flat, true, &opts)
        .tag(ErrorKind::Reindex)?;
    let total_renames: usize = planned.iter().map(|r| r.plan.len()).sum();

    // Text-mode preview & confirmation.
    if !json_mode && !quiet {
        print_reindex_plan(&target, &planned);
    }

    // Nothing to do.
    if total_renames == 0 {
        if json_mode {
            let payload = ReindexPayload::from_reports(&planned, args.dry_run, spacing, padding);
            emit_json_result(ndjson, &OkEnvelope::new("reindex", payload))?;
        } else if !quiet {
            println!("==> Nothing to reindex.");
        }
        return Ok(());
    }

    // Dry-run stops here: we've already printed / emitted the plan.
    if args.dry_run {
        if json_mode {
            let payload = ReindexPayload::from_reports(&planned, true, spacing, padding);
            emit_json_result(ndjson, &OkEnvelope::new("reindex", payload))?;
        } else if !quiet {
            println!("==> Dry run — {total_renames} rename(s) planned, none applied.");
        }
        return Ok(());
    }

    // Confirmation gate. JSON mode skips the prompt (automation) and
    // assumes the flag is opt-in at the CLI level. Text mode prompts on
    // TTY; non-TTY without --yes is rejected.
    if !json_mode && !args.yes && !confirm_reindex(total_renames)? {
        println!("Aborted.");
        return Ok(());
    }
    if json_mode && !args.yes {
        // For JSON callers: require --yes explicitly so a misconfigured
        // script doesn't rewrite the user's content tree silently.
        let msg: Box<dyn std::error::Error + 'static> =
            "reindex in JSON mode requires --yes to confirm a destructive run".into();
        return Err(CliError::new(ErrorKind::Usage, msg));
    }

    // Apply the plan for real.
    let applied = reindex::reindex_tree(&target, spacing, padding, args.flat, false, &opts)
        .tag(ErrorKind::Reindex)?;

    if json_mode {
        let payload = ReindexPayload::from_reports(&applied, false, spacing, padding);
        emit_json_result(ndjson, &OkEnvelope::new("reindex", payload))?;
    } else if !quiet {
        let applied_count: usize = applied.iter().map(|r| r.plan.len()).sum();
        println!("==> Reindex complete — {applied_count} rename(s) applied.");
    }
    Ok(())
}

/// Human-readable dump of what reindex is about to do. Printed before the
/// confirmation prompt in text mode.
fn print_reindex_plan(target: &Path, reports: &[reindex::DirReport]) {
    println!("==> Reindex plan ({})", target.display());
    let mut total = 0usize;
    for r in reports {
        if r.plan.is_empty() {
            continue;
        }
        println!("  {}/", r.dir.display());
        for rn in &r.plan {
            println!("    {}  →  {}", rn.from, rn.to);
            total += 1;
        }
    }
    if total == 0 {
        println!("  (nothing to do)");
    } else {
        println!(
            "  {total} rename(s) across {} director{}.",
            reports.iter().filter(|r| !r.plan.is_empty()).count(),
            if reports.iter().filter(|r| !r.plan.is_empty()).count() == 1 {
                "y"
            } else {
                "ies"
            }
        );
    }
}

/// Interactive confirmation. Returns `Ok(true)` on 'y', `Ok(false)` on
/// anything else. Non-TTY stdin without `--yes` is rejected rather than
/// silently proceeding.
fn confirm_reindex(total_renames: usize) -> Result<bool, CliError> {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let msg: Box<dyn std::error::Error + 'static> =
            "reindex without --yes on non-interactive stdin refused to proceed".into();
        return Err(CliError::new(ErrorKind::Usage, msg));
    }
    print!("Apply {total_renames} rename(s)? [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).tag(ErrorKind::Io)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES"))
}
