# kbmeta

Knowledge base metadata CLI for Markdown documents following the OKF spec
(see [doc/OKF-02-SPEC.md](doc/OKF-02-SPEC.md)).

kbmeta generates YAML frontmatter with an LLM, keeps tags/topics/technologies
consistent through controlled vocabularies, indexes documents into SQLite, and
lets you filter or search them from the command line.

## Install

```bash
cargo install --path .
```

## Quick start

```bash
kbmeta init                                   # create .kb/ state, prompt, vocabularies
kbmeta cook notes/new-note.md                 # generate metadata, write kb-ready copy
kbmeta index                                  # index documents, regenerate index.md files
kbmeta search "ssh keys git"                  # ranked search
kbmeta query --tag "+debug,rust,-wip"         # exact metadata filter
```

## Commands

| Command  | Purpose |
|----------|---------|
| `init`   | Create `.kb/` (config, SQLite index, default prompt, vocabularies). |
| `cook`   | One-shot pipeline: clean → build LLM context → suggest metadata → write kb-ready output. |
| `index`  | Rebuild the SQLite index and generate folder `index.md` files. |
| `scan`   | Report documents with missing or stale metadata. |
| `query`  | Filter indexed documents by exact metadata. No ranking. |
| `search` | Ranked relevance search over indexed metadata, with optional filters. |
| `clean`  | Clean a Markdown document using a source-specific policy (advanced, manual). |

Global options:

- `-r, --root <dir>`: workspace root (default `.`)
- `--config <file>`: config path; overrides `KBMETA_CONFIG` and `.kb/config.toml`
- `--dry-run`: show planned actions without writing files

Run `kbmeta <command> --help` for the full flag list.

### init

Creates `.kb/config.toml`, `.kb/index.sqlite`, the default cook prompt
`.kb/prompts/cook-default.md`, and copies controlled vocabularies from
`doc/knowledge/*.yaml` into `.kb/vocab/` when present. If `.kb/` already
exists, asks before rewriting it; in a non-interactive shell it leaves it alone.

### cook

```bash
kbmeta cook <input.md> [-a|--accept] [-r|--rewrite]
```

`cook` is the only command that calls an LLM. It sends the document to the
configured provider and gets its metadata back:

1. **Clean**: strip source-specific noise from the Markdown.
2. **Context**: build the LLM input from the cleaned document.
3. **Suggest**: send it to the provider (`[provider]` in config) with the
   system prompt `.kb/prompts/<cook.prompt_name>.md` and the controlled
   vocabularies. The model returns JSON: `type`, `title`, `description`,
   `tags`, `topics`, `technologies`, `languages`, `filename`, and optionally
   `new_terms`.
4. **Write**: validate the JSON, canonicalize terms, and write the document
   with YAML frontmatter to the output directory.

With `kind = "mock"` no LLM is called; a fixed suggestion is returned, which is
useful for trying the workflow offline.

- Interactive by default: prints suggested metadata and asks
  `a=accept, e=edit, r=reject`. Edit opens `editor` from config.
- `--accept`: batch mode, no prompt. Fails if the output file exists unless
  `--rewrite` is given.
- Output goes to `cook.output_dir` (default `knowledge/staging`), file name
  derived from the suggested title.
- Suggestions are canonicalized against the controlled vocabulary. Terms the
  model reports as new are re-checked; accepted candidates are printed for
  manual review and never added to `.kb/vocab/` automatically.

### index

```bash
kbmeta index [--path <subdir>]... [--rewrite] [--strict] [--normalize-tags]
```

- `--path`: reindex only these subfolders of `documents_root` (repeatable).
  Unchanged documents are skipped using content hashes.
- `--rewrite`: overwrite existing `index.md` files.
- `--strict`: fail on any document with invalid metadata.
- `--normalize-tags`: lowercase, trim, spaces to dashes before indexing.

### scan

```bash
kbmeta scan [--skip-invalid]
```

### query

Exact metadata filtering.

| Flag | Meaning |
|------|---------|
| `-t, --doc-type <value>` | exact type match |
| `-g, --tag <expr>` | tag expression |
| `-o, --topic <expr>` | topic expression |
| `-k, --tech <expr>` | technology expression |
| `-l, --lang <expr>` | language expression |
| `-n, --title <text>` | case-insensitive title substring |
| `-f, --format files\|csv\|table\|json\|markdown` | output format (default `files`) |
| `-F, --fields <csv>` | fields for csv/table/json |
| `-L, --limit <n>` | max rows |
| `--fail-empty` | non-zero exit when nothing matches |
| `-j, --json` | shortcut for `--format json` |

Output defaults: `files` prints one root-relative path per line; `csv` and
`table` use `path,title,type`; `json` prints all fields.

### search

Ranked search: answers "which documents are about these words?" without
knowing the taxonomy.

```bash
kbmeta search "<text>" [filters] [-f files|table|json|markdown] [-L <n>] [--fail-empty] [-j]
```

