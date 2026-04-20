//! Auto file-name index reindexing — planning, on-disk rename, walker.
//!
//! Three entry points:
//!
//! - [`plan_reindex`] — pure function. Takes a list of numbered entries and
//!   the spacing/padding parameters, returns the rename plan. No I/O.
//! - [`apply_plan`] — executes the rename plan via a two-phase rename
//!   (source → temp, then temp → target) so the plan is collision-safe.
//! - [`read_entries`] / [`reindex_tree`] — walks one directory (or a whole
//!   tree) to build the `Vec<Entry>` that drives `plan_reindex`, grouping
//!   image-with-sidecar bundles and skipping support files (`config.toml`,
//!   `description.*`, hidden files, and at root the site-description file
//!   + assets dir).
//!
//! See `docs/dev/auto-reindex.md` for the full feature spec.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::imaging;

/// One on-disk artifact belonging to an [`Entry`].
///
/// For most entries this is a single file or directory. For an image bundle
/// it includes the image plus any sidecars (`.txt` / `.md`) sharing the same
/// numeric stem — they all get renumbered together so the pairing the scan
/// stage relies on stays intact.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryMember {
    /// The on-disk basename, exactly as it appears in `readdir` (e.g.
    /// `"010-Dawn.jpg"`, `"010-Dawn.txt"`, or `"020-Travel"`).
    pub original_name: String,
    /// The portion of `original_name` that follows the entry's stem,
    /// including any leading `.` for files (e.g. `".jpg"`, `".txt"`, `""`
    /// for a directory).
    pub suffix: String,
}

/// One logical unit in a directory: an album, group, page, or image bundle.
///
/// Entries with `number == None` are unnumbered — `plan_reindex` skips them
/// and leaves them at their original position on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The numeric prefix parsed from the original name. `None` means the
    /// entry has no prefix and should be left alone.
    pub number: Option<u32>,
    /// The non-numeric portion of the original name, preserving the original
    /// case and dashes (e.g. `"My-Best-Photos"` from `"010-My-Best-Photos"`).
    /// May be empty for inputs like `"001"` or `"001-"`.
    pub stem: String,
    /// Files / directories that share this entry's number on disk.
    pub members: Vec<EntryMember>,
}

/// A planned rename within a single directory. `from` and `to` are basenames;
/// the walker prepends the directory when applying.
#[derive(Debug, Clone, PartialEq)]
pub struct Rename {
    pub from: String,
    pub to: String,
}

/// Plan the renames for a directory's worth of entries.
///
/// Order is preserved: numbered entries are assigned sequential numbers in
/// the order given, starting at `1 * 10^spacing`. Unnumbered entries are
/// skipped (no renames emitted for them). No-op renames (where the new name
/// equals the original) are filtered out.
///
/// The caller is responsible for sorting `entries` before calling — this
/// function trusts the input order.
pub fn plan_reindex(entries: &[Entry], spacing: u32, padding: u32) -> Vec<Rename> {
    // Do the step arithmetic in u64 so large directories at spacing=9 (step
    // = 10^9) don't overflow. `Entry::number` stays u32 because that's what
    // the filename parser produces; only the computed new index needs the
    // wider type.
    let step = 10u64.pow(spacing);
    let pad = padding as usize;
    let mut plan = Vec::new();
    let mut next: u64 = 1;
    for entry in entries {
        if entry.number.is_none() {
            continue;
        }
        let new_number = next * step;
        next += 1;
        let prefix = format!("{:0pad$}", new_number, pad = pad);
        for member in &entry.members {
            let new_name = if entry.stem.is_empty() {
                format!("{}{}", prefix, member.suffix)
            } else {
                format!("{}-{}{}", prefix, entry.stem, member.suffix)
            };
            if new_name != member.original_name {
                plan.push(Rename {
                    from: member.original_name.clone(),
                    to: new_name,
                });
            }
        }
    }
    plan
}

/// Prefix used for all temp names during the two-phase rename. Leading dot
/// keeps them out of casual `ls`. Also the sentinel for detecting a dirty
/// directory left over by a prior failed run.
const TEMP_PREFIX: &str = ".reindex-tmp-";

/// Reject any `name` that isn't a simple single-component basename. Used by
/// [`apply_plan`] to keep `dir.join(name)` strictly inside `dir`.
fn validate_basename(name: &str, side: &'static str) -> Result<(), ApplyError> {
    let invalid =
        name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\');
    if invalid {
        return Err(ApplyError::InvalidName {
            name: name.to_string(),
            side,
        });
    }
    Ok(())
}

/// Result of a successful [`apply_plan`] call.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyReport {
    /// Renames that committed to disk, in the order they were applied.
    pub executed: Vec<Rename>,
}

/// Reasons [`apply_plan`] may refuse or abort.
#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("plan contains duplicate source `{0}`")]
    DuplicateFrom(String),
    #[error("plan contains duplicate target `{0}`")]
    DuplicateTo(String),
    #[error("target `{target}` already exists and is not being renamed by this plan")]
    TargetExists { target: String },
    /// Rename entry's `from` or `to` is not a single-component basename.
    /// Rejecting these up front prevents a caller from tricking `apply_plan`
    /// into renaming across directory boundaries via `..` or path separators.
    #[error(
        "plan {side} `{name}` is not a valid basename (empty, contains a path separator, or is `.`/`..`)"
    )]
    InvalidName { name: String, side: &'static str },
    /// Rename entry's `from` or `to` starts with [`TEMP_PREFIX`]. Reserved
    /// because letting a user name collide with the sentinel would make the
    /// directory look permanently "dirty" to future runs.
    #[error("plan {side} `{name}` uses the reserved `{TEMP_PREFIX}*` prefix")]
    ReservedName { name: String, side: &'static str },
    #[error(
        "directory contains leftover `{TEMP_PREFIX}*` files from a prior failed run — \
        inspect and remove them before retrying"
    )]
    DirtyTemps,
    /// Could not read `dir` to check for stale temps. Treated as a hard
    /// failure rather than letting the dirty-temp check silently pass.
    #[error("failed to read directory `{dir}` to check for leftover temps: {source}")]
    ReadDir {
        dir: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("phase 1 (stage to temp) failed on source `{source}`: {cause}")]
    Phase1 {
        source: String,
        #[source]
        cause: std::io::Error,
    },
    #[error(
        "phase 2 (temp to target) failed after {completed} successful renames.\n  \
        source: `{source_original}`\n  temp: `{temp}`\n  target: `{target}`\n  \
        cause: {cause}\n\
        Temp files (`{TEMP_PREFIX}*`) remain in the directory — inspect and \
        move them manually."
    )]
    Phase2 {
        source_original: String,
        temp: String,
        target: String,
        #[source]
        cause: std::io::Error,
        /// Number of renames from this plan that committed successfully
        /// before the failure.
        completed: usize,
    },
}

