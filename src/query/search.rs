//! Broad relevance search over the SQLite index (`kbmeta search`).
//!
//! # Purpose
//!
//! `search` answers "which documents are about these words?" without making the
//! caller know the metadata taxonomy. A term such as `git` may be stored as a
//! title word, in a description, or as a tag, topic, technology or language; the
//! search looks in all of them and ranks the hits. This is the counterpart of
//! `kbmeta query`, which does precise metadata retrieval and no ranking.
//!
//! # Two inputs, two roles
//!
//! - **Text** (`text` argument of [`search`]) decides *relevance*. It is split
//!   into terms and matched OR-like: a document needs at least one matching
//!   term, and documents matching more terms (or in heavier fields) rank higher.
//! - **Filters** ([`SearchFilters`]) decide *eligibility*. They come only from
//!   explicit CLI options (`--tag`, `--topic`, `--tech`, `--lang`, `--doc-type`,
//!   `--title`), are combined with `AND`, and never add to the score. Filters
//!   are never inferred from the text.
//!
//! Filter expressions use the same parser, validation and SQL compiler as
//! `query` (see [`crate::query::filters`] and [`crate::query::signed_filter`]),
//! so `+a,-b,c` means "a AND NOT b AND (c)" in both commands.
//!
//! # Pipeline
//!
//! ```text
//! text ──► tokenize ──► terms ──┐
//!                               ▼
//!           one SQL query: every searchable value containing any term
//!           (instr(kb_lower(value), term) > 0), restricted to documents
//!           passing the filters
//!                               │
//!                               ▼
//!           per row: classify match (exact word / word prefix / substring)
//!           keep best factor per (term, field) for each document
//!                               │
//!                               ▼
//!           score = Σ field weight × best factor   (rounded to 0.1)
//!                               │
//!                               ▼
//!           sort by score desc, then path asc ──► truncate to limit
//! ```
//!
//! # Tokenization
//!
//! [`tokenize`] splits on every non-alphanumeric character, lowercases, drops
//! the fixed [`STOP_WORDS`] list and removes duplicate terms while keeping first
//! occurrence order. `"How to use SSH keys with Git"` becomes
//! `["use", "ssh", "keys", "git"]`. Stored metadata is never modified; values
//! are lowercased only in memory for comparison.
//!
//! # Searched fields and weights
//!
//! | Field                 | Source                         | Weight |
//! |-----------------------|--------------------------------|-------:|
//! | [`Field::Title`]       | `documents.title`              |     10 |
//! | [`Field::Description`] | `documents.description`        |      5 |
//! | [`Field::Technology`]  | `technologies.name`            |      5 |
//! | [`Field::Topic`]       | `topics.name`                  |      4 |
//! | [`Field::Tag`]         | `tags.name`                    |      3 |
//! | [`Field::Language`]    | `languages.name`               |      2 |
//!
//! # Match quality
//!
//! Each field value is compared to each term (see [`match_factor`]):
//!
//! | Match                                      | Factor | `git` against      |
//! |--------------------------------------------|-------:|--------------------|
//! | exact: whole value or one of its words     |    1.0 | `Git Branching`    |
//! | prefix: a word of the value starts with it |    0.7 | `GitHub Actions`   |
//! | substring: anywhere else in the value      |    0.4 | `Legit`            |
//!
//! "Words" of a value are produced by the same splitting as the query text, so
//! `version-control` has the words `version` and `control`.
//!
//! # Scoring
//!
//! For each document, the best factor is kept per `(term, field)` pair; the
//! score is the sum of `weight × factor` over those pairs. Consequences:
//!
//! - A term counts once per field kind, so a document with ten tags containing
//!   `git` gets the tag weight once, not ten times.
//! - The same term in several field kinds adds up: title `Git ...` (10) plus
//!   technology `git` (5) gives 15.
//! - Every additional matching term adds its own contribution, so a document
//!   matching `ssh` *and* `git` outranks one matching only `git` in the same
//!   fields.
//!
//! Scores are rounded to one decimal to keep JSON output stable and readable.
//!
//! # Determinism
//!
//! Results depend only on the database contents and the request. Ties on score
//! are broken by `documents.path` ascending, so equal inputs always produce the
//! same order. The internal `HashMap` iteration order never leaks into output
//! because results are fully sorted before truncation.
//!
//! # Performance
//!
//! A single prepared statement fetches all candidate rows, so there are no
//! per-document queries (no N+1). The SQL `kb_lower` pre-filter removes values that
//! cannot match; precise classification and scoring happen in Rust. The command
//! never reads Markdown files, scans the filesystem, or calls an LLM.
//!
//! # Known limitations
//!
//! - Case folding uses Rust [`str::to_lowercase`] (Unicode-aware, via the
//!   `kb_lower` SQL function registered by [`search`]), so `über` finds `Über`.
//!   Diacritics are not folded: `uber` does not find `über`.
//! - Substring matching is literal: no stemming (`key` ≠ `keys` except by
//!   prefix), no synonyms, no typo tolerance.
//! - Weights, factors and stop words are compile-time constants by design.

