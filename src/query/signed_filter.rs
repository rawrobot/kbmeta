use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Parsed signed expression split into required, excluded, and OR-group values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedSignedFilter {
    /// Values prefixed with `+`; all must match.
    pub required: Vec<String>,
    /// Values prefixed with `-`; none must match.
    pub excluded: Vec<String>,
    /// Bare values; at least one must match when non-empty.
    pub any_of: Vec<String>,
}

/// Parse error for signed filter expressions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedFilterParseError {
    /// A term had a sign but no value, for example `+` or `-`.
    MalformedTerm { term: String },
    /// A value failed caller-provided validation.
    InvalidValue { value: String, detail: String },
    /// The same value appeared as both required and excluded.
    ConflictingPrefix { value: String },
}

impl Display for SignedFilterParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedTerm { term } => {
                write!(f, "malformed filter term '{}': missing value", term)
            }
            Self::InvalidValue { value, detail } => {
                write!(f, "invalid filter value '{}': {}", value, detail)
            }
            Self::ConflictingPrefix { value } => {
                write!(f, "conflicting filter for '{}': cannot be both required and excluded", value)
            }
        }
    }
}

impl Error for SignedFilterParseError {}

/// Validator callback used by [`SignedFilterParser::parse`].
pub type ValueValidator = fn(&str) -> Result<(), SignedFilterParseError>;

/// Parser for comma-separated signed expressions (`+a,-b,c`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SignedFilterParser;

impl SignedFilterParser {
    /// Parses an expression into [`ParsedSignedFilter`].
    ///
    /// Rules:
    /// - `+value` => required
    /// - `-value` => excluded
    /// - `value` => OR-group (`any_of`)
    /// - empty terms are ignored
    /// - conflicting required/excluded values return an error
    pub fn parse(
        &self,
        input: &str,
        validator: ValueValidator,
    ) -> Result<ParsedSignedFilter, SignedFilterParseError> {
        let mut required = Vec::new();
        let mut excluded = Vec::new();
        let mut any_of = Vec::new();

        let mut req_set = BTreeSet::new();
        let mut exc_set = BTreeSet::new();
        let mut any_set = BTreeSet::new();

        for raw in input.split(',') {
            let term = raw.trim();
            if term.is_empty() {
                continue;
            }

            let (prefix, rest) = match term.chars().next() {
                Some('+') => ('+', &term[1..]),
                Some('-') => ('-', &term[1..]),
                _ => (' ', term),
            };

            let value = rest.trim();
            if value.is_empty() {
                return Err(SignedFilterParseError::MalformedTerm {
                    term: term.to_string(),
                });
            }
            validator(value)?;

            match prefix {
                '+' => {
                    if exc_set.contains(value) {
                        return Err(SignedFilterParseError::ConflictingPrefix {
                            value: value.to_string(),
                        });
                    }
                    if req_set.insert(value.to_string()) {
                        required.push(value.to_string());
                    }
                }
                '-' => {
                    if req_set.contains(value) {
                        return Err(SignedFilterParseError::ConflictingPrefix {
                            value: value.to_string(),
                        });
                    }
                    if exc_set.insert(value.to_string()) {
                        excluded.push(value.to_string());
                    }
                }
                _ => {
                    if any_set.insert(value.to_string()) {
                        any_of.push(value.to_string());
                    }
                }
            }
        }

        Ok(ParsedSignedFilter {
            required,
            excluded,
            any_of,
        })
    }
}

/// SQL relation mapping used by [`RelationFilterSqlCompiler`].
#[derive(Debug, Clone, Copy)]
pub struct RelationSqlConfig<'a> {
    /// Alias of the outer item/document table, for example `i`.
    pub item_alias: &'a str,
    /// Link table name, for example `document_tags`.
    pub link_table: &'a str,
    /// Link table FK column pointing to the outer item id.
    pub link_item_fk: &'a str,
    /// Link table FK column pointing to vocabulary id.
    pub link_vocab_fk: &'a str,
    /// Vocabulary table name, for example `tags`.
    pub vocab_table: &'a str,
    /// Vocabulary PK column name.
    pub vocab_id_col: &'a str,
    /// Vocabulary value column name, typically `name`.
    pub vocab_value_col: &'a str,
}

/// Compiles signed filters into parameterized SQL fragments.
#[derive(Debug, Clone, Copy, Default)]
pub struct RelationFilterSqlCompiler;

impl RelationFilterSqlCompiler {
    /// Compiles the filter into a `WHERE` fragment and ordered bind parameters.
    ///
    /// The output uses `?` positional placeholders and `EXISTS` / `NOT EXISTS`
    /// subqueries based on `cfg` table/column mapping.
    pub fn compile(
        &self,
        filter: &ParsedSignedFilter,
        cfg: RelationSqlConfig<'_>,
    ) -> (String, Vec<String>) {
        let mut clauses = Vec::new();
        let mut params = Vec::new();

        for value in &filter.required {
            clauses.push(exists_clause(cfg, false, 1));
            params.push(value.clone());
        }

        for value in &filter.excluded {
            clauses.push(exists_clause(cfg, true, 1));
            params.push(value.clone());
        }

        if !filter.any_of.is_empty() {
            clauses.push(exists_clause(cfg, false, filter.any_of.len()));
            params.extend(filter.any_of.iter().cloned());
        }

        (clauses.join(" AND "), params)
    }
}