- Text is split into terms (stop words dropped) and matched OR-like across
  title, description, technologies, topics, tags and languages.
- Score = field weight × match quality, summed per term and field kind.
  Weights: title 10, description 5, technology 5, topic 4, tag 3, language 2.
  Match quality: exact word 1.0, word prefix 0.7, substring 0.4.
- Case folding is Unicode-aware (`über` finds `Über`); diacritics are not folded.
- Filters (`-t -g -o -k -l -n`, same syntax as `query`) restrict candidates and
  never affect the score.
- Ties are ordered by path; default limit 10.

Full design: [doc/search-cmd.md](doc/search-cmd.md).

## Filter expressions

`--tag`, `--topic`, `--tech` and `--lang` accept signed expressions, shared by
`query` and `search`:

- bare term: OR group
- `+term`: required (AND)
- `-term`: excluded (AND NOT)

| Expression | Meaning |
|------------|---------|
| `rust,c++` | rust OR c++ |
| `+debug,rust,c++` | debug AND (rust OR c++) |
| `+debug,-wip` | debug AND NOT wip |

Leading `-` works without `=`: `--tag -wip`.

## Configuration

`.kb/config.toml` (lookup order: `--config`, then `KBMETA_CONFIG`, then
`<root>/.kb/config.toml`):

```toml
documents_root = "knowledge/documents"
keep_intermediate = false      # keep clean/context/suggestion files in .kb/intermediate
# editor = "nvim"

[cook]
prompt_name = "cook-default"   # loads .kb/prompts/<name>.md
max_filename_len = 50
output_dir = "knowledge/staging"

[provider]
kind = "mock"                  # mock | openai-api | nvidia
endpoint = "http://localhost:8000/v1/chat/completions"
model = "meta/llama-3.1-8b-instruct"
api_key_env = "NVIDIA_API_KEY" # name of the env var holding the key
timeout_seconds = 60
# temperature = 0.2
# top_p = 0.9
# max_tokens = 1024
structured_output = false      # request JSON Schema output (openai-api); nvidia falls back to json_object
# reasoning_budget = 4096      # nvidia only
# enable_thinking = false      # nvidia only
```

Providers:

- `mock`: deterministic, offline; for tests and dry runs.
- `openai-api`: any OpenAI-compatible chat-completions endpoint.
- `nvidia`: NVIDIA NIM; supports `reasoning_budget` and `enable_thinking`.
  These options are ignored (with a warning) by other providers.

### Example: local LLM with llama.cpp

[config/](config/) holds a working setup for a local model served by
llama.cpp (OpenAI-compatible API):

- [config/config.toml](config/config.toml): `openai-api` provider at
  `http://127.0.0.1:8080`, Qwen3-4B, long timeout and token limit for a small
  local model, `structured_output = false` (llama.cpp supports `json_object`,
  not `json_schema`).
- [config/prompts/metadata-v5.md](config/prompts/metadata-v5.md): compact
  metadata prompt, selected by `prompt_name = "metadata-v5"`. It omits
  `new_terms` on purpose: the extra task overloads small models. Output is
  still canonicalized against the vocabulary; only new-term reporting is
  skipped. Keep `structured_output = false` with such prompts, since the JSON
  schema requires `new_terms`.

Prompts are always loaded from `.kb/prompts/<prompt_name>.md`, so copy the
prompt there and point kbmeta at the config:

```bash
llama-server -m ~/.local/share/llama/models/Qwen3-4B-Q4_K_M.gguf --port 8080

kbmeta init
cp config/prompts/metadata-v5.md .kb/prompts/
kbmeta --config config/config.toml cook notes/new-note.md
# or: export KBMETA_CONFIG=config/config.toml
```

Adjust `documents_root` and `cook.output_dir` (`KB` and `staging` in the
example) to your layout.

### Controlled vocabularies

`.kb/vocab/{tags,topics,technologies,languages,doc-types}.yaml`, seeded by
`init` from `doc/knowledge/`. Each entry has a canonical name and aliases:

```yaml
tags:
  - canonical: version-control
    aliases:
      - git
      - github
```

Metadata always stores the canonical name.

## Logging

- Log level via `RUST_LOG` (e.g. `RUST_LOG=debug`).
- Logs go to stderr and to `.kb/logs/kbmeta.log` (override with
  `KBMETA_LOG_FILE`).
- With `RUST_LOG=debug`, raw LLM answers are also written in full to
  `.kb/logs/llm-answers.log`.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | success |
| 1 | general error (usage, bad input, I/O) |
| 2 | permanent error: retrying is pointless (missing API key, bad URL, HTTP 4xx, invalid metadata, output collision) |
| 3 | transient error: retry later (timeout, rate limit, HTTP 5xx) |

## Further reading

- [doc/OKF-02-SPEC.md](doc/OKF-02-SPEC.md): OKF metadata spec
- [doc/search-cmd.md](doc/search-cmd.md): search design
- [doc/features_todo.md](doc/features_todo.md): planned ideas (FTS5)
