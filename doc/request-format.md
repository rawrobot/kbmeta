
## Input format

Implement a tag-filter parser and SQL compiler for a CLI tool.

Generalization note:

- The same signed-filter grammar should be reusable for other set-like fields
  (topics, technologies, languages), not only tags.

The user passes a single string via `--tag "<expr>"`. Grammar (no nesting, no parentheses):

- Terms are comma-separated: `term1,term2,term3`
- A term with a leading `+` is REQUIRED (tag must be present) — AND'd together
- A term with a leading `-` is EXCLUDED (tag must NOT be present) — AND'd together
- A bare term (no prefix) belongs to an OR-group — at least one bare term must match
- All groups (required AND, excluded AND, OR-group) are combined with AND at the top level
- Tag names are alphanumeric plus `_`, `-`, `+`, `.` (e.g. "c++", "wip-2024") — only the
  FIRST character is inspected for the +/- prefix; the rest of the string is the tag name verbatim
- Whitespace around terms should be trimmed
- Empty terms (e.g. from "a,,b") should be ignored
- If the same tag appears with conflicting prefixes (e.g. "+rust,-rust"), raise a clear error
- Duplicate terms in the same group should be de-duplicated while preserving first-seen order
- If the input is empty or has zero terms, return "no filter" (matches everything)

### Examples
| Input                      | Meaning                                   |
|-----------------------------|--------------------------------------------|
| `rust,c++`                  | tag=rust OR tag=c++                        |
| `+debug,rust,c++`           | tag=debug AND (tag=rust OR tag=c++)        |
| `+debug,-wip`               | tag=debug AND NOT tag=wip                  |
| `+debug`                    | tag=debug                                  |
| `-wip`                      | NOT tag=wip                                |

## Output: intermediate representation

Parse into a struct/dataclass, not directly to SQL string:

    ParsedTagFilter {
      required: Vec<String>,   // AND, must all be present
      excluded: Vec<String>,   // AND, must all be absent
      any_of:   Vec<String>,   // OR, at least one must be present (empty = no constraint)
    }

## Output: SQL compilation

Given schema:
  documents(id, ...)
    tags(id, name)
  document_tags(document_id, tag_id)

Compile ParsedTagFilter into a parameterized WHERE clause fragment (placeholders, NOT string
interpolation — must be safe against SQL injection) using EXISTS/NOT EXISTS subqueries:

- Each `required` tag  -> one `EXISTS (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = i.id AND t.name = ?)`, AND'd together
- Each `excluded` tag  -> one `NOT EXISTS (... same shape ... AND t.name = ?)`, AND'd together
- `any_of` (if non-empty) -> a single `EXISTS (... WHERE it.item_id = i.id AND t.name IN (?, ?, ...))`
- Combine required + excluded + any_of clauses with AND; if all three are empty, emit no
  WHERE clause (or `1=1`)
- Return both the SQL fragment (with `?`/`$1`-style placeholders — specify which) and the
  ordered list of bound parameter values

Implementation detail for this project:

- SQL fragment uses alias `i` for `documents` table (compatible with `FROM documents i`)
- Placeholder style is `?` (rusqlite positional binds)
- SQL fragment references association tables using logical item-style names to stay reusable:
  `item_tags(item_id, tag_id)` and `tags(id, name)`

## Requirements

- Language: Rust
- DB placeholder style: `?` (rusqlite)
- Include unit tests covering: OR-only, required-only, excluded-only, mixed, conflicting
  +/- on same tag, empty input, malformed input (bare `+` or `-` with no tag name)
- Include unit test for invalid tag chars and deterministic parameter ordering in SQL output
- Function signatures:
    fn parse_tag_filter(input: &str) -> Result<ParsedTagFilter, ParseError>
    fn compile_to_sql(filter: &ParsedTagFilter) -> (String, Vec<String>)
- No external parser-combinator libraries needed — plain string splitting/matching only

Parser validation rules:

- Trim whitespace around each comma-separated term
- Inspect only first character for prefix classification (`+`, `-`, or bare)
- Treat the rest of term as raw tag name and validate allowed chars `[A-Za-z0-9_.+-]`
- Bare `+` or bare `-` is malformed and must return a clear parse error