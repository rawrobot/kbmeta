You are implementing the preprocessing layer of a Rust-based local knowledge-base tool.

# Goal

Implement two deterministic preprocessing stages:

    Original Markdown
          │
          ▼
    Medium Cleaner
          │
          ▼
    Clear Markdown
          │
          ▼
    LLM Context Builder
          │
          ▼
    LLM Context

These are preparation stages for a later metadata-provider pipeline:

    LLM Context
          │
          ▼
    Local LLM
          │
          ▼
    Metadata proposal
          │
          ▼
    Human review
          │
          ▼
    OKF document

Do NOT implement the LLM, metadata generation, SQLite, indexing,
classification, or final OKF generation in this task.

The output of this task must be suitable for those later stages.

---

# 1. Stage 1 — Medium Cleaner

Create a deterministic Rust component that converts a Medium-exported
Markdown article into clean Markdown suitable for keeping in the KB.

Input:

    Original Markdown

Output:

    Clear Markdown

The original input must never be modified.

## Preserve

Preserve all substantive article content:

- title / H1
- headings
- paragraphs
- lists
- numbered lists
- blockquotes
- code blocks
- inline code
- tables
- technical links
- meaningful images
- diagrams
- image captions
- technical examples

Do not summarise, paraphrase, rewrite, or otherwise alter the meaning.

The cleaner is a source-format cleaner, not a semantic processor.

## Remove Medium-specific noise

Remove obvious Medium-specific presentation noise, including:

- author avatar/profile markup
- author profile links where they are not part of the article
- decorative images
- Medium navigation elements
- social-media footer
- author biography
- "Who Am I?" sections when they are clearly author biography
- sponsorship/follow sections
- Medium-specific boilerplate
- tracking-only URL parameters where they can be identified safely

Do not remove technical content merely because it looks unusual.

If uncertain whether content is article content or noise, preserve it.

The cleaner should be conservative.

## Source URL

Preserve the original article URL.

Expose it as part of the cleaned-document metadata:

    source_url

Also preserve the source type:

    source_type: medium

Do not lose the URL even if the original Markdown contains it only in
Medium-generated markup.

The exact final OKF frontmatter schema is NOT part of this task.

---

# 2. Clear Markdown

The output of Stage 1 is the version intended to become the permanent
knowledge-base document.

It should remain normal, readable Markdown.

Example:

    ---
    source_url: https://medium.com/...
    source_type: medium
    ---

    # Linux Environment Variables

    ## PATH

    The PATH variable ...

    ## HOME

    The HOME variable ...

Do not turn the document into plain text.

Do not generate summaries.

Do not remove technical information for the purpose of reducing token count.

The Clear Markdown should be suitable for:

- human reading
- Git
- Markdown editors
- future OKF conversion
- future indexing

---

# 3. Filename generation

Generate an FS-friendly `.md` filename for the Clear Markdown document.

## Rules

1. Prefer the first Markdown H1 as the filename source.
2. If there is no H1, use the original filename.
3. Remove common author/source suffixes when they are clearly identifiable.
4. Remove common date suffixes when clearly identifiable.
5. Normalize Unicode to ASCII where practical.
6. Keep only:
   - ASCII letters
   - digits
   - `_`
   - `-`
7. Convert whitespace and punctuation separators to `_`.
8. Collapse repeated `_`.
9. Remove leading/trailing `_` and `-`.
10. Limit the filename base to approximately 120 characters.
11. Always append `.md`.
12. Never silently overwrite an existing file.

Example:

    Original filename:
    The silent config layer running beneath every Linux system - Dhanush Nehru - Medium.md

    Markdown H1:
    # Linux Environment Variables

    Result:

    Linux_Environment_Variables.md

The human-readable title must remain:

    Linux Environment Variables

Do NOT reconstruct the title from the filename.

For example:

    gRPC → gRPC
    eBPF → eBPF
    OpenCV → OpenCV
    UIO → UIO
    PetaLinux → PetaLinux

