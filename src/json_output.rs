//! Machine-readable JSON envelopes for every CLI command + the error path.
//!
//! Every command, when invoked with `--format json`, emits exactly one JSON
//! document to stdout (for success) or to stderr (for errors). These types
//! define the on-the-wire shape of those documents and are the automation
//! contract: GUIs and shell scripts parse them instead of scraping the
//! human-readable text output.
//!
//! `--format ndjson` extends this with streaming: progress events are emitted
//! as compact JSON lines (one per event, tagged `"type": "progress"`) as they
//! happen, followed by a final `"type": "result"` line with the same envelope
//! shape. Consumers read line-by-line and branch on the `type` field.

use crate::cache::CacheStats;
use crate::config::ConfigError;
use crate::generate;
use crate::scan;
use serde::Serialize;
use std::path::{Path, PathBuf};

// ============================================================================
// Error envelope
// ============================================================================

/// Classification of a CLI failure. Drives both the JSON `kind` field and
/// the process exit code so automated callers can branch on failure type
/// without parsing messages.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Config,
    Io,
    Scan,
    Process,
    Generate,
    Validation,
    Usage,
    Internal,
}

impl ErrorKind {
    /// Process exit code for this error kind. 0 is reserved for success;
    /// 2 is reserved for clap/usage errors (clap emits those directly).
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Internal => 1,
            ErrorKind::Usage => 2,
            ErrorKind::Config => 3,
            ErrorKind::Io => 4,
            ErrorKind::Scan => 5,
            ErrorKind::Process => 6,
            ErrorKind::Generate => 7,
            ErrorKind::Validation => 8,
        }
    }
}