fn exists_clause(cfg: RelationSqlConfig<'_>, negated: bool, bind_count: usize) -> String {
    let placeholders = if bind_count == 1 {
        "?".to_string()
    } else {
        vec!["?"; bind_count].join(", ")
    };
    let comparator = if bind_count == 1 {
        format!("t.{} = {}", cfg.vocab_value_col, placeholders)
    } else {
        format!("t.{} IN ({})", cfg.vocab_value_col, placeholders)
    };
    let prefix = if negated { "NOT EXISTS" } else { "EXISTS" };

    format!(
        "{} (SELECT 1 FROM {} it JOIN {} t ON t.{} = it.{} WHERE it.{} = {}.id AND {})",
        prefix,
        cfg.link_table,
        cfg.vocab_table,
        cfg.vocab_id_col,
        cfg.link_vocab_fk,
        cfg.link_item_fk,
        cfg.item_alias,
        comparator,
    )
}

pub(crate) fn allow_tag_chars(value: &str) -> Result<(), SignedFilterParseError> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+' | '.'))
    {
        Ok(())
    } else {
        Err(SignedFilterParseError::InvalidValue {
            value: value.to_string(),
            detail: "allowed chars are [A-Za-z0-9_.+-]".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ParsedSignedFilter, RelationFilterSqlCompiler, RelationSqlConfig, SignedFilterParseError,
        SignedFilterParser, allow_tag_chars,
    };

    #[test]
    fn parses_or_only_expression() {
        let got = SignedFilterParser
            .parse("rust,c++", allow_tag_chars)
            .expect("parse should succeed");
        assert_eq!(got.any_of, vec!["rust", "c++"]);
    }

    #[test]
    fn parses_required_only_expression() {
        let got = SignedFilterParser
            .parse("+debug,+trace", allow_tag_chars)
            .expect("parse should succeed");
        assert_eq!(got.required, vec!["debug", "trace"]);
    }

    #[test]
    fn parses_excluded_only_expression() {
        let got = SignedFilterParser
            .parse("-wip,-draft", allow_tag_chars)
            .expect("parse should succeed");
        assert_eq!(got.excluded, vec!["wip", "draft"]);
    }

    #[test]
    fn parses_mixed_expression() {
        let got = SignedFilterParser
            .parse(" +debug , rust , c++ , -wip ", allow_tag_chars)
            .expect("parse should succeed");
        assert_eq!(got.required, vec!["debug"]);
        assert_eq!(got.excluded, vec!["wip"]);
        assert_eq!(got.any_of, vec!["rust", "c++"]);
    }

    #[test]
    fn rejects_conflicting_prefixes_for_same_value() {
        let err = SignedFilterParser
            .parse("+rust,-rust", allow_tag_chars)
            .expect_err("parse should fail");
        assert_eq!(
            err,
            SignedFilterParseError::ConflictingPrefix {
                value: "rust".to_string(),
            }
        );
    }

    #[test]
    fn empty_input_yields_no_filter() {
        let got = SignedFilterParser
            .parse(" , ,   ", allow_tag_chars)
            .expect("parse should succeed");
        assert_eq!(got, ParsedSignedFilter::default());
    }

    #[test]
    fn malformed_input_bare_plus_or_minus_errors() {
        let err_plus = SignedFilterParser
            .parse("+", allow_tag_chars)
            .expect_err("bare plus should fail");
        assert_eq!(
            err_plus,
            SignedFilterParseError::MalformedTerm {
                term: "+".to_string(),
            }
        );

        let err_minus = SignedFilterParser
            .parse("-", allow_tag_chars)
            .expect_err("bare minus should fail");
        assert_eq!(
            err_minus,
            SignedFilterParseError::MalformedTerm {
                term: "-".to_string(),
            }
        );
    }

    #[test]
    fn invalid_tag_chars_error() {
        let err = SignedFilterParser
            .parse("rust!", allow_tag_chars)
            .expect_err("invalid tag should fail");
        assert_eq!(
            err,
            SignedFilterParseError::InvalidValue {
                value: "rust!".to_string(),
                detail: "allowed chars are [A-Za-z0-9_.+-]".to_string(),
            }
        );
    }

    #[test]
    fn compiles_mixed_filter_with_stable_param_order() {
        let filter = SignedFilterParser
            .parse("+debug,-wip,rust,c++", allow_tag_chars)
            .expect("parse should succeed");
        let compiler = RelationFilterSqlCompiler;
        let cfg = RelationSqlConfig {
            item_alias: "i",
            link_table: "item_tags",
            link_item_fk: "item_id",
            link_vocab_fk: "tag_id",
            vocab_table: "tags",
            vocab_id_col: "id",
            vocab_value_col: "name",
        };
        let (sql, params) = compiler.compile(&filter, cfg);

        assert!(sql.contains("EXISTS"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(sql.contains("IN (?, ?)"));
        assert_eq!(
            params,
            vec![
                "debug".to_string(),
                "wip".to_string(),
                "rust".to_string(),
                "c++".to_string(),
            ]
        );
    }

    #[test]
    fn compiler_reuses_same_logic_for_non_tag_relation() {
        let filter = ParsedSignedFilter {
            required: vec!["reference".to_string()],
            excluded: vec![],
            any_of: vec!["guide".to_string(), "note".to_string()],
        };
        let cfg = RelationSqlConfig {
            item_alias: "d",
            link_table: "document_topics",
            link_item_fk: "document_id",
            link_vocab_fk: "topic_id",
            vocab_table: "topics",
            vocab_id_col: "id",
            vocab_value_col: "name",
        };

        let (sql, params) = RelationFilterSqlCompiler.compile(&filter, cfg);
        assert!(sql.contains("FROM document_topics it JOIN topics t"));
        assert!(sql.contains("it.document_id = d.id"));
        assert_eq!(
            params,
            vec![
                "reference".to_string(),
                "guide".to_string(),
                "note".to_string(),
            ]
        );
    }
}