Do not title-case technical names.

---

# 4. Stage 2 — LLM Context Builder

Create a second deterministic component:

    LlmContextBuilder

Input:

    Clear Markdown

Output:

    compact Markdown intended specifically as input to the metadata LLM.

This output is NOT the KB document.

Its purpose is to reduce the amount of text sent to the local LLM while
preserving enough information to determine:

- summary
- tags
- topics
- technologies
- programming languages
- important technical relationships

The LLM Context Builder must NOT use an LLM.

---

# 5. LLM Context principles

The LLM context should remain Markdown.

Prefer preserving:

- document title
- important headings
- introductory paragraphs
- section descriptions
- technical concepts
- technology/tool/protocol names
- important lists
- important relationships
- short code examples when they identify a technology or concept
- conclusions
- important warnings or limitations

Potentially reduce:

- repetitive explanations
- very long code blocks
- repeated examples
- decorative content
- navigation
- redundant prose
- boilerplate

Do NOT blindly truncate the document.

Do NOT create a new semantic summary.

The result must still contain enough original information for the LLM to
understand what the article is actually about.

The LLM Context Builder should be conservative in v1.

It is better to send somewhat more text than to lose an important technical
concept.

---

# 6. Important distinction between the two stages

The two outputs have different purposes.

    Original Markdown
          │
          ▼
    MediumCleaner
          │
          ▼
    Clear Markdown
    └── permanent KB content
          │
          ▼
    LlmContextBuilder
          │
          ▼
    LLM Context
    └── temporary/generated input

Never replace Clear Markdown with LLM Context.

Never use LLM Context as the canonical KB document.

---

# 7. Rust architecture

Use clean separation between parsing, cleaning, filename handling and
context generation.

Suggested structure:

    src/
      document/
        mod.rs
        markdown.rs
        cleaner.rs
        filename.rs
        llm_context.rs

Possible public types:

    pub struct CleanDocument {
        pub title: Option<String>,
        pub source_url: Option<String>,
        pub source_type: Option<String>,
        pub filename: String,
        pub markdown: String,
    }

    pub struct LlmContext {
        pub markdown: String,
    }

Interfaces:

    pub trait DocumentCleaner {
        fn clean(&self, input: &str) -> Result<CleanDocument>;
    }

    pub trait ContextBuilder {
        fn build(&self, document: &CleanDocument) -> Result<LlmContext>;
    }

Use appropriate error types rather than panics.

Do not couple these components to the future LLM implementation.

---

# 8. Markdown parsing

Use a proper Markdown parser rather than implementing Markdown parsing
with regular expressions.

Choose an appropriate mature Rust Markdown library.

The implementation should preserve Markdown structure.

Do not convert the entire document to HTML and then back to Markdown.

---

# 9. CLI

Implement:

    kbmeta clean <input>

Generate Clear Markdown.

Implement:

    kbmeta context <input>

Generate LLM Context.

Implement:

    kbmeta process <input>

Run both stages:

    input
      ↓
    Clear Markdown
      ↓
    LLM Context

Support:

    --output <directory>
    --dry-run
    --verbose

The default behaviour must never modify or delete the source file.

A useful output could be:

    input:
      article.md

    clear markdown:
      Linux_Environment_Variables.md

    llm context:
      Linux_Environment_Variables.llm.md

The exact naming of the temporary LLM context file can be decided
idiomatically, but it must be clearly distinguishable from the permanent
KB document.

---

# 10. Processing metadata

The preprocessing result should make it possible for later stages to know:

- original filename
- generated filename
- title
- source URL
- source type
- Clear Markdown
- LLM Context

Do not implement the final OKF schema yet.

Do not invent additional metadata unless required by the implementation.

---

# 11. Safety

The tool must be safe to run repeatedly.

Requirements:

