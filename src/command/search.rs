use crate::cli::SearchFormatArg;
use crate::command::open_db;
use crate::query::search::{SearchFilters, SearchResult, search};
use anyhow::{Result, bail};
use std::path::Path;

pub(crate) struct SearchArgs {
    pub(crate) text: String,
    pub(crate) doc_type: Option<String>,
    pub(crate) tags: Option<String>,
    pub(crate) topics: Option<String>,
    pub(crate) technologies: Option<String>,
    pub(crate) languages: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) format: SearchFormatArg,
    pub(crate) limit: usize,
    pub(crate) fail_empty: bool,
    pub(crate) json: bool,
}

pub(crate) fn run(root: &Path, args: &SearchArgs) -> Result<()> {
    let format = if args.json {
        SearchFormatArg::Json
    } else {
        args.format
    };

    // Parse filters before touching the database so invalid expressions fail fast.
    let filters = SearchFilters::parse(
        args.doc_type.as_deref(),
        args.title.as_deref(),
        args.tags.as_deref(),
        args.topics.as_deref(),
        args.technologies.as_deref(),
        args.languages.as_deref(),
    )?;
    let conn = open_db(root)?;
    let results = search(&conn, &args.text, &filters, args.limit)?;

    render(&results, format)?;

    if args.fail_empty && results.is_empty() {
        bail!("no documents matched search");
    }
    Ok(())
}

fn render(results: &[SearchResult], format: SearchFormatArg) -> Result<()> {
    if format == SearchFormatArg::Json {
        println!("{}", serde_json::to_string_pretty(results)?);
        return Ok(());
    }
    if results.is_empty() {
        eprintln!("No matching documents found.");
        return Ok(());
    }

    match format {
        SearchFormatArg::Files => {
            for r in results {
                println!("{}", r.path);
            }
        }
        SearchFormatArg::Markdown => {
            for r in results {
                println!("- [{}](./{})", r.title.as_deref().unwrap_or("Untitled"), r.path);
            }
        }
        SearchFormatArg::Table => print!("{}", format_table(results)),
        SearchFormatArg::Json => unreachable!("handled above"),
    }
    Ok(())
}

fn format_table(results: &[SearchResult]) -> String {
    let rows: Vec<[String; 4]> = results
        .iter()
        .map(|r| {
            [
                r.id.to_string(),
                r.score.to_string(),
                r.title.clone().unwrap_or_default(),
                r.path.clone(),
            ]
        })
        .collect();
    let header = ["ID", "SCORE", "TITLE", "PATH"].map(str::to_string);

    let mut widths = header.clone().map(|h| h.chars().count());
    for row in &rows {
        for (w, col) in widths.iter_mut().zip(row) {
            *w = (*w).max(col.chars().count());
        }
    }

    let mut out = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        let line = row
            .iter()
            .zip(widths)
            .map(|(col, w)| format!("{col:<w$}"))
            .collect::<Vec<_>>()
            .join("   ");
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_table;
    use crate::query::search::SearchResult;

    #[test]
    fn table_aligns_columns() {
        let out = format_table(&[
            SearchResult {
                id: 123,
                path: "KB/a.md".to_string(),
                title: Some("Git Expert".to_string()),
                score: 17.0,
            },
            SearchResult {
                id: 4,
                path: "KB/ssh.md".to_string(),
                title: None,
                score: 12.5,
            },
        ]);
        assert_eq!(
            out,
            "ID    SCORE   TITLE        PATH\n\
             123   17      Git Expert   KB/a.md\n\
             4     12.5                 KB/ssh.md\n"
        );
    }
}
