use std::cmp::{Ordering, Reverse};
use std::collections::{hash_map::DefaultHasher, BinaryHeap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "lancedb-backend")]
use arrow_array::{
    Array, Float32Array, RecordBatch, RecordBatchIterator, StringArray, UInt64Array,
};
#[cfg(feature = "lancedb-backend")]
use arrow_schema::{DataType, Field, Schema, SchemaRef};
#[cfg(feature = "lancedb-backend")]
use futures::TryStreamExt;
#[cfg(feature = "lancedb-backend")]
use lancedb::query::{ExecutableQuery, QueryBase};
#[cfg(feature = "lancedb-backend")]
use lancedb::Table;
#[cfg(feature = "lancedb-backend")]
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    pub category: String,
    pub scope: String,
    pub importance: f32,
    pub tags: Vec<String>,
    pub timestamp_ms: u64,
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct NewMemoryEntry {
    pub text: String,
    pub category: String,
    pub scope: String,
    pub importance: f32,
    pub tags: Vec<String>,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct RecallQuery {
    pub query: String,
    pub query_embedding: Option<Vec<f32>>,
    pub scope: Option<String>,
    pub category: Option<String>,
    pub limit: usize,
    pub vector_weight: Option<f32>,
    pub lexical_weight: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct RecallResult {
    pub entry: MemoryEntry,
    pub score: f32,
}

pub trait StorageBackend: Send {
    fn store(&mut self, new_entry: NewMemoryEntry) -> Result<MemoryEntry, StorageError>;
    fn recall(&self, query: RecallQuery) -> Vec<RecallResult>;
    fn forget_by_id(&mut self, id: &str) -> Result<bool, StorageError>;
    fn list(&self, limit: usize) -> Vec<MemoryEntry>;
    fn stats(&self) -> serde_json::Value;
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Persisted {
    entries: Vec<MemoryEntry>,
}

pub struct PersistentMemoryStore {
    path: PathBuf,
    entries: Vec<MemoryEntry>,
    next_id: u64,
}

impl PersistentMemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        if !path.exists() {
            let persisted = Persisted::default();
            let bytes = serde_json::to_vec_pretty(&persisted)?;
            fs::write(&path, bytes)?;
        }

        let bytes = fs::read(&path)?;
        let persisted: Persisted = serde_json::from_slice(&bytes)?;
        let next_id = persisted
            .entries
            .iter()
            .filter_map(|e| e.id.strip_prefix("mem-")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;

        Ok(Self {
            path,
            entries: persisted.entries,
            next_id,
        })
    }

    pub fn list(&self, limit: usize) -> Vec<MemoryEntry> {
        let n = limit.max(1);
        self.entries.iter().rev().take(n).cloned().collect()
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "count": self.entries.len(),
            "path": self.path,
        })
    }

    pub fn store(&mut self, new_entry: NewMemoryEntry) -> Result<MemoryEntry, StorageError> {
        if new_entry.text.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "text cannot be empty".to_string(),
            ));
        }

        let entry = MemoryEntry {
            id: format!("mem-{}", self.next_id),
            text: new_entry.text.to_lowercase(),
            category: new_entry.category,
            scope: new_entry.scope,
            importance: new_entry.importance.clamp(0.0, 1.0),
            tags: new_entry
                .tags
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect(),
            timestamp_ms: now_ms(),
            embedding: new_entry.embedding,
        };

        self.next_id += 1;
        self.entries.push(entry.clone());
        self.persist()?;

        Ok(entry)
    }

    pub fn forget_by_id(&mut self, id: &str) -> Result<bool, StorageError> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        let changed = self.entries.len() != before;
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    pub fn recall(&self, query: RecallQuery) -> Vec<RecallResult> {
        recall_entries(&self.entries, query)
    }

    fn persist(&self) -> Result<(), StorageError> {
        let persisted = Persisted {
            entries: self.entries.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&persisted)?;
        fs::write(&self.path, bytes)?;
        Ok(())
    }
}

impl StorageBackend for PersistentMemoryStore {
    fn store(&mut self, new_entry: NewMemoryEntry) -> Result<MemoryEntry, StorageError> {
        Self::store(self, new_entry)
    }

    fn recall(&self, query: RecallQuery) -> Vec<RecallResult> {
        Self::recall(self, query)
    }

    fn forget_by_id(&mut self, id: &str) -> Result<bool, StorageError> {
        Self::forget_by_id(self, id)
    }