use crate::query::filters::{DOCUMENT_ALIAS, MetadataFilters};
use anyhow::{Context, Result};
use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, ToSql, params_from_iter};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

/// Explicit metadata constraints for `search`.
///
/// Same type as the `query` filters: tag/topic/technology/language expressions
/// parsed by the shared signed-filter parser, plus exact `doc_type` and
/// case-insensitive `title` substring. All present filters are combined with
/// `AND`. They restrict the candidate set only; they never contribute to the
/// score and are never derived from the text query.
pub type SearchFilters = MetadataFilters;

/// Common words removed from the text query before matching.
///
/// Fixed by design (not configurable). They would otherwise match as
/// substrings almost everywhere (`a`, `in`, `to`) and flood the results.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "in", "of", "for", "to", "with", "and", "or", "is", "are", "how", "what",
    "where",
];

/// Factor applied to a field weight when a term equals the value or one of its words.
const EXACT_FACTOR: f64 = 1.0;
/// Factor applied when a word of the value starts with the term (`git` in `github`).
const PREFIX_FACTOR: f64 = 0.7;
/// Factor applied when the term occurs only inside a word (`git` in `legit`).
const SUBSTRING_FACTOR: f64 = 0.4;

/// Indexed field searched by broad text search.
///
/// Each variant corresponds to one branch of [`FIELD_VALUES_SQL`] and carries
/// its own relevance weight (see [`Field::weight`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Field {
    /// `documents.title`.
    Title,
    /// `documents.description`.
    Description,
    /// `technologies.name` linked via `document_technologies`.
    Technology,
    /// `topics.name` linked via `document_topics`.
    Topic,
    /// `tags.name` linked via `document_tags`.
    Tag,
    /// `languages.name` linked via `document_languages`.
    Language,
}

impl Field {
    /// Relevance weight of a full (exact) match in this field.
    ///
    /// Title is the strongest signal of what a document is about; description
    /// and technology are next; languages are the weakest because many
    /// documents share them.
    fn weight(self) -> f64 {
        match self {
            Self::Title => 10.0,
            Self::Description => 5.0,
            Self::Technology => 5.0,
            Self::Topic => 4.0,
            Self::Tag => 3.0,
            Self::Language => 2.0,
        }
    }

    /// Maps the numeric `kind` column of [`FIELD_VALUES_SQL`] back to a field.
    ///
    /// Returns `None` for unknown codes so a mismatch between SQL and Rust is
    /// skipped instead of panicking.
    fn from_kind(kind: i64) -> Option<Self> {
        Some(match kind {
            0 => Self::Title,
            1 => Self::Description,
            2 => Self::Technology,
            3 => Self::Topic,
            4 => Self::Tag,
            5 => Self::Language,
            _ => return None,
        })
    }
}

/// Flattens every searchable value into rows of `(doc_id, kind, value)`.
///
/// Documents contribute title and description; vocabulary tables contribute one
/// row per linked name. `kind` is a small integer code decoded by
/// [`Field::from_kind`]. `UNION ALL` is used because duplicates cannot occur
/// (each branch reads a different source) and it avoids a sort.
const FIELD_VALUES_SQL: &str = "
    SELECT id AS doc_id, 0 AS kind, title AS value FROM documents WHERE title IS NOT NULL
    UNION ALL
    SELECT id, 1, description FROM documents WHERE description IS NOT NULL
    UNION ALL
    SELECT l.document_id, 2, v.name FROM document_technologies l JOIN technologies v ON v.id = l.technology_id
    UNION ALL
    SELECT l.document_id, 3, v.name FROM document_topics l JOIN topics v ON v.id = l.topic_id
    UNION ALL
    SELECT l.document_id, 4, v.name FROM document_tags l JOIN tags v ON v.id = l.tag_id
    UNION ALL
    SELECT l.document_id, 5, v.name FROM document_languages l JOIN languages v ON v.id = l.language_id";

