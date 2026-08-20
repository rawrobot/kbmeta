# kbmeta

Knowledge base metadata CLI.

## Native Command Interface (Phase 1)

Preferred commands:

- `init`: bootstrap `.kb` state
- `cook`: one-shot doc processing flow (clean -> context -> suggest -> write output)
- `index`: reindex metadata DB and regenerate folder `index.md` files
- `scan`: validate and inspect index drift
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
