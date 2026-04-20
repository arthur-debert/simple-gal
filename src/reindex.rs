//! Auto file-name index reindexing — pure planning + on-disk rename.
//!
//! Two entry points:
//!
//! - [`plan_reindex`] — pure function. Takes a list of numbered entries and
//!   the spacing/padding parameters, returns the rename plan. No I/O.
//! - [`apply_plan`] — executes the rename plan via a two-phase rename
//!   (source → temp, then temp → target) so the plan is collision-safe.
//!
//! Directory discovery and entry construction live in the walker (TODO, task
//! #4). See `docs/dev/auto-reindex.md` for the full feature spec.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use thiserror::Error;

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
}