/// Single ranked search hit.
///
/// Deliberately minimal: identity plus score, no content and no metadata
/// lists. Serialized as-is for `--json` output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    /// `documents.id`; stable for the lifetime of the index.
    pub id: i64,
    /// Exact `documents.path` value, never rewritten or made relative.
    pub path: String,
    /// `documents.title`, `None` when the document has no title.
    pub title: Option<String>,
    /// Relevance score, rounded to one decimal; higher is more relevant.
    pub score: f64,
}

/// Turns free text into search terms.
///
/// Steps: split on non-alphanumeric characters, lowercase, drop
/// [`STOP_WORDS`], drop duplicates (first occurrence wins). Output contains
/// only alphanumeric characters and is bound as a plain SQL parameter.
///
/// ```text
/// "SSH key usage in Git"          -> ["ssh", "key", "usage", "git"]
/// "how to use ssh keys with git"  -> ["use", "ssh", "keys", "git"]
/// "git, GIT git"                  -> ["git"]
/// ```
pub fn tokenize(text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    words(text)
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

/// Lowercased alphanumeric words of `text`, in order, duplicates kept.
///
/// Shared by query tokenization and field-value matching so both sides split
/// text identically.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

/// Ranks documents by weighted relevance of `text` across indexed metadata,
/// restricted to documents satisfying `filters`.
///
/// Matching is OR-like: a document needs at least one matching term. Results are
/// ordered by score descending, then path ascending, so output is deterministic.
///
/// Returns an empty list (not an error) when `text` has no terms after
/// tokenization, when `limit` is 0, or when nothing matches. Invalid filter
/// expressions are rejected earlier, by [`SearchFilters::parse`].
///
/// # Errors
///
/// Fails only on SQLite errors (prepare/execute/row decoding).
pub fn search(
    conn: &Connection,
    text: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let terms = tokenize(text);
    if terms.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    register_kb_lower(conn)?;

    // Candidate query: one substring test per term, OR-ed, so any value
    // containing any term is returned. `kb_lower` folds case with the same
    // Unicode rules as `tokenize`, so the SQL pre-filter and the Rust
    // classification agree. Bind order must follow placeholder order in the
    // SQL: terms first, then filter binds appended below.
    let (where_parts, mut binds) = filters.where_clauses();
    let mut sql = format!(
        "SELECT d.id, d.path, d.title, f.kind, f.value
         FROM ({FIELD_VALUES_SQL}) f
         JOIN documents d ON d.id = f.doc_id
         WHERE ({})",
        vec!["instr(kb_lower(f.value), ?) > 0"; terms.len()].join(" OR ")
    );
    let mut term_binds: Vec<String> = terms.clone();
    term_binds.append(&mut binds);
    let binds = term_binds;

    // Filters are applied as a subquery over `documents` aliased as
    // `DOCUMENT_ALIAS`, because the shared filter SQL references that alias.
    if !where_parts.is_empty() {
        sql.push_str(&format!(
            " AND d.id IN (SELECT {DOCUMENT_ALIAS}.id FROM documents {DOCUMENT_ALIAS} WHERE {})",
            where_parts.join(" AND ")
        ));
    }

    let bind_refs: Vec<&dyn ToSql> = binds.iter().map(|v| v as &dyn ToSql).collect();
    let mut stmt = conn
        .prepare(&sql)
        .with_context(|| "failed to prepare search query")?;
    let rows = stmt
        .query_map(params_from_iter(bind_refs), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .with_context(|| "failed to execute search query")?;

    // Classify every candidate row. The SQL test only guarantees a raw
    // substring match; `match_factor` decides exact / prefix / substring precisely.
    let mut docs: HashMap<i64, Candidate> = HashMap::new();
    for row in rows {
        let (id, path, title, kind, value) = row?;
        let Some(field) = Field::from_kind(kind) else {
            continue;
        };
        let doc = docs.entry(id).or_insert_with(|| Candidate {
            path,
            title,
            best: HashMap::new(),
        });
        let value_words: Vec<String> = words(&value).collect();
        let value_lower = value.to_lowercase();
        for (term_idx, term) in terms.iter().enumerate() {
            let factor = match_factor(term, &value_lower, &value_words);
            if factor > 0.0 {
                let best = doc.best.entry((term_idx, field)).or_insert(0.0);
                *best = best.max(factor);
            }
        }
    }

    // Score. Every candidate has a substring hit, so score is positive;
    // the `score > 0` guard is defensive.
    let mut results: Vec<SearchResult> = docs
        .into_iter()
        .filter_map(|(id, doc)| {
            // Per term, each field counts once (its best value), so many matching
            // tags cannot outweigh a title match.
            let score: f64 = doc
                .best
                .iter()
                .map(|((_, field), factor)| field.weight() * factor)
                .sum();
            (score > 0.0).then(|| SearchResult {
                id,
                path: doc.path,
                title: doc.title,
                score: (score * 10.0).round() / 10.0,
            })
        })
        .collect();

    // Full sort before truncation: HashMap order must never affect output.
    results.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.path.cmp(&b.path))
    });
    results.truncate(limit);
    Ok(results)
}

