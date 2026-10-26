use super::SessionQuery;

#[test]
fn plain_words_keep_relaxed_plan_and_plural_stemming() {
    let query = SessionQuery::parse("sessions caches class pass api_v2 foo-bar");
    assert!(!query.is_structured());
    assert_eq!(
        query.primary_match(),
        Some(
            "\"session\" * AND \"cach\" * AND \"class\" * AND \"pass\" * AND \"api_v2\" * AND \"foo-bar\" *"
        )
    );
    assert!(query.fallback_match().unwrap().contains(" OR "));
}

#[test]
fn punctuation_splits_filename_tokens() {
    let query = SessionQuery::parse("fix -> session_picker.rs!!!");
    assert_eq!(
        query.primary_match(),
        Some("\"fix\" * AND \"session_picker\" * AND \"rs\" *")
    );
    assert_eq!(
        query.fallback_match(),
        Some("\"fix\" * OR \"session_picker\" * OR \"rs\" *")
    );
}

#[test]
fn phrases_are_exact_and_never_relaxed() {
    let query = SessionQuery::parse("\"rate limits\" retry");
    assert!(query.is_structured());
    assert_eq!(
        query.primary_match(),
        Some("\"rate limits\" AND \"retry\" *")
    );
    assert_eq!(query.fallback_match(), None);
}

#[test]
fn scopes_apply_to_every_fragment() {
    let query = SessionQuery::parse("title:session_picker.rs content:\"borrow checker\"");
    assert_eq!(
        query.primary_match(),
        Some("title : (\"session_picker\" * AND \"rs\" *) AND content : (\"borrow checker\")")
    );
    assert_eq!(query.fallback_match(), None);
}

#[test]
fn exclusions_bind_to_the_entire_positive_expression() {
    let query = SessionQuery::parse("rust tokio -title:archive -\"out of date\"");
    assert_eq!(
        query.primary_match(),
        Some("(\"rust\" * AND \"tokio\" *) NOT (title : (\"archive\" *) OR (\"out of date\"))")
    );
    assert_eq!(query.fallback_match(), None);
}

#[test]
fn empty_or_negative_only_queries_cannot_search_all_history() {
    for input in [
        "",
        "  ",
        "-> _ …",
        "\"\"",
        "title:",
        "content:",
        "-cache",
        "-title:cache",
        "-\"rate limit\"",
    ] {
        let query = SessionQuery::parse(input);
        assert_eq!(query.primary_match(), None, "{input}");
        assert_eq!(query.fallback_match(), None, "{input}");
    }
}

#[test]
fn unfinished_input_is_safe_for_live_search() {
    let query = SessionQuery::parse("title:\"rate limit");
    assert_eq!(query.primary_match(), Some("title : (\"rate limit\")"));
    assert_eq!(query, SessionQuery::parse("title:\"rate limit\""));
    assert_eq!(
        SessionQuery::parse("title: retry").primary_match(),
        Some("\"retry\" *")
    );
}

#[test]
fn escaped_quotes_are_quoted_again_for_fts() {
    let query = SessionQuery::parse(r#""say \"hello\"""#);
    assert_eq!(query.primary_match(), Some(r#""say ""hello""""#));
    assert_eq!(query.fallback_match(), None);
}

#[test]
fn unknown_fields_and_fts_operators_are_literal_words() {
    let query = SessionQuery::parse("owner:alice OR NEAR(foo) NOT bar");
    assert!(!query.is_structured());
    assert_eq!(
        query.primary_match(),
        Some(
            "\"owner\" * AND \"alice\" * AND \"OR\" * AND \"NEAR\" * AND \"foo\" * AND \"NOT\" * AND \"bar\" *"
        )
    );
}

#[test]
fn unicode_terms_are_preserved() {
    let query = SessionQuery::parse("café 日本語");
    assert_eq!(query.primary_match(), Some("\"café\" * AND \"日本語\" *"));
    assert_eq!(query.fallback_match(), Some("\"café\" * OR \"日本語\" *"));
}

#[test]
fn hyphens_inside_words_are_not_exclusions() {
    let query = SessionQuery::parse("rate-limit");
    assert!(!query.is_structured());
    assert_eq!(query.primary_match(), Some("\"rate-limit\" *"));
    assert_eq!(query.fallback_match(), None);
}

#[test]
fn quoting_can_search_literal_syntax() {
    let query = SessionQuery::parse("\"title:cache\" \"-retry\"");
    assert_eq!(
        query.primary_match(),
        Some("\"title:cache\" AND \"-retry\"")
    );
}
