//! Cover the async entry point used by the CLI and ACP, including a cold index.
use std::io;
use std::path::{Path, PathBuf};

use xai_grok_session_search::{
    IndexableSession, SearchIndexManager, SessionSearchRequest, SessionSource, execute_search,
};

struct Source(PathBuf);

#[async_trait::async_trait]
impl SessionSource for Source {
    async fn list_sessions(&self) -> io::Result<Vec<IndexableSession>> {
        Ok([
            ("current", "Retry handling"),
            ("old", "Archived retry handling"),
        ]
        .into_iter()
        .map(|(id, title)| IndexableSession {
            session_id: id.into(),
            cwd: "/workspace".into(),
            updated_at_unix: 1_700_000_000,
            title: title.into(),
            updates_path: Some(self.0.join("prompt.txt")),
        })
        .collect())
    }

    async fn load_session(&self, id: &str, cwd: &str) -> io::Result<Option<IndexableSession>> {
        Ok(self
            .list_sessions()
            .await?
            .into_iter()
            .find(|session| session.session_id == id && session.cwd == cwd))
    }
}

fn extract(path: &Path) -> io::Result<(String, u64)> {
    let content = std::fs::read_to_string(path)?;
    let bytes = content.len() as u64;
    Ok((content, bytes))
}

#[tokio::test]
async fn cold_bootstrap_and_async_query_honor_structured_constraints() {
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::write(temp.path().join("prompt.txt"), "fix the rate limit").unwrap();
    let manager = SearchIndexManager::start(|root| Box::new(Source(root)), extract);
    let mut req = SessionSearchRequest {
        query: "title:retry content:\"rate limit\" -title:archived".into(),
        cwd: Some("/workspace".into()),
        limit: 1,
        offset: 0,
        include_content: true,
    };
    let response = execute_search(Some(&manager), temp.path(), &req)
        .await
        .unwrap();
    assert!(!response.bootstrapping);
    assert_eq!(response.total_estimate, Some(1));
    assert_eq!(response.next_offset, None);
    let hit = response.results.first().unwrap();
    assert_eq!(hit.session_id, "current");
    assert_eq!(hit.matched_fields, ["title", "content"]);
    assert!(hit.snippet.as_deref().unwrap().contains("[rate limit]"));

    req.offset = 1;
    let response = execute_search(Some(&manager), temp.path(), &req)
        .await
        .unwrap();
    assert!(response.results.is_empty());
    assert_eq!(response.total_estimate, Some(1));

    req.offset = 0;
    req.cwd = Some("/another-workspace".into());
    let response = execute_search(Some(&manager), temp.path(), &req)
        .await
        .unwrap();
    assert!(response.results.is_empty());
    assert_eq!(response.total_estimate, Some(0));
}
