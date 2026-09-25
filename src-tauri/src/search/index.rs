//! Local full-text search over the message cache (tantivy). The index stores only message ids;
//! results are hydrated from SQLite.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, FAST, INDEXED, STORED, TEXT};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

use crate::error::Result;

const WRITER_HEAP: usize = 30_000_000;
const COMMIT_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct Fields {
    msg_id: Field,
    account_id: Field,
    subject: Field,
    from: Field,
    to: Field,
    body: Field,
    date: Field,
}

pub struct SearchDoc<'a> {
    pub msg_id: i64,
    pub account_id: i64,
    pub subject: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub body: &'a str,
    pub date: i64,
}

pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    dirty: AtomicBool,
}

fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let fields = Fields {
        msg_id: b.add_u64_field("msg_id", INDEXED | STORED | FAST),
        account_id: b.add_u64_field("account_id", INDEXED),
        subject: b.add_text_field("subject", TEXT),
        from: b.add_text_field("from", TEXT),
        to: b.add_text_field("to", TEXT),
        body: b.add_text_field("body", TEXT),
        date: b.add_i64_field("date", FAST | STORED),
    };
    (b.build(), fields)
}

impl SearchIndex {
    pub fn open(dir: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        let (schema, fields) = schema();
        let directory = MmapDirectory::open(dir).map_err(tantivy::TantivyError::from)?;
        let index = match Index::open_or_create(directory, schema.clone()) {
            Ok(i) => i,
            // Schema changed between versions: the index is only a cache, rebuild it.
            Err(tantivy::TantivyError::SchemaError(_)) => {
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                Index::create_in_dir(dir, schema)?
            }
            Err(e) => return Err(e.into()),
        };
        Self::from_index(index, fields)
    }

    #[cfg(test)]
    pub fn in_memory() -> Arc<Self> {
        let (schema, fields) = schema();
        Self::from_index(Index::create_in_ram(schema), fields).unwrap()
    }

    fn from_index(index: Index, fields: Fields) -> Result<Arc<Self>> {
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let writer = index.writer(WRITER_HEAP)?;
        Ok(Arc::new(Self {
            index,
            reader,
            writer: Mutex::new(writer),
            fields,
            dirty: AtomicBool::new(false),
        }))
    }

    /// Adds or replaces the document for a message. Visible after the next commit.
    pub fn upsert(&self, d: SearchDoc<'_>) -> Result<()> {
        let f = self.fields;
        let writer = self.writer.lock().expect("search writer poisoned");
        writer.delete_term(Term::from_field_u64(f.msg_id, d.msg_id as u64));
        writer.add_document(doc!(
            f.msg_id => d.msg_id as u64,
            f.account_id => d.account_id as u64,
            f.subject => d.subject,
            f.from => d.from,
            f.to => d.to,
            f.body => d.body,
            f.date => d.date,
        ))?;
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }

    pub fn remove(&self, msg_ids: &[i64]) {
        let writer = self.writer.lock().expect("search writer poisoned");
        for id in msg_ids {
            writer.delete_term(Term::from_field_u64(self.fields.msg_id, *id as u64));
        }
        self.dirty.store(true, Ordering::Release);
    }

    pub fn remove_account(&self, account_id: i64) {
        let writer = self.writer.lock().expect("search writer poisoned");
        writer.delete_term(Term::from_field_u64(self.fields.account_id, account_id as u64));
        self.dirty.store(true, Ordering::Release);
    }

    pub fn commit(&self) -> Result<()> {
        if self.dirty.swap(false, Ordering::AcqRel) {
            self.writer.lock().expect("search writer poisoned").commit()?;
            self.reader.reload()?;
        }
        Ok(())
    }

    /// Batches commits: many upserts during sync, one commit every couple of seconds.
    pub fn spawn_committer(self: &Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(COMMIT_INTERVAL).await;
                let idx = this.clone();
                let res = tokio::task::spawn_blocking(move || idx.commit()).await;
                if let Ok(Err(e)) = res {
                    tracing::warn!("search commit failed: {e}");
                }
            }
        });
    }

    /// Returns matching message ids, best match first. Supports `from:`, `to:`, `subject:`.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<i64>> {
        let f = self.fields;
        let mut parser = QueryParser::for_index(&self.index, vec![f.subject, f.from, f.to, f.body]);
        parser.set_conjunction_by_default();
        parser.set_field_boost(f.subject, 2.0);
        let (query, _errors) = parser.parse_query_lenient(query);
        let searcher = self.reader.searcher();
        let top = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score())?;
        let mut ids = Vec::with_capacity(top.len());
        for (_score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            if let Some(id) = doc.get_first(f.msg_id).and_then(|v| v.as_u64()) {
                ids.push(id as i64);
            }
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrip_with_field_queries() {
        let idx = SearchIndex::in_memory();
        let doc = |id, subject, from, body| SearchDoc { msg_id: id, account_id: 1, subject, from, to: "me@example.de", body, date: 0 };
        idx.upsert(doc(1, "Angebot Dachsanierung", "Anna <anna@example.de>", "Anbei das Angebot")).unwrap();
        idx.upsert(doc(2, "Rechnung März", "Buchhaltung <bh@example.de>", "Bitte überweisen")).unwrap();
        idx.commit().unwrap();

        assert_eq!(idx.search("angebot", 10).unwrap(), vec![1]);
        assert_eq!(idx.search("from:anna", 10).unwrap(), vec![1]);
        assert_eq!(idx.search("überweisen", 10).unwrap(), vec![2]);
        // Lenient parsing must not fail on odd user input.
        assert!(idx.search("\"unbalanced (", 10).is_ok());

        // Replacing a document must not duplicate it.
        idx.upsert(doc(1, "Angebot v2", "Anna <anna@example.de>", "neu")).unwrap();
        idx.commit().unwrap();
        assert_eq!(idx.search("angebot", 10).unwrap(), vec![1]);

        idx.remove(&[1]);
        idx.commit().unwrap();
        assert!(idx.search("angebot", 10).unwrap().is_empty());
    }
}