/// Per-document scoring state accumulated while reading candidate rows.
struct Candidate {
    /// Exact `documents.path`, carried through to [`SearchResult::path`].
    path: String,
    /// `documents.title`, carried through to [`SearchResult::title`].
    title: Option<String>,
    /// Best match factor per (term index, field).
    ///
    /// Keeping only the maximum per pair is what makes a term count once per
    /// field kind, regardless of how many values of that kind match.
    best: HashMap<(usize, Field), f64>,
}

/// Grades how well `term` matches one field value.
///
/// `value_lower` is the whole value lowercased; `value_words` is the value
/// split by [`words`]. Checks run from strongest to weakest and the first hit
/// wins:
///
/// 1. [`EXACT_FACTOR`] — whole value equals the term, or any word does.
/// 2. [`PREFIX_FACTOR`] — any word starts with the term.
/// 3. [`SUBSTRING_FACTOR`] — term occurs anywhere in the value.
/// 4. `0.0` — no match.
fn match_factor(term: &str, value_lower: &str, value_words: &[String]) -> f64 {
    if value_lower == term || value_words.iter().any(|w| w == term) {
        EXACT_FACTOR
    } else if value_words.iter().any(|w| w.starts_with(term)) {
        PREFIX_FACTOR
    } else if value_lower.contains(term) {
        SUBSTRING_FACTOR
    } else {
        0.0
    }
}

/// Registers `kb_lower(text)`: Unicode-aware lowercase via [`str::to_lowercase`].
///
/// SQLite's built-in `lower()` and `LIKE` fold ASCII only. Registered per call
/// because the function lives on the connection; re-registering is cheap and
/// keeps `search` usable on any connection. NULL in, NULL out.
fn register_kb_lower(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "kb_lower",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|s| s.to_lowercase())),
    )
    .with_context(|| "failed to register kb_lower SQL function")
}

#[cfg(test)]
mod tests {
    use super::{SearchFilters, match_factor, search, tokenize, words};
    use crate::database::schema::ensure_schema;
    use rusqlite::Connection;

    fn factor(term: &str, value: &str) -> f64 {
        let words: Vec<String> = words(value).collect();
        match_factor(term, &value.to_lowercase(), &words)
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn.execute_batch(
            "
            INSERT INTO documents(id, path, title, description, doc_type, content_hash, indexed_at_utc) VALUES
              (1, 'KB/git/ssh.md', 'Git SSH Configuration', 'Keys for remotes', 'Reference', 'h', 't'),
              (2, 'KB/git/branching.md', 'Git Branching', NULL, 'Guide', 'h', 't'),
              (3, 'KB/rust/memory.md', 'Memory Model', 'Ownership and borrowing', 'Reference', 'h', 't'),
              (4, 'KB/misc/legit.md', 'Legit Tricks', NULL, 'Note', 'h', 't');
            INSERT INTO tags(id, name) VALUES (1, 'debug'), (2, 'wip'), (3, 'ssh');
            INSERT INTO document_tags VALUES (1, 1), (1, 3), (2, 1), (2, 2);
            INSERT INTO technologies(id, name) VALUES (1, 'git'), (2, 'rust');
            INSERT INTO document_technologies VALUES (1, 1), (2, 1), (3, 2);
            ",
        )
        .unwrap();
        conn
    }

    fn paths(results: &[super::SearchResult]) -> Vec<&str> {
        results.iter().map(|r| r.path.as_str()).collect()
    }

