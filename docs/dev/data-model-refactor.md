# Data Model Refactor — Images as First-Class Entities

> **Status:** design / pre-implementation. This refactor precedes the
> auto-reindex simplification (which is largely subsumed by it). Open
> questions are called out inline.

## 1. Motivation (validated against code)

Today the manifest is a strict tree: `Manifest { albums: Vec<Album> }`,
`Album { images: Vec<Image> }`. Two byte-identical images at two different
paths become two separate `Image` records — they share one cache entry
(content-addressed at `cache.rs:137`) but have duplicated metadata and
output bookkeeping.

Concrete consequences that surfaced during the analysis:

- **Ordering is entangled with filename identity.** The `NNN-` prefix is
  both an ordering hint and a bit of output-filename identity — renaming
  a file re-sorts the album AND rewrites every AVIF output filename for
  that image. The auto-reindex feature exists to paper over this
  entanglement.
- **Reusing an image across albums requires duplicating the file.** The
  cache dedupes the encode (work savings), but the manifest still models
  two `Image` records with independent sidecars, URLs, and positions.
- **"All Photos" has no dedup.** A flat iterator over
  `for album in manifest.albums; for image in album.images` would list a
  shared image twice.
- **Sidecar-per-path is a feature, not a bug.** The current metadata
  precedence is sidecar `.txt` > IPTC caption for description; IPTC >
  filename for title (`metadata.rs:29-35`). That means per-album caption
  overrides already work if the user puts different `.txt` next to each
  copy. Preserve this.

## 2. Target model

Proposed top-level shape (names indicative):

```rust
struct Manifest {
    config: SiteConfig,
    images: Vec<Image>,          // first-class, keyed by ImageId
    albums: Vec<Album>,
    pages: Vec<Page>,             // unchanged
    nav: Vec<NavItem>,            // unchanged
}

/// Stable identity for an image across the whole site.
struct ImageId(String);           // SHA-256 of the source file bytes

struct Image {
    id: ImageId,
    /// A "canonical" path used for IO (first occurrence wins).
    source_path: PathBuf,
    /// Raw pixel dimensions — content-derived, config-independent.
    width: u32,
    height: u32,
    /// IPTC-derived metadata resolved once per unique content.
    iptc_title: Option<String>,
    iptc_description: Option<String>,
    /// All source filesystem paths where this content appears.
    /// Lets us clean up stale output across renames.
    aliases: Vec<PathBuf>,
}

struct Album {
    slug: String,
    title: String,
    description: Option<String>,
    /// Image refs in album-specific order. Position is implicit (index).
    images: Vec<ImageRef>,
}

struct ImageRef {
    image_id: ImageId,
    /// Per-album filename-derived title (e.g. "Dawn" from `001-Dawn.jpg`).
    /// Only filename title — IPTC lives on the Image.
    filename_title: Option<String>,
    /// Per-album sidecar-derived description. Sidecar stays path-local.
    sidecar_description: Option<String>,
    /// Processed output variants for this ref, under this album's
    /// resolved config. A shared image in two albums with different
    /// `[images]` / `[thumbnails]` config has two different variant
    /// lists here — the cache (keyed by `content_hash + params_hash`)
    /// transparently deduplicates only when the params also match.
    variants: Vec<ProcessedVariant>,
}
```

**Important:** only content-derived fields live on `Image`. Anything that
depends on album config (responsive sizes, thumbnail aspect ratio,
quality, sharpening) lives under `ImageRef.variants` because config
cascades per-album — the same source image in Album A (sizes=[800, 1400])
and Album B (sizes=[400, 800]) produces two different sets of outputs.
The cache's existing `(content_hash, params_hash)` key still deduplicates
encode work only when the config actually matches.

Resolution rules (per-album, at render time):

- Title = `ref.filename_title` → `image.iptc_title` → fallback.
- Description = `ref.sidecar_description` → `image.iptc_description` →
  none.

The same `Image` in two albums sees two different `ImageRef`s, so
different `filename_title` / `sidecar_description` naturally flow
through. Single encode, single cache entry, dedup'd "All Photos."

## 3. Open design questions

### 3.1 Image identity: bytes only, or bytes + IPTC?

- **(a) Bytes only** (proposed): `ImageId = SHA-256(file_bytes)`. Any
  IPTC edit yields a new `ImageId` because the bytes change. Matches
  what the cache already does.
- **(b) Pixel bytes only:** decode → re-hash raw RGBA. Stable across
  IPTC edits but requires decoding every candidate, even cache hits —
  and costs a re-hash on every scan.

**Recommendation: (a).** The user's "new caption shouldn't invalidate
the images" goal is served better by moving caption off the Image
entirely (onto the `ImageRef` as `sidecar_description`). Bytes-only
identity keeps scan fast.

### 3.2 AVIF output filenames — per-album copies or shared pool?

A single content-hashed source now backs multiple albums' HTML pages.
Two ways:

