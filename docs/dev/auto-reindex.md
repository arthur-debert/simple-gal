# Auto Index Reindexing — Feature Spec

> **Status:** design / pre-implementation. This is a sensitive feature because it
> renames files in the user's content tree. Nothing here is implemented yet.
> Open questions are called out inline; answer them before writing code.

## 1. Motivation

Simple-gal orders albums, groups, pages, and images by a numeric filename prefix
(`010-Landscapes/`, `001-dawn.jpg`, `040-about.md`). Numbering can be sparse
and unpadded — that's deliberate: leaving gaps (10, 20, 30…) lets a user slot a
new entry in with a single rename rather than cascading renumbering.

Over time those gaps get used up (or over-allocated), and the user wants a
one-shot way to **normalize** — to collapse existing sparse numbers back to
tidy, evenly-spaced, padded prefixes while preserving order.

## 2. Core semantics

### 2.1 Order preservation is non-negotiable

Reindexing **must never reorder** entries. The output sequence matches the
input sequence, parsed under the rules already in `src/naming.rs`. Any tie-break
rule the scan stage applies today is the same rule reindex uses.

### 2.2 Entries that get renumbered

Within a given directory, reindex applies to any entry that has a numeric
prefix under `parse_entry_name()` — that's:

- Albums (numbered subdirectories containing images).
- Groups (numbered subdirectories containing subdirectories).
- Pages (numbered `.md` files at root / group level).
- Images (numbered image files inside an album).
- Image sidecars (`.txt` / `.md` sharing a numbered image's stem) — follow
  their image's new number so the pair stays linked.

### 2.3 Entries that are left alone

- Unnumbered entries (`wip-drafts/`, `site.md`, `description.txt`, `assets/`,
  `config.toml`) — these already have meaning as "hidden from nav" or
  "non-content metadata". Reindex never touches them.
- Non-content files (config.toml, description.*, assets dir contents).

### 2.4 Spacing and padding

Two orthogonal parameters drive the output prefix:

- **spacing (`N`)** → step size = `10^N`. So:
  - `spacing=0` → `1, 2, 3, 4, …`
  - `spacing=1` → `10, 20, 30, 40, …`
  - `spacing=2` → `100, 200, 300, …`
- **padding (`P`)** → zero-pad the number to width `P`. `padding=0` means "no
  zero padding" (the minimum width that fits the number). Padding is
  independent of spacing.

Examples:
| spacing | padding | output                    |
| ------- | ------- | ------------------------- |
| 0       | 0       | `1-Foo`, `2-Bar`          |
| 1       | 0       | `10-Foo`, `20-Bar`        |
| 1       | 3       | `010-Foo`, `020-Bar`      |
| 2       | 4       | `0100-Foo`, `0200-Bar`    |
| 0       | 3       | `001-Foo`, `002-Bar`      |

Decided: spacing is power-of-10. Decided: always start at `1 * step`; no
`--start` / `--offset` flag.

### 2.5 Scope: flat vs recursive

The command operates on a target directory (defaults to CWD, optional positional
path arg). Behavior depends on what the target contains:

- **Target is the content root or an album group** (contains subdirectories
  and/or pages): reindex the numbered children at that level.
- **Target is an album** (contains images): reindex the images at that level.

By default the walk is **recursive**: after reindexing the children of the
target, descend into each numbered subdirectory and reindex there too.
`--flat` limits reindex to the target directory only.


## 3. CLI surface

```
simple-gal reindex [PATH] [--spacing N] [--padding P] [--flat] [--dry-run] [--yes]
```

- `PATH` — target directory. Defaults to CWD.
- `--spacing N` — exponent of 10 (see 2.4). Default from config, fallback `1`.
- `--padding P` — zero-pad width. Default from config, fallback `3`.
- `--flat` — don't recurse into child directories.
- `--dry-run` — print the rename plan; do not touch the filesystem. Exits 0.
- `--yes` — skip the interactive confirmation (see §5).

`--format json` / `ndjson` / `progress` — as with every other command, emits a
machine-readable envelope (`op: "reindex"`) describing the planned and executed
renames.

Clapfig handles config → flag precedence: CLI flags override the corresponding
config values for this invocation, unset flags fall through to `[auto_indexing]`
config, which falls through to compiled defaults.

Decided: subcommand name is `reindex`.

## 4. Config surface

Add a new section to `SiteConfig`, parallel to `[processing]`:

```toml
[auto_indexing]
auto    = "off"   # "export_only" | "source_only" | "both" | "off"
spacing = 1
padding = 3
```

Fields:

- **`auto`** — when reindexing runs automatically during `build`:
  - `off` (default) — never auto-run; only the explicit `reindex` command does.
  - `source_only` — reindex source files during scan, before process. File
    renames land on disk and are visible in the user's working tree.
  - `export_only` — source untouched; the manifest is rewritten in-memory so
    generated URLs and copied output file names use the normalized prefix.
  - `both` — reindex source files in place; the dist pipeline then regenerates
    from the new source, so output follows naturally. Kept as a distinct value
    from `source_only` because it states user intent ("keep source and dist
    aligned") even though the on-disk effect matches `source_only`.
- **`spacing` / `padding`** — supply defaults for the CLI command and values
  for auto runs.

`[auto_indexing]` cascades per-directory, following the same pattern as the
rest of `SiteConfig`: a root setting applies everywhere, a subdirectory can
override (for example, set `auto = "off"` to exempt one album from an
otherwise-site-wide auto-reindex policy).

## 5. Safety & UX

Source-file renames are destructive from the user's point of view; the feature
must be loud and reversible by accident.

### 5.1 Two-phase rename

To avoid collisions when the renumbered set intersects existing filenames
(e.g. existing `002-foo.jpg` and we're about to produce `002-bar.jpg` from
`005-bar.jpg`), every directory is renamed in two phases:

1. Rename each source → a unique temp name (e.g. `.reindex-tmp-<uuid>-<name>`).
2. Rename each temp → final name.

This is atomic per-directory: if phase 1 partly fails we can roll back by
reading the temp prefix; if phase 2 fails we stop with a clear error and
leave the temp names in place so the user can recover.

### 5.2 Dry-run & confirmation

- `--dry-run` prints the full plan (`OLD → NEW`, one line per rename, grouped
  by directory) and exits without touching anything.
- Interactive (TTY) runs without `--yes` print the plan and prompt for
  confirmation. Non-TTY / scripted runs require `--yes` to proceed.
- Auto runs triggered via config (`auto_indexing.auto != off`) do not prompt;
  they log what they renamed in the build output.

### 5.3 Sidecar tracking

When renaming `010-dawn.jpg` → `001-dawn.jpg`, also rename `010-dawn.txt` /
`010-dawn.md` in lockstep. The sidecar pairing logic already exists in
`scan.rs` — reuse it.

### 5.4 Cache interaction

The process-stage cache keys by source path + content hash. Renaming source
files invalidates path-based cache entries and forces re-processing on the
next build. This is correct behavior; just flag it in the reindex output so
users aren't surprised by a slow build afterward.

### 5.5 Things reindex must refuse

- Git state: if a source file has uncommitted changes, warn (don't fail). The
  user may want to commit first so the diff is a clean rename. Optional
  `--allow-dirty` to silence.
- Duplicate numbers at the same level: still renumber, but log a warning —
  the scan stage already has deterministic tie-break rules; reindex uses the
  same order.
- An empty directory with no numbered entries is a no-op, not an error.

Decided: no `git mv`. Plain `fs::rename` is enough — reindex only changes
names, so content similarity is 100% and git detects the rename cleanly
without special handling.

## 6. Module layout

Following the project's "pure logic first, CLI on top" principle:

- **`src/reindex.rs`** (new) — pure logic module.
  - `plan_reindex(entries: &[Entry], spacing: u32, padding: u32) -> Vec<Rename>`
    — takes a list of parsed entries + params, returns the rename plan. No I/O.
  - `apply_plan(plan: &[Rename], root: &Path) -> Result<ApplyReport>` — executes
    the two-phase rename on disk. Isolated so the plan can be unit-tested
    without touching the filesystem.
  - Walks recursively or flat depending on caller.
- **`src/config.rs`** — add `AutoIndexingConfig` struct with confique defaults.
- **`src/main.rs`** — add `Command::Reindex(ReindexArgs)` + `run_reindex()`,
  mirroring the shape of existing commands (format handling, quiet, json
  envelopes via `json_output`).
- **`src/json_output.rs`** — add `ReindexPayload` with `planned: Vec<_>`,
  `executed: Vec<_>`, `dry_run: bool`.
- **Auto-hook in `src/process.rs` or `src/scan.rs`** — consult
  `config.auto_indexing.auto` and run before scan (source modes) or rewrite
  the manifest after scan (export mode).

## 7. Testing plan

All unit tests — the pure logic module makes this natural.

**`plan_reindex` tests** (the critical surface — get these right and the rest
follows):

- Basic 3-entry renumber at spacing 0, padding 0 → `1, 2, 3`.
- Sparse `1, 10, 100` input → `10, 20, 30` at spacing 1.
- Padding applied independent of spacing.
- Unnumbered entries are skipped (not in plan).
- Image + sidecar emit two renames in the plan for that image.
- Already-normalized input produces an empty plan (no-op detection).
- Duplicate numbers at the same level: order determined by existing scan
  tie-break; plan is deterministic.

**`apply_plan` tests** (filesystem side — use `tempfile::TempDir`):

- Happy path: files renamed on disk.
- Collision: plan that would overwrite existing targets goes through phase 1
  temp names without error, phase 2 lands cleanly.
- Mid-flight failure simulation: after phase 1, the tempdir state contains
  recoverable names (test via the tempdir inspection, not by forcing a failure).

**CLI integration** — skip per project convention. The CLI is a thin wrapper;
`plan_reindex` / `apply_plan` carry the correctness contract.

**Fixture reuse** — `fixtures/content/` already has a mix of padded and
unpadded numbers (`001-dawn.jpg`, `010-night.jpg`, `020-Travel`); copy it to
a temp dir, reindex, assert resulting names.

## 8. Implementation order

Build bottom-up, landing each layer with tests green before the next:

1. `AutoIndexingConfig` struct + defaults + confique wiring + tests.
2. `plan_reindex` pure function + unit tests (using synthetic `Entry` lists,
   no filesystem).
3. `apply_plan` filesystem function + unit tests using `TempDir`.
4. Recursive walk wiring — separate helper that discovers directories to
   plan-and-apply, feeding each into `plan_reindex` / `apply_plan`.
5. CLI command wiring (`Command::Reindex`, `run_reindex`, JSON payload).
6. Auto-run integration in `build`: source-mode hook (before scan), export-mode
   hook (rewrite manifest in scan output).
7. Docs: man-page-style section in README.md; update CLAUDE.md pipeline
   description if the auto-hook changes the stage list.

## 9. Resolved decisions

- Spacing is `10^N` (step = `10^spacing`).
- Always start at `1 × step`; no `--start` / `--offset` flag.
- Subcommand name is `reindex`.
- Recursive walk into both albums and groups by default; `--flat` opts out.
- `auto = "both"` kept as distinct from `source_only` for intent clarity.
- `[auto_indexing]` cascades per-directory.
- No `git mv`; plain `fs::rename`.