/// Extra context for config-file parse failures so a GUI can highlight
/// the exact token without re-parsing.
#[derive(Debug, Serialize)]
pub struct ConfigErrorPayload {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// The top-level shape emitted to stderr when any command fails in JSON mode.
#[derive(Debug, Serialize)]
pub struct ErrorEnvelope {
    pub ok: bool,
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigErrorPayload>,
}

impl ErrorEnvelope {
    pub fn new(kind: ErrorKind, err: &(dyn std::error::Error + 'static)) -> Self {
        let message = err.to_string();
        let mut causes = Vec::new();
        let mut src = err.source();
        while let Some(cause) = src {
            causes.push(cause.to_string());
            src = cause.source();
        }
        // Only attach a `config` payload for parse-location-carrying
        // variants (currently `ConfigError::Toml`). Validation/IO config
        // errors have no file position, so we leave the field unset
        // instead of emitting an empty `path` that would confuse clients.
        let config = find_config_error(err).and_then(config_error_payload);
        Self {
            ok: false,
            kind,
            message,
            causes,
            config,
        }
    }
}

fn find_config_error<'a>(err: &'a (dyn std::error::Error + 'static)) -> Option<&'a ConfigError> {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(err);
    while let Some(e) = current {
        if let Some(cfg) = e.downcast_ref::<ConfigError>() {
            return Some(cfg);
        }
        current = e.source();
    }
    None
}

fn config_error_payload(cfg: &ConfigError) -> Option<ConfigErrorPayload> {
    match cfg {
        ConfigError::Toml {
            path,
            source,
            source_text,
        } => {
            let (line, column) = source
                .span()
                .map(|span| offset_to_line_col(source_text, span.start))
                .unwrap_or((None, None));
            let snippet = source
                .span()
                .and_then(|span| extract_snippet(source_text, span.start));
            Some(ConfigErrorPayload {
                path: path.clone(),
                line,
                column,
                snippet,
            })
        }
        // Validation / IO config errors carry no file position — skip
        // the payload entirely rather than emit an empty `path`.
        _ => None,
    }
}

fn offset_to_line_col(text: &str, offset: usize) -> (Option<usize>, Option<usize>) {
    let offset = offset.min(text.len());
    let prefix = &text[..offset];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let col = prefix.rfind('\n').map(|i| offset - i - 1).unwrap_or(offset) + 1;
    (Some(line), Some(col))
}

fn extract_snippet(text: &str, offset: usize) -> Option<String> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(text.len());
    Some(text[start..end].to_string())
}

// ============================================================================
// Success envelopes
// ============================================================================

/// Wrapper written to stdout for every successful command in JSON mode.
/// The `command` tag lets a GUI dispatch on the payload shape.
#[derive(Debug, Serialize)]
pub struct OkEnvelope<T: Serialize> {
    pub ok: bool,
    pub command: &'static str,
    pub data: T,
}

impl<T: Serialize> OkEnvelope<T> {
    pub fn new(command: &'static str, data: T) -> Self {
        Self {
            ok: true,
            command,
            data,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub albums: usize,
    pub images: usize,
    pub pages: usize,
}

// ----- scan -----

#[derive(Debug, Serialize)]
pub struct ScanPayload<'a> {
    pub source: &'a Path,
    pub counts: Counts,
    pub manifest: &'a scan::Manifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_manifest_path: Option<PathBuf>,
}

impl<'a> ScanPayload<'a> {
    pub fn new(
        manifest: &'a scan::Manifest,
        source: &'a Path,
        saved_manifest_path: Option<PathBuf>,
    ) -> Self {
        let images = manifest.albums.iter().map(|a| a.images.len()).sum();
        Self {
            source,
            counts: Counts {
                albums: manifest.albums.len(),
                images,
                pages: manifest.pages.len(),
            },
            manifest,
            saved_manifest_path,
        }
    }
}

// ----- process -----

#[derive(Debug, Serialize)]
pub struct CacheStatsPayload {
    pub cached: u32,
    pub copied: u32,
    pub encoded: u32,
    pub total: u32,
}

impl From<&CacheStats> for CacheStatsPayload {
    fn from(s: &CacheStats) -> Self {
        Self {
            cached: s.hits,
            copied: s.copies,
            encoded: s.misses,
            total: s.total(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ProcessPayload {
    pub processed_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub cache: CacheStatsPayload,
}

// ----- generate -----

#[derive(Debug, Serialize)]
pub struct GeneratePayload<'a> {
    pub output: &'a Path,
    pub counts: GenerateCounts,
    pub albums: Vec<GeneratedAlbum>,
    pub pages: Vec<GeneratedPage>,
}

#[derive(Debug, Serialize)]
pub struct GenerateCounts {
    pub albums: usize,
    pub image_pages: usize,
    pub pages: usize,
}

#[derive(Debug, Serialize)]
pub struct GeneratedAlbum {
    pub title: String,
    pub path: String,
    pub index_html: String,
    pub image_count: usize,
}

#[derive(Debug, Serialize)]
pub struct GeneratedPage {
    pub title: String,
    pub slug: String,
    pub is_link: bool,
}

impl<'a> GeneratePayload<'a> {
    pub fn new(manifest: &'a generate::Manifest, output: &'a Path) -> Self {
        let image_pages = manifest.albums.iter().map(|a| a.images.len()).sum();
        let pages_count = manifest.pages.iter().filter(|p| !p.is_link).count();
        let albums = manifest
            .albums
            .iter()
            .map(|a| GeneratedAlbum {
                title: a.title.clone(),
                path: a.path.clone(),
                index_html: format!("{}/index.html", a.path),
                image_count: a.images.len(),
            })
            .collect();
        let pages = manifest
            .pages
            .iter()
            .map(|p| GeneratedPage {
                title: p.title.clone(),
                slug: p.slug.clone(),
                is_link: p.is_link,
            })
            .collect();
        Self {
            output,
            counts: GenerateCounts {
                albums: manifest.albums.len(),
                image_pages,
                pages: pages_count,
            },
            albums,
            pages,
        }
    }
}

// ----- build -----

#[derive(Debug, Serialize)]
pub struct BuildPayload<'a> {
    pub source: &'a Path,
    pub output: &'a Path,
    pub counts: GenerateCounts,
    pub cache: CacheStatsPayload,
}

// ----- check -----

#[derive(Debug, Serialize)]
pub struct CheckPayload<'a> {
    pub valid: bool,
    pub source: &'a Path,
    pub counts: Counts,
}

// ----- config -----

/// JSON envelope for any `simple-gal config <action>` invocation.
///
/// Mirrors clapfig's [`ConfigResult`][clapfig::ConfigResult] but flattens
/// each variant into a tagged `action` so consumers can branch on a single
/// field without parsing free-form text.
#[derive(Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ConfigOpPayload {
    /// `config gen` (printed to stdout).
    Gen { toml: String },
    /// `config gen --output PATH` (written to a file).
    GenWritten { path: PathBuf },
    /// `config schema` (printed to stdout).
    Schema { schema: serde_json::Value },
    /// `config schema --output PATH` (written to a file).
    SchemaWritten { path: PathBuf },
    /// `config get KEY`.
    Get {
        key: String,
        value: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        doc: Vec<String>,
    },
    /// `config set KEY VALUE`.
    Set { key: String, value: String },
    /// `config unset KEY`.
    Unset { key: String },
    /// `config` / `config list` — flat key/value listing.
    List { entries: Vec<ConfigListEntry> },
}

/// One row of a `config list` listing.
#[derive(Debug, Serialize)]
pub struct ConfigListEntry {
    pub key: String,
    pub value: String,
}

impl ConfigOpPayload {
    /// Convert clapfig's `ConfigResult` into the wire envelope.
    ///
    /// For `Schema`, the JSON string clapfig produced is re-parsed into a
    /// `serde_json::Value` so the schema lands as structured JSON inside
    /// the envelope (rather than as a string-of-JSON that consumers would
    /// have to double-parse).
    pub fn from_result(result: &clapfig::ConfigResult) -> Self {
        use clapfig::ConfigResult as R;
        match result {
            R::Template(t) => ConfigOpPayload::Gen { toml: t.clone() },
            R::TemplateWritten { path } => ConfigOpPayload::GenWritten { path: path.clone() },
            R::Schema(s) => ConfigOpPayload::Schema {
                // Schema strings are produced by serde_json::to_string_pretty
                // upstream, so re-parsing is infallible in practice.
                schema: serde_json::from_str(s)
                    .unwrap_or_else(|_| serde_json::Value::String(s.clone())),
            },
            R::SchemaWritten { path } => ConfigOpPayload::SchemaWritten { path: path.clone() },
            R::KeyValue { key, value, doc } => ConfigOpPayload::Get {
                key: key.clone(),
                value: value.clone(),
                doc: doc.clone(),
            },
            R::ValueSet { key, value } => ConfigOpPayload::Set {
                key: key.clone(),
                value: value.clone(),
            },
            R::ValueUnset { key } => ConfigOpPayload::Unset { key: key.clone() },
            R::Listing { entries } => ConfigOpPayload::List {
                entries: entries
                    .iter()
                    .map(|(k, v)| ConfigListEntry {
                        key: k.clone(),
                        value: v.clone(),
                    })
                    .collect(),
            },
        }
    }
}

// ============================================================================
// Helpers for writing envelopes
// ============================================================================

/// Serialize `value` to pretty JSON on stdout, followed by a newline.
/// Returns the serde error so the caller can route a serialization
/// failure through the normal error envelope + exit-code path — we never
/// want to print a truncated document and silently exit 0.
pub fn emit_stdout<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{s}");
    Ok(())
}

/// Serialize `value` to pretty JSON on stderr, followed by a newline. Used
/// for error envelopes so stdout stays clean on failure.
pub fn emit_stderr<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let s = serde_json::to_string_pretty(value)?;
    eprintln!("{s}");
    Ok(())
}

/// Serialize `value` to compact JSON on stderr, followed by a newline. Used
/// for error envelopes in NDJSON mode.
pub fn emit_stderr_compact<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let s = serde_json::to_string(value)?;
    eprintln!("{s}");
    Ok(())
}

