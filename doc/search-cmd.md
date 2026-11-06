# `kbmeta search` Command Specification

## 1. Purpose

Add a new `kbmeta search` command for **fast, broad document retrieval**, primarily for LLM/agent use.

The command must hide the knowledge-base metadata taxonomy from the caller while still allowing optional precise metadata filtering.

The existing `kbmeta query` command remains unchanged.

### Command roles

| Command | Purpose |
|---|---|
| `kbmeta search` | Broad relevance search |
| `kbmeta search ... --tag/--topic/...` | Broad search with metadata constraints |
| `kbmeta query` | Precise metadata query |
| `kbmeta read` | Read document content |

---

# 2. Command-line interface

```text
kbmeta search [OPTIONS] <TEXT>
```

Examples:

```bash
kbmeta search "ssh key usage git"
```

```bash
kbmeta search "ssh git" --tag "+debug,-wip"
```

```bash
kbmeta search "concurrency" --tech rust
```

```bash
kbmeta search "networking" --topic "+linux,+embedded"
```

---

# 3. Options

```text
-l, --limit <LIMIT>
    Maximum number of results.
    Default: 10

-f, --format <FORMAT>
    Output format.
    Possible values:
      files
      table
      json
      markdown
    Default: files

-j, --json
    Equivalent to --format json.

-t, --tag <EXPRESSION>
    Restrict results using the existing tag expression syntax.

-o, --topic <EXPRESSION>
    Restrict results using the existing topic expression syntax.

-k, --tech <EXPRESSION>
    Restrict results using the existing technology expression syntax.

-l, --lang <EXPRESSION>
    Restrict results using the existing language expression syntax.

-d, --doc-type <TYPE>
    Restrict results to the specified document type.

-n, --title <TEXT>
    Restrict results to documents whose title contains TEXT.

--fail-empty
    Return a non-zero exit status when no documents match.
```

Use the project's existing short-option conventions where conflicts exist; long options are authoritative.

Global existing options remain available:

```text
-r, --root <ROOT>
--config <CONFIG>
--dry-run
```

---

# 4. Two search modes

`search` supports two distinct mechanisms.

## Mode A — Broad text search

```bash
kbmeta search "ssh key git"
```

The text is treated as search terms.

The implementation determines relevance across indexed metadata.

The caller does **not** need to know whether `git` is stored as:

- tag
- topic
- technology
- title
- description

There is no metadata taxonomy reasoning.

---

## Mode B — Broad search with metadata filters

Example:

```bash
kbmeta search "ssh git" --tag "+debug,-wip"
```

This means:

> Find documents relevant to `ssh git`, restricted to documents satisfying the tag expression.

The broad search determines relevance.

The metadata option restricts the candidate set.

---

# 5. Metadata expressions

Metadata filter syntax must be **exactly the same as the existing `kbmeta query` syntax**.

Examples:

```text
debug,rust
```

means:

```text
debug OR rust
```

```text
+debug,+rust
```

means:

```text
debug AND rust
```

```text
+debug,-wip
```

means:

```text
debug AND NOT wip
```

The existing parser should be reused.

Do **not** create a second expression parser for `search`.

---

# 6. Explicit metadata filters

Supported filters:

```text
--tag
--topic
--tech
--lang
--doc-type
--title
```

Examples:

```bash
kbmeta search "debugging" --tag "+debug,-wip"
```

```bash
kbmeta search "memory" --tech rust
```

```bash
kbmeta search "concurrency" --topic "+concurrency,-beginner"
```

```bash
kbmeta search "compiler" --lang rust
```

```bash
kbmeta search "Git" --doc-type Reference
```

```bash
kbmeta search "Git" --title ssh
```

---

# 7. Meaning of combined text and metadata search

Given:

```bash
kbmeta search "ssh git" --tag "+debug,-wip"
```

the processing is:

```text
TEXT SEARCH
    ssh
    git
       │
       ▼
candidate documents
       │
       ▼
TAG FILTER
    +debug
    -wip
       │
       ▼
final candidates
       │
       ▼
ranking
```

The metadata expression is a **constraint**, not another relevance term.

---

# 8. Multiple metadata fields

Different metadata filters are combined with **AND**.

For example:

```bash
kbmeta search "ssh git" \
    --tag "+debug,-wip" \
    --tech rust
```

means:

```text
relevant to "ssh git"
AND
tag expression matches
AND
technology expression matches
```

This is intentional because the user explicitly supplied multiple filters.

The implementation must never infer these filters from the text query.

---

# 9. Critical distinction from text search

This:

```bash
kbmeta search "ssh git"
```

does **not** mean:

```text
tag = ssh
AND
topic = git
AND
technology = git
```

It is a broad relevance search.

This:

