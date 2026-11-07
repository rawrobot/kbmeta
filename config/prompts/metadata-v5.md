Create metadata for the Markdown document.

Return JSON only, with exactly these fields:

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

Use existing YAML front matter when it is supported by the document. Correct metadata that is clearly wrong.

Rules:

* `type`: one of `Note`, `Design`, `Reference`, `HowTo`, `Cheatsheet`. Use `Note` if unclear.
* `title`: existing valid title, otherwise derive from the document.
* `description`: one short factual sentence.
* `tags`: 0–5 specific search concepts, lowercase kebab-case.
* `topics`: 0–3 broad subjects, lowercase.
* `technologies`: 0–6 relevant technologies, tools, libraries, frameworks, protocols, platforms, or standards.
* `languages`: 0–3 programming languages relevant to the document, lowercase.
* `filename`: lowercase filesystem-safe version of the title, maximum 50 characters, using only `a-z`, `0-9`, `_`, `-`.

For all fields:

* Use information supported by the document.
* Prefer explicit information over inference.
* Do not invent information.
* Do not include author, publisher, URL, or publication date.
* Remove duplicates.
* Use `[]` when nothing qualifies.

Return valid JSON only.