- **(a) Per-album copy** (proposed): album output dirs each get their
  own `{source_stem}-{size}.avif`. The cache deduplicates the encode
  work; the output dir gets copies (cheap, already implemented at
  `process.rs:591-621`). No URL/path change. Deployment-compatible.
- **(b) Shared pool:** one `pool/{id_prefix}-{size}.avif` referenced
  from every album page. Cheaper disk in `dist/`, but every deployed
  site's HTML references change → full cache-bust for users.

**Recommendation: (a).** Disk in `dist/` is cheap; user trust in stable
URLs is expensive.

### 3.3 Sidecar resolution — per-ref (today) or per-image?

Today sidecars are read per-path during scan, so placing a different
`.txt` next to each copy gives you different descriptions. The new
model puts that on `ImageRef.sidecar_description`, preserving the same
behavior. No change needed. Keep it per-ref.

### 3.4 How does a user express "use this image in another album"?

Filesystem-is-source-of-truth is a core premise, so the answer has to
live on disk, not in a database. Options:

- **(a) Symlinks:** `content/other-album/07-sunset.jpg ->
  ../first-album/05-sunset.jpg`. Zero schema change. Scan already
  follows the path; content hash dedups automatically.
- **(b) Hard duplicates:** user just copies the file. Works today via
  the cache. No new mechanism needed — the refactor only dedups the
  manifest-level bookkeeping.
- **(c) A reference file:** `other-album/07-sunset.jpg.ref` pointing
  at a canonical path. Adds a new on-disk convention.

**Recommendation:** (b) is already how things work and needs zero UX
change. (a) is a natural extension users can choose themselves. Don't
add (c) unless a real use-case demands it.

### 3.5 JSON manifest schema — bump version, or maintain dual shape?

Tooling that reads `manifest.json` today expects the nested shape.

- **(a) Hard break with a version bump:** new `schema_version: 2` at
  root; consumers have to adapt. Simplest code.
- **(b) Dual-serialize for one release:** emit both shapes during a
  deprecation window.

**Recommendation: (a).** The generator, process stage, and generate
stage are the only in-repo consumers, and we update them in lockstep.
External tooling is not known to exist for this project. Cleaner to
cut once.

## 4. Phased implementation plan

Each phase is its own PR, each lands with a green suite, each is
reviewable standalone. Targets `main`.

### Phase 1 — Introduce canonical image IDs, scan links both views *(landed)*

- Add `ImageId`, `CanonicalImage { id, source_path, aliases }`,
  `Manifest.canonical_images: Vec<CanonicalImage>`, and
  `Image.canonical_id: Option<ImageId>` while keeping the existing
  `Album.images: Vec<Image>` shape exactly as it was. No type renames
  at this layer — consumer code still reads `album.images` and doesn't
  see the new fields yet.
- Scan populates both views in one pass: albums still contain their
  per-album `Image` records, and a post-scan `build_canonical_index`
  hashes each source file (SHA-256, reusing `cache::hash_file` so the
  manifest and cache share the same digest format), dedupes by
  `ImageId`, and stamps every ref's `canonical_id`.
- No behavior change. All existing tests still pass.
- Unit tests: byte-identical images in two albums collapse to one
  canonical entry with both paths captured (first scan occurrence
  wins `source_path`; others land in `aliases`); byte-different
  images with the same filename get separate IDs; `aliases` stays
  empty for singletons; every `Image.canonical_id` points at a real
  canonical entry.
- Landed: ~600 LOC, including the plan doc.

### Phase 2 — Migrate process to consume canonical images

- Process stage iterates `(album, &album.images[i], album.resolved_config)`
  tuples, resolving each image's `canonical_id` into
  `manifest.canonical_images` for shared content lookup. It computes
  the params hash from the album's resolved config and consults the
  cache; cache hits skip re-encoding, misses encode once and insert.
  **One encode per unique `(content_hash, params_hash)` across the
  whole site** — two albums with the same config on a shared image =
  one encode; two albums with different configs = two encodes. This
  is already the cache's existing contract (`cache.rs:137`).
- Output path construction still walks `album.images` for the
  per-album-copy layout. Each album image's produced variant paths
  get attached to the ref (new `variants` field added in this phase).
- Cache unchanged (already content-addressed + params-aware).
- Tests: reuse fixture + add (a) same bytes, same config, two albums
  → one encode, two output copies; (b) same bytes, different
  `[thumbnails].aspect_ratio` per album → two encodes, correct
  per-album outputs.
- Estimated: ~300-500 LOC.

### Phase 3 — Migrate generate to read canonical + refs

- Generate walks `album.images` as today, but for each image resolves
  title and description with precedence: ref-level (filename title,
  sidecar description) → canonical-level (IPTC, once populated on
  `CanonicalImage`) → none.
- Full-index page iterates `manifest.canonical_images` directly and
  renders one entry per unique content — natural dedup — linking into
  every album that references it. Existing template keeps working;
  the iteration source changes.
- URL construction: no changes (URLs are already ordinal-based per
  album).
- Tests: existing renderer tests should mostly pass with minor
  updates; add a two-album-same-image test that confirms one entry
  in `/all-photos/`.