// ============================================================================
// NDJSON (newline-delimited JSON) streaming helpers
// ============================================================================
//
// In `--format ndjson` mode, each line on stdout is a self-contained JSON
// object. Progress events stream as they happen (one per line), and the
// final line is the result envelope. Every line carries a `"type"` field
// so consumers can branch without parsing the full shape:
//
//   {"type":"progress","event":"album_started","title":"Landscapes","image_count":5}
//   {"type":"progress","event":"image_processed","index":1, ...}
//   {"type":"result","ok":true,"command":"build","data":{...}}

/// Wrapper that tags a progress event with `"type": "progress"` for NDJSON.
#[derive(Serialize)]
struct NdjsonProgress<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    event: &'a crate::process::ProcessEvent,
}

/// Wrapper that tags a result envelope with `"type": "result"` for NDJSON.
#[derive(Serialize)]
struct NdjsonResult<'a, T: Serialize> {
    r#type: &'static str,
    #[serde(flatten)]
    envelope: &'a T,
}

/// Emit a `ProcessEvent` as a single compact JSON line on stdout,
/// tagged with `"type": "progress"`.
pub fn emit_ndjson_progress(event: &crate::process::ProcessEvent) -> Result<(), serde_json::Error> {
    let wrapped = NdjsonProgress {
        r#type: "progress",
        event,
    };
    let s = serde_json::to_string(&wrapped)?;
    println!("{s}");
    Ok(())
}