    #[test]
    fn tokenize_lowercases_and_drops_stop_words() {
        assert_eq!(tokenize("How to use SSH keys with Git"), vec!["use", "ssh", "keys", "git"]);
        assert_eq!(tokenize("SSH key usage in Git"), vec!["ssh", "key", "usage", "git"]);
        assert_eq!(tokenize("git, GIT git"), vec!["git"]);
        assert!(tokenize("the of and").is_empty());
    }

    #[test]
    fn match_factor_prefers_exact_then_prefix_then_substring() {
        assert_eq!(factor("git", "Git Branching"), 1.0);
        assert_eq!(factor("git", "GitHub Actions"), 0.7);
        assert_eq!(factor("git", "Legit"), 0.4);
        assert_eq!(factor("git", "Rust"), 0.0);
    }

    #[test]
    fn multi_term_match_ranks_higher() {
        let conn = db();
        let results = search(&conn, "ssh git", &SearchFilters::default(), 10).unwrap();
        assert_eq!(results[0].path, "KB/git/ssh.md");
        assert_eq!(results[1].path, "KB/git/branching.md");
        assert!(results[0].score > results[1].score);
    }

    #[test]
    fn scores_use_field_weights() {
        let conn = db();
        let results = search(&conn, "git", &SearchFilters::default(), 10).unwrap();
        // title exact (10) + technology exact (5)
        assert_eq!(results[0].score, 15.0);
        let legit = results.iter().find(|r| r.path == "KB/misc/legit.md").unwrap();
        assert_eq!(legit.score, 4.0);
    }

    #[test]
    fn ties_are_ordered_by_path() {
        let conn = db();
        let results = search(&conn, "git", &SearchFilters::default(), 10).unwrap();
        assert_eq!(
            paths(&results),
            vec!["KB/git/branching.md", "KB/git/ssh.md", "KB/misc/legit.md"]
        );
    }

    #[test]
    fn tag_expression_restricts_candidates() {
        let conn = db();
        let filters =
            SearchFilters::parse(None, None, Some("+debug,-wip"), None, None, None).unwrap();
        let results = search(&conn, "git", &filters, 10).unwrap();
        assert_eq!(paths(&results), vec!["KB/git/ssh.md"]);
    }

    #[test]
    fn multiple_filters_are_anded() {
        let conn = db();
        let filters =
            SearchFilters::parse(Some("Reference"), None, None, None, Some("rust"), None).unwrap();
        let results = search(&conn, "memory git", &filters, 10).unwrap();
        assert_eq!(paths(&results), vec!["KB/rust/memory.md"]);
    }

    #[test]
    fn invalid_expression_is_rejected() {
        let err = SearchFilters::parse(None, None, Some("version control"), None, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("invalid filter expression"));
    }

    #[test]
    fn limit_and_empty_results() {
        let conn = db();
        let results = search(&conn, "git", &SearchFilters::default(), 1).unwrap();
        assert_eq!(results.len(), 1);
        let none = search(&conn, "kubernetes", &SearchFilters::default(), 10).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn non_ascii_case_is_folded() {
        let conn = db();
        conn.execute_batch(
            "
            INSERT INTO documents(id, path, title, description, doc_type, content_hash, indexed_at_utc)
              VALUES (5, 'KB/de/ueber.md', 'Über Ärger', NULL, 'Note', 'h', 't');
            INSERT INTO tags(id, name) VALUES (4, 'ŁÓDŹ');
            INSERT INTO document_tags VALUES (5, 4);
            ",
        )
        .unwrap();

        // Exact word in title, typed in either case.
        for text in ["über", "ÜBER"] {
            let results = search(&conn, text, &SearchFilters::default(), 10).unwrap();
            assert_eq!(paths(&results), vec!["KB/de/ueber.md"], "query {text:?}");
            assert_eq!(results[0].score, 10.0);
        }
        // Prefix of a non-ASCII word, and a non-ASCII tag.
        assert_eq!(
            paths(&search(&conn, "ärg", &SearchFilters::default(), 10).unwrap()),
            vec!["KB/de/ueber.md"]
        );
        assert_eq!(
            paths(&search(&conn, "łódź", &SearchFilters::default(), 10).unwrap()),
            vec!["KB/de/ueber.md"]
        );
        // Diacritics are not folded.
        assert!(search(&conn, "uber", &SearchFilters::default(), 10).unwrap().is_empty());
    }
}
