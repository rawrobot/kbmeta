# Migration plan: MetadataBlock → okf crate

## Current state

kbmeta invented an "OKF-style" metadata format: Markdown + YAML frontmatter with 7
fields (`type`, `title`, `description`, `tags`, `topics`, `technologies`, `languages`).
This is implemented as:

- `MetadataBlock` struct with serde derives (`document/metadata_block.rs`)
- Custom YAML parsing via `serde_yaml` (same file)
- JSON round-trip for LLM responses (`provider/nvidia.rs` — `serde_json::from_str::<MetadataBlock>`)
- Editor round-trip for interactive suggest (`command/suggest.rs` — `serde_yaml::from_str::<MetadataBlock>`)
- Vocabulary canonicalization (`provider/trait.rs` — `SuggestionRules`, `apply_rules`, `derive_rules`)

## What okf gives us

The `okf` crate (v0.2.1) is a **zero-dependency** pure-Rust implementation of the
[Open Knowledge Format v0.2 spec](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md).
It provides:

- `okf::Document` = YAML frontmatter + markdown body — parse/serialize/validate
- `okf::Frontmatter` — ordered key/value mapping with typed getters:
  - `type_()`, `title()`, `description()` — standard OKF fields
  - `tags()` — built-in OKF tags list
  - `get(key)` / `set(key, value)` — arbitrary extension keys
  - `extension_keys()` — lists non-standard keys present
- `okf::validate_bundle` / `okf::lint_bundle` — conformance checking
- `okf::Bundle` — directory tree walker with link graph
- Own YAML-subset parser (no serde_yaml dependency)

**Critical**: `Frontmatter` is NOT a serde struct. It wraps an ordered `Mapping` and
exposes typed accessors. Custom keys (`topics`, `technologies`, `languages`) go through
`get()`/`set()` as `okf::Value` scalars or sequences.

## Field mapping

| kbmeta MetadataBlock | okf equivalent | Access method |
|---|---|---|
| `doc_type` (Option\<String\>) | `type` | `frontmatter.type_()` |
| `title` (Option\<String\>) | `title` | `frontmatter.title()` |
| `description` (Option\<String\>) | `description` | `frontmatter.description()` |
| `tags` (Vec\<String\>) | `tags` | `frontmatter.tags()` |
| `topics` (Vec\<String\>) | custom extension | `frontmatter.get("topics")` |
| `technologies` (Vec\<String\>) | custom extension | `frontmatter.get("technologies")` |
| `languages` (Vec\<String\>) | custom extension | `frontmatter.get("languages")` |

## Changes by file

### 1. Cargo.toml

- Add `okf = "0.2"` dependency
- Remove `serde_yaml` (used only for MetadataBlock; okf has its own YAML parser)

### 2. document/metadata_block.rs — DELETE or gut

**Option A (clean break)**: Delete the file entirely. All functionality moves to okf types.

**Option B (thin adapter)**: Keep as a compatibility shim with helper functions that
convert between okf::Frontmatter and the fields the pipeline needs.

**Recommended: Option A.** The file is 378 lines, mostly serde derives and custom YAML
parsing that okf replaces directly. A shim adds indirection with no benefit.

Functions replaced:
- `parse_markdown()` → `okf::Document::parse()`
- `to_yaml()` → `okf::Document::serialize()`
- `upsert_metadata_block()` → build `okf::Document`, serialize
- `missing_required_fields()` → `okf::validate` or check `type_().is_none()`
- `parse_markdown()` (YAML header detection) → `okf::Document::parse()` handles this

### 3. document/mod.rs

- Remove `pub mod metadata_block;` (or rename to adapter)
- Update `Document` struct to hold `okf::Document` instead of `Option<MetadataBlock>`
- Update `parse_document()` to use `okf::Document::parse()`

### 4. provider/trait.rs — SuggestionRules and apply_rules

Current flow: LLM returns `MetadataBlock` → `apply_rules()` canonicalizes fields.

New flow: LLM returns JSON → build `okf::Frontmatter` from JSON → `apply_rules()` works
on `okf::Frontmatter` directly.