    fn list(&self, limit: usize) -> Vec<MemoryEntry> {
        Self::list(self, limit)
    }

    fn stats(&self) -> serde_json::Value {
        Self::stats(self)
    }
}

#[cfg(feature = "lancedb-backend")]
pub struct LanceDbBackend {
    uri: String,
    table_name: String,
    rt: tokio::runtime::Runtime,
    table: Table,
    id_seq: u64,
}

#[cfg(feature = "lancedb-backend")]
impl LanceDbBackend {
    pub fn open(uri: impl Into<String>) -> Result<Self, StorageError> {
        let uri = uri.into();
        let table_name = "memories".to_string();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| StorageError::InvalidInput(format!("tokio runtime init failed: {e}")))?;
        let db = rt
            .block_on(async { lancedb::connect(&uri).execute().await })
            .map_err(|e| StorageError::InvalidInput(format!("lancedb connect failed: {e}")))?;

        let table = match rt.block_on(async { db.open_table(&table_name).execute().await }) {
            Ok(t) => t,
            Err(_) => {
                let schema = schema_ref();
                rt.block_on(async { db.create_empty_table(&table_name, schema).execute().await })
                    .map_err(|e| {
                        StorageError::InvalidInput(format!("lancedb create table failed: {e}"))
                    })?
            }
        };

        let count = rt
            .block_on(async { table.count_rows(None).await })
            .map_err(|e| StorageError::InvalidInput(format!("lancedb count failed: {e}")))?;

        Ok(Self {
            uri,
            table_name,
            rt,
            table,
            id_seq: (count as u64) + 1,
        })
    }

    fn parse_entries_from_batches(&self, batches: &[RecordBatch]) -> Vec<MemoryEntry> {
        let mut out = Vec::new();
        for batch in batches {
            let ids = as_string(batch, "id");
            let texts = as_string(batch, "text");
            let categories = as_string(batch, "category");
            let scopes = as_string(batch, "scope");
            let importances = as_f32(batch, "importance");
            let tags = as_string(batch, "tags");
            let timestamps = as_u64(batch, "timestamp_ms");
            let embeddings = as_string(batch, "embedding_json");

            let n = batch.num_rows();
            for i in 0..n {
                let raw_tags = tags.map(|a| a.value(i).to_string()).unwrap_or_default();
                let tags_vec = serde_json::from_str::<Vec<String>>(&raw_tags).unwrap_or_default();

                out.push(MemoryEntry {
                    id: ids
                        .map(|a| a.value(i).to_string())
                        .unwrap_or_else(|| format!("unknown-{i}")),
                    text: texts.map(|a| a.value(i).to_string()).unwrap_or_default(),
                    category: categories
                        .map(|a| a.value(i).to_string())
                        .unwrap_or_else(|| "other".to_string()),
                    scope: scopes
                        .map(|a| a.value(i).to_string())
                        .unwrap_or_else(|| "global".to_string()),
                    importance: importances.map(|a| a.value(i)).unwrap_or(0.7),
                    tags: tags_vec,
                    timestamp_ms: timestamps.map(|a| a.value(i)).unwrap_or(0),
                    embedding: embeddings
                        .and_then(|a| serde_json::from_str::<Vec<f32>>(a.value(i)).ok()),
                });
            }
        }
        out
    }
}

#[cfg(feature = "lancedb-backend")]
impl StorageBackend for LanceDbBackend {
    fn store(&mut self, new_entry: NewMemoryEntry) -> Result<MemoryEntry, StorageError> {
        if new_entry.text.trim().is_empty() {
            return Err(StorageError::InvalidInput(
                "text cannot be empty".to_string(),
            ));
        }

        let entry = MemoryEntry {
            id: format!("mem-{}", self.id_seq),
            text: new_entry.text.to_lowercase(),
            category: new_entry.category,
            scope: new_entry.scope,
            importance: new_entry.importance.clamp(0.0, 1.0),
            tags: new_entry
                .tags
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect(),
            timestamp_ms: now_ms(),
            embedding: new_entry.embedding,
        };

        self.id_seq += 1;
        let tags_json = serde_json::to_string(&entry.tags)?;
        let embedding_json = serde_json::to_string(&entry.embedding.clone().unwrap_or_default())?;

        let batch = RecordBatch::try_new(
            schema_ref(),
            vec![
                Arc::new(StringArray::from(vec![entry.id.clone()])),
                Arc::new(StringArray::from(vec![entry.text.clone()])),
                Arc::new(StringArray::from(vec![entry.category.clone()])),
                Arc::new(StringArray::from(vec![entry.scope.clone()])),
                Arc::new(Float32Array::from(vec![entry.importance])),
                Arc::new(StringArray::from(vec![tags_json])),
                Arc::new(UInt64Array::from(vec![entry.timestamp_ms])),
                Arc::new(StringArray::from(vec![embedding_json])),
            ],
        )
        .map_err(|e| StorageError::InvalidInput(format!("record batch build failed: {e}")))?;

        let schema = batch.schema();
        let reader = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);
        self.rt
            .block_on(async { self.table.add(reader).execute().await })
            .map_err(|e| StorageError::InvalidInput(format!("lancedb add failed: {e}")))?;

