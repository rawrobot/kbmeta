# Feature ideas / TODO

Ideas not yet scheduled. Each entry: motivation, sketch, costs, open questions.

---

## FTS5 for `kbmeta search`

### Status

Not started. Non-ASCII case folding, the main pain point, is already solved
without FTS5 by the `kb_lower` SQL function (see `src/query/search.rs`).
Revisit when one of the triggers below applies.

### Triggers: when FTS5 becomes worth it

- Search should cover document **bodies**, not only metadata.
- Index grows to a size where the full scan of metadata values becomes slow
  (current approach scans every title, description and vocab value per query).
- **Stemming** wanted (`key` matches `keys`, `configure` matches `configuration`).
- **Diacritic folding** wanted (`uber` matches `über`).

### What FTS5 gives

- `unicode61` tokenizer: Unicode case folding; optional diacritic removal
  (`remove_diacritics 0|1|2`).
- Optional `porter` stemmer (English only), stacked on `unicode61`.
- Inverted index: lookup cost does not grow with a full scan.
- `bm25()` ranking with per-column weights.
- Built into bundled SQLite (`rusqlite` `bundled` feature): no new crate.

### How it compares with the current scorer

| Current rule (`src/query/search.rs`)      | FTS5 equivalent                              |
|-------------------------------------------|----------------------------------------------|
| OR-matched terms                          | `a OR b`                                     |
| Field weights 10/5/5/4/3/2                | `bm25(fts, 10, 5, 5, 4, 3, 2)`, one column per field |
| Exact word = 1.0                          | Native token match                           |
| Prefix = 0.7 (`git` → `GitHub`)           | `git*` matches, but bm25 cannot tell prefix from exact |
| Substring = 0.4 (`git` → `Legit`)         | Not possible with `unicode61`; needs a second table with `trigram` tokenizer (terms < 3 chars never match) |
| Term counts once per field kind           | No: bm25 term frequency counts repeats       |
| Score rounded to 0.1, stable              | bm25 gives negative floats, IDF-dependent (changes as corpus changes) |
| Tie-break by path                         | `ORDER BY rank, path`                        |

### Design options

1. **Candidate pre-filter only (recommended first step).**
   FTS5 `MATCH` returns candidate doc ids; existing Rust `match_factor` and
   weights keep scoring. Ranking semantics unchanged. Loses substring-only
   hits unless a trigram table is added.
2. **Full bm25 ranking.** Simpler query, but output order and scores change;
   integration tests asserting ranking must be rewritten; scores no longer
   stable across corpus changes.
3. **Hybrid with trigram.** Two FTS tables (`unicode61` for word/prefix,
   `trigram` for substring). Closest to current behaviour, most maintenance.

### Schema sketch

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
  title, description, technologies, topics, tags, languages,
  tokenize = 'unicode61 remove_diacritics 0'
);
-- rowid = documents.id; vocab columns hold space-joined names
```

Use `content=''` (contentless) or a regular table; external-content tables
do not fit because vocab lives in link tables, not in `documents`.

### Hidden costs (do not skip)

- **Sync in every write path.** `rebuild_index`, `rebuild_index_scoped`
  (incremental), `delete_document_relations`, and any vocab change must
  rewrite the document's FTS row. A miss means silently stale search results.
  Add a test per path: rebuild, scoped rebuild, delete, tag change.
- **Schema migration.** Schema uses only `CREATE TABLE IF NOT EXISTS`, no
  version. Existing `.kb` databases get an empty FTS table and search returns
  nothing until a full rebuild. Introduce `PRAGMA user_version` and force a
  rebuild (or backfill) when the version bumps.
- **Query escaping.** `AND`, `OR`, `NOT`, `NEAR` and `"` are FTS5 syntax.
  Quote every term: `"term"` (or `"term"*` for prefix). Terms from
  `tokenize` are alphanumeric, so quoting is enough.
- **Tests.** The `db()` helper in `search.rs` inserts rows directly; it must
  also populate `documents_fts`, or go through the rebuild code.
- **Behaviour change.** Document any lost substring matches and ranking
  differences in `doc/search-cmd.md`.

### Open questions

- Fold diacritics or not? (`uber` → `über` is convenient for search, may be
  surprising for exact vocabulary terms.)
- Stemming: English-only `porter` acceptable for a mixed-language KB?
- Body indexing: index cleaned markdown at `index` time? Size impact on
  `.kb/*.db`?
