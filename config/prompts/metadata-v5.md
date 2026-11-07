Create metadata for Markdown document.

Return JSON only, exactly these fields:

{
"type": "",
"title": "",
"description": "",
"tags": [],
"topics": [],
"technologies": [],
"languages": [],
"filename": ""
}

Use existing YAML front matter when document supports it. Correct clearly wrong metadata.

Rules:

* `type`: one of `Note`, `Design`, `Reference`, `HowTo`, `Cheatsheet`. `Note` if unclear.
* `title`: existing valid title, else derive from document.
* `description`: one short factual sentence.
* `tags`: 0–5 specific search concepts, lowercase kebab-case.
* `topics`: 0–3 broad subjects, lowercase.
* `technologies`: 0–6 relevant technologies, tools, libraries, frameworks, protocols, platforms, or standards.
* `languages`: 0–3 programming languages relevant to document, lowercase.
* `filename`: lowercase filesystem-safe title, max 50 characters, only `a-z`, `0-9`, `_`, `-`.

All fields:

* Only information supported by document; explicit over inferred; never invent.
* No author, publisher, URL, or publication date.
* No duplicates.
* `[]` when nothing qualifies.

Return valid JSON only.