/// Execute a plan of renames inside `dir` using a two-phase protocol:
/// source → temp, then temp → target. This is atomic-enough-per-directory to
/// handle the common case where a renumbered set intersects existing
/// filenames (e.g. `005-Bar` → `002-Bar` while `002-Foo` already exists and
/// is also being renamed).
///
/// ## Failure semantics
///
/// - Validation (duplicate sources/targets, pre-existing non-plan targets,
///   leftover temps) fails before any rename happens.
/// - Phase 1 failure triggers a best-effort rollback: temp names are renamed
///   back to their originals. The error is still returned so the caller
///   knows something went wrong.
/// - Phase 2 failure does **not** roll back. Temp files are left on disk
///   with their `.reindex-tmp-*` names so the user can recover manually.
///   The error names exactly which temp, target, and source were involved.
///
/// An empty plan is a no-op and returns successfully without scanning `dir`.
pub fn apply_plan(dir: &Path, plan: &[Rename]) -> Result<ApplyReport, ApplyError> {
    if plan.is_empty() {
        return Ok(ApplyReport { executed: vec![] });
    }

    // ----- validation -----

    // Every `from` / `to` must be a plain, single-component basename.
    // Rejecting path separators and traversal markers up front keeps
    // `dir.join(name)` strictly inside `dir`.
    for r in plan {
        validate_basename(&r.from, "source")?;
        validate_basename(&r.to, "target")?;
    }
    // TEMP_PREFIX is reserved for our own bookkeeping — a user-level rename
    // that lands on `.reindex-tmp-*` would make the directory look
    // permanently dirty to future runs.
    for r in plan {
        if r.from.starts_with(TEMP_PREFIX) {
            return Err(ApplyError::ReservedName {
                name: r.from.clone(),
                side: "source",
            });
        }
        if r.to.starts_with(TEMP_PREFIX) {
            return Err(ApplyError::ReservedName {
                name: r.to.clone(),
                side: "target",
            });
        }
    }

    let mut seen_from: HashSet<&str> = HashSet::with_capacity(plan.len());
    for r in plan {
        if !seen_from.insert(r.from.as_str()) {
            return Err(ApplyError::DuplicateFrom(r.from.clone()));
        }
    }
    let mut seen_to: HashSet<&str> = HashSet::with_capacity(plan.len());
    for r in plan {
        if !seen_to.insert(r.to.as_str()) {
            return Err(ApplyError::DuplicateTo(r.to.clone()));
        }
    }
    for r in plan {
        // A target that is itself being renamed away in phase 1 is fine — its
        // name becomes free before phase 2 tries to claim it.
        if seen_from.contains(r.to.as_str()) {
            continue;
        }
        if dir.join(&r.to).exists() {
            return Err(ApplyError::TargetExists {
                target: r.to.clone(),
            });
        }
    }
    // Refuse to proceed if a prior run left stale temps behind. Failing to
    // read the directory is a hard error — silently skipping this check
    // would defeat the whole safety guarantee.
    let iter = fs::read_dir(dir).map_err(|source| ApplyError::ReadDir {
        dir: dir.to_path_buf(),
        source,
    })?;
    for entry in iter.flatten() {
        if let Some(name) = entry.file_name().to_str()
            && name.starts_with(TEMP_PREFIX)
        {
            return Err(ApplyError::DirtyTemps);
        }
    }

    // ----- phase 1: source → temp -----

    let run_id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );

    // (original_from, temp_name, target_to).
    // Temp names are `{TEMP_PREFIX}{run_id}-{i}` with no original-name
    // suffix — keeping them short avoids NAME_MAX failures for files
    // already close to the 255-byte limit. The staged map below carries
    // the original name for rollback / error reporting.
    let mut staged: Vec<(String, String, String)> = Vec::with_capacity(plan.len());
    for (i, r) in plan.iter().enumerate() {
        let temp_name = format!("{TEMP_PREFIX}{run_id}-{i}");
        let src = dir.join(&r.from);
        let tmp = dir.join(&temp_name);
        if let Err(cause) = fs::rename(&src, &tmp) {
            // Rollback anything we already staged, in reverse order.
            for (orig, tmp_name, _) in staged.iter().rev() {
                let _ = fs::rename(dir.join(tmp_name), dir.join(orig));
            }
            return Err(ApplyError::Phase1 {
                source: r.from.clone(),
                cause,
            });
        }
        staged.push((r.from.clone(), temp_name, r.to.clone()));
    }

    // ----- phase 2: temp → target -----

    let mut executed: Vec<Rename> = Vec::with_capacity(staged.len());
    for (orig_from, tmp_name, target_to) in &staged {
        let tmp = dir.join(tmp_name);
        let dst = dir.join(target_to);
        if let Err(cause) = fs::rename(&tmp, &dst) {
            return Err(ApplyError::Phase2 {
                source_original: orig_from.clone(),
                temp: tmp_name.clone(),
                target: target_to.clone(),
                cause,
                completed: executed.len(),
            });
        }
        executed.push(Rename {
            from: orig_from.clone(),
            to: target_to.clone(),
        });
    }

    Ok(ApplyReport { executed })
}

// ==========================================================================
// Walker — read a directory into a Vec<Entry>
// ==========================================================================

/// Context for filtering a directory's entries.
///
/// These knobs are config-driven but only take effect at the content root:
/// inside an album or group, `assets_dir` and the site-description file have
/// no meaning, so we just don't skip them there.
#[derive(Debug, Clone, Copy)]
pub struct WalkOptions<'a> {
    /// `true` if `dir` is the content root. When `true`, the walker also
    /// skips `assets_dir` and the site-description file. When `false`,
    /// those names are treated as ordinary entries (and in practice never
    /// appear in albums anyway).
    pub is_root: bool,
    /// Content root's assets-dir name (from `SiteConfig::assets_dir`). Only
    /// consulted when `is_root`.
    pub assets_dir: Option<&'a str>,
    /// Stem of the site-description file (from `SiteConfig::site_description_file`,
    /// default `"site"`). Only consulted when `is_root`; when set, the walker
    /// skips `{stem}.md` and `{stem}.txt`.
    pub site_description_file: &'a str,
}