```bash
kbmeta search "ssh git" --tag ssh --tech git
```

does mean:

```text
relevant to ssh git
AND
tag = ssh
AND
technology = git
```

because the user explicitly requested those filters.

---

# 10. Text tokenization

The text query should use simple deterministic tokenization.

Example:

```text
SSH key usage in Git
```

becomes approximately:

```text
ssh
key
usage
git
```

Normalize search terms to lowercase.

Do not modify the stored metadata.

---

# 11. Stop words

The initial implementation may remove a small fixed list of common words:

```text
a
an
the
in
of
for
to
with
and
or
is
are
how
what
where
```

Example:

```text
how to use ssh keys with git
```

becomes approximately:

```text
ssh
keys
git
```

Do not make stop-word configuration part of the first implementation.

---

# 12. Fields searched by broad text search

Search these indexed fields:

### Documents

```text
documents.title
documents.description
```

### Metadata

```text
tags.name
topics.name
technologies.name
languages.name
```

Do not read Markdown source files during `search`.

The SQLite index is the search source.

---

# 13. Relevance scoring

Use deterministic weighted scoring.

Initial weights:

| Field | Weight |
|---|---:|
| title | 10 |
| description | 5 |
| technology | 5 |
| topic | 4 |
| tag | 3 |
| language | 2 |

A document receives points for matching query terms.

A document matching multiple query terms should generally rank above one matching only a single term.

There is **no requirement for every text-search term to match**.

---

# 14. Exact and partial matches

Recommended initial scoring:

```text
exact token match       100% of field weight
prefix match             70%
substring match          40%
```

Example for a title weight of `10`:

```text
git       → 10
github    → 7
legit     → 4
```

Prefer token/word-boundary matching over arbitrary substring matching.

The implementation may initially use SQLite `LIKE` if necessary, provided the resulting behaviour remains deterministic.

---

# 15. Multiple terms

Example:

```bash
kbmeta search "ssh git"
```

Document A:

```text
title = Git SSH Configuration
```

matches both terms.

Document B:

```text
title = Git Branching
```

matches only `git`.

Document A should therefore rank higher.

The search is **OR-like with relevance ranking**, not strict AND.

---

# 16. Result identity

Every result must contain:

```text
id
path
title
score
```

Example:

```json
[
  {
    "id": 123,
    "path": "KB/ai/prompts/git/git_expert_system_prompt.md",
    "title": "Git Expert System Prompt",
    "score": 17
  }
]
```

The `path` must be the exact value stored in:

```sql
documents.path
```

Do not reconstruct or modify it.

---

# 17. JSON output

JSON is the primary format for agent use.

```bash
kbmeta search "ssh git" --json
```

Example:

```json
[
  {
    "id": 123,
    "path": "KB/ai/prompts/git/git_expert_system_prompt.md",
    "title": "Git Expert System Prompt",
    "score": 17
  },
  {
    "id": 456,
    "path": "KB/security/ssh.md",
    "title": "SSH Keys",
    "score": 12
  }
]
```

Do not include document contents.

Do not include unnecessary metadata in the initial result.

The agent should retrieve the document separately.

---

# 18. Files output

Default output may be:

```text
KB/ai/prompts/git/git_expert_system_prompt.md
KB/security/ssh.md
```

This is intended for shell use.

---

# 19. Table output

Example:

```text
ID    SCORE    TITLE                       PATH
123   17       Git Expert System Prompt   KB/ai/prompts/git/git_expert_system_prompt.md
456   12       SSH Keys                   KB/security/ssh.md
```

Keep the output compact.

---

# 20. Empty results

JSON:

```json
[]
```

Normal output:

```text
No matching documents found.
```

With:

```text
--fail-empty
```

the command exits with a non-zero status.

The command must **not** suggest alternative metadata values.

For example, it must not turn:

```text
git
```

into:

```text
version-control
git-tools
software
```

---

# 21. Search errors

Invalid metadata expressions should produce the same type of validation error as `kbmeta query`.

For example:

```bash
kbmeta search "git" --tag "version control"
```

must reject the invalid expression rather than inventing:

```text
version-control
```

The existing expression parser and validation should be reused.

---

# 22. Limit

Default:

```text
10
```

Example:

```bash
kbmeta search "git ssh" --limit 20 --json
```

The final output must contain no more than the requested number of documents.

The implementation may retrieve additional internal candidates before ranking.

---

# 23. Performance

`search` must remain cheap.

Requirements:

- SQLite only
- no LLM
- no external services
- no Markdown scanning
- no document reads
- prepared SQL statements
- avoid N+1 queries where practical
- deterministic results

The command is expected to run in milliseconds for a normal knowledge base.

---

# 24. Search/read separation

`search` finds candidates.

A separate `read` operation retrieves document contents.