- Estimated: ~400-700 LOC.

### Phase 4 — Promote canonical_images to authoritative + schema_version bump

- Move IPTC title/description, raw dimensions, and any other
  content-derived metadata onto `CanonicalImage` (populated by process
  when it first decodes the image).
- Simplify `Image` to the per-album reference shape: filename title,
  sidecar description, position, variants, `canonical_id`. This is
  the point at which we can rename `Image` → `ImageRef` and
  `CanonicalImage` → `Image` if we want the final naming; or keep
  the current names for stability.
- Bump the manifest's `schema_version` to `2`. Coordinate the bump
  with `arthur-debert/simple-gal-action` per §5.1.
- Update every remaining test that hand-constructs manifests.
- CHANGELOG + README updates.
- Estimated: ~200-400 LOC, mostly test migration.

### Phase 5 — Auto-reindex simplification (optional, post-refactor)

Once the refactor lands, the auto-reindex feature collapses:

- AVIF filename is already `{source_stem}-{size}.avif`, and source
  filenames' `NNN-` prefixes no longer affect ordering (order is
  `Vec<ImageRef>` position). So renaming source files has no build-
  output effect; it's purely cosmetic.
- `[auto_indexing].auto` config mode enum collapses to a single
  boolean (or goes away). Keep the `reindex` CLI command as a source
  tidying tool.
- Estimated: ~100-200 LOC, mostly deletion.

## 5. Migration & backward compatibility

- **Content directory:** no changes required. Users' filesystems are
  already valid input.
- **Deployed `dist/`:** URL paths unchanged (slug-based). AVIF output
  filenames unchanged (per-album copies). No cache-bust.
- **JSON manifest consumers:** hard break at Phase 4. See §5.1.
- **Cache on disk:** unchanged. Content hash + params hash is still
  the key.

### 5.1 GitHub Action coordination

The official `arthur-debert/simple-gal-action` is a known external
consumer (it installs the binary and runs `build`). Before shipping
Phase 4:

1. **Audit** the action's source for any manifest.json parsing. If it
   only invokes the CLI and ships `dist/`, no change is required and
   this whole concern evaporates.
2. **If it does parse the manifest**, open a tracking issue on
   `arthur-debert/simple-gal-action` describing the schema shape
   change and linking this doc.
3. **Release the action as a new major version** after Phase 4 lands.
   GitHub Actions don't auto-update across majors (users pin
   `@v1` or `@v2`), so a major bump gives existing users a stable
   floor on the old simple-gal/schema version while opting in to the
   new shape by bumping their workflow ref.
4. **Document the version compat** in both repos' READMEs:
   `simple-gal-action@v1` → `simple-gal` up to the last pre-refactor
   release; `simple-gal-action@v2` → `simple-gal` from v0.20.0+.

## 6. Interaction with auto-reindex

Auto-reindex as it ships in v0.19.0 is essentially a workaround for
the current model. After this refactor:

- The reindex CLI command stays useful as a source-tidying utility
  (nice `ls` / git diff). Output build is unaffected.
- `[auto_indexing].auto = "export_only"` becomes tautological — output
  is already normalized by virtue of ordinal-based naming. The enum
  can collapse or be dropped.
- The in-place rename of `source_only` is a purely optional
  convenience; no behavior hangs on it.

**Implication:** don't ship the auto-reindex simplification before the
refactor. The code that would normalize AVIF filenames is exactly the
code that gets rewritten in Phase 2.

## 7. Risks

1. **Sidecar-per-path behavior must be preserved.** Regression here
   silently reverts user-visible captions. Covered by tests, but worth
   flagging in every phase PR.
2. **Phase 1 doubles the manifest size briefly** (both shapes
   co-exist). Minor memory/serialization cost during the transition.
3. **Generate stage tests are fixture-heavy.** Phase 3 has the largest
   test-migration blast radius. Plan for it.
4. **Scan determinism.** When two albums reference the same image,
   which one becomes the `Image.source_path`? Rule: first-by-
   directory-scan-order. Test it explicitly.

## 8. Rough effort

- 5 PRs (phases), each independently reviewable + green CI.
- ~2-3 weeks of focused work with dense test coverage.
- Minor version bump (v0.20.0) when Phase 4 lands (schema_version
  bump is user-visible).

## 9. Decision log

All six design questions approved:

- [x] Image identity = **bytes only** (§3.1).
- [x] AVIF filenames = **per-album copies** (§3.2).
- [x] Sidecars stay **per-ref** (§3.3).
- [x] Cross-album reuse = **user duplicates or symlinks, no new
      convention** (§3.4).
- [x] Manifest schema = **hard break + version bump** (§3.5), with
      `simple-gal-action` coordination per §5.1.
- [x] Auto-reindex simplification **deferred** to Phase 5 (§6).

Additional decision:
- [x] Processed variants live on `ImageRef`, not `Image`, because
      config cascades per-album and the same source can have different
      responsive sizes / thumbnail ratios / quality in different
      albums (§2).

Phase 1 can start.
