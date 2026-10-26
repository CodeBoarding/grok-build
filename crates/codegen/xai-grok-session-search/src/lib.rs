//! Full-text search over local grok sessions, backed by a rebuildable SQLite FTS5 cache under `<grok home>/sessions/session_search.sqlite`.
//!
//! Layering, bottom up:
//!
//! - [`query`] parses user syntax into strict or relaxed FTS5 search plans.
//! - [`fts`] owns the schema, executes those plans with BM25 ranking, and fences `meta` writes.
//! - `recovery` classifies an unusable database file, quarantines it, and recreates an empty one; `db` adds the open-and-retry helpers on top.
//! - `doc` turns a session plus its extracted text into an index document.
//! - `bootstrap` runs the full reindex, held to one process at a time by a lease.
//! - `manager` debounces per-session upserts and answers queries.
//!
//! The crate never reads the session store directly: a caller supplies a [`SessionSource`] and a [`ContentExtractor`].
//! That keeps the `updates.jsonl` wire format owned by the store instead of duplicated here.

#![deny(clippy::indexing_slicing)]

mod bootstrap;
mod db;
mod doc;
pub mod fts;
mod manager;
pub mod query;
mod recovery;
mod source;

pub use manager::{
    SearchIndexManager, SearchIndexStatus, SessionSearchRequest, SessionSearchResponse,
    evict_session, execute_search,
};
pub use query::SessionQuery;
pub use source::{ContentExtractor, IndexableSession, SessionSource, SessionSourceFactory};