        Ok(entry)
    }

    fn recall(&self, query: RecallQuery) -> Vec<RecallResult> {
        let mut lq = self.table.query();
        if let Some(scope) = &query.scope {
            lq = lq.only_if(format!("scope = '{}'", escape_sql(scope)));
        }
        if let Some(category) = &query.category {
            let expr = format!("category = '{}'", escape_sql(category));
            lq = lq.only_if(expr);
        }
        lq = lq.limit(20_000);

        let batches = match self.rt.block_on(async { lq.execute().await }) {
            Ok(stream) => self
                .rt
                .block_on(async { stream.try_collect::<Vec<_>>().await })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        let entries = self.parse_entries_from_batches(&batches);
        recall_entries(&entries, query)
    }

    fn forget_by_id(&mut self, id: &str) -> Result<bool, StorageError> {
        let escaped = escape_sql(id);
        let before = self
            .rt
            .block_on(async {
                self.table
                    .count_rows(Some(format!("id = '{escaped}'")))
                    .await
            })
            .map_err(|e| StorageError::InvalidInput(format!("lancedb count failed: {e}")))?;

        if before == 0 {
            return Ok(false);
        }

        self.rt
            .block_on(async { self.table.delete(&format!("id = '{escaped}'")).await })
            .map_err(|e| StorageError::InvalidInput(format!("lancedb delete failed: {e}")))?;
        Ok(true)
    }

    fn list(&self, limit: usize) -> Vec<MemoryEntry> {
        let query = self.table.query().limit(limit.max(1));
        let batches = match self.rt.block_on(async { query.execute().await }) {
            Ok(stream) => self
                .rt
                .block_on(async { stream.try_collect::<Vec<_>>().await })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        self.parse_entries_from_batches(&batches)
    }

    fn stats(&self) -> serde_json::Value {
        let count = self
            .rt
            .block_on(async { self.table.count_rows(None).await })
            .unwrap_or(0);
        serde_json::json!({
            "backend": "lancedb",
            "lancedb_uri": self.uri,
            "table": self.table_name,
            "count": count
        })
    }
}

#[cfg(feature = "lancedb-backend")]
fn schema_ref() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
        Field::new("category", DataType::Utf8, false),
        Field::new("scope", DataType::Utf8, false),
        Field::new("importance", DataType::Float32, false),
        Field::new("tags", DataType::Utf8, false),
        Field::new("timestamp_ms", DataType::UInt64, false),
        Field::new("embedding_json", DataType::Utf8, false),
    ]))
}

#[cfg(feature = "lancedb-backend")]
fn as_string<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a StringArray> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
}

#[cfg(feature = "lancedb-backend")]
fn as_f32<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a Float32Array> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
}

#[cfg(feature = "lancedb-backend")]
fn as_u64<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a UInt64Array> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<UInt64Array>())
}

