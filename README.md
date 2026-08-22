# kbmeta

Knowledge base metadata CLI.

## Native Command Interface (Phase 1)

Preferred commands:

- `init`: bootstrap `.kb` state
- `cook`: one-shot doc processing flow (clean -> context -> suggest -> write output)
- `index`: reindex metadata DB and regenerate folder `index.md` files
- `scan`: validate and inspect index drift
- `query`: filter indexed documents and print results
- `clean`: optional advanced manual cleanup helper

Global options:

- `--root`, `-r`
- `--config`
- `--dry-run`

## Deprecated Commands (still available in Phase 1)

The following commands are deprecated and hidden from help, but still runnable for compatibility:

- `rebuild` -> use `index`
- `index-folders` -> use `index`
- `context` -> use `cook`
- `suggest` -> use `cook`

These commands print deprecation warnings when invoked.

## Deprecation Timeline

- Phase 1 (current): deprecated commands are hidden from help and print migration warnings.
- Phase 2 (next compatibility cycle): deprecated commands are removed from CLI entry points.
- Internal modules remain reusable for native commands.

## Changelog (Phase 1)

- Added native `index` command that orchestrates DB rebuild plus folder index generation.
- Added global `--dry-run` support for mutation command workflows.
- Added scoped indexing with repeated `--path` and cache-aware incremental rebuild behavior.
- Marked `rebuild`, `index-folders`, `context`, and `suggest` as deprecated.
- Added `query` command with reusable signed-filter expressions for tag/topic/tech/lang.

## Query Syntax

`query` supports signed expressions for metadata sets:

- Bare term: OR group (`rust,c++`)
- `+term`: required (AND)
- `-term`: excluded (AND NOT)

Examples:

- `--tag "rust,c++"` -> rust OR c++
- `--tag "+debug,rust,c++"` -> debug AND (rust OR c++)
- `--tag "+debug,-wip"` -> debug AND NOT wip

The same syntax is available for:

- `--tag` (`-g`)
- `--topic` (`-o`)
- `--tech` (`-k`)
- `--lang` (`-l`)

## Query Flags

- `-t, --doc-type <value>` exact type match
- `-g, --tag <expr>` signed tag expression
- `-o, --topic <expr>` signed topic expression
- `-k, --tech <expr>` signed technology expression
- `-l, --lang <expr>` signed language expression
- `-n, --title <text>` case-insensitive title contains
- `-f, --format files|csv|table|json` output format (default `files`)
- `-F, --fields <csv>` output fields for table/json
- `-L, --limit <n>` max rows
- `--fail-empty` non-zero exit on empty result
- `-j, --json` shortcut for `--format json`

Output defaults:

- `files`: one filename per line (root-relative)
- `csv`: header + comma-separated rows, default fields `path,title,type`
- `table`: fields `path,title,type`
- `json`: all available fields

## Examples

Run end-to-end metadata generation for one file:

```bash
kbmeta -r . cook knowledge/documents/new-note.md
```

Reindex all docs and regenerate folder indexes:

```bash
kbmeta -r . index
```

Reindex only selected folders and rewrite existing folder indexes:

```bash
kbmeta -r . index --path rust --path architecture --rewrite
```

Preview index actions without writing:

```bash
kbmeta -r . --dry-run index --path rust
```

Query for documents by metadata:

```bash
kbmeta -r . query --tag "+debug,rust,-wip"
```

Query in JSON with selected fields:

```bash
kbmeta -r . query --lang rust --json --fields path,title,tags
```

Query in CSV:

```bash
kbmeta -r . query --tag Rust --format csv --fields path,title,type
```