impl Default for WalkOptions<'_> {
    fn default() -> Self {
        WalkOptions {
            is_root: false,
            assets_dir: None,
            site_description_file: "site",
        }
    }
}

/// Extensions that are treated as image-sidecar companions when they share
/// an image's numeric stem. `.txt` matches what scan uses today; `.md` is
/// broader (scan ignores `.md` sidecars) but bundling it here keeps a
/// user-added companion markdown file from being orphaned by a rename.
const SIDECAR_EXTS: &[&str] = &["txt", "md"];

/// Returns `true` for names the walker should ignore entirely.
///
/// Shared rules (every directory):
/// - Hidden files (leading `.`), which also covers stale `.reindex-tmp-*`.
/// - `config.toml`, `description.txt`, `description.md`.
/// - Build artifacts: `processed`, `dist`, `manifest.json`.
///
/// Root-only rules (`opts.is_root`):
/// - `{assets_dir}` (if configured).
/// - `{site_description_file}.md` and `{site_description_file}.txt`.
fn is_skipped(name: &str, opts: &WalkOptions<'_>) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name,
        "config.toml"
            | "description.txt"
            | "description.md"
            | "processed"
            | "dist"
            | "manifest.json"
    ) || (opts.is_root
        && (opts.assets_dir == Some(name)
            || name == format!("{}.md", opts.site_description_file).as_str()
            || name == format!("{}.txt", opts.site_description_file).as_str()))
}

/// Parsed shape of a file/dir basename.
struct DiskName {
    /// Numeric prefix parsed from the basename (`None` if there isn't one).
    number: Option<u32>,
    /// The non-numeric portion between the number and the extension,
    /// preserving original case. Empty for inputs like `"001"` or
    /// `"001-"`.
    stem: String,
    /// Extension including the leading dot (`".jpg"`, `".md"`), or `""`
    /// for directories and extension-less files.
    suffix: String,
}

/// Parse a directory entry's basename into `(number, stem, suffix)`.
///
/// Handles:
/// - `"010-Dawn.jpg"` → `number=10, stem="Dawn", suffix=".jpg"`
/// - `"020-Travel"`   → `number=20, stem="Travel", suffix=""`
/// - `"040-about.md"` → `number=40, stem="about", suffix=".md"`
/// - `"001"` / `"001-"` → `number=1, stem="", suffix=""`
/// - `"site.md"`      → `number=None, stem="site", suffix=".md"`
fn parse_disk_name(name: &str, is_dir: bool) -> DiskName {
    // Split the extension off files; directories keep their whole name.
    let (base, suffix) = if is_dir {
        (name, String::new())
    } else {
        // `rfind('.')` — we only treat it as an extension if the dot is not
        // at position 0 (hidden files were already filtered upstream, but
        // be defensive) and something follows it.
        match name.rfind('.') {
            Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], name[i..].to_string()),
            _ => (name, String::new()),
        }
    };
    // Parse `base` for `NNN-stem` or pure `NNN`.
    if let Some(dash) = base.find('-') {
        let prefix = &base[..dash];
        if let Ok(num) = prefix.parse::<u32>() {
            return DiskName {
                number: Some(num),
                stem: base[dash + 1..].to_string(),
                suffix,
            };
        }
    }
    if let Ok(num) = base.parse::<u32>() {
        return DiskName {
            number: Some(num),
            stem: String::new(),
            suffix,
        };
    }
    DiskName {
        number: None,
        stem: base.to_string(),
        suffix,
    }
}

/// Is this extension one the processing stage treats as an image source?
fn is_image_ext(ext_with_dot: &str) -> bool {
    let stripped = ext_with_dot.strip_prefix('.').unwrap_or(ext_with_dot);
    imaging::supported_input_extensions()
        .iter()
        .any(|e| e.eq_ignore_ascii_case(stripped))
}

/// Is this a sidecar extension (`.txt` / `.md`) that should bundle with a
/// matching image in the same directory?
fn is_sidecar_ext(ext_with_dot: &str) -> bool {
    let stripped = ext_with_dot.strip_prefix('.').unwrap_or(ext_with_dot);
    SIDECAR_EXTS
        .iter()
        .any(|e| e.eq_ignore_ascii_case(stripped))
}

/// One raw candidate from `readdir` — not yet grouped into an [`Entry`].
struct RawCandidate {
    name: String,
    is_dir: bool,
    parsed: DiskName,
}

/// Read `dir`, group images with their sidecars, and return a deterministic
/// [`Entry`] list suitable for feeding [`plan_reindex`].
///
/// Sort order: numbered entries first (by `number`, then by `stem`),
/// unnumbered entries last (by `stem`). Unnumbered entries are included in
/// the output so callers can report them as "skipped" in dry-run output;
/// `plan_reindex` will emit no renames for them.
pub fn read_entries(dir: &Path, opts: &WalkOptions<'_>) -> std::io::Result<Vec<Entry>> {
    let mut candidates: Vec<RawCandidate> = Vec::new();
    for dirent in fs::read_dir(dir)? {
        let dirent = dirent?;
        let name = match dirent.file_name().to_str() {
            Some(s) => s.to_string(),
            // Skip names that aren't valid UTF-8 — reindex doesn't try to
            // be clever about them.
            None => continue,
        };
        if is_skipped(&name, opts) {
            continue;
        }
        let file_type = dirent.file_type()?;
        let is_dir = file_type.is_dir();
        let parsed = parse_disk_name(&name, is_dir);
        candidates.push(RawCandidate {
            name,
            is_dir,
            parsed,
        });
    }

    // Split into image candidates and everything else. Images anchor
    // bundles; `.txt`/`.md` that match an image's (number, stem) attach to
    // it. Any sidecar without a matching image becomes its own entry —
    // probably a `.md` page at the content root.
    let mut image_indices: Vec<usize> = Vec::new();
    let mut other_indices: Vec<usize> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if !c.is_dir && is_image_ext(&c.parsed.suffix) {
            image_indices.push(i);
        } else {
            other_indices.push(i);
        }
    }

    // Track which non-image candidates were claimed as sidecars.
    let mut claimed: Vec<bool> = vec![false; candidates.len()];
    let mut entries: Vec<Entry> = Vec::new();

    for img_i in &image_indices {
        let img = &candidates[*img_i];
        let mut members = vec![EntryMember {
            original_name: img.name.clone(),
            suffix: img.parsed.suffix.clone(),
        }];
        for other_i in &other_indices {
            if claimed[*other_i] {
                continue;
            }
            let other = &candidates[*other_i];
            if other.is_dir || !is_sidecar_ext(&other.parsed.suffix) {
                continue;
            }
            if other.parsed.number == img.parsed.number && other.parsed.stem == img.parsed.stem {
                members.push(EntryMember {
                    original_name: other.name.clone(),
                    suffix: other.parsed.suffix.clone(),
                });
                claimed[*other_i] = true;
            }
        }
        entries.push(Entry {
            number: img.parsed.number,
            stem: img.parsed.stem.clone(),
            members,
        });
    }

    for other_i in other_indices {
        if claimed[other_i] {
            continue;
        }
        let c = &candidates[other_i];
        entries.push(Entry {
            number: c.parsed.number,
            stem: c.parsed.stem.clone(),
            members: vec![EntryMember {
                original_name: c.name.clone(),
                suffix: c.parsed.suffix.clone(),
            }],
        });
    }

    // Deterministic order: numbered entries first, by (number, stem);
    // unnumbered last, by stem.
    entries.sort_by(|a, b| match (a.number, b.number) {
        (Some(na), Some(nb)) => na.cmp(&nb).then_with(|| a.stem.cmp(&b.stem)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.stem.cmp(&b.stem),
    });

    Ok(entries)
}

