# kbmeta Pipeline Design

## Goals

- Make command internals composable so multi-step workflows do not duplicate logic.
- Keep only task-native commands in primary UX while preserving selected low-level tools for advanced use.
- Provide a one-shot command (`cook`) that produces KB-ready output without modifying input.
- Provide a single indexing command (`index`) that runs DB indexing and folder index generation.
- Keep all relative-path behavior deterministic by resolving from command `--root`.
- Keep LLM behavior configurable with root-local prompt files and safe fallback behavior.

## High-Level Organization

```mermaid
flowchart LR
    CLI[CLI Parser] --> CMD[Command Dispatcher]

    CMD --> C1[clean command]
    CMD --> C2[cook command]
    CMD --> C3[index command]
    CMD --> C4[scan command]
    CMD --> C5[query command]
    CMD --> C6[deprecated command adapters]

    C1 --> OPS[Pipeline Operations]
    C2 --> OPS
    C3 --> OPS
    C6 --> OPS

    C5 --> QRY[query filter engine]

    OPS --> DOC[document module]
    OPS --> PROV[provider module]
    OPS --> DB[database module]
    OPS --> CFG[config module]
    OPS --> PATH[pathing module]
    QRY --> DB
```

## Pipeline Operation Types

`src/pipeline/mod.rs` defines the abstract operation layer:

- `CleanOperation`
- `ContextOperation`
- `SuggestOperation`
- default implementations:
- `DefaultCleanOperation`
- `DefaultContextOperation`
- `DefaultSuggestOperation`

These abstractions let commands compose behavior rather than calling each other.

## Path Resolution Model

- All command inputs and config-derived relative paths resolve from `--root`.
- A shared helper (`pathing::resolve_from_root`) is used across command, pipeline, and config paths.
- Absolute paths are preserved as-is.

This prevents cwd-dependent behavior and keeps command execution deterministic.

## Cook Command Flow

```mermaid
sequenceDiagram
    participant U as User
    participant K as kbmeta cook
    participant O as Pipeline Ops
    participant P as Provider
    participant I as SQLite Index

    U->>K: cook INPUT
    K->>O: run clean operation
    O-->>K: cleaned markdown + filename
    K->>O: run context operation
    O-->>K: compact llm context
    K->>O: build rules from existing KB docs
    K->>O: load prompt from .kb/prompts/<prompt_name>.md
    K->>P: suggest metadata using llm context
    P-->>K: metadata suggestion
    K->>O: validate required fields + merge metadata
    K->>K: review action accept/edit/reject (interactive by default)
    K->>K: if output exists, ask overwrite confirmation
    O-->>K: final KB-ready markdown
    K->>K: write output into cook.output_dir
    K->>I: rebuild index
    K-->>U: final output path
```

  ## Init Command Flow

  ```mermaid
  sequenceDiagram
    participant U as User
    participant K as kbmeta init
    participant FS as Filesystem

    U->>K: init
    K->>FS: check .kb exists
    alt .kb exists and interactive tty
      K->>U: ask rewrite existing .kb?
      alt no
        K-->>U: skip init and keep existing state
      else yes
        K->>FS: remove .kb
        K->>FS: write fresh config and db
      end
    else .kb exists and non-interactive
      K-->>U: keep existing .kb (no rewrite)
    else .kb missing
      K->>FS: create .kb and defaults
    end
    K->>FS: ensure .kb/prompts/cook-default.md exists
  ```

## Data Shapes

```mermaid
classDiagram
    class CleanDocument {
      +title: Option<String>
      +source_url: Option<String>
      +source_type: String
      +filename: String
      +markdown: String
    }

    class LlmContext {
      +markdown: String
    }

    class MetadataBlock {
      +doc_type: Option<String>
      +title: Option<String>
      +description: Option<String>
      +tags: Vec<String>
      +topics: Vec<String>
      +technologies: Vec<String>
      +languages: Vec<String>
    }

    class CookArtifacts {
      +clean: CleanDocument
      +context: LlmContext
      +rules: SuggestionRules
      +metadata: MetadataBlock
      +final_markdown: String
      +output_path: PathBuf
      +prompt_file_path: Option<PathBuf>
      +prompt_name: String
      +expected_prompt_path: Option<PathBuf>
    }

    CookArtifacts --> CleanDocument
    CookArtifacts --> LlmContext
    CookArtifacts --> MetadataBlock
```

## Command Policy

Primary native commands:

- `init`
- `cook`
- `index`
- `scan`
- `query`
- `clean`

Deprecated-but-compatible commands in Phase 1:

- `rebuild` (use `index`)
- `index-folders` (use `index`)
- `context` (use `cook`)
- `suggest` (use `cook`)

Deprecation policy:

- deprecated commands are hidden from top-level help
- deprecated commands remain runnable for one compatibility cycle
- invoking a deprecated command prints a migration warning
- Phase 2 removes deprecated command entry points

- `cook` is non-destructive for source input.
- `cook` defaults to interactive review (accept/edit/reject prompt).
- batch/non-interactive mode uses explicit `--accept` (or `-a`).
- in batch mode, reject/edit are intentionally unavailable.
- interactive edit uses editor configured in config (or environment fallback).
- if output collision occurs:
- interactive mode asks whether to overwrite.
- batch mode (`--accept`) fails fast with collision error.
- metadata is always canonicalized through provider rules.
- tag normalization applies in suggest path and cook path.
- hard post-model limits are enforced:
- tags max 10
- topics max 5
- optional intermediates are controlled by config (`keep_intermediate`).
- `--dry-run` is a global flag at CLI root and applies to mutation commands.

## Index Policy

- `index` is the native indexing entry point.
- `index` orchestrates:
- DB index refresh (full or scoped)
- folder `index.md` generation
- `index --path <dir>` can be repeated to reindex selected folders only.
- `index --rewrite` rewrites existing folder `index.md` files.
- when `--dry-run` is set globally, both indexing phases are preview-only.

## Query Policy

- `query` is read-only and uses indexed metadata from SQLite.
- default output is filenames only (root-relative), one per line.
- signed-filter expression syntax is shared across set-like fields:
- bare term => OR group
- `+term` => required (AND)
- `-term` => excluded (AND NOT)
- reusable signed-filter parser/compiler powers these flags:
- `--tag` / `-g`
- `--topic` / `-o`
- `--tech` / `-k`
- `--lang` / `-l`
- query supports output modes:
- `files` (default)
- `csv` (header + comma-separated rows)
- `table` (`path,title,type` default fields)
- `json` (all fields by default)

```mermaid
flowchart TD
  A[kbmeta query flags] --> B[build SQL WHERE fragments]
  B --> C[signed filter parser]
  C --> D[relation SQL compiler]
  D --> E[SQLite documents query]
  E --> F[hydrate tag/topic/tech/lang fields]
  F --> G[render files or table or json]
```

## Rebuild Engine Policy

- `rebuild` without scope remains full refresh behavior.
- `rebuild --path <dir>` can be repeated to reindex selected folders only.
- scoped paths are resolved relative to `documents_root` (unless absolute).
- scoped rebuild is incremental and cache-aware:
- unchanged documents are skipped when `content_hash` matches stored value.
- changed documents are reinserted with updated vocab links.
- removed files inside scope are pruned from `documents` and link tables.
- orphaned vocab entries are cleaned after scoped updates.

```mermaid
flowchart TD
  A[index --path ...] --> B[discover docs under scopes]
  B --> C[load existing scoped docs + hashes]
  C --> D{hash changed?}
  D -- no --> E[skip unchanged]
  D -- yes --> F[replace document + links]
  B --> G[seen paths set]
  C --> H[existing scoped paths]
  G --> I[remove missing scoped docs]
  H --> I
  F --> J[cleanup orphan vocab]
  I --> J
```

## Phase 2 Plan

- remove `rebuild`, `index-folders`, `context`, and `suggest` from CLI surface
- keep internal modules reusable for `cook` and `index`
- retain migration guidance in docs for one more release after removal

## Config Model

Top-level:

- `documents_root`: source discovery root
- `keep_intermediate`: optional artifact persistence
- `editor`: interactive edit command

Cook section (`[cook]`):

- `prompt_name`: prompt loaded from `.kb/prompts/<name>.md`
- `max_filename_len`: output filename stem cap
- `output_dir`: destination for kb-ready cooked documents

Provider section (`[provider]`):

- provider kind/runtime settings (`mock` or `nvidia` etc.)

Default separation:

- `documents_root = "knowledge/documents"`
- `cook.output_dir = "knowledge/staging"`

## Prompt Selection and Fallback

- `cook` attempts to load `.kb/prompts/<prompt_name>.md` from root.
- If found and non-empty, it is used as the system prompt body.
- If missing/empty, cook falls back to provider built-in default prompt.
- Runtime output prints which prompt source is being used, including expected missing path when fallback happens.

## Why This Structure

- Composition: one code path for reusable operations.
- Testability: each operation can be tested independently.
- UX: task-oriented native interface reduces command sprawl and confusion.
- Extensibility: new pipelines can be built by reusing operation traits.
