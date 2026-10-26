//! User-facing session search syntax, independent of storage and transport.
//!
//! Plain words retain prefix matching and the AND-then-OR fallback. Quoted
//! phrases, `title:`/`content:` scopes, and `-` exclusions opt into strict
//! matching: relaxing those queries would violate the user's constraints.

/// A parsed query and its safe FTS5 execution plan.
///
/// Parsing is forgiving for live search: an unfinished quote extends to the
/// end of the input and empty clauses are ignored. Queries without a positive
/// searchable clause return no matches, rather than searching all history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionQuery {
    primary: Option<String>,
    fallback: Option<String>,
    structured: bool,
}

#[derive(Debug, Clone, Copy)]
enum Field {
    Title,
    Content,
}

impl Field {
    fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Content => "content",
        }
    }
}

impl SessionQuery {
    pub fn parse(input: &str) -> Self {
        let mut rest = input.trim_start();
        let mut positive = Vec::new();
        let mut negative = Vec::new();
        let mut structured = false;

        while !rest.is_empty() {
            let excluded = rest.strip_prefix('-').is_some_and(|tail| {
                tail.chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '"' || c == '_')
            });
            if excluded {
                rest = rest.strip_prefix('-').unwrap_or(rest);
                structured = true;
            }

            let field = if let Some(tail) = rest.strip_prefix("title:") {
                rest = tail;
                Some(Field::Title)
            } else if let Some(tail) = rest.strip_prefix("content:") {
                rest = tail;
                Some(Field::Content)
            } else {
                None
            };
            structured |= field.is_some();

            let clauses = if let Some(tail) = rest.strip_prefix('"') {
                structured = true;
                let (phrase, tail) = take_phrase(tail);
                rest = tail;
                // FTS5 tokenizes punctuation itself. Do not stem or prefix a
                // phrase: "rate limit" must not match "rate limiting".
                if phrase.chars().any(char::is_alphanumeric) {
                    // FTS5 treats NUL as the end of the expression even when
                    // SQLite receives a length-delimited parameter.
                    let escaped = phrase.replace('\0', " ").replace('"', "\"\"");
                    vec![format!("\"{escaped}\"")]
                } else {
                    Vec::new()
                }
            } else {
                let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let (word, tail) = rest.split_at(end);
                rest = tail;
                sanitize_token(word).map(token_prefix).collect()
            };

            // A scoped filename can split into several tokens; scope all of
            // them. Likewise, -foo.rs excludes the combination, not either word.
            if !clauses.is_empty() {
                let clause = if field.is_some() || excluded {
                    let joined = clauses.join(" AND ");
                    match field {
                        Some(field) => format!("{} : ({joined})", field.name()),
                        None => format!("({joined})"),
                    }
                } else {
                    // Keep ordinary fragments separate for the OR fallback.
                    positive.extend(clauses);
                    rest = rest.trim_start();
                    continue;
                };
                if excluded {
                    negative.push(clause);
                } else {
                    positive.push(clause);
                }
            }
            rest = rest.trim_start();
        }

        let fallback = (!structured && positive.len() > 1).then(|| positive.join(" OR "));
        let primary = if positive.is_empty() {
            None
        } else {
            let mut expression = positive.join(" AND ");
            if !negative.is_empty() {
                expression = format!("({expression}) NOT ({})", negative.join(" OR "));
            }
            Some(expression)
        };
        Self {
            primary,
            fallback,
            structured,
        }
    }

    /// True when the query uses syntax the remote keyword search cannot honor.
    /// Clients can use this to route to the local index and suppress literal
    /// title matches that have not been checked against the full query.
    pub fn is_structured(&self) -> bool {
        self.structured
    }

    /// Primary MATCH expression, or `None` for an empty/negative-only query.
    pub fn primary_match(&self) -> Option<&str> {
        self.primary.as_deref()
    }

    /// Optional relaxed expression. Execute only if the primary has zero hits
    /// across *all* pages, never merely because the requested page is empty.
    pub fn fallback_match(&self) -> Option<&str> {
        self.fallback.as_deref()
    }
}

/// Consume a quote-delimited phrase, accepting escaped quotes and backslashes.
fn take_phrase(input: &str) -> (String, &str) {
    let mut phrase = String::new();
    let mut chars = input.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '"' {
            return (
                phrase,
                input.get(index + ch.len_utf8()..).unwrap_or_default(),
            );
        }
        if ch == '\\'
            && chars
                .peek()
                .is_some_and(|(_, next)| matches!(next, '"' | '\\'))
        {
            if let Some((_, escaped)) = chars.next() {
                phrase.push(escaped);
            }
        } else {
            phrase.push(ch);
        }
    }
    (phrase, "")
}

/// Split punctuation rather than gluing filename fragments into nonexistent
/// tokens. Keep Unicode words; SQLite's unicode61 tokenizer indexes them too.
fn sanitize_token(token: &str) -> impl Iterator<Item = &str> {
    token
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .filter(|part| part.chars().any(char::is_alphanumeric))
}

/// Preserve the existing query-side plural stemming for plain ASCII words.
fn token_prefix(token: &str) -> String {
    let stem = if token.len() < 4 || !token.chars().all(|c| c.is_ascii_alphabetic()) {
        token
    } else {
        let lower = token.to_ascii_lowercase();
        let suffix_len = if lower.ends_with("es") {
            2
        } else if lower.ends_with('s') && !lower.ends_with("ss") {
            1
        } else {
            0
        };
        token.get(..token.len() - suffix_len).unwrap_or(token)
    };
    format!("\"{stem}\" *")
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