// ==========================================================================
// Tree driver — walk a target tree, plan + apply per directory
// ==========================================================================

/// One directory's contribution to a [`reindex_tree`] run.
#[derive(Debug, Clone)]
pub struct DirReport {
    /// Directory that was planned.
    pub dir: PathBuf,
    /// Renames computed for that directory. Empty when nothing needs to
    /// change (already normalized, or no numbered entries).
    pub plan: Vec<Rename>,
    /// `true` if the plan was applied to disk; `false` when `dry_run` was
    /// set or the plan was empty.
    pub applied: bool,
}

/// Failures from a [`reindex_tree`] run. Wraps walker IO and the planner /
/// applier errors so callers can match them without juggling three types.
#[derive(Debug, Error)]
pub enum ReindexError {
    #[error("failed to read directory `{dir}`: {source}")]
    Io {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("apply failed in `{dir}`: {source}")]
    Apply {
        dir: PathBuf,
        #[source]
        source: ApplyError,
    },
}

/// Walk `root`, plan the renames at each directory, and apply them unless
/// `dry_run` is set.
///
/// With `flat = true` only `root` itself is processed. Otherwise the walker
/// descends into numbered subdirectories (unnumbered dirs are hidden-by-
/// convention and deliberately left alone).
///
/// The walker reads each directory *after* any parent-directory rename has
/// been applied, so recursion always sees current on-disk names. This is
/// intentional: it means the rename plan at the parent level is visible in
/// children's paths but doesn't require any bookkeeping here.
///
/// `opts` controls root-only skipping (`assets_dir`, site-description file).
/// For descended directories the walker passes a child `WalkOptions` with
/// `is_root: false`.
pub fn reindex_tree(
    root: &Path,
    spacing: u32,
    padding: u32,
    flat: bool,
    dry_run: bool,
    opts: &WalkOptions<'_>,
) -> Result<Vec<DirReport>, ReindexError> {
    let mut reports = Vec::new();
    process_dir(root, spacing, padding, flat, dry_run, opts, &mut reports)?;
    Ok(reports)
}

