# Integration Test Fixtures

Each fixture is a standalone workspace skeleton used by command-level integration tests.

- `kb_minimal`: two markdown documents, one with metadata and one without.
- `kb_invalid_metadata`: one valid and one malformed metadata block.
- `kb_tags_raw`: metadata tags with mixed casing and separators for normalization tests.
- `kb_missing_required`: one valid and one metadata block missing required fields.
- `kb_suggest`: document used by suggest command accept/edit/reject flow tests.

All fixtures are copied to a temporary directory before mutation by tests.