- never modify the source file by default;
- never delete the source;
- never overwrite an existing KB file silently;
- report filename collisions clearly;
- preserve UTF-8;
- handle malformed Markdown gracefully;
- handle missing H1;
- handle missing source URL;
- handle empty documents;
- handle very long filenames;
- handle Unicode filenames;
- return useful errors.

`--dry-run` must not write files.

---

# 12. Tests

Write unit tests for all important behaviour.

## Medium cleaning tests

Test documents containing:

- Medium author block
- Medium avatar
- Medium image
- decorative image
- technical diagram
- author biography
- social footer
- tracking URL
- H1
- multiple headings
- lists
- code blocks
- blockquotes
- tables
- links
- inline code

Verify that technical content remains intact.

## Filename tests

Test:

    # Linux Environment Variables
    → Linux_Environment_Variables.md

    # Moving an FPGA-Mapped Application from /dev/mem to Generic UIO
    → Moving_an_FPGA-Mapped_Application_from_dev_mem_to_Generic_UIO.md

Also test:

- Unicode
- punctuation
- very long titles
- missing H1
- existing `.md`
- repeated spaces
- repeated separators
- technical capitalization

Do not require an exact filename for every unusual Unicode case unless the
rule is explicitly defined.

## LLM context tests

Test that:

- title is preserved;
- important headings are preserved;
- technical terminology remains;
- important lists remain;
- very long irrelevant code blocks can be reduced;
- repeated boilerplate is removed;
- output remains valid Markdown;
- the source document is not modified.

---

# 13. Example input

Use this type of Medium article as a realistic test:

    # Linux Environment Variables

    ## PATH

    The PATH variable ...

    ## HOME

    ...

    ## LD_PRELOAD

    ...

    ## LD_LIBRARY_PATH

    ...

    ## Security Rules

    ...

    ## Who Am I?

    Hi, I'm the author...

    Follow me...
    Sponsor me...

The Clear Markdown should retain the technical article and remove the
author/footer material.

The LLM Context should retain the concepts needed to identify that the
article concerns Linux environment variables, shell configuration,
system administration, and security.

Do not hard-code these particular topics into the implementation. They are
only an example of expected behaviour.

---

# 14. Token reduction

Do not optimise prematurely.

The implementation should eventually allow us to compare:

    original size
    clear Markdown size
    LLM Context size

Prefer character/byte counts initially.

Token counting is NOT required in v1 because we do not want to introduce
an LLM tokenizer dependency just for reporting.

A later implementation can add tokenizer-specific measurements.

---

# 15. What is explicitly OUT OF SCOPE

Do NOT implement:

- LLM inference
- llama.cpp integration
- Ollama integration
- metadata extraction
- metadata validation
- human metadata review UI
- SQLite
- embeddings
- vector search
- RAG
- document classification
- automatic directory classification
- automatic document relocation
- tree generation
- search indexes
- graph generation
- final OKF frontmatter
- OKF validation

Those will be separate stages.

The eventual architecture is:

    Source
      │
      ▼
    Preprocessing
      │
      ├── Clear Markdown
      │
      └── LLM Context
              │
              ▼
        Metadata Provider
              │
              ▼
        Metadata Proposal
              │
              ▼
          Human Review
              │
              ▼
             OKF
              │
              ▼
          SQLite index
              │
              ▼
        Search / Analysis

This task ends after producing Clear Markdown and LLM Context.

# 16. Quality criteria

The implementation is successful if:

1. Medium-specific noise is removed without damaging technical content.
2. The resulting Markdown is clean enough to keep permanently in the KB.
3. Filenames are predictable and filesystem-friendly.
4. The LLM context is materially smaller than the Clear Markdown for typical
   articles.
5. The LLM context still contains the information necessary to identify the
   document's subject and technical concepts.
6. Both transformations are deterministic.
7. No source files are modified or deleted.
8. The code is modular enough to add other source cleaners later.

Start by implementing the smallest complete version of these two stages.
Do not expand the scope.