/// Emit a result envelope as a single compact JSON line on stdout,
/// tagged with `"type": "result"`. This is always the last line in
/// an NDJSON stream.
pub fn emit_ndjson_result<T: Serialize>(envelope: &T) -> Result<(), serde_json::Error> {
    let wrapped = NdjsonResult {
        r#type: "result",
        envelope,
    };
    let s = serde_json::to_string(&wrapped)?;
    println!("{s}");
    Ok(())
}

// ============================================================================
// Structured progress tracking (`--format progress`)
// ============================================================================
//
// Pre-computes percent-complete, stage, and image/variant counters so GUI
// consumers can drive a progress bar without interpreting raw events.
//
// Weight model (empirically measured):
//   scan     =  2%  — filesystem walk, near-instant
//   process  = 90%  — image encoding dominates wall time
//   generate =  8%  — HTML templating + file writes
//
// Within process, the unit of progress is one image variant (a responsive
// size or thumbnail). `ProgressTracker` estimates total variants from the
// config upfront, then increments as `ImageProcessed` events arrive.

const SCAN_WEIGHT: f64 = 2.0;
const PROCESS_WEIGHT: f64 = 90.0;
// generate weight is implicitly 100 - SCAN_WEIGHT - PROCESS_WEIGHT = 8.0

/// Pipeline stage reported in progress events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Scan,
    Process,
    Generate,
}

/// A single progress line emitted in `--format progress` mode.
#[derive(Debug, Serialize)]
pub struct ProgressEvent {
    pub r#type: &'static str,
    pub stage: Stage,
    /// Overall percent complete, 0.0–100.0.
    pub percent: f64,
    pub images_done: usize,
    pub images_total: usize,
    pub variants_done: usize,
    pub variants_total: usize,
}

/// Tracks build progress across the three pipeline stages.
///
/// Created after scan completes (when totals are known). Call
/// [`on_image_processed`] for each `ProcessEvent::ImageProcessed` to
/// advance the counters.
pub struct ProgressTracker {
    pub images_total: usize,
    pub variants_total: usize,
    images_done: usize,
    variants_done: usize,
}

impl ProgressTracker {
    /// Create a tracker from the scan results.
    ///
    /// `variants_per_image` is the config-based estimate:
    /// `responsive_sizes.len() + 1 (thumbnail) + full_index_flag`.
    pub fn new(images_total: usize, variants_per_image: usize) -> Self {
        Self {
            images_total,
            variants_total: images_total * variants_per_image,
            images_done: 0,
            variants_done: 0,
        }
    }

    /// Build the scan-complete progress event (stage boundary).
    pub fn scan_complete(&self) -> ProgressEvent {
        ProgressEvent {
            r#type: "progress",
            stage: Stage::Scan,
            percent: SCAN_WEIGHT,
            images_done: 0,
            images_total: self.images_total,
            variants_done: 0,
            variants_total: self.variants_total,
        }
    }

    /// Record a completed image and return the updated progress event.
    ///
    /// `variant_count` is the actual number of variants this image produced
    /// (taken from `ProcessEvent::ImageProcessed.variants.len()`).
    pub fn on_image_processed(&mut self, variant_count: usize) -> ProgressEvent {
        self.images_done += 1;
        self.variants_done += variant_count;

        let fraction = if self.variants_total > 0 {
            (self.variants_done as f64 / self.variants_total as f64).min(1.0)
        } else {
            1.0
        };
        let percent = SCAN_WEIGHT + fraction * PROCESS_WEIGHT;

        ProgressEvent {
            r#type: "progress",
            stage: Stage::Process,
            percent,
            images_done: self.images_done,
            images_total: self.images_total,
            variants_done: self.variants_done,
            variants_total: self.variants_total,
        }
    }