fn process_dir(
    dir: &Path,
    spacing: u32,
    padding: u32,
    flat: bool,
    dry_run: bool,
    opts: &WalkOptions<'_>,
    reports: &mut Vec<DirReport>,
) -> Result<(), ReindexError> {
    let entries = read_entries(dir, opts).map_err(|source| ReindexError::Io {
        dir: dir.to_path_buf(),
        source,
    })?;
    let plan = plan_reindex(&entries, spacing, padding);
    let applied = if !dry_run && !plan.is_empty() {
        apply_plan(dir, &plan).map_err(|source| ReindexError::Apply {
            dir: dir.to_path_buf(),
            source,
        })?;
        true
    } else {
        false
    };
    reports.push(DirReport {
        dir: dir.to_path_buf(),
        plan: plan.clone(),
        applied,
    });
    if flat {
        return Ok(());
    }
    // Descend into numbered subdirectories. We re-read the directory so we
    // see post-rename names; anything numbered is an album or group that
    // reindex should also normalize.
    let child_opts = WalkOptions {
        is_root: false,
        assets_dir: None,
        site_description_file: opts.site_description_file,
    };
    let iter = fs::read_dir(dir).map_err(|source| ReindexError::Io {
        dir: dir.to_path_buf(),
        source,
    })?;
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for dirent in iter {
        let dirent = dirent.map_err(|source| ReindexError::Io {
            dir: dir.to_path_buf(),
            source,
        })?;
        let name = match dirent.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if is_skipped(&name, opts) {
            continue;
        }
        let file_type = dirent.file_type().map_err(|source| ReindexError::Io {
            dir: dir.to_path_buf(),
            source,
        })?;
        if !file_type.is_dir() {
            continue;
        }
        let parsed = parse_disk_name(&name, true);
        if parsed.number.is_none() {
            // Unnumbered subdir = hidden from nav by convention; leave it alone.
            continue;
        }
        subdirs.push(dir.join(&name));
    }
    // Deterministic recursion order.
    subdirs.sort();
    for sub in subdirs {
        process_dir(&sub, spacing, padding, flat, dry_run, &child_opts, reports)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a single-member numbered entry from a stem and suffix.
    fn dir(number: u32, stem: &str) -> Entry {
        let original = if stem.is_empty() {
            format!("{}", number)
        } else {
            format!("{}-{}", number, stem)
        };
        Entry {
            number: Some(number),
            stem: stem.to_string(),
            members: vec![EntryMember {
                original_name: original,
                suffix: String::new(),
            }],
        }
    }

    /// Build a single-file numbered entry with the given extension.
    fn file(number: u32, stem: &str, ext: &str) -> Entry {
        let original = if stem.is_empty() {
            format!("{}{}", number, ext)
        } else {
            format!("{}-{}{}", number, stem, ext)
        };
        Entry {
            number: Some(number),
            stem: stem.to_string(),
            members: vec![EntryMember {
                original_name: original,
                suffix: ext.to_string(),
            }],
        }
    }

    /// Build an image-with-sidecars entry. The image and sidecars share the
    /// same numeric stem on disk, mirroring how scan groups them.
    fn bundle(number: u32, stem: &str, exts: &[&str]) -> Entry {
        let members = exts
            .iter()
            .map(|ext| EntryMember {
                original_name: format!("{}-{}{}", number, stem, ext),
                suffix: ext.to_string(),
            })
            .collect();
        Entry {
            number: Some(number),
            stem: stem.to_string(),
            members,
        }
    }

    fn unnumbered(name: &str) -> Entry {
        Entry {
            number: None,
            stem: name.to_string(),
            members: vec![EntryMember {
                original_name: name.to_string(),
                suffix: String::new(),
            }],
        }
    }

    // ----- spacing & padding -----

    #[test]
    fn three_entries_spacing0_padding0() {
        let entries = vec![dir(1, "Foo"), dir(2, "Bar"), dir(3, "Baz")];
        let plan = plan_reindex(&entries, 0, 0);
        // Already normalized → empty plan.
        assert_eq!(plan, vec![]);
    }

    #[test]
    fn sparse_input_collapses_at_spacing1_padding0() {
        let entries = vec![dir(1, "A"), dir(10, "B"), dir(100, "C")];
        let plan = plan_reindex(&entries, 1, 0);
        // Sequential at step=10: A→10, B→20, C→30. The middle entry
        // already had number=10 but its position in the input was second,
        // so it gets the second slot (20).
        assert_eq!(
            plan,
            vec![
                Rename {
                    from: "1-A".into(),
                    to: "10-A".into(),
                },
                Rename {
                    from: "10-B".into(),
                    to: "20-B".into(),
                },
                Rename {
                    from: "100-C".into(),
                    to: "30-C".into(),
                },
            ]
        );
    }

    #[test]
    fn padding_independent_of_spacing() {
        let entries = vec![dir(1, "A"), dir(2, "B")];
        let plan = plan_reindex(&entries, 0, 3);
        assert_eq!(
            plan,
            vec![
                Rename {
                    from: "1-A".into(),
                    to: "001-A".into(),
                },
                Rename {
                    from: "2-B".into(),
                    to: "002-B".into(),
                },
            ]
        );
    }

    #[test]
    fn spacing2_padding4() {
        let entries = vec![dir(7, "A"), dir(8, "B"), dir(9, "C")];
        let plan = plan_reindex(&entries, 2, 4);
        assert_eq!(
            plan.iter().map(|r| r.to.as_str()).collect::<Vec<_>>(),
            vec!["0100-A", "0200-B", "0300-C"]
        );
    }

    // ----- skipping & order preservation -----

    #[test]
    fn unnumbered_entries_are_skipped() {
        let entries = vec![
            dir(5, "A"),
            unnumbered("wip-drafts"),
            dir(10, "B"),
            unnumbered("assets"),
            dir(20, "C"),
        ];
        let plan = plan_reindex(&entries, 1, 3);
        assert_eq!(
            plan,
            vec![
                Rename {
                    from: "5-A".into(),
                    to: "010-A".into(),
                },
                Rename {
                    from: "10-B".into(),
                    to: "020-B".into(),
                },
                Rename {
                    from: "20-C".into(),
                    to: "030-C".into(),
                },
            ]
        );
    }

    #[test]
    fn order_is_preserved_from_input_order() {
        // Caller hands them in a specific order; reindex respects it.
        // (Caller would normally sort by number; here we deliberately
        // hand in an out-of-order list to prove the function trusts input.)
        let entries = vec![dir(50, "First"), dir(10, "Second"), dir(99, "Third")];
        let plan = plan_reindex(&entries, 1, 3);
        assert_eq!(
            plan.iter().map(|r| r.to.as_str()).collect::<Vec<_>>(),
            vec!["010-First", "020-Second", "030-Third"]
        );
    }

    // ----- image bundles & sidecars -----

    #[test]
    fn image_with_sidecars_keeps_pair() {
        let entries = vec![bundle(5, "Dawn", &[".jpg", ".txt"])];
        let plan = plan_reindex(&entries, 1, 3);
        assert_eq!(
            plan,
            vec![
                Rename {
                    from: "5-Dawn.jpg".into(),
                    to: "010-Dawn.jpg".into(),
                },
                Rename {
                    from: "5-Dawn.txt".into(),
                    to: "010-Dawn.txt".into(),
                },
            ]
        );
    }

    #[test]
    fn image_with_md_and_txt_sidecars() {
        let entries = vec![bundle(2, "Dusk", &[".jpg", ".txt", ".md"])];
        let plan = plan_reindex(&entries, 0, 3);
        assert_eq!(
            plan.iter().map(|r| r.to.as_str()).collect::<Vec<_>>(),
            vec!["001-Dusk.jpg", "001-Dusk.txt", "001-Dusk.md"]
        );
    }

    // ----- no-op detection -----

    #[test]
    fn already_normalized_emits_empty_plan() {
        let entries = vec![dir(10, "A"), dir(20, "B"), dir(30, "C")];
        let plan = plan_reindex(&entries, 1, 0);
        assert!(plan.is_empty(), "expected no renames, got {:?}", plan);
    }

    #[test]
    fn already_normalized_with_padding_emits_empty_plan() {
        let entries = vec![
            file(10, "Dawn", ".jpg"),
            file(20, "Dusk", ".jpg"),
            file(30, "Night", ".jpg"),
        ];
        // file() builds names with the raw number; here that's already
        // exactly what spacing=1 padding=0 produces.
        let plan = plan_reindex(&entries, 1, 0);
        assert!(plan.is_empty(), "expected no renames, got {:?}", plan);
    }

    #[test]
    fn partial_renames_only_emits_changed_pairs() {
        // Mix: one already-correct and one needing a change.
        let entries = vec![dir(10, "A"), dir(15, "B"), dir(30, "C")];
        let plan = plan_reindex(&entries, 1, 0);
        assert_eq!(
            plan,
            vec![Rename {
                from: "15-B".into(),
                to: "20-B".into(),
            }]
        );
    }

    // ----- edge cases -----

    #[test]
    fn empty_input_emits_empty_plan() {
        let plan = plan_reindex(&[], 1, 3);
        assert!(plan.is_empty());
    }

    #[test]
    fn only_unnumbered_emits_empty_plan() {
        let entries = vec![unnumbered("wip"), unnumbered("assets")];
        let plan = plan_reindex(&entries, 1, 3);
        assert!(plan.is_empty());
    }

    #[test]
    fn empty_stem_does_not_emit_trailing_dash() {
        // Original was "001" or "001-" — the entry has empty stem. After
        // renumbering we must not emit "010-" (lone trailing dash).
        let entries = vec![Entry {
            number: Some(1),
            stem: String::new(),
            members: vec![EntryMember {
                original_name: "001".into(),
                suffix: String::new(),
            }],
        }];
        let plan = plan_reindex(&entries, 1, 3);
        assert_eq!(
            plan,
            vec![Rename {
                from: "001".into(),
                to: "010".into(),
            }]
        );
    }

    #[test]
    fn empty_stem_with_extension() {
        let entries = vec![Entry {
            number: Some(1),
            stem: String::new(),
            members: vec![EntryMember {
                original_name: "1.jpg".into(),
                suffix: ".jpg".into(),
            }],
        }];
        let plan = plan_reindex(&entries, 0, 3);
        assert_eq!(
            plan,
            vec![Rename {
                from: "1.jpg".into(),
                to: "001.jpg".into(),
            }]
        );
    }

    #[test]
    fn duplicate_numbers_assigned_sequentially_in_input_order() {
        // Caller gives two entries with the same original number — the
        // function trusts the order and assigns sequential outputs.
        let entries = vec![dir(10, "Foo"), dir(10, "Bar")];
        let plan = plan_reindex(&entries, 1, 3);
        assert_eq!(
            plan,
            vec![
                Rename {
                    from: "10-Foo".into(),
                    to: "010-Foo".into(),
                },
                Rename {
                    from: "10-Bar".into(),
                    to: "020-Bar".into(),
                },
            ]
        );
    }

    // ==========================================================================
    // apply_plan — filesystem side
    // ==========================================================================

    use std::collections::BTreeSet;
    use tempfile::TempDir;

    /// Touch an empty file in `dir`.
    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "").unwrap();
    }

    /// Sorted list of filenames in `dir`.
    fn list(dir: &Path) -> BTreeSet<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    fn r(from: &str, to: &str) -> Rename {
        Rename {
            from: from.into(),
            to: to.into(),
        }
    }

    // ----- empty & happy path -----

    #[test]
    fn apply_empty_plan_is_noop() {
        let tmp = TempDir::new().unwrap();
        let report = apply_plan(tmp.path(), &[]).unwrap();
        assert!(report.executed.is_empty());
    }

    #[test]
    fn apply_happy_path_renames_files() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-Foo.jpg");
        touch(tmp.path(), "10-Bar.jpg");
        let plan = vec![
            r("5-Foo.jpg", "010-Foo.jpg"),
            r("10-Bar.jpg", "020-Bar.jpg"),
        ];
        let report = apply_plan(tmp.path(), &plan).unwrap();
        assert_eq!(report.executed, plan);
        assert_eq!(
            list(tmp.path()),
            ["010-Foo.jpg", "020-Bar.jpg"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn apply_renames_directories() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("5-Landscapes")).unwrap();
        std::fs::create_dir(tmp.path().join("10-Travel")).unwrap();
        let plan = vec![
            r("5-Landscapes", "010-Landscapes"),
            r("10-Travel", "020-Travel"),
        ];
        apply_plan(tmp.path(), &plan).unwrap();
        assert!(tmp.path().join("010-Landscapes").is_dir());
        assert!(tmp.path().join("020-Travel").is_dir());
    }

    #[test]
    fn apply_sidecar_lockstep() {
        // Image + sidecars share a numeric stem; apply_plan sees them as
        // three independent renames — all land together.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-Dawn.jpg");
        touch(tmp.path(), "5-Dawn.txt");
        touch(tmp.path(), "5-Dawn.md");
        let plan = vec![
            r("5-Dawn.jpg", "010-Dawn.jpg"),
            r("5-Dawn.txt", "010-Dawn.txt"),
            r("5-Dawn.md", "010-Dawn.md"),
        ];
        apply_plan(tmp.path(), &plan).unwrap();
        assert_eq!(
            list(tmp.path()),
            ["010-Dawn.jpg", "010-Dawn.md", "010-Dawn.txt"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    // ----- collision safety -----

    #[test]
    fn apply_collision_where_target_is_also_a_source() {
        // Classic two-phase win: A→B and B→C. Phase 1 moves both to temp,
        // phase 2 lands both at their final names.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        touch(tmp.path(), "B");
        let plan = vec![r("A", "B"), r("B", "C")];
        apply_plan(tmp.path(), &plan).unwrap();
        assert_eq!(
            list(tmp.path()),
            ["B", "C"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn apply_renumber_with_shifted_overlap() {
        // More realistic: renumbering 5→10 and 10→20 while both exist.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-A");
        touch(tmp.path(), "10-B");
        let plan = vec![r("5-A", "10-A"), r("10-B", "20-B")];
        apply_plan(tmp.path(), &plan).unwrap();
        assert_eq!(
            list(tmp.path()),
            ["10-A", "20-B"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    // ----- pre-flight validation -----

    #[test]
    fn apply_rejects_duplicate_from() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        let plan = vec![r("A", "B"), r("A", "C")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::DuplicateFrom(s) if s == "A"));
    }

    #[test]
    fn apply_rejects_duplicate_to() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        touch(tmp.path(), "B");
        let plan = vec![r("A", "X"), r("B", "X")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::DuplicateTo(s) if s == "X"));
    }

    #[test]
    fn apply_rejects_non_plan_target_collision() {
        // A file exists at the target name and is NOT a plan source, so
        // phase 2 would clobber it. Detect and refuse.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-Foo");
        touch(tmp.path(), "10-Existing"); // unrelated existing file at the target name
        let plan = vec![r("5-Foo", "10-Existing")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        match err {
            ApplyError::TargetExists { target } => assert_eq!(target, "10-Existing"),
            other => panic!("expected TargetExists, got {other:?}"),
        }
        // Filesystem untouched.
        assert_eq!(
            list(tmp.path()),
            ["10-Existing", "5-Foo"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn apply_rejects_directory_with_leftover_temps() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        touch(tmp.path(), ".reindex-tmp-stale-0-leftover");
        let plan = vec![r("A", "B")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::DirtyTemps));
        // File untouched.
        assert!(tmp.path().join("A").exists());
    }

    // ----- phase 1 failure + rollback -----

    #[test]
    fn apply_phase1_missing_source_rolls_back_prior_renames() {
        // Plan claims C→Z exists, but we only put A and B on disk. Phase 1
        // renames A→temp and B→temp successfully, then fails on C. Rollback
        // should restore A and B to their original names.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-A");
        touch(tmp.path(), "10-B");
        // Deliberately no 15-C on disk.
        let plan = vec![r("5-A", "10-A"), r("10-B", "20-B"), r("15-C", "30-C")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        match err {
            ApplyError::Phase1 { source, .. } => assert_eq!(source, "15-C"),
            other => panic!("expected Phase1, got {other:?}"),
        }
        // Originals restored; no temps linger.
        assert_eq!(
            list(tmp.path()),
            ["10-B", "5-A"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn apply_phase1_fails_on_single_missing_source() {
        let tmp = TempDir::new().unwrap();
        let plan = vec![r("ghost", "010-ghost")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::Phase1 { .. }));
        assert!(list(tmp.path()).is_empty());
    }

    // ----- basename / reserved-name validation -----

    #[test]
    fn apply_rejects_source_with_path_separator() {
        let tmp = TempDir::new().unwrap();
        let plan = vec![r("sub/file", "010-file")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::InvalidName { side: "source", .. }
        ));
    }

    #[test]
    fn apply_rejects_target_with_path_separator() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        let plan = vec![r("A", "sub/B")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::InvalidName { side: "target", .. }
        ));
        // Filesystem untouched.
        assert_eq!(
            list(tmp.path()),
            ["A"].iter().map(|s| s.to_string()).collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn apply_rejects_parent_traversal() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        let plan = vec![r("A", "..")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::InvalidName { .. }));
    }

    #[test]
    fn apply_rejects_empty_name() {
        let tmp = TempDir::new().unwrap();
        let plan = vec![r("", "X")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(err, ApplyError::InvalidName { .. }));
    }

    #[test]
    fn apply_rejects_reserved_source_prefix() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), ".reindex-tmp-looks-like-a-temp");
        let plan = vec![r(".reindex-tmp-looks-like-a-temp", "X")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::ReservedName { side: "source", .. }
        ));
    }

    #[test]
    fn apply_rejects_reserved_target_prefix() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "A");
        let plan = vec![r("A", ".reindex-tmp-sneaky")];
        let err = apply_plan(tmp.path(), &plan).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::ReservedName { side: "target", .. }
        ));
    }

    // ----- read_dir failure is hard -----

    #[test]
    fn apply_surfaces_read_dir_failure() {
        // Pass a path that doesn't exist — read_dir fails with ENOENT and
        // apply_plan should surface it as ApplyError::ReadDir rather than
        // silently proceeding and defeating the dirty-temp check.
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let plan = vec![r("A", "B")];
        let err = apply_plan(&missing, &plan).unwrap_err();
        assert!(matches!(err, ApplyError::ReadDir { .. }));
    }

    // ----- overflow safety (plan_reindex) -----

    #[test]
    fn plan_reindex_no_overflow_at_max_spacing() {
        // With u32 arithmetic, spacing=9 would overflow at the 5th entry
        // (5 * 10^9 > u32::MAX). u64 internals handle this cleanly.
        let entries: Vec<Entry> = (1..=10).map(|n| dir(n, "E")).collect();
        let plan = plan_reindex(&entries, 9, 0);
        // 10 entries, all renamed (1-E .. 10-E) → (1000000000-E ..
        // 10000000000-E). Sanity-check the first and last.
        assert_eq!(plan[0].to, "1000000000-E");
        assert_eq!(plan[plan.len() - 1].to, "10000000000-E");
    }

    // ==========================================================================
    // Walker
    // ==========================================================================

    fn mkdir(root: &Path, name: &str) {
        std::fs::create_dir(root.join(name)).unwrap();
    }

    /// Convenience for building a WalkOptions with `is_root: true`.
    fn root_opts() -> WalkOptions<'static> {
        WalkOptions {
            is_root: true,
            assets_dir: Some("assets"),
            site_description_file: "site",
        }
    }

    /// Read entries, collect their on-disk names (across all members) for
    /// assertion in a deterministic shape.
    fn entry_names(entries: &[Entry]) -> Vec<Vec<String>> {
        entries
            .iter()
            .map(|e| e.members.iter().map(|m| m.original_name.clone()).collect())
            .collect()
    }

    // ----- classification -----

    #[test]
    fn walker_skips_metadata_and_hidden() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "config.toml");
        touch(tmp.path(), "description.txt");
        touch(tmp.path(), "description.md");
        touch(tmp.path(), ".DS_Store");
        touch(tmp.path(), ".reindex-tmp-leftover");
        touch(tmp.path(), "010-Dawn.jpg");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(
            entry_names(&entries),
            vec![vec!["010-Dawn.jpg".to_string()]]
        );
    }

    #[test]
    fn walker_skips_assets_and_site_only_at_root() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "site.md");
        mkdir(tmp.path(), "assets");
        touch(tmp.path(), "010-Landscapes.jpg");
        let entries = read_entries(tmp.path(), &root_opts()).unwrap();
        assert_eq!(
            entry_names(&entries),
            vec![vec!["010-Landscapes.jpg".to_string()]]
        );
    }

    #[test]
    fn walker_keeps_assets_in_non_root_dir() {
        // Inside an album, a subdirectory literally named "assets" is just
        // another subdir — we don't skip it.
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "assets");
        touch(tmp.path(), "010-Dawn.jpg");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn walker_honors_custom_site_description_file_stem() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "intro.md");
        touch(tmp.path(), "site.md");
        touch(tmp.path(), "010-Landscapes.jpg");
        let opts = WalkOptions {
            is_root: true,
            assets_dir: Some("assets"),
            site_description_file: "intro",
        };
        let entries = read_entries(tmp.path(), &opts).unwrap();
        // "intro.md" is the site description → skipped.
        // "site.md" is NOT the configured stem → kept.
        let names: Vec<_> = entries
            .iter()
            .flat_map(|e| &e.members)
            .map(|m| m.original_name.as_str())
            .collect();
        assert!(names.contains(&"site.md"));
        assert!(!names.contains(&"intro.md"));
    }

    // ----- sidecar bundling -----

    #[test]
    fn walker_bundles_txt_sidecar_with_image() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-Dawn.jpg");
        touch(tmp.path(), "010-Dawn.txt");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].number, Some(10));
        assert_eq!(entries[0].stem, "Dawn");
        assert_eq!(entries[0].members.len(), 2);
    }

    #[test]
    fn walker_bundles_md_sidecar_with_image() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-Dawn.jpg");
        touch(tmp.path(), "010-Dawn.md");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].members.len(), 2);
    }

    #[test]
    fn walker_orphan_md_stays_as_its_own_entry() {
        // A `.md` whose stem doesn't match any image is a standalone page
        // (at root) or a loose file (in an album). Either way it gets its
        // own Entry, not attached to some other image.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-Dawn.jpg");
        touch(tmp.path(), "040-about.md");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn walker_sidecar_with_different_number_is_not_attached() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-Dawn.jpg");
        touch(tmp.path(), "020-Dawn.txt"); // same stem, different number
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    // ----- sorting & ordering -----

    #[test]
    fn walker_sorts_numbered_first_then_unnumbered() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "020-Later.jpg");
        touch(tmp.path(), "010-Earlier.jpg");
        mkdir(tmp.path(), "wip");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        let names: Vec<_> = entries
            .iter()
            .map(|e| e.members[0].original_name.clone())
            .collect();
        assert_eq!(names, vec!["010-Earlier.jpg", "020-Later.jpg", "wip"]);
    }

    #[test]
    fn walker_sorts_duplicate_numbers_by_stem() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-Zebra.jpg");
        touch(tmp.path(), "010-Alpha.jpg");
        let entries = read_entries(tmp.path(), &WalkOptions::default()).unwrap();
        let stems: Vec<_> = entries.iter().map(|e| e.stem.clone()).collect();
        assert_eq!(stems, vec!["Alpha", "Zebra"]);
    }

    // ----- tree driver -----

    #[test]
    fn tree_flat_only_touches_root() {
        // Root has one album at position 1 → renamed 5-Album → 010-Album.
        // With --flat, the album's contents are deliberately left alone.
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "5-Album");
        touch(&tmp.path().join("5-Album"), "1-inner.jpg");
        touch(&tmp.path().join("5-Album"), "2-inner.jpg");
        let reports = reindex_tree(tmp.path(), 1, 3, true, false, &root_opts()).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(tmp.path().join("010-Album").exists());
        assert!(!tmp.path().join("5-Album").exists());
        // Contents of album: untouched (flat mode skipped recursion).
        assert!(tmp.path().join("010-Album/1-inner.jpg").exists());
        assert!(tmp.path().join("010-Album/2-inner.jpg").exists());
    }

    #[test]
    fn tree_recursive_descends_into_numbered_subdirs() {
        // Root: `1-A.jpg` + `10-Album` → positions 1, 2 → `010-A.jpg`, `020-Album`.
        // Then recursion into the (now-renamed) album renumbers its images.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "1-A.jpg");
        mkdir(tmp.path(), "10-Album");
        touch(&tmp.path().join("10-Album"), "1-inner.jpg");
        touch(&tmp.path().join("10-Album"), "5-other.jpg");
        let reports = reindex_tree(tmp.path(), 1, 3, false, false, &root_opts()).unwrap();
        assert_eq!(reports.len(), 2);
        assert!(tmp.path().join("010-A.jpg").exists());
        assert!(tmp.path().join("020-Album/010-inner.jpg").exists());
        assert!(tmp.path().join("020-Album/020-other.jpg").exists());
    }

    #[test]
    fn tree_skips_unnumbered_subdirs() {
        // `wip/` is unnumbered → hidden-by-convention → reindex leaves its
        // contents alone even in recursive mode.
        let tmp = TempDir::new().unwrap();
        mkdir(tmp.path(), "wip");
        touch(&tmp.path().join("wip"), "1-Draft.jpg");
        let reports =
            reindex_tree(tmp.path(), 1, 3, false, false, &WalkOptions::default()).unwrap();
        // Root has one entry (wip), unnumbered, skipped → no renames there.
        assert_eq!(reports.len(), 1);
        assert!(reports[0].plan.is_empty());
        // wip/1-Draft.jpg untouched.
        assert!(tmp.path().join("wip/1-Draft.jpg").exists());
    }

    #[test]
    fn tree_dry_run_leaves_filesystem_untouched() {
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "1-A.jpg");
        touch(tmp.path(), "2-B.jpg");
        let reports = reindex_tree(tmp.path(), 1, 3, false, true, &root_opts()).unwrap();
        // Plan was computed, but not applied.
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].plan.len(), 2);
        assert!(!reports[0].applied);
        // Originals intact.
        assert!(tmp.path().join("1-A.jpg").exists());
        assert!(tmp.path().join("2-B.jpg").exists());
    }

    #[test]
    fn tree_empty_plan_reports_not_applied() {
        // Already normalized → empty plan → `applied` stays false.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "010-A.jpg");
        touch(tmp.path(), "020-B.jpg");
        let reports = reindex_tree(tmp.path(), 1, 3, false, false, &root_opts()).unwrap();
        assert!(reports[0].plan.is_empty());
        assert!(!reports[0].applied);
    }

    #[test]
    fn tree_surfaces_read_dir_error() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let err = reindex_tree(&missing, 1, 3, false, false, &root_opts()).unwrap_err();
        assert!(matches!(err, ReindexError::Io { .. }));
    }

    #[test]
    fn tree_preserves_sidecars_across_rename() {
        // The bundle support lives in the walker + plan_reindex pairing;
        // this is the end-to-end confirmation that a sidecar follows its
        // image through an actual reindex_tree run.
        let tmp = TempDir::new().unwrap();
        touch(tmp.path(), "5-Dawn.jpg");
        touch(tmp.path(), "5-Dawn.txt");
        reindex_tree(tmp.path(), 1, 3, false, false, &root_opts()).unwrap();
        assert!(tmp.path().join("010-Dawn.jpg").exists());
        assert!(tmp.path().join("010-Dawn.txt").exists());
        assert!(!tmp.path().join("5-Dawn.jpg").exists());
        assert!(!tmp.path().join("5-Dawn.txt").exists());
    }
}
