//! Exercise the public API against a real SQLite FTS5 index, including ranking,
//! snippets, pagination, and compatibility with existing keyword searches.
use tempfile::TempDir;
use xai_grok_session_search::fts::{QueryResult, SessionDoc, SessionSearchIndex};

fn fixture() -> (TempDir, SessionSearchIndex) {
    let temp = TempDir::new().unwrap();
    let index = SessionSearchIndex::open_or_create(&temp.path().join("search.sqlite")).unwrap();
    for (id, cwd, title, content) in [
        (
            "exact",
            "/project",
            "Retry handling",
            "fix the rate limit with exponential backoff",
        ),
        (
            "separated",
            "/project",
            "Retry handling",
            "rate control with a configurable limit",
        ),
        (
            "prefix",
            "/project",
            "Retry handling",
            "rate limiting with exponential backoff",
        ),
        (
            "archived",
            "/project",
            "Archived retry handling",
            "rate limit old implementation",
        ),
        ("title-only", "/project", "Rate limit", "unrelated body"),
        (
            "other-workspace",
            "/other",
            "Retry handling",
            "rate limit exponential backoff",
        ),
    ] {
        index
            .upsert_doc(&SessionDoc {
                session_id: id.into(),
                cwd: cwd.into(),
                title: title.into(),
                content: content.into(),
                updated_at_unix: 1_700_000_000,
                content_hash: String::new(),
            })
            .unwrap();
    }
    (temp, index)
}