    /// Build the generate-started progress event (stage boundary).
    pub fn generate_started(&self) -> ProgressEvent {
        ProgressEvent {
            r#type: "progress",
            stage: Stage::Generate,
            percent: SCAN_WEIGHT + PROCESS_WEIGHT,
            images_done: self.images_done,
            images_total: self.images_total,
            variants_done: self.variants_done,
            variants_total: self.variants_total,
        }
    }
}

/// Emit a progress event as a compact JSON line on stdout.
pub fn emit_progress(event: &ProgressEvent) -> Result<(), serde_json::Error> {
    let s = serde_json::to_string(event)?;
    println!("{s}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_distinct() {
        let kinds = [
            ErrorKind::Internal,
            ErrorKind::Usage,
            ErrorKind::Config,
            ErrorKind::Io,
            ErrorKind::Scan,
            ErrorKind::Process,
            ErrorKind::Generate,
            ErrorKind::Validation,
        ];
        let codes: Vec<i32> = kinds.iter().map(|k| k.exit_code()).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "exit codes must be unique");
        assert!(!codes.contains(&0), "0 is reserved for success");
    }

    #[test]
    fn error_envelope_collects_causes() {
        use std::io;
        let err = io::Error::other("outer");
        let env = ErrorEnvelope::new(ErrorKind::Io, &err);
        assert!(!env.ok);
        assert_eq!(env.message, "outer");
    }

    #[test]
    fn offset_to_line_col_first_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 3);
        assert_eq!(line, Some(1));
        assert_eq!(col, Some(4));
    }

    #[test]
    fn offset_to_line_col_second_line() {
        let (line, col) = offset_to_line_col("hello\nworld", 8);
        assert_eq!(line, Some(2));
        assert_eq!(col, Some(3));
    }

    // =========================================================================
    // ProgressTracker tests
    // =========================================================================

    #[test]
    fn progress_scan_complete_is_2_percent() {
        let tracker = ProgressTracker::new(10, 4);
        let ev = tracker.scan_complete();
        assert_eq!(ev.stage, Stage::Scan);
        assert!((ev.percent - 2.0).abs() < f64::EPSILON);
        assert_eq!(ev.images_total, 10);
        assert_eq!(ev.variants_total, 40);
        assert_eq!(ev.images_done, 0);
        assert_eq!(ev.variants_done, 0);
    }

    #[test]
    fn progress_first_image_advances_correctly() {
        let mut tracker = ProgressTracker::new(10, 4); // 40 variants
        let ev = tracker.on_image_processed(4);
        assert_eq!(ev.stage, Stage::Process);
        assert_eq!(ev.images_done, 1);
        assert_eq!(ev.variants_done, 4);
        // 2 + (4/40)*90 = 2 + 9 = 11
        assert!((ev.percent - 11.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_all_images_reaches_92_percent() {
        let mut tracker = ProgressTracker::new(3, 4); // 12 variants
        tracker.on_image_processed(4);
        tracker.on_image_processed(4);
        let ev = tracker.on_image_processed(4);
        assert_eq!(ev.images_done, 3);
        assert_eq!(ev.variants_done, 12);
        // 2 + (12/12)*90 = 92
        assert!((ev.percent - 92.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_generate_started_is_92_percent() {
        let mut tracker = ProgressTracker::new(2, 3); // 6 variants
        tracker.on_image_processed(3);
        tracker.on_image_processed(3);
        let ev = tracker.generate_started();
        assert_eq!(ev.stage, Stage::Generate);
        assert!((ev.percent - 92.0).abs() < f64::EPSILON);
        assert_eq!(ev.images_done, 2);
    }

    #[test]
    fn progress_fewer_variants_than_estimated_clamps() {
        // Images produce fewer variants than the config estimate
        let mut tracker = ProgressTracker::new(2, 4); // estimate: 8 variants
        tracker.on_image_processed(2); // actual: 2
        tracker.on_image_processed(2); // actual: 2, total: 4 out of 8
        let ev = tracker.generate_started();
        // variants_done=4 < variants_total=8, so process didn't fill 90%
        // generate_started clamps to 92% regardless
        assert!((ev.percent - 92.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_zero_images() {
        let tracker = ProgressTracker::new(0, 4);
        assert_eq!(tracker.variants_total, 0);
        let ev = tracker.scan_complete();
        assert!((ev.percent - 2.0).abs() < f64::EPSILON);
    }
}