#[cfg(feature = "lancedb-backend")]
fn escape_sql(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn recall_entries(entries: &[MemoryEntry], query: RecallQuery) -> Vec<RecallResult> {
    let now = now_ms();
    let terms = tokenize(&query.query);
    let limit = query.limit.clamp(1, 50);
    let has_vector = query.query_embedding.is_some();
    if terms.is_empty() && !has_vector {
        return Vec::new();
    }
    let vector_weight = query.vector_weight.unwrap_or(0.6).clamp(0.0, 1.0);
    let lexical_weight = query
        .lexical_weight
        .unwrap_or(1.0 - vector_weight)
        .clamp(0.0, 1.0);

    let candidates: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| {
            if let Some(scope) = &query.scope {
                if entry.scope != *scope {
                    return None;
                }
            }
            if let Some(cat) = &query.category {
                if entry.category != *cat {
                    return None;
                }
            }
            Some(idx)
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    let cap = (limit * 4).clamp(16, 96);
    let mut ranked: BinaryHeap<Reverse<RankedItem>> = BinaryHeap::with_capacity(cap);
    let anchor = terms.first();
    for idx in candidates {
        let entry = &entries[idx];
        if !has_vector && anchor.is_some_and(|a| !entry.text.contains(a)) {
            continue;
        }
        let doc_len = approx_doc_len(entry);
        let mut lexical_hits = 0.0_f32;
        let mut bm25_local = 0.0_f32;
        for term in &terms {
            let tf = term_frequency(entry, term);
            if tf <= 0.0 {
                continue;
            }
            lexical_hits += 1.0;
            let k1 = 1.2_f32;
            let b = 0.75_f32;
            let avg_anchor = 32.0_f32;
            let denom = tf + k1 * (1.0 - b + b * (doc_len / avg_anchor));
            bm25_local += (tf * (k1 + 1.0)) / denom.max(1e-6);
        }
        let vector_score = if let (Some(qv), Some(dv)) = (&query.query_embedding, &entry.embedding)
        {
            cosine_similarity(qv, dv).unwrap_or(0.0)
        } else {
            0.0
        };

        if lexical_hits <= 0.0 && bm25_local <= 0.0 && vector_score <= 0.0 {
            continue;
        }
        let lexical = if terms.is_empty() {
            0.0
        } else {
            lexical_hits / (terms.len() as f32)
        };
        let bm25_norm = if terms.is_empty() {
            0.0
        } else {
            bm25_local / (terms.len() as f32)
        };
        let lexical_base = 0.65 * bm25_norm + 0.35 * lexical;
        let mut score = if has_vector {
            (lexical_weight * lexical_base) + (vector_weight * ((vector_score + 1.0) / 2.0))
        } else {
            lexical_base
        };
        score = apply_recency_boost(score, now, entry.timestamp_ms);
        score = apply_importance_weight(score, entry.importance);
        score = apply_length_norm(score, entry.text.len());

        if score >= 0.12 {
            let item = RankedItem { idx, score };
            if ranked.len() < cap {
                ranked.push(Reverse(item));
            } else if let Some(Reverse(min_item)) = ranked.peek() {
                if item.score > min_item.score {
                    let _ = ranked.pop();
                    ranked.push(Reverse(item));
                }
            }
        }
    }

    let mut ranked = ranked
        .into_iter()
        .map(|Reverse(item)| (item.idx, item.score))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));

    let mut out = Vec::with_capacity(limit);
    let mut selected_signatures: HashSet<u64> = HashSet::new();
    for (idx, score) in ranked {
        if out.len() >= limit {
            break;
        }
        let entry = &entries[idx];
        let sig = signature(entry);
        if selected_signatures.contains(&sig) {
            continue;
        }

        selected_signatures.insert(sig);
        out.push(RecallResult {
            entry: entry.clone(),
            score,
        });
    }

    out
}

#[derive(Debug, Clone, Copy)]
struct RankedItem {
    idx: usize,
    score: f32,
}

impl PartialEq for RankedItem {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.score.to_bits() == other.score.to_bits()
    }
}

impl Eq for RankedItem {}

impl PartialOrd for RankedItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankedItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.idx.cmp(&other.idx))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn approx_doc_len(entry: &MemoryEntry) -> f32 {
    let text_tokens = (entry.text.len() / 5).max(1);
    let tag_tokens = entry.tags.len().max(1);
    (text_tokens + tag_tokens) as f32
}