fn ids(result: &QueryResult) -> Vec<&str> {
    let mut ids: Vec<_> = result
        .results
        .iter()
        .map(|hit| hit.session_id.as_str())
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn phrases_require_adjacent_exact_tokens() {
    let (_temp, index) = fixture();
    let result = index
        .query("content:\"rate limit\"", Some("/project"), 20, 0, true)
        .unwrap();
    assert_eq!(ids(&result), ["archived", "exact"]);
    for hit in result.results {
        assert_eq!(hit.matched_fields, ["content"]);
        assert!(hit.snippet.unwrap().contains("[rate limit]"));
    }
}

#[test]
fn field_scopes_and_exclusions_compose() {
    let (_temp, index) = fixture();
    let result = index
        .query(
            "title:retry content:\"rate limit\" -title:archived",
            Some("/project"),
            20,
            0,
            true,
        )
        .unwrap();
    assert_eq!(ids(&result), ["exact"]);
    assert_eq!(
        result.results.first().unwrap().matched_fields,
        ["title", "content"]
    );
}

#[test]
fn exclusions_search_both_fields_unless_scoped() {
    let (_temp, index) = fixture();
    let result = index
        .query(
            "\"rate limit\" -archived -title:rate",
            Some("/project"),
            20,
            0,
            false,
        )
        .unwrap();
    assert_eq!(ids(&result), ["exact"]);
    let result = index
        .query(
            "\"rate limit\" -content:archived",
            Some("/project"),
            20,
            0,
            false,
        )
        .unwrap();
    assert_eq!(ids(&result), ["archived", "exact", "title-only"]);
}

#[test]
fn strict_queries_never_fall_back_to_partial_hits() {
    let (_temp, index) = fixture();
    for query in [
        "title:missing retry",
        "\"rate limit\" missing",
        "retry -retry",
        "title:retry content:unrelated",
    ] {
        let result = index.query(query, None, 20, 0, false).unwrap();
        assert!(result.results.is_empty(), "{query}");
        assert_eq!(result.total_estimate, Some(0));
    }
    // Existing plain keyword queries still offer partial matches.
    assert!(
        !index
            .query("missing retry", None, 20, 0, false)
            .unwrap()
            .results
            .is_empty()
    );
}

#[test]
fn structured_pagination_uses_the_same_constraints_and_stable_order() {
    let (_temp, index) = fixture();
    let query = "title:retry -title:archived";
    let all = index.query(query, Some("/project"), 20, 0, false).unwrap();
    let mut paged = Vec::new();
    for offset in 0..3 {
        let page = index
            .query(query, Some("/project"), 1, offset, false)
            .unwrap();
        assert_eq!(page.total_estimate, Some(3));
        assert_eq!(page.next_offset, (offset < 2).then_some(offset + 1));
        assert!(page.results.first().unwrap().snippet.is_none());
        paged.extend(page.results.into_iter().map(|hit| hit.session_id));
    }
    assert_eq!(
        paged,
        all.results
            .into_iter()
            .map(|hit| hit.session_id)
            .collect::<Vec<_>>()
    );
    let beyond = index.query(query, Some("/project"), 1, 99, false).unwrap();
    assert!(beyond.results.is_empty());
    assert_eq!(beyond.total_estimate, Some(3));
    assert_eq!(beyond.next_offset, None);
}

#[test]
fn empty_negative_and_incomplete_queries_do_not_error() {
    let (_temp, index) = fixture();
    for query in ["-retry", "-title:retry", "\"\"", "title:", "content:", "…"] {
        let result = index.query(query, None, 20, 0, true).unwrap();
        assert!(result.results.is_empty(), "{query}");
        assert_eq!(result.total_estimate, Some(0));
    }
    let result = index
        .query("content:\"rate limit", Some("/project"), 20, 0, false)
        .unwrap();
    assert_eq!(ids(&result), ["archived", "exact"]);
}

#[test]
fn user_text_cannot_inject_fts_or_sql_operators() {
    let (_temp, index) = fixture();
    for query in [
        "title:missing OR retry",
        "title:missing) OR (retry",
        "\"missing\" OR retry",
        "title:missing'; DROP TABLE session_docs; --",
        r#""say \"hello\"""#,
        "content:{title}:NEAR(retry)",
    ] {
        let result = index.query(query, None, 20, 0, false).unwrap();
        assert!(result.results.is_empty(), "{query}");
    }
    assert_eq!(
        index
            .query("retry", None, 20, 0, false)
            .unwrap()
            .total_estimate,
        Some(5)
    );
}

#[test]
fn punctuation_and_partial_input_produce_valid_match_expressions() {
    let (_temp, index) = fixture();
    for prefix in ["", "title:", "content:", "retry -", "retry -title:"] {
        for value in [
            "\"",
            "\"\"",
            "\"retry",
            "\"retry\"tail",
            "\"rate\0limit\"",
            "\"rate\\\"limit\"",
            "{retry}",
            "(retry)",
            "_",
            "->",
            "*",
            "日本語",
        ] {
            let query = format!("{prefix}{value}");
            assert!(index.query(&query, None, 20, 0, false).is_ok(), "{query:?}");
        }
    }
}

#[test]
fn unicode_queries_match_indexed_text() {
    let (_temp, index) = fixture();
    index
        .upsert_doc(&SessionDoc {
            session_id: "unicode".into(),
            cwd: "/project".into(),
            updated_at_unix: 0,
            title: "café 日本語".into(),
            content: "résumé café".into(),
            content_hash: String::new(),
        })
        .unwrap();
    for query in ["日本語", "title:café", "content:\"résumé café\""] {
        assert_eq!(
            ids(&index.query(query, None, 20, 0, false).unwrap()),
            ["unicode"]
        );
    }
}

#[test]
fn plain_query_empty_page_does_not_activate_or_fallback() {
    let (_temp, index) = fixture();
    let result = index
        .query("retry old", Some("/project"), 1, 1, false)
        .unwrap();
    assert!(result.results.is_empty());
    assert_eq!(result.total_estimate, Some(1));
    assert_eq!(result.next_offset, None);
}

#[test]
fn scoped_filenames_keep_all_fragments_in_the_same_field() {
    let (_temp, index) = fixture();
    index
        .upsert_doc(&SessionDoc {
            session_id: "filename".into(),
            cwd: "/project".into(),
            updated_at_unix: 0,
            title: "session_picker.rs".into(),
            content: "session_picker".into(),
            content_hash: String::new(),
        })
        .unwrap();
    assert_eq!(
        ids(&index
            .query("title:session_picker.rs", None, 20, 0, false)
            .unwrap()),
        ["filename"]
    );
    assert!(
        index
            .query("content:session_picker.rs", None, 20, 0, false)
            .unwrap()
            .results
            .is_empty()
    );
}