Changes:
- `SuggestionRules` stays mostly the same (it's vocabulary canonicalization, not metadata format)
- `apply_rules()` signature changes from `MetadataBlock` → `okf::Frontmatter`
- `derive_rules()` stays the same (reads from existing `Document` structs)
- List field access: `fm.get("topics")` → parse as `Vec<String>` → canonicalize → `fm.set("topics", Value::Sequence(...))`

### 5. provider/nvidia.rs — LLM response parsing

Current: `serde_json::from_str::<MetadataBlock>(&content)`

New: Parse JSON into `okf::Frontmatter` via manual construction:
```rust
let json: serde_json::Value = serde_json::from_str(&content)?;
let mut fm = okf::Frontmatter::new();
if let Some(t) = json.get("type").and_then(|v| v.as_str()) {
    fm.set("type", okf::Value::Scalar(t.to_string()));
}
// ... same for title, description, tags, topics, technologies, languages
```

The LLM prompt stays nearly identical (JSON output with the same 7 keys). Only the
parsing target changes.

### 6. provider/mock.rs

Returns `MetadataBlock` → returns `okf::Frontmatter` (built with `set()` calls).

### 7. pipeline/mod.rs — Cook pipeline

- `CookArtifacts.metadata` type: `MetadataBlock` → `okf::Frontmatter`
- Validation gate: `missing_required_fields(&metadata)` → `metadata.type_().is_none()` + check title/description
- `upsert_metadata_block(&clean.markdown, &metadata)` → build `okf::Document` from clean markdown + frontmatter, serialize
- `write_intermediate_artifacts`: YAML dump changes from `serde_yaml::to_string` to `okf::Document::serialize`

### 8. command/suggest.rs — Interactive editor

Currently: metadata as YAML → editor → parse back as `MetadataBlock`

New: metadata as YAML (okf-serialized) → editor → parse back via `okf::Document::parse()`

The editor sees nearly identical YAML. The only difference is field ordering (okf has
`PREFERRED_KEY_ORDER`).

### 9. document/cleaner.rs

`CleanDocument` struct stays unchanged (it's about cleaned markdown, not metadata).
The `upsert_metadata_block` call in the pipeline changes to use okf serialization.

### 10. document/index_folders.rs

Uses `Document.metadata` → `Document.metadata` (now `okf::Frontmatter` or `okf::Document`).
Field access changes from `fm.title` to `fm.frontmatter.title()` etc.

### 11. database/schema.rs + rebuild.rs

SQLite stores `title` and `doc_type` as TEXT columns. These are populated from
`Document.metadata` during rebuild. Change source from `metadata.title` to
`metadata.frontmatter.title()` etc. No schema change needed.

### 12. query/render.rs

No change — it reads from SQLite, not from MetadataBlock.

### 13. command/cook.rs, command/context.rs, command/scan.rs, command/rebuild.rs

Pass-through types change. No logic changes.

## LLM prompt impact

The system prompt and `cook-default.md` tell the LLM to return JSON with 7 keys.
This stays the same. The LLM doesn't know about our Rust types — it returns JSON,
we parse it into `okf::Frontmatter`. No prompt changes needed.

## Migration order (dependency-safe)

1. **Add `okf` dep, remove `serde_yaml`** — Cargo.toml
2. **Replace metadata_block.rs** with thin adapter or delete + inline conversions
3. **Update provider/trait.rs** — `apply_rules` / `derive_rules` work on `okf::Frontmatter`
4. **Update provider/nvidia.rs** — JSON → `okf::Frontmatter` parsing
5. **Update provider/mock.rs** — return `okf::Frontmatter`
6. **Update pipeline/mod.rs** — `CookArtifacts.metadata` type, validation, serialization
7. **Update document/mod.rs** — `Document` struct uses `okf::Document`
8. **Update command/suggest.rs** — editor round-trip uses okf parse/serialize
9. **Update document/index_folders.rs** — field access
10. **Update database/rebuild.rs** — field access from Document
11. **Run full test suite**, fix breakage

## Risks

1. **okf YAML parser vs serde_yaml**: okf uses a YAML-subset parser. It handles
   block/flow collections, quoted/plain scalars, block scalars (`|`, `>`), and comments.
   It rejects anchors, tags, multiple documents. This is fine for OKF frontmatter but
   may reject edge cases that serde_yaml accepted. Test with existing documents.

2. **Frontmatter is not serde**: Can't `serde_json::from_str::<Frontmatter>()`. Need
   manual JSON → Frontmatter conversion. This is ~20 lines of code, not a blocker.

3. **Extension key types**: `topics`, `technologies`, `languages` are accessed via
   `get(key)` returning `Option<&Value>`. Need helper functions to extract
   `Vec<String>` from `Value::Sequence`.

4. **Serialize round-trip**: `okf::Document::serialize()` produces YAML frontmatter.
   Need to verify it produces output compatible with what the editor and pipeline expect.

## What we gain

- Spec-conformant OKF v0.2 frontmatter (provenance, trust, lifecycle fields available)
- `okf validate` / `okf lint` integration for free
- `okf index` replaces custom `index_folders.rs` logic
- Link graph / backlinks from `okf::Bundle`
- No more custom YAML parsing code
- Community-compatible format (other OKF tools can read our documents)