fn term_frequency(entry: &MemoryEntry, term: &str) -> f32 {
    if term.is_empty() {
        return 0.0;
    }
    if entry.text.contains(term) {
        1.0
    } else {
        0.0
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32, StorageError> {
    if a.len() != b.len() {
        return Err(StorageError::InvalidInput(
            "vector dimension mismatch".to_string(),
        ));
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        return Ok(0.0);
    }
    Ok(dot / denom)
}

fn apply_recency_boost(score: f32, now_ms: u64, timestamp_ms: u64) -> f32 {
    let age_ms = now_ms.saturating_sub(timestamp_ms) as f32;
    let age_days = age_ms / (1000.0 * 60.0 * 60.0 * 24.0);
    let boost = 0.10 / (1.0 + (age_days / 14.0));
    score + boost
}

fn apply_importance_weight(score: f32, importance: f32) -> f32 {
    score * (0.7 + 0.3 * importance.clamp(0.0, 1.0))
}

fn apply_length_norm(score: f32, text_len: usize) -> f32 {
    if text_len <= 500 {
        return score;
    }
    let ratio = (text_len as f32) / 500.0;
    let norm = 1.0 / (1.0 + 0.5 * ratio.log2());
    score * norm.clamp(0.4, 1.0)
}

fn signature(entry: &MemoryEntry) -> u64 {
    let mut hasher = DefaultHasher::new();
    entry.category.hash(&mut hasher);
    entry.scope.hash(&mut hasher);
    entry
        .text
        .chars()
        .take(120)
        .for_each(|c| c.hash(&mut hasher));
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_recall_forget_roundtrip() {
        let path = std::env::temp_dir().join(format!("prx-store-{}.json", now_ms()));
        let mut store = PersistentMemoryStore::open(&path).expect("open store");

        let stored = store
            .store(NewMemoryEntry {
                text: "Use jina embeddings with retrieval.query".to_string(),
                category: "fact".to_string(),
                scope: "global".to_string(),
                importance: 0.9,
                tags: vec!["jina".to_string(), "embedding".to_string()],
                embedding: None,
            })
            .expect("store");

        let recalled = store.recall(RecallQuery {
            query: "jina query embeddings".to_string(),
            query_embedding: None,
            scope: None,
            category: None,
            limit: 3,
            vector_weight: None,
            lexical_weight: None,
        });
        assert!(!recalled.is_empty());
        assert_eq!(recalled[0].entry.id, stored.id);

        let deleted = store.forget_by_id(&stored.id).expect("forget");
        assert!(deleted);

        let recalled_after = store.recall(RecallQuery {
            query: "jina query embeddings".to_string(),
            query_embedding: None,
            scope: None,
            category: None,
            limit: 3,
            vector_weight: None,
            lexical_weight: None,
        });
        assert!(recalled_after.is_empty());

        let _ = fs::remove_file(path);
    }

    #[cfg(feature = "lancedb-backend")]
    #[test]
    fn lancedb_backend_roundtrip() {
        let path = std::env::temp_dir().join(format!("prx-lancedb-{}", now_ms()));
        let uri = path.display().to_string();
        let mut backend = LanceDbBackend::open(&uri).expect("open lancedb backend");

        let stored = backend
            .store(NewMemoryEntry {
                text: "Use lancedb backend for durable local memory".to_string(),
                category: "fact".to_string(),
                scope: "global".to_string(),
                importance: 0.8,
                tags: vec!["lancedb".to_string(), "storage".to_string()],
                embedding: None,
            })
            .expect("store");

        let recalled = backend.recall(RecallQuery {
            query: "lancedb durable memory".to_string(),
            query_embedding: None,
            scope: None,
            category: None,
            limit: 5,
            vector_weight: None,
            lexical_weight: None,
        });
        assert!(!recalled.is_empty());
        assert_eq!(recalled[0].entry.id, stored.id);

        let deleted = backend.forget_by_id(&stored.id).expect("forget");
        assert!(deleted);
    }

    #[test]
    fn vector_fusion_can_override_lexical_bias() {
        let path = std::env::temp_dir().join(format!("prx-store-vec-{}.json", now_ms()));
        let mut store = PersistentMemoryStore::open(&path).expect("open store");

        let _ = store
            .store(NewMemoryEntry {
                text: "alpha lexical strong".to_string(),
                category: "fact".to_string(),
                scope: "global".to_string(),
                importance: 0.7,
                tags: vec!["alpha".to_string()],
                embedding: Some(vec![0.0, 1.0]),
            })
            .expect("store alpha");

        let beta = store
            .store(NewMemoryEntry {
                text: "beta lexical weak".to_string(),
                category: "fact".to_string(),
                scope: "global".to_string(),
                importance: 0.7,
                tags: vec!["beta".to_string()],
                embedding: Some(vec![1.0, 0.0]),
            })
            .expect("store beta");

        let recalled = store.recall(RecallQuery {
            query: "alpha lexical".to_string(),
            query_embedding: Some(vec![1.0, 0.0]),
            scope: None,
            category: None,
            limit: 2,
            vector_weight: Some(0.95),
            lexical_weight: Some(0.05),
        });

        assert!(!recalled.is_empty());
        assert_eq!(recalled[0].entry.id, beta.id);
        let _ = fs::remove_file(path);
    }
}