The intended agent workflow is:

```text
User question
      │
      ▼
kbmeta search "ssh key git" --json
      │
      ▼
candidate documents
      │
      ▼
agent selects document
      │
      ▼
kbmeta read <id> --json
      │
      ▼
document content
      │
      ▼
answer
```

`search` must never read the Markdown content.

---

# 25. Recommended `read` interface

The search result contains:

```json
{
  "id": 123,
  "path": "KB/ai/prompts/git/git_expert_system_prompt.md",
  "title": "Git Expert System Prompt"
}
```

The preferred agent operation is:

```bash
kbmeta read 123 --json
```

The `id` is safer for the agent because it comes directly from the search result.

For human/shell use, path-based reading may also be supported:

```bash
kbmeta read "KB/ai/prompts/git/git_expert_system_prompt.md"
```

The two interfaces should resolve to the same document.

---

# 26. Relationship with `query`

Do not remove or change the semantics of `query`.

### Precise query

```bash
kbmeta query --tag "+debug,rust,-wip"
```

### Broad search

```bash
kbmeta search "debug rust"
```

### Broad search + precise constraint

```bash
kbmeta search "debug rust" --tag "+debug,rust,-wip"
```

This gives the CLI three useful levels:

```text
query
    precise metadata retrieval

search
    broad relevance retrieval

search + filters
    broad relevance retrieval constrained by metadata
```

---

# 27. Rust implementation

Keep the search implementation behind a dedicated interface.

For example:

```rust
pub struct SearchResult {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
    pub score: f64,
}
```

Conceptually:

```rust
pub fn search(
    conn: &Connection,
    text: &str,
    filters: &SearchFilters,
    limit: usize,
) -> Result<Vec<SearchResult>>;
```

Where:

```rust
pub struct SearchFilters {
    pub tags: Option<FilterExpression>,
    pub topics: Option<FilterExpression>,
    pub technologies: Option<FilterExpression>,
    pub languages: Option<FilterExpression>,
    pub doc_type: Option<String>,
    pub title: Option<String>,
}
```

Reuse the existing filter-expression implementation from `query`.

Do not implement a second expression parser.

---

# 28. Phase 1 implementation

Keep the first implementation deliberately small:

```text
kbmeta search <TEXT>
kbmeta search <TEXT> --tag <EXPR>
kbmeta search <TEXT> --topic <EXPR>
kbmeta search <TEXT> --tech <EXPR>
kbmeta search <TEXT> --lang <EXPR>
kbmeta search <TEXT> --doc-type <TYPE>
kbmeta search <TEXT> --title <TEXT>
kbmeta search <TEXT> --limit N
kbmeta search <TEXT> --json
```

Pipeline:

```text
text query
    ↓
tokenize
    ↓
normalize
    ↓
remove stop words
    ↓
SQLite candidate search
    ↓
apply explicit metadata filters
    ↓
calculate score
    ↓
deduplicate
    ↓
sort
    ↓
limit
    ↓
output
```

---

# 29. Acceptance criteria

### Broad search

```bash
kbmeta search "git"
```

returns Git-related documents.

### Multi-term search

```bash
kbmeta search "ssh git"
```

returns documents relevant to either or both concepts, ranked by relevance.

### Explicit tag expression

```bash
kbmeta search "debugging" --tag "+debug,rust,-wip"
```

uses the existing tag expression semantics.

### Explicit technology expression

```bash
kbmeta search "memory" --tech "+rust,-python"
```

uses the existing technology expression semantics.

### Combined search

```bash
kbmeta search "ssh git" --tag "+debug,-wip"
```

performs broad text retrieval constrained by the tag expression.

### No accidental taxonomy

```bash
kbmeta search "ssh git"
```

does not require the caller or LLM to decide whether `ssh` or `git` is a tag, topic, or technology.

### Exact path

If SQLite contains:

```text
KB/ai/prompts/git/git_expert_system_prompt.md
```

the JSON result contains exactly that path.

### JSON

```bash
kbmeta search "git" --json
```

returns:

```text
id
path
title
score
```

### Empty search

No matches returns:

```json
[]
```

without guessing alternatives.

### Performance

The command does not:

- invoke an LLM
- read Markdown documents
- scan the filesystem
- access external services

### Determinism

The same database and search request produce the same results and ordering.

---

## Final interface

The intended agent-facing API is therefore very small:

```bash
kbmeta search "ssh key usage git" --json
```

then:

```bash
kbmeta read 123 --json
```

Optional precision can be added without changing the basic model:

```bash
kbmeta search "ssh git" --tag "+debug,-wip" --json
```

This keeps the **LLM responsible for understanding the question**, while `kbmeta` remains responsible for **retrieval, metadata semantics, filtering, ranking, and exact document identity**.