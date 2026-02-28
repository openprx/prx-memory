use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use prx_memory_core::{EvolutionPolicy, EvolutionRunner, VariantCandidate};
use prx_memory_embed::{
    build_embedding_provider, EmbeddingProviderConfig, EmbeddingRequest, EmbeddingTask,
    GeminiConfig, OpenAiCompatibleConfig, ProviderError as EmbeddingProviderError,
};
use prx_memory_rerank::{
    build_rerank_provider, CohereRerankConfig, JinaRerankConfig, PineconeRerankConfig,
    ProviderError as RerankProviderError, RerankProviderConfig, RerankRequest,
};
use prx_memory_skill::{
    resource_text as skill_resource_text, resources as skill_resources, SKILL_ID,
};
#[cfg(feature = "lancedb-backend")]
use prx_memory_storage::LanceDbBackend;
use prx_memory_storage::{
    MemoryEntry, NewMemoryEntry, PersistentMemoryStore, RecallQuery, RecallResult, StorageBackend,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

const DEFAULT_MCP_PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    store: Arc<Mutex<Box<dyn StorageBackend>>>,
    scopes: ScopeManager,
    standards: StandardizationConfig,
    auto_store_counter: Mutex<usize>,
    metrics: Arc<Mutex<MetricsRegistry>>,
    sessions: Arc<Mutex<HashMap<String, SessionState>>>,
    session_counter: Mutex<u64>,
}

#[derive(Debug, Clone)]
struct ScopeManager {
    agent_id: String,
    default_scope: String,
    allowed_scope_rules: Vec<String>,
    agent_access: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StandardProfile {
    ZeroConfig,
    Governed,
}

#[derive(Debug, Clone)]
struct StandardizationConfig {
    profile: StandardProfile,
    default_project_tag: String,
    default_tool_tag: String,
    default_domain_tag: String,
}

impl StandardizationConfig {
    fn from_env() -> Self {
        let profile = match std::env::var("PRX_MEMORY_STANDARD_PROFILE") {
            Ok(v) => {
                let lowered = v.trim().to_ascii_lowercase();
                if matches!(
                    lowered.as_str(),
                    "governed" | "strict" | "production" | "prod"
                ) {
                    StandardProfile::Governed
                } else {
                    StandardProfile::ZeroConfig
                }
            }
            Err(_) => StandardProfile::ZeroConfig,
        };
        let default_project_tag = std::env::var("PRX_MEMORY_DEFAULT_PROJECT_TAG")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "prx-memory".to_string());
        let default_tool_tag = std::env::var("PRX_MEMORY_DEFAULT_TOOL_TAG")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "mcp".to_string());
        let default_domain_tag = std::env::var("PRX_MEMORY_DEFAULT_DOMAIN_TAG")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "general".to_string());
        Self {
            profile,
            default_project_tag,
            default_tool_tag,
            default_domain_tag,
        }
    }

    fn profile_label(&self) -> &'static str {
        match self.profile {
            StandardProfile::ZeroConfig => "zero-config",
            StandardProfile::Governed => "governed",
        }
    }

    fn default_governed_for_store(&self) -> bool {
        matches!(self.profile, StandardProfile::Governed)
    }

    fn default_governed_for_update(&self) -> bool {
        matches!(self.profile, StandardProfile::Governed)
    }

    fn default_governed_for_import(&self) -> bool {
        matches!(self.profile, StandardProfile::Governed)
    }
}

#[derive(Debug, Clone)]
struct StreamEvent {
    seq: u64,
    payload: Value,
    created_ms: u64,
}

#[derive(Debug, Clone)]
struct SessionState {
    next_seq: u64,
    events: VecDeque<StreamEvent>,
    last_touch_ms: u64,
    acked_seq: u64,
    lease_expires_ms: u64,
}

#[derive(Debug, Clone)]
struct SessionEventPage {
    events: Vec<StreamEvent>,
    effective_from: u64,
    next_from: u64,
    ack_applied: Option<u64>,
    lease_expires_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionAccessError {
    NotFound,
    Expired,
    Poisoned,
}

#[derive(Debug, Default, Clone)]
struct ToolMetric {
    ok: u64,
    err: u64,
    total_latency_ms: f64,
    max_latency_ms: f64,
}

#[derive(Debug, Default, Clone)]
struct StageMetric {
    total_latency_ms: f64,
    count: u64,
    max_latency_ms: f64,
}

#[derive(Debug, Clone)]
struct BoundedLabelCounter {
    max_labels: usize,
    counts: HashMap<String, u64>,
    overflow: u64,
}

impl BoundedLabelCounter {
    fn new(max_labels: usize) -> Self {
        Self {
            max_labels: max_labels.max(1),
            counts: HashMap::new(),
            overflow: 0,
        }
    }

    fn record(&mut self, raw_label: &str) {
        let label = sanitize_label_value(raw_label);
        if let Some(v) = self.counts.get_mut(&label) {
            *v = v.saturating_add(1);
            return;
        }
        if self.counts.len() < self.max_labels {
            self.counts.insert(label, 1);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }
}

#[derive(Debug, Clone)]
struct MetricsRegistry {
    tool: HashMap<String, ToolMetric>,
    recall_stage: HashMap<String, StageMetric>,
    recall_scope: BoundedLabelCounter,
    recall_category: BoundedLabelCounter,
    recall_rerank_provider: BoundedLabelCounter,
    remote_rerank_attempts: u64,
    remote_rerank_warnings: u64,
    sessions_created: u64,
    sessions_renewed: u64,
    sessions_expired: u64,
    session_access_not_found: u64,
    session_access_poisoned: u64,
}

impl MetricsRegistry {
    fn from_env() -> Self {
        Self {
            tool: HashMap::new(),
            recall_stage: HashMap::new(),
            recall_scope: BoundedLabelCounter::new(env_usize(
                "PRX_METRICS_MAX_RECALL_SCOPE_LABELS",
                32,
                1,
                256,
            )),
            recall_category: BoundedLabelCounter::new(env_usize(
                "PRX_METRICS_MAX_RECALL_CATEGORY_LABELS",
                32,
                1,
                256,
            )),
            recall_rerank_provider: BoundedLabelCounter::new(env_usize(
                "PRX_METRICS_MAX_RERANK_PROVIDER_LABELS",
                16,
                1,
                128,
            )),
            remote_rerank_attempts: 0,
            remote_rerank_warnings: 0,
            sessions_created: 0,
            sessions_renewed: 0,
            sessions_expired: 0,
            session_access_not_found: 0,
            session_access_poisoned: 0,
        }
    }
}

#[derive(Debug, Default, Clone)]
struct EmbedRuntimeStats {
    cache_hits: u64,
    cache_misses: u64,
    cache_evictions: u64,
    rate_wait_events: u64,
    rate_wait_ms_total: u64,
}

#[derive(Debug, Clone)]
struct EmbedCacheEntry {
    value: Vec<f32>,
    expire_at_ms: u64,
}

#[derive(Debug, Clone)]
struct EmbedRuntime {
    entries: HashMap<String, EmbedCacheEntry>,
    lru: VecDeque<String>,
    capacity: usize,
    ttl_ms: u64,
    tokens: f64,
    max_tokens: f64,
    refill_per_sec: f64,
    last_refill_ms: u64,
    stats: EmbedRuntimeStats,
}

impl McpServer {
    pub fn new() -> Self {
        let db_path =
            std::env::var("PRX_MEMORY_DB").unwrap_or_else(|_| "./data/memory-db.json".to_string());
        Self::with_db_path(db_path).expect("initialize storage for mcp server")
    }

    pub fn with_db_path(db_path: impl Into<String>) -> Result<Self, String> {
        let db_path = db_path.into();
        let backend = std::env::var("PRX_MEMORY_BACKEND").unwrap_or_else(|_| "json".to_string());
        let store: Box<dyn StorageBackend> = match backend.as_str() {
            #[cfg(feature = "lancedb-backend")]
            "lancedb" => Box::new(LanceDbBackend::open(db_path).map_err(|e| e.to_string())?),
            _ => Box::new(PersistentMemoryStore::open(db_path).map_err(|e| e.to_string())?),
        };
        let initial_count = store.list(200_000).len();
        let scopes = ScopeManager::from_env();
        let standards = StandardizationConfig::from_env();
        Ok(Self {
            store: Arc::new(Mutex::new(store)),
            scopes,
            standards,
            auto_store_counter: Mutex::new(initial_count),
            metrics: Arc::new(Mutex::new(MetricsRegistry::from_env())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            session_counter: Mutex::new(1),
        })
    }

    pub fn handle_request(&self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        if request.jsonrpc != "2.0" {
            return Some(JsonRpcResponse::error(
                request.id.unwrap_or(Value::Null),
                -32600,
                "invalid jsonrpc version",
            ));
        }

        let is_notification = request.id.is_none();
        let id = request.id.clone().unwrap_or(Value::Null);

        if is_notification && request.method == "notifications/initialized" {
            return None;
        }

        let response = match request.method.as_str() {
            "initialize" => {
                let protocol_version = request
                    .params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_MCP_PROTOCOL_VERSION);
                JsonRpcResponse::success(
                    id,
                    json!({
                        "protocolVersion": protocol_version,
                        "serverInfo": {"name": "prx-memory-mcp", "version": "0.2.0"},
                        "capabilities": {
                            "tools": {
                                "listChanged": false
                            },
                            "resources": {
                                "subscribe": false,
                                "listChanged": false
                            },
                            "resourceTemplates": {
                                "listChanged": false
                            }
                        }
                    }),
                )
            }
            "ping" => JsonRpcResponse::success(id, json!({})),
            "tools/list" => JsonRpcResponse::success(id, self.tools_list_result()),
            "tools/call" => self.handle_tools_call(id, request.params),
            "resources/list" => JsonRpcResponse::success(id, self.resources_list_result()),
            "resources/templates/list" => {
                JsonRpcResponse::success(id, self.resources_templates_list_result())
            }
            "resources/read" => self.handle_resources_read(id, request.params),
            _ => JsonRpcResponse::error(id, -32601, "method not found"),
        };

        Some(response)
    }

    fn record_tool_metrics(&self, tool: &str, latency_ms: f64, is_error: bool) {
        let mut locked = match self.metrics.lock() {
            Ok(v) => v,
            Err(_) => return,
        };
        let metric = locked.tool.entry(tool.to_string()).or_default();
        if is_error {
            metric.err = metric.err.saturating_add(1);
        } else {
            metric.ok = metric.ok.saturating_add(1);
        }
        metric.total_latency_ms += latency_ms;
        metric.max_latency_ms = metric.max_latency_ms.max(latency_ms);
    }

    fn record_recall_stage(&self, stage: &str, latency_ms: f64) {
        let mut locked = match self.metrics.lock() {
            Ok(v) => v,
            Err(_) => return,
        };
        let metric = locked.recall_stage.entry(stage.to_string()).or_default();
        metric.count = metric.count.saturating_add(1);
        metric.total_latency_ms += latency_ms;
        metric.max_latency_ms = metric.max_latency_ms.max(latency_ms);
    }

    fn record_recall_dimensions(
        &self,
        scope: Option<&str>,
        category: Option<&str>,
        rerank_provider: Option<&str>,
    ) {
        let mut locked = match self.metrics.lock() {
            Ok(v) => v,
            Err(_) => return,
        };
        locked
            .recall_scope
            .record(scope.unwrap_or("mixed_or_default_scope"));
        locked
            .recall_category
            .record(category.unwrap_or("all_categories"));
        locked
            .recall_rerank_provider
            .record(rerank_provider.unwrap_or("auto"));
    }

    fn record_remote_rerank_attempt(&self) {
        if let Ok(mut locked) = self.metrics.lock() {
            locked.remote_rerank_attempts = locked.remote_rerank_attempts.saturating_add(1);
        }
    }

    fn record_remote_rerank_warning(&self) {
        if let Ok(mut locked) = self.metrics.lock() {
            locked.remote_rerank_warnings = locked.remote_rerank_warnings.saturating_add(1);
        }
    }

    fn record_session_expired(&self, count: usize) {
        if count == 0 {
            return;
        }
        if let Ok(mut locked) = self.metrics.lock() {
            locked.sessions_expired = locked.sessions_expired.saturating_add(count as u64);
        }
    }

    fn record_session_access_error(&self, err: SessionAccessError) {
        if let Ok(mut locked) = self.metrics.lock() {
            match err {
                SessionAccessError::NotFound | SessionAccessError::Expired => {
                    locked.session_access_not_found =
                        locked.session_access_not_found.saturating_add(1);
                }
                SessionAccessError::Poisoned => {
                    locked.session_access_poisoned =
                        locked.session_access_poisoned.saturating_add(1);
                }
            }
        }
    }

    fn render_metrics_text(&self) -> String {
        let mut lines = vec![
            "# TYPE prx_memory_tool_calls_total counter".to_string(),
            "# TYPE prx_memory_tool_latency_ms_sum counter".to_string(),
            "# TYPE prx_memory_tool_latency_ms_count counter".to_string(),
            "# TYPE prx_memory_recall_stage_latency_ms_sum counter".to_string(),
            "# TYPE prx_memory_recall_stage_latency_ms_count counter".to_string(),
            "# TYPE prx_memory_recall_scope_requests_total counter".to_string(),
            "# TYPE prx_memory_recall_category_requests_total counter".to_string(),
            "# TYPE prx_memory_recall_rerank_provider_requests_total counter".to_string(),
            "# TYPE prx_memory_metrics_label_overflow_total counter".to_string(),
            "# TYPE prx_memory_metrics_label_limit gauge".to_string(),
            "# TYPE prx_memory_recall_remote_rerank_attempts_total counter".to_string(),
            "# TYPE prx_memory_recall_remote_rerank_warnings_total counter".to_string(),
            "# TYPE prx_memory_recall_remote_rerank_warning_ratio gauge".to_string(),
            "# TYPE prx_memory_sessions_active gauge".to_string(),
            "# TYPE prx_memory_sessions_created_total counter".to_string(),
            "# TYPE prx_memory_sessions_renewed_total counter".to_string(),
            "# TYPE prx_memory_sessions_expired_total counter".to_string(),
            "# TYPE prx_memory_session_access_errors_total counter".to_string(),
            "# TYPE prx_memory_tool_error_ratio gauge".to_string(),
            "# TYPE prx_memory_alert_state gauge".to_string(),
        ];

        let active_sessions = self.sessions.lock().map(|v| v.len()).unwrap_or(0);
        if let Ok(locked) = self.metrics.lock() {
            let mut total_calls = 0_u64;
            let mut total_errors = 0_u64;
            let mut tools = locked.tool.keys().cloned().collect::<Vec<_>>();
            tools.sort();
            for tool in tools {
                let m = &locked.tool[&tool];
                let tool_label = prom_label_value(&tool);
                lines.push(format!(
                    "prx_memory_tool_calls_total{{tool=\"{}\",status=\"ok\"}} {}",
                    tool_label, m.ok
                ));
                lines.push(format!(
                    "prx_memory_tool_calls_total{{tool=\"{}\",status=\"error\"}} {}",
                    tool_label, m.err
                ));
                lines.push(format!(
                    "prx_memory_tool_latency_ms_sum{{tool=\"{}\"}} {:.3}",
                    tool_label, m.total_latency_ms
                ));
                lines.push(format!(
                    "prx_memory_tool_latency_ms_count{{tool=\"{}\"}} {}",
                    tool_label,
                    m.ok + m.err
                ));
                lines.push(format!(
                    "prx_memory_tool_latency_ms_max{{tool=\"{}\"}} {:.3}",
                    tool_label, m.max_latency_ms
                ));
                total_calls = total_calls.saturating_add(m.ok + m.err);
                total_errors = total_errors.saturating_add(m.err);
            }

            let mut stages = locked.recall_stage.keys().cloned().collect::<Vec<_>>();
            stages.sort();
            for stage in stages {
                let m = &locked.recall_stage[&stage];
                let stage_label = prom_label_value(&stage);
                lines.push(format!(
                    "prx_memory_recall_stage_latency_ms_sum{{stage=\"{}\"}} {:.3}",
                    stage_label, m.total_latency_ms
                ));
                lines.push(format!(
                    "prx_memory_recall_stage_latency_ms_count{{stage=\"{}\"}} {}",
                    stage_label, m.count
                ));
                lines.push(format!(
                    "prx_memory_recall_stage_latency_ms_max{{stage=\"{}\"}} {:.3}",
                    stage_label, m.max_latency_ms
                ));
            }

            for (scope, count) in sorted_counter(&locked.recall_scope.counts) {
                lines.push(format!(
                    "prx_memory_recall_scope_requests_total{{scope=\"{}\"}} {}",
                    prom_label_value(&scope),
                    count
                ));
            }
            for (category, count) in sorted_counter(&locked.recall_category.counts) {
                lines.push(format!(
                    "prx_memory_recall_category_requests_total{{category=\"{}\"}} {}",
                    prom_label_value(&category),
                    count
                ));
            }
            for (provider, count) in sorted_counter(&locked.recall_rerank_provider.counts) {
                lines.push(format!(
                    "prx_memory_recall_rerank_provider_requests_total{{provider=\"{}\"}} {}",
                    prom_label_value(&provider),
                    count
                ));
            }
            lines.push(format!(
                "prx_memory_metrics_label_overflow_total{{dimension=\"scope\"}} {}",
                locked.recall_scope.overflow
            ));
            lines.push(format!(
                "prx_memory_metrics_label_overflow_total{{dimension=\"category\"}} {}",
                locked.recall_category.overflow
            ));
            lines.push(format!(
                "prx_memory_metrics_label_overflow_total{{dimension=\"rerank_provider\"}} {}",
                locked.recall_rerank_provider.overflow
            ));
            lines.push(format!(
                "prx_memory_metrics_label_limit{{dimension=\"scope\"}} {}",
                locked.recall_scope.max_labels
            ));
            lines.push(format!(
                "prx_memory_metrics_label_limit{{dimension=\"category\"}} {}",
                locked.recall_category.max_labels
            ));
            lines.push(format!(
                "prx_memory_metrics_label_limit{{dimension=\"rerank_provider\"}} {}",
                locked.recall_rerank_provider.max_labels
            ));

            lines.push(format!(
                "prx_memory_recall_remote_rerank_attempts_total {}",
                locked.remote_rerank_attempts
            ));
            lines.push(format!(
                "prx_memory_recall_remote_rerank_warnings_total {}",
                locked.remote_rerank_warnings
            ));
            let remote_warning_ratio = if locked.remote_rerank_attempts == 0 {
                0.0
            } else {
                locked.remote_rerank_warnings as f64 / locked.remote_rerank_attempts as f64
            };
            lines.push(format!(
                "prx_memory_recall_remote_rerank_warning_ratio {:.6}",
                remote_warning_ratio
            ));

            lines.push(format!("prx_memory_sessions_active {}", active_sessions));
            lines.push(format!(
                "prx_memory_sessions_created_total {}",
                locked.sessions_created
            ));
            lines.push(format!(
                "prx_memory_sessions_renewed_total {}",
                locked.sessions_renewed
            ));
            lines.push(format!(
                "prx_memory_sessions_expired_total {}",
                locked.sessions_expired
            ));
            lines.push(format!(
                "prx_memory_session_access_errors_total{{kind=\"not_found_or_expired\"}} {}",
                locked.session_access_not_found
            ));
            lines.push(format!(
                "prx_memory_session_access_errors_total{{kind=\"internal\"}} {}",
                locked.session_access_poisoned
            ));

            let tool_error_ratio = if total_calls == 0 {
                0.0
            } else {
                total_errors as f64 / total_calls as f64
            };
            lines.push(format!(
                "prx_memory_tool_error_ratio {:.6}",
                tool_error_ratio
            ));

            let ratio_warn = env_f64("PRX_ALERT_TOOL_ERROR_RATIO_WARN", 0.05, 0.0, 1.0);
            let ratio_crit = env_f64("PRX_ALERT_TOOL_ERROR_RATIO_CRIT", 0.20, 0.0, 1.0);
            let remote_warn = env_f64("PRX_ALERT_REMOTE_WARNING_RATIO_WARN", 0.25, 0.0, 1.0);
            let remote_crit = env_f64("PRX_ALERT_REMOTE_WARNING_RATIO_CRIT", 0.60, 0.0, 1.0);
            let label_overflow = locked.recall_scope.overflow
                + locked.recall_category.overflow
                + locked.recall_rerank_provider.overflow;

            lines.push(format!(
                "prx_memory_alert_state{{signal=\"tool_error_ratio\"}} {}",
                alert_level(tool_error_ratio, ratio_warn, ratio_crit)
            ));
            lines.push(format!(
                "prx_memory_alert_state{{signal=\"remote_warning_ratio\"}} {}",
                alert_level(remote_warning_ratio, remote_warn, remote_crit)
            ));
            lines.push(format!(
                "prx_memory_alert_state{{signal=\"metrics_label_overflow\"}} {}",
                if label_overflow > 0 { 2 } else { 0 }
            ));
        }

        let embed_stats = embed_runtime_stats();
        lines.push(format!(
            "prx_memory_embed_cache_hits_total {}",
            embed_stats.cache_hits
        ));
        lines.push(format!(
            "prx_memory_embed_cache_misses_total {}",
            embed_stats.cache_misses
        ));
        lines.push(format!(
            "prx_memory_embed_cache_evictions_total {}",
            embed_stats.cache_evictions
        ));
        lines.push(format!(
            "prx_memory_embed_rate_wait_events_total {}",
            embed_stats.rate_wait_events
        ));
        lines.push(format!(
            "prx_memory_embed_rate_wait_ms_total {}",
            embed_stats.rate_wait_ms_total
        ));
        lines.join("\n")
    }

    fn render_metrics_summary(&self) -> Value {
        let active_sessions = self.sessions.lock().map(|v| v.len()).unwrap_or(0);
        let locked = match self.metrics.lock() {
            Ok(v) => v,
            Err(_) => {
                return json!({
                    "status": "error",
                    "message": "metrics lock poisoned"
                })
            }
        };

        let (total_calls, total_errors) = locked.tool.values().fold((0_u64, 0_u64), |acc, m| {
            (acc.0 + m.ok + m.err, acc.1 + m.err)
        });
        let tool_error_ratio = if total_calls == 0 {
            0.0
        } else {
            total_errors as f64 / total_calls as f64
        };
        let remote_warning_ratio = if locked.remote_rerank_attempts == 0 {
            0.0
        } else {
            locked.remote_rerank_warnings as f64 / locked.remote_rerank_attempts as f64
        };
        let label_overflow_total = locked.recall_scope.overflow
            + locked.recall_category.overflow
            + locked.recall_rerank_provider.overflow;
        let ratio_warn = env_f64("PRX_ALERT_TOOL_ERROR_RATIO_WARN", 0.05, 0.0, 1.0);
        let ratio_crit = env_f64("PRX_ALERT_TOOL_ERROR_RATIO_CRIT", 0.20, 0.0, 1.0);
        let remote_warn = env_f64("PRX_ALERT_REMOTE_WARNING_RATIO_WARN", 0.25, 0.0, 1.0);
        let remote_crit = env_f64("PRX_ALERT_REMOTE_WARNING_RATIO_CRIT", 0.60, 0.0, 1.0);

        let tool_alert = alert_level(tool_error_ratio, ratio_warn, ratio_crit);
        let remote_alert = alert_level(remote_warning_ratio, remote_warn, remote_crit);
        let overflow_alert = if label_overflow_total > 0 { 2 } else { 0 };
        let overall = [tool_alert, remote_alert, overflow_alert]
            .into_iter()
            .max()
            .unwrap_or(0);

        json!({
            "status": "ok",
            "overall_alert_level": overall,
            "tool_error_ratio": tool_error_ratio,
            "remote_warning_ratio": remote_warning_ratio,
            "label_overflow_total": label_overflow_total,
            "active_sessions": active_sessions,
            "session_counters": {
                "created": locked.sessions_created,
                "renewed": locked.sessions_renewed,
                "expired": locked.sessions_expired,
                "access_not_found_or_expired": locked.session_access_not_found,
                "access_internal": locked.session_access_poisoned
            },
            "thresholds": {
                "tool_error_ratio_warn": ratio_warn,
                "tool_error_ratio_crit": ratio_crit,
                "remote_warning_ratio_warn": remote_warn,
                "remote_warning_ratio_crit": remote_crit
            },
            "cardinality_limits": {
                "recall_scope": locked.recall_scope.max_labels,
                "recall_category": locked.recall_category.max_labels,
                "rerank_provider": locked.recall_rerank_provider.max_labels
            }
        })
    }

    fn session_ttl_ms() -> u64 {
        std::env::var("PRX_MEMORY_STREAM_SESSION_TTL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(600_000)
            .clamp(1_000, 86_400_000)
    }

    fn cleanup_expired_sessions_locked(
        sessions: &mut HashMap<String, SessionState>,
        now: u64,
    ) -> usize {
        let before = sessions.len();
        sessions.retain(|_, state| state.lease_expires_ms > now);
        before.saturating_sub(sessions.len())
    }

    fn create_session(&self) -> (String, u64) {
        let mut counter = self
            .session_counter
            .lock()
            .expect("session counter lock poisoned");
        let id = format!("sess-{}-{}", now_ms(), *counter);
        *counter = counter.saturating_add(1);

        let now = now_ms();
        let lease_expires_ms = now.saturating_add(Self::session_ttl_ms());
        let mut sessions = self.sessions.lock().expect("sessions lock poisoned");
        let expired = Self::cleanup_expired_sessions_locked(&mut sessions, now);
        self.record_session_expired(expired);
        sessions.insert(
            id.clone(),
            SessionState {
                next_seq: 1,
                events: VecDeque::new(),
                last_touch_ms: now,
                acked_seq: 0,
                lease_expires_ms,
            },
        );
        if let Ok(mut locked) = self.metrics.lock() {
            locked.sessions_created = locked.sessions_created.saturating_add(1);
        }
        (id, lease_expires_ms)
    }

    fn renew_session_lease(&self, session_id: &str) -> Result<u64, SessionAccessError> {
        let now = now_ms();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionAccessError::Poisoned)?;
        let expired = Self::cleanup_expired_sessions_locked(&mut sessions, now);
        self.record_session_expired(expired);
        let Some(state) = sessions.get_mut(session_id) else {
            return Err(SessionAccessError::NotFound);
        };
        if state.lease_expires_ms <= now {
            let _ = sessions.remove(session_id);
            return Err(SessionAccessError::Expired);
        }
        state.last_touch_ms = now;
        state.lease_expires_ms = now.saturating_add(Self::session_ttl_ms());
        if let Ok(mut locked) = self.metrics.lock() {
            locked.sessions_renewed = locked.sessions_renewed.saturating_add(1);
        }
        Ok(state.lease_expires_ms)
    }

    fn append_session_event(
        &self,
        session_id: &str,
        payload: Value,
    ) -> Result<(u64, u64), SessionAccessError> {
        let now = now_ms();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionAccessError::Poisoned)?;
        let expired = Self::cleanup_expired_sessions_locked(&mut sessions, now);
        self.record_session_expired(expired);
        let Some(state) = sessions.get_mut(session_id) else {
            return Err(SessionAccessError::NotFound);
        };
        if state.lease_expires_ms <= now {
            let _ = sessions.remove(session_id);
            return Err(SessionAccessError::Expired);
        }
        let seq = state.next_seq;
        state.next_seq = state.next_seq.saturating_add(1);
        state.last_touch_ms = now;
        state.lease_expires_ms = now.saturating_add(Self::session_ttl_ms());
        state.events.push_back(StreamEvent {
            seq,
            payload,
            created_ms: now,
        });
        while state.events.len() > 512 {
            let _ = state.events.pop_front();
        }
        Ok((seq, state.lease_expires_ms))
    }

    fn collect_session_events(
        &self,
        session_id: &str,
        from_seq: u64,
        limit: usize,
        ack_seq: Option<u64>,
    ) -> Result<SessionEventPage, SessionAccessError> {
        let now = now_ms();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SessionAccessError::Poisoned)?;
        let expired = Self::cleanup_expired_sessions_locked(&mut sessions, now);
        self.record_session_expired(expired);
        let Some(state) = sessions.get_mut(session_id) else {
            return Err(SessionAccessError::NotFound);
        };
        if state.lease_expires_ms <= now {
            let _ = sessions.remove(session_id);
            return Err(SessionAccessError::Expired);
        }
        let ack_applied = ack_seq.map(|ack| {
            state.acked_seq = state.acked_seq.max(ack);
            while matches!(state.events.front(), Some(ev) if ev.seq <= state.acked_seq) {
                let _ = state.events.pop_front();
            }
            state.acked_seq
        });

        state.last_touch_ms = now;
        state.lease_expires_ms = now.saturating_add(Self::session_ttl_ms());
        let oldest_seq = state
            .events
            .front()
            .map(|e| e.seq)
            .unwrap_or(state.next_seq);
        let effective_from = from_seq.max(oldest_seq);
        let events = state
            .events
            .iter()
            .filter(|e| e.seq >= effective_from)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_from = events.last().map(|e| e.seq + 1).unwrap_or(effective_from);
        Ok(SessionEventPage {
            events,
            effective_from,
            next_from,
            ack_applied,
            lease_expires_ms: state.lease_expires_ms,
        })
    }

    fn tools_list_result(&self) -> Value {
        json!({
            "tools": [
                {
                    "name": "memory_store",
                    "description": "Store a governed memory entry into local durable memory database.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["text"],
                        "properties": {
                            "text": {"type": "string"},
                            "category": {"type": "string"},
                            "scope": {"type": "string"},
                            "importance": {"type": "number"},
                            "importance_level": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                            "governed": {"type": "boolean"},
                            "use_vector": {"type": "boolean"},
                            "tags": {"type": "array", "items": {"type": "string"}},
                            "project_tag": {"type": "string"},
                            "tool_tag": {"type": "string"},
                            "domain_tag": {"type": "string"}
                        }
                    }
                },
                {
                    "name": "memory_recall",
                    "description": "Recall memories using lexical ranking, with optional third-party semantic rerank.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["query"],
                        "properties": {
                            "query": {"type": "string"},
                            "scope": {"type": "string"},
                            "category": {"type": "string"},
                            "limit": {"type": "integer"},
                            "use_vector": {"type": "boolean"},
                            "use_remote": {"type": "boolean"},
                            "provider": {"type": "string", "enum": ["openai-compatible", "jina", "gemini"]},
                            "rerank_provider": {"type": "string", "enum": ["jina", "none"]},
                            "vector_weight": {"type": "number"},
                            "lexical_weight": {"type": "number"},
                            "candidate_pool": {"type": "integer"}
                        }
                    }
                },
                {
                    "name": "memory_stats",
                    "description": "Get memory statistics with scope/category breakdown.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {"type": "string"}
                        }
                    }
                },
                {
                    "name": "memory_list",
                    "description": "List memories with optional scope and category filtering.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {"type": "string"},
                            "category": {"type": "string"},
                            "limit": {"type": "integer"},
                            "offset": {"type": "integer"}
                        }
                    }
                },
                {
                    "name": "memory_update",
                    "description": "Update an existing memory by id with governance and ACL checks.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["id"],
                        "properties": {
                            "id": {"type": "string"},
                            "text": {"type": "string"},
                            "category": {"type": "string"},
                            "scope": {"type": "string"},
                            "importance": {"type": "number"},
                            "importance_level": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                            "tags": {"type": "array", "items": {"type": "string"}},
                            "project_tag": {"type": "string"},
                            "tool_tag": {"type": "string"},
                            "domain_tag": {"type": "string"},
                            "governed": {"type": "boolean"}
                        }
                    }
                },
                {
                    "name": "memory_store_dual",
                    "description": "Store technical layer and principle layer memories in one transaction with recall verification.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["symptom", "cause", "fix", "prevention"],
                        "properties": {
                            "symptom": {"type":"string"},
                            "cause": {"type":"string"},
                            "fix": {"type":"string"},
                            "prevention": {"type":"string"},
                            "include_principle": {"type":"boolean"},
                            "principle_tag": {"type":"string"},
                            "principle_rule": {"type":"string"},
                            "trigger": {"type":"string"},
                            "action": {"type":"string"},
                            "scope": {"type":"string"},
                            "tags": {"type":"array","items":{"type":"string"}},
                            "project_tag": {"type":"string"},
                            "tool_tag": {"type":"string"},
                            "domain_tag": {"type":"string"},
                            "governed": {"type":"boolean"},
                            "use_vector": {"type":"boolean"},
                            "tech_importance_level": {"type":"string", "enum": ["low", "medium", "high", "critical"]},
                            "principle_importance_level": {"type":"string", "enum": ["low", "medium", "high", "critical"]}
                        }
                    }
                },
                {
                    "name": "memory_export",
                    "description": "Export memories by scope/category to JSON payload or file.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {"type": "string"},
                            "category": {"type": "string"},
                            "limit": {"type": "integer"},
                            "include_embeddings": {"type": "boolean"},
                            "output_path": {"type": "string"}
                        }
                    }
                },
                {
                    "name": "memory_import",
                    "description": "Import memory entries from payload into local store.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["entries"],
                        "properties": {
                            "entries": {"type":"array"},
                            "governed": {"type":"boolean"},
                            "use_vector": {"type":"boolean"},
                            "skip_duplicates": {"type":"boolean"}
                        }
                    }
                },
                {
                    "name": "memory_migrate",
                    "description": "Migrate memory data from JSON file.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["source_path"],
                        "properties": {
                            "source_path": {"type":"string"},
                            "governed": {"type":"boolean"},
                            "use_vector": {"type":"boolean"},
                            "skip_duplicates": {"type":"boolean"}
                        }
                    }
                },
                {
                    "name": "memory_reembed",
                    "description": "Rebuild embeddings for existing memories.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {"type":"string"},
                            "category": {"type":"string"},
                            "limit": {"type":"integer"}
                        }
                    }
                },
                {
                    "name": "memory_compact",
                    "description": "Compact duplicate memories in selected scope/category.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {"type":"string"},
                            "category": {"type":"string"},
                            "limit": {"type":"integer"},
                            "dry_run": {"type":"boolean"}
                        }
                    }
                },
                {
                    "name": "memory_forget",
                    "description": "Delete memory by id.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["id"],
                        "properties": {
                            "id": {"type": "string"}
                        }
                    }
                },
                {
                    "name": "memory_evolve",
                    "description": "Select best memory strategy variant using train+holdout acceptance.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["parent_score", "candidates"],
                        "properties": {
                            "parent_score": {"type": "number"},
                            "lambda": {"type": "number"},
                            "mu": {"type": "number"},
                            "candidates": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "required": ["id", "score_train", "score_holdout", "cost_penalty", "risk_penalty", "constraints_satisfied"],
                                    "properties": {
                                        "id": {"type": "string"},
                                        "score_train": {"type": "number"},
                                        "score_holdout": {"type": "number"},
                                        "cost_penalty": {"type": "number"},
                                        "risk_penalty": {"type": "number"},
                                        "constraints_satisfied": {"type": "boolean"}
                                    }
                                }
                            }
                        }
                    }
                },
                {
                    "name": "memory_skill_manifest",
                    "description": "Get built-in governance skill metadata and optional content.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "include_content": {"type":"boolean"}
                        }
                    }
                }
            ]
        })
    }

    fn resources_list_result(&self) -> Value {
        let resources = skill_resources()
            .iter()
            .map(|resource| {
                json!({
                    "uri": resource.uri,
                    "name": resource.name,
                    "description": resource.description,
                    "mimeType": resource.mime_type
                })
            })
            .collect::<Vec<_>>();
        json!({
            "resources": resources
        })
    }

    fn resources_templates_list_result(&self) -> Value {
        let templates = resource_templates()
            .iter()
            .map(|template| {
                json!({
                    "uriTemplate": template.uri_template,
                    "name": template.name,
                    "description": template.description,
                    "mimeType": template.mime_type
                })
            })
            .collect::<Vec<_>>();
        json!({
            "resourceTemplates": templates
        })
    }

    fn handle_resources_read(&self, id: Value, params: Value) -> JsonRpcResponse {
        let parsed: ResourceReadParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(err) => {
                return JsonRpcResponse::error(id, -32602, format!("invalid params: {err}"));
            }
        };

        let rendered = render_template_resource(&parsed.uri, &self.standards).or_else(|| {
            skill_resource_text(&parsed.uri).map(|text| RenderedResource {
                mime_type: "text/markdown",
                text: text.to_string(),
            })
        });
        let Some(rendered) = rendered else {
            return JsonRpcResponse::error(id, -32602, "unknown resource uri");
        };

        JsonRpcResponse::success(
            id,
            json!({
                "contents": [{
                    "uri": parsed.uri,
                    "mimeType": rendered.mime_type,
                    "text": rendered.text
                }]
            }),
        )
    }

    fn handle_tools_call(&self, id: Value, params: Value) -> JsonRpcResponse {
        let parsed: ToolsCallParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(err) => {
                return JsonRpcResponse::error(id, -32602, format!("invalid params: {err}"));
            }
        };

        let start = Instant::now();
        let tool = parsed.name.clone();
        let response = match parsed.name.as_str() {
            "memory_store" => self.exec_memory_store(id, parsed.arguments),
            "memory_recall" => self.exec_memory_recall(id, parsed.arguments),
            "memory_stats" => self.exec_memory_stats(id, parsed.arguments),
            "memory_list" => self.exec_memory_list(id, parsed.arguments),
            "memory_update" => self.exec_memory_update(id, parsed.arguments),
            "memory_store_dual" => self.exec_memory_store_dual(id, parsed.arguments),
            "memory_export" => self.exec_memory_export(id, parsed.arguments),
            "memory_import" => self.exec_memory_import(id, parsed.arguments),
            "memory_migrate" => self.exec_memory_migrate(id, parsed.arguments),
            "memory_reembed" => self.exec_memory_reembed(id, parsed.arguments),
            "memory_compact" => self.exec_memory_compact(id, parsed.arguments),
            "memory_forget" => self.exec_memory_forget(id, parsed.arguments),
            "memory_evolve" => self.exec_memory_evolve(id, parsed.arguments),
            "memory_skill_manifest" => self.exec_memory_skill_manifest(id, parsed.arguments),
            _ => JsonRpcResponse::error(id, -32601, "unknown tool"),
        };
        self.record_tool_metrics(
            &tool,
            start.elapsed().as_secs_f64() * 1000.0,
            response.error.is_some(),
        );
        response
    }

    fn exec_memory_store(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryStoreInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        let governed = args
            .governed
            .unwrap_or_else(|| self.standards.default_governed_for_store());
        if governed && enforce_dual_layer() {
            return JsonRpcResponse::error(
                id,
                -32602,
                "governed single-layer writes are disabled; use memory_store_dual",
            );
        }
        let category = args
            .category
            .unwrap_or_else(|| infer_default_category(&args.text).to_string());
        let tags = normalize_tags_with_defaults(
            args.tags.unwrap_or_default(),
            args.project_tag.as_deref(),
            args.tool_tag.as_deref(),
            args.domain_tag.as_deref(),
            &self.standards,
        );
        let target_scope = args.scope.unwrap_or_else(|| self.scopes.default_scope());
        let (importance, importance_level) =
            match resolve_importance(args.importance_level.as_deref(), args.importance) {
                Ok(v) => v,
                Err(msg) => return JsonRpcResponse::error(id, -32602, msg),
            };

        let mut locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let outcome = match store_layer_with_rules(
            &self.scopes,
            &self.auto_store_counter,
            locked.as_mut(),
            StoreLayerRequest {
                text: args.text,
                category,
                scope: target_scope.clone(),
                importance,
                importance_level,
                tags,
                governed,
                use_vector: args.use_vector.unwrap_or(false),
                enforce_verify: false,
                allow_auto_maintenance: true,
            },
        ) {
            Ok(v) => v,
            Err(msg) => return JsonRpcResponse::error(id, -32602, msg),
        };
        let mut entry = outcome.entry;
        entry.embedding = None;
        let mut structured_content = match serde_json::to_value(&entry) {
            Ok(v) => v,
            Err(_) => json!({}),
        };
        if let Some(obj) = structured_content.as_object_mut() {
            obj.insert(
                "auto_maintenance".to_string(),
                json!(outcome.auto_maintenance),
            );
        }

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": structured_content,
                "content": [{"type":"text", "text": format!("stored {}", entry.id)}],
                "governance": {
                    "governed": governed,
                    "importance_level": importance_level
                }
            }),
        )
    }

    fn exec_memory_store_dual(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryStoreDualInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        let governed = args.governed.unwrap_or(true);
        let use_vector = args.use_vector.unwrap_or(false);
        let include_principle = args.include_principle.unwrap_or(true);
        if governed && !include_principle {
            return JsonRpcResponse::error(
                id,
                -32602,
                "governed dual-layer writes require include_principle=true",
            );
        }
        let scope = args.scope.unwrap_or_else(|| self.scopes.default_scope());
        let tags = normalize_tags_with_defaults(
            args.tags.unwrap_or_default(),
            args.project_tag.as_deref(),
            args.tool_tag.as_deref(),
            args.domain_tag.as_deref(),
            &self.standards,
        );

        let (tech_importance, tech_level) =
            match resolve_importance(args.tech_importance_level.as_deref(), Some(0.75)) {
                Ok(v) => v,
                Err(msg) => return JsonRpcResponse::error(id, -32602, msg),
            };

        let tech_text = format!(
            "Pitfall: {}. Cause: {}. Fix: {}. Prevention: {}.",
            args.symptom.trim(),
            args.cause.trim(),
            args.fix.trim(),
            args.prevention.trim()
        );

        let mut principle_payload = None::<(String, f32, &'static str)>;
        if include_principle {
            let principle_tag = match args.principle_tag {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return JsonRpcResponse::error(
                        id,
                        -32602,
                        "principle_tag is required when include_principle=true",
                    )
                }
            };
            let principle_rule = match args.principle_rule {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return JsonRpcResponse::error(
                        id,
                        -32602,
                        "principle_rule is required when include_principle=true",
                    )
                }
            };
            let trigger = match args.trigger {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return JsonRpcResponse::error(
                        id,
                        -32602,
                        "trigger is required when include_principle=true",
                    )
                }
            };
            let action = match args.action {
                Some(v) if !v.trim().is_empty() => v,
                _ => {
                    return JsonRpcResponse::error(
                        id,
                        -32602,
                        "action is required when include_principle=true",
                    )
                }
            };
            let (principle_importance, principle_level) =
                match resolve_importance(args.principle_importance_level.as_deref(), Some(0.75)) {
                    Ok(v) => v,
                    Err(msg) => return JsonRpcResponse::error(id, -32602, msg),
                };

            let principle_text = format!(
                "Decision principle ({}): {}. Trigger: {}. Action: {}.",
                principle_tag.trim(),
                principle_rule.trim(),
                trigger.trim(),
                action.trim()
            );
            principle_payload = Some((principle_text, principle_importance, principle_level));
        }

        let mut locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let technical = match store_layer_with_rules(
            &self.scopes,
            &self.auto_store_counter,
            locked.as_mut(),
            StoreLayerRequest {
                text: tech_text,
                category: "fact".to_string(),
                scope: scope.clone(),
                importance: tech_importance,
                importance_level: tech_level,
                tags: tags.clone(),
                governed,
                use_vector,
                enforce_verify: true,
                allow_auto_maintenance: true,
            },
        ) {
            Ok(v) => v,
            Err(msg) => return JsonRpcResponse::error(id, -32602, msg),
        };

        let principle = if let Some((text, importance, level)) = principle_payload {
            match store_layer_with_rules(
                &self.scopes,
                &self.auto_store_counter,
                locked.as_mut(),
                StoreLayerRequest {
                    text,
                    category: "decision".to_string(),
                    scope: scope.clone(),
                    importance,
                    importance_level: level,
                    tags,
                    governed,
                    use_vector,
                    enforce_verify: true,
                    allow_auto_maintenance: true,
                },
            ) {
                Ok(v) => Some(v),
                Err(msg) => {
                    let _ = locked.forget_by_id(&technical.entry.id);
                    return JsonRpcResponse::error(
                        id,
                        -32602,
                        format!("principle layer store failed, rolled back technical layer: {msg}"),
                    );
                }
            }
        } else {
            None
        };
        let auto_maintenance = [
            technical.auto_maintenance,
            principle.as_ref().and_then(|v| v.auto_maintenance.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        let mut tech_clean = technical.entry;
        tech_clean.embedding = None;
        let principle_clean = principle.as_ref().map(|v| {
            let mut e = v.entry.clone();
            e.embedding = None;
            e
        });
        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "technical": tech_clean,
                    "principle": principle_clean,
                    "auto_maintenance": auto_maintenance,
                    "dual_layer_completed": true
                },
                "content": [{"type":"text","text":"dual-layer memory stored and verified"}]
            }),
        )
    }

    fn exec_memory_recall(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let total_start = Instant::now();
        let args: MemoryRecallInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        let query_text = args.query.clone();
        let limit = args.limit.unwrap_or(5).clamp(1, 20);
        let candidate_pool = args.candidate_pool.unwrap_or(limit * 6).clamp(limit, 200);
        self.record_recall_dimensions(
            args.scope.as_deref(),
            args.category.as_deref(),
            args.rerank_provider.as_deref(),
        );
        let query_embedding = if args.use_vector.unwrap_or(false) {
            match embed_one(&query_text, EmbeddingTask::Query) {
                Ok(v) => Some(v),
                Err(msg) => return JsonRpcResponse::error(id, -32002, msg),
            }
        } else {
            None
        };

        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let local_start = Instant::now();
        let mut results = recall_with_acl(
            locked.as_ref(),
            &self.scopes,
            RecallAclRequest {
                query: query_text.clone(),
                query_embedding,
                requested_scope: args.scope,
                category: args.category,
                candidate_pool,
                vector_weight: args.vector_weight,
                lexical_weight: args.lexical_weight,
            },
        );
        drop(locked);
        self.record_recall_stage("local", local_start.elapsed().as_secs_f64() * 1000.0);

        let mut warning: Option<String> = None;
        if args.use_remote.unwrap_or(false) && !results.is_empty() {
            self.record_remote_rerank_attempt();
            let remote_start = Instant::now();
            match semantic_rerank_with_remote(
                &query_text,
                &mut results,
                args.provider.as_deref(),
                args.rerank_provider.as_deref(),
            ) {
                Ok(maybe_warning) => {
                    warning = maybe_warning;
                }
                Err(msg) => {
                    warning = Some(msg);
                }
            }
            if warning.is_some() {
                self.record_remote_rerank_warning();
            }
            self.record_recall_stage("remote", remote_start.elapsed().as_secs_f64() * 1000.0);
        }
        results.truncate(limit);
        self.record_recall_stage("total", total_start.elapsed().as_secs_f64() * 1000.0);

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "count": results.len(),
                    "warning": warning,
                    "agent_id": self.scopes.agent_id,
                    "items": results.iter().map(|r| {
                        let mut e = r.entry.clone();
                        e.embedding = None;
                        json!({"entry": e, "score": r.score})
                    }).collect::<Vec<_>>()
                },
                "content": [{
                    "type":"text",
                    "text": if let Some(w) = warning {
                        format!("Recalled {} entries. {}", results.len(), w)
                    } else {
                        format!("Recalled {} entries.", results.len())
                    }
                }]
            }),
        )
    }

    fn exec_memory_forget(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryForgetInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        let mut locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let deleted = match locked.forget_by_id(&args.id) {
            Ok(v) => v,
            Err(err) => return JsonRpcResponse::error(id, -32001, err.to_string()),
        };

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {"deleted": deleted, "id": args.id},
                "content": [{"type":"text", "text": if deleted {"deleted"} else {"not found"}}]
            }),
        )
    }

    fn exec_memory_update(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryUpdateInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        let governed = args
            .governed
            .unwrap_or_else(|| self.standards.default_governed_for_update());
        let mut locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let existing = locked.list(200_000).into_iter().find(|e| e.id == args.id);
        let Some(existing) = existing else {
            return JsonRpcResponse::error(id, -32602, "memory id not found");
        };

        if !self.scopes.can_access_scope(&existing.scope) {
            return JsonRpcResponse::error(id, -32602, "scope access denied for existing memory");
        }

        let merged_scope = args.scope.unwrap_or(existing.scope.clone());
        let merged_category = args.category.unwrap_or(existing.category.clone());
        let merged_text = args.text.unwrap_or(existing.text.clone());
        let merged_tags = normalize_tags_with_defaults(
            args.tags.unwrap_or(existing.tags.clone()),
            args.project_tag.as_deref(),
            args.tool_tag.as_deref(),
            args.domain_tag.as_deref(),
            &self.standards,
        );
        let (merged_importance, importance_level) =
            match resolve_importance(args.importance_level.as_deref(), args.importance) {
                Ok((importance, level)) => (importance, level),
                Err(_) => (
                    existing.importance,
                    importance_level_from_numeric(existing.importance),
                ),
            };
        let merged_embedding = if merged_text != existing.text {
            match embed_one(&merged_text, EmbeddingTask::Passage) {
                Ok(v) => Some(v),
                Err(_) => existing.embedding.clone(),
            }
        } else {
            existing.embedding.clone()
        };

        if !self.scopes.can_access_scope(&merged_scope) {
            return JsonRpcResponse::error(id, -32602, "scope access denied for target scope");
        }
        if let Some(msg) = self
            .scopes
            .validate_scope_write(&merged_scope, &merged_tags)
        {
            return JsonRpcResponse::error(id, -32602, msg);
        }

        if governed {
            if let Err(msg) = validate_governed_input(
                &merged_text,
                &merged_category,
                &merged_tags,
                importance_level,
            ) {
                return JsonRpcResponse::error(id, -32602, msg);
            }
        }

        match locked.forget_by_id(&args.id) {
            Ok(true) => {}
            Ok(false) => return JsonRpcResponse::error(id, -32602, "memory id not found"),
            Err(err) => return JsonRpcResponse::error(id, -32001, err.to_string()),
        }

        let updated = match locked.store(NewMemoryEntry {
            text: merged_text,
            category: merged_category,
            scope: merged_scope,
            importance: merged_importance,
            tags: merged_tags,
            embedding: merged_embedding,
        }) {
            Ok(v) => v,
            Err(err) => return JsonRpcResponse::error(id, -32001, err.to_string()),
        };

        let mut updated_clean = updated;
        updated_clean.embedding = None;
        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "replaced_id": args.id,
                    "entry": updated_clean
                },
                "content": [{"type":"text", "text": "memory updated"}]
            }),
        )
    }

    fn exec_memory_export(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryExportInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        if let Some(scope) = &args.scope {
            if !self.scopes.can_access_scope(scope) {
                return JsonRpcResponse::error(id, -32602, format!("scope access denied: {scope}"));
            }
        }

        let limit = args.limit.unwrap_or(500).clamp(1, 20_000);
        let include_embeddings = args.include_embeddings.unwrap_or(false);
        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };
        let rows = locked.list(200_000);
        drop(locked);

        let mut items = filter_entries_by_acl(
            rows,
            &self.scopes,
            args.scope.as_deref(),
            args.category.as_deref(),
        );
        items.truncate(limit);
        if !include_embeddings {
            for row in &mut items {
                row.embedding = None;
            }
        }

        if let Some(path) = args.output_path {
            let payload = json!({ "entries": items });
            let bytes = match serde_json::to_vec_pretty(&payload) {
                Ok(v) => v,
                Err(err) => return JsonRpcResponse::error(id, -32001, err.to_string()),
            };
            if let Err(err) = fs::write(&path, bytes) {
                return JsonRpcResponse::error(id, -32001, err.to_string());
            }
            return JsonRpcResponse::success(
                id,
                json!({
                    "structuredContent": {
                        "count": payload["entries"].as_array().map(|v| v.len()).unwrap_or(0),
                        "output_path": path
                    },
                    "content": [{"type":"text","text":"memory export completed"}]
                }),
            );
        }

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "count": items.len(),
                    "items": items
                },
                "content": [{"type":"text","text": format!("exported {} memories", items.len())}]
            }),
        )
    }

    fn exec_memory_import(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryImportInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        let options = ImportOptions {
            governed: args
                .governed
                .unwrap_or_else(|| self.standards.default_governed_for_import()),
            use_vector: args.use_vector.unwrap_or(false),
            skip_duplicates: args.skip_duplicates.unwrap_or(true),
        };
        let summary = self.import_entries(args.entries, options);
        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "created": summary.created,
                    "skipped": summary.skipped,
                    "failed": summary.failed,
                    "errors": summary.errors
                },
                "content": [{"type":"text","text": format!("import done: created={}, skipped={}, failed={}", summary.created, summary.skipped, summary.failed)}]
            }),
        )
    }

    fn exec_memory_migrate(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryMigrateInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        let bytes = match fs::read(&args.source_path) {
            Ok(v) => v,
            Err(err) => return JsonRpcResponse::error(id, -32001, err.to_string()),
        };

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum MigratePayload {
            EntriesObj { entries: Vec<ImportedMemoryEntry> },
            EntriesArray(Vec<ImportedMemoryEntry>),
        }

        let entries = match serde_json::from_slice::<MigratePayload>(&bytes) {
            Ok(MigratePayload::EntriesObj { entries }) => entries,
            Ok(MigratePayload::EntriesArray(entries)) => entries,
            Err(err) => {
                return JsonRpcResponse::error(id, -32602, format!("invalid migrate file: {err}"))
            }
        };

        let options = ImportOptions {
            governed: args.governed.unwrap_or(false),
            use_vector: args.use_vector.unwrap_or(false),
            skip_duplicates: args.skip_duplicates.unwrap_or(true),
        };
        let summary = self.import_entries(entries, options);
        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "source_path": args.source_path,
                    "created": summary.created,
                    "skipped": summary.skipped,
                    "failed": summary.failed,
                    "errors": summary.errors
                },
                "content": [{"type":"text","text":"memory migration completed"}]
            }),
        )
    }

    fn exec_memory_reembed(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryReembedInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        if let Some(scope) = &args.scope {
            if !self.scopes.can_access_scope(scope) {
                return JsonRpcResponse::error(id, -32602, format!("scope access denied: {scope}"));
            }
        }
        let limit = args.limit.unwrap_or(200).clamp(1, 5_000);

        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };
        let rows = locked.list(200_000);
        drop(locked);

        let targets = filter_entries_by_acl(
            rows,
            &self.scopes,
            args.scope.as_deref(),
            args.category.as_deref(),
        )
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();

        let mut updated = 0usize;
        let mut failed = 0usize;
        let mut errors = Vec::new();
        for item in targets {
            let embedding = match embed_one(&item.text, EmbeddingTask::Passage) {
                Ok(v) => Some(v),
                Err(err) => {
                    failed += 1;
                    errors.push(format!("{}: {}", item.id, err));
                    continue;
                }
            };

            let mut locked = match self.store.lock() {
                Ok(v) => v,
                Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
            };
            match locked.forget_by_id(&item.id) {
                Ok(true) => {}
                Ok(false) => {
                    failed += 1;
                    errors.push(format!("{}: missing during reembed", item.id));
                    continue;
                }
                Err(err) => {
                    failed += 1;
                    errors.push(format!("{}: {}", item.id, err));
                    continue;
                }
            }
            match locked.store(NewMemoryEntry {
                text: item.text,
                category: item.category,
                scope: item.scope,
                importance: item.importance,
                tags: item.tags,
                embedding,
            }) {
                Ok(_) => updated += 1,
                Err(err) => {
                    failed += 1;
                    errors.push(err.to_string());
                }
            }
        }

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "updated": updated,
                    "failed": failed,
                    "errors": errors
                },
                "content": [{"type":"text","text": format!("reembed done: updated={}, failed={}", updated, failed)}]
            }),
        )
    }

    fn exec_memory_compact(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryCompactInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        if let Some(scope) = &args.scope {
            if !self.scopes.can_access_scope(scope) {
                return JsonRpcResponse::error(id, -32602, format!("scope access denied: {scope}"));
            }
        }
        let limit = args.limit.unwrap_or(50_000).clamp(1, 200_000);
        let dry_run = args.dry_run.unwrap_or(true);

        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };
        let rows = locked.list(200_000);
        drop(locked);
        let filtered = filter_entries_by_acl(
            rows,
            &self.scopes,
            args.scope.as_deref(),
            args.category.as_deref(),
        )
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();

        let mut keep_keys = HashSet::new();
        let mut duplicate_ids = Vec::new();
        for row in filtered {
            let key = format!(
                "{}|{}|{}",
                row.scope,
                row.category,
                compact_query(&row.text, 16)
            );
            if !keep_keys.insert(key) {
                duplicate_ids.push(row.id);
            }
        }

        let mut deleted = 0usize;
        if !dry_run {
            let mut locked = match self.store.lock() {
                Ok(v) => v,
                Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
            };
            for mid in &duplicate_ids {
                if matches!(locked.forget_by_id(mid), Ok(true)) {
                    deleted += 1;
                }
            }
        }

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "dry_run": dry_run,
                    "duplicates": duplicate_ids.len(),
                    "deleted": deleted,
                    "candidate_ids": duplicate_ids
                },
                "content": [{"type":"text","text": format!("compact {}: duplicates={}, deleted={}", if dry_run {"preview"} else {"apply"}, duplicate_ids.len(), deleted)}]
            }),
        )
    }

    fn import_entries(
        &self,
        entries: Vec<ImportedMemoryEntry>,
        options: ImportOptions,
    ) -> ImportSummary {
        let mut created = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut errors = Vec::new();

        for (idx, raw) in entries.into_iter().enumerate() {
            let scope = raw.scope.unwrap_or_else(|| self.scopes.default_scope());
            let category = raw.category.unwrap_or_else(|| "other".to_string());
            let tags = normalize_tags_with_defaults(
                raw.tags.unwrap_or_default(),
                raw.project_tag.as_deref(),
                raw.tool_tag.as_deref(),
                raw.domain_tag.as_deref(),
                &self.standards,
            );
            let (importance, importance_level) =
                match resolve_importance(raw.importance_level.as_deref(), raw.importance) {
                    Ok(v) => v,
                    Err(err) => {
                        failed += 1;
                        errors.push(format!("entry#{idx}: {err}"));
                        continue;
                    }
                };

            if !self.scopes.can_access_scope(&scope) {
                failed += 1;
                errors.push(format!("entry#{idx}: scope access denied: {scope}"));
                continue;
            }
            if let Some(msg) = self.scopes.validate_scope_write(&scope, &tags) {
                failed += 1;
                errors.push(format!("entry#{idx}: {msg}"));
                continue;
            }
            if options.governed {
                if let Err(msg) =
                    validate_governed_input(&raw.text, &category, &tags, importance_level)
                {
                    failed += 1;
                    errors.push(format!("entry#{idx}: {msg}"));
                    continue;
                }
            }

            let embedding = if let Some(v) = raw.embedding {
                Some(v)
            } else if options.use_vector {
                match embed_one(&raw.text, EmbeddingTask::Passage) {
                    Ok(v) => Some(v),
                    Err(err) => {
                        failed += 1;
                        errors.push(format!("entry#{idx}: {}", err));
                        continue;
                    }
                }
            } else {
                None
            };

            let mut locked = match self.store.lock() {
                Ok(v) => v,
                Err(_) => {
                    failed += 1;
                    errors.push(format!("entry#{idx}: storage lock poisoned"));
                    continue;
                }
            };
            if options.skip_duplicates {
                let similar = locked.recall(RecallQuery {
                    query: compact_query(&raw.text, 10),
                    query_embedding: None,
                    scope: Some(scope.clone()),
                    category: Some(category.clone()),
                    limit: 1,
                    vector_weight: None,
                    lexical_weight: None,
                });
                if similar.first().is_some_and(|r| r.score > 0.93) {
                    skipped += 1;
                    continue;
                }
            }

            match locked.store(NewMemoryEntry {
                text: raw.text,
                category,
                scope,
                importance,
                tags,
                embedding,
            }) {
                Ok(_) => created += 1,
                Err(err) => {
                    failed += 1;
                    errors.push(format!("entry#{idx}: {}", err));
                }
            }
        }

        ImportSummary {
            created,
            skipped,
            failed,
            errors,
        }
    }

    fn exec_memory_stats(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryStatsInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        if let Some(scope) = &args.scope {
            if !self.scopes.can_access_scope(scope) {
                return JsonRpcResponse::error(id, -32602, format!("scope access denied: {scope}"));
            }
        }

        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let backend_stats = locked.stats();
        let rows = locked.list(200_000);
        let filtered = filter_entries_by_acl(rows, &self.scopes, args.scope.as_deref(), None);

        let mut scope_counts: HashMap<String, usize> = HashMap::new();
        let mut category_counts: HashMap<String, usize> = HashMap::new();
        for entry in &filtered {
            *scope_counts.entry(entry.scope.clone()).or_insert(0) += 1;
            *category_counts.entry(entry.category.clone()).or_insert(0) += 1;
        }
        let decision_ratio = if filtered.is_empty() {
            0.0
        } else {
            (*category_counts.get("decision").unwrap_or(&0) as f32) / (filtered.len() as f32)
        };

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "count": filtered.len(),
                    "decision_ratio": decision_ratio,
                    "scope_counts": scope_counts,
                    "category_counts": category_counts,
                    "agent_id": self.scopes.agent_id,
                    "allowed_scopes": self.scopes.accessible_scopes(),
                    "standardization": {
                        "profile": self.standards.profile_label(),
                        "default_tags": {
                            "project": self.standards.default_project_tag.clone(),
                            "tool": self.standards.default_tool_tag.clone(),
                            "domain": self.standards.default_domain_tag.clone()
                        }
                    },
                    "backend_stats": backend_stats
                },
                "content": [{
                    "type":"text",
                    "text": format!("stats count={}, decision_ratio={:.3}", filtered.len(), decision_ratio)
                }]
            }),
        )
    }

    fn exec_memory_list(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryListInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        if let Some(scope) = &args.scope {
            if !self.scopes.can_access_scope(scope) {
                return JsonRpcResponse::error(id, -32602, format!("scope access denied: {scope}"));
            }
        }

        let limit = args.limit.unwrap_or(20).clamp(1, 100);
        let offset = args.offset.unwrap_or(0).min(20_000);
        let fetch = (offset + limit).min(200_000);

        let locked = match self.store.lock() {
            Ok(v) => v,
            Err(_) => return JsonRpcResponse::error(id, -32000, "storage lock poisoned"),
        };

        let rows = locked.list(fetch);
        let filtered = filter_entries_by_acl(
            rows,
            &self.scopes,
            args.scope.as_deref(),
            args.category.as_deref(),
        );
        let items = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>();

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "count": items.len(),
                    "offset": offset,
                    "limit": limit,
                    "items": items
                },
                "content": [{
                    "type":"text",
                    "text": format!("listed {} memories", items.len())
                }]
            }),
        )
    }

    fn exec_memory_evolve(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemoryEvolveInput = match parse_args(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };

        let policy = EvolutionPolicy {
            lambda: args.lambda.unwrap_or(0.2),
            mu: args.mu.unwrap_or(0.2),
        };

        let candidates = args
            .candidates
            .into_iter()
            .map(|c| VariantCandidate {
                id: c.id,
                score_train: c.score_train,
                score_holdout: c.score_holdout,
                cost_penalty: c.cost_penalty,
                risk_penalty: c.risk_penalty,
                constraints_satisfied: c.constraints_satisfied,
            })
            .collect::<Vec<_>>();

        let decision = EvolutionRunner::new(policy).run_generation(args.parent_score, &candidates);

        JsonRpcResponse::success(
            id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": format!(
                            "accepted_variant={:?}, effective_score={:.4}, reason={}",
                            decision.accepted_variant_id,
                            decision.effective_score,
                            decision.reason
                        )
                    }
                ],
                "structuredContent": {
                    "accepted_variant_id": decision.accepted_variant_id,
                    "effective_score": decision.effective_score,
                    "reason": decision.reason
                }
            }),
        )
    }

    fn exec_memory_skill_manifest(&self, id: Value, arguments: Option<Value>) -> JsonRpcResponse {
        let args: MemorySkillManifestInput = match parse_args_optional(arguments) {
            Ok(v) => v,
            Err(resp) => return with_id(resp, id),
        };
        let include_content = args.include_content.unwrap_or(false);
        let resources = self
            .resources_list_result()
            .get("resources")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let content = if include_content {
            json!(skill_resources()
                .iter()
                .map(|resource| json!({
                    "uri": resource.uri,
                    "mimeType": resource.mime_type,
                    "text": resource.text
                }))
                .collect::<Vec<_>>())
        } else {
            Value::Null
        };

        JsonRpcResponse::success(
            id,
            json!({
                "structuredContent": {
                    "skill_id": SKILL_ID,
                    "resources": resources,
                    "content": content
                },
                "content": [{
                    "type": "text",
                    "text": "skill manifest ready"
                }]
            }),
        )
    }

    pub fn serve_stdio(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        let mut stdout = io::stdout();
        let mut line = String::new();

        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }

            let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
            if trimmed.is_empty() {
                continue;
            }

            let (payload, frame) = if is_stdio_header_line(trimmed) {
                let content_length = match read_stdio_content_length(&mut reader, trimmed) {
                    Ok(v) => v,
                    Err(err) => {
                        let response = JsonRpcResponse::error(
                            Value::Null,
                            -32700,
                            format!("invalid stdio frame: {err}"),
                        );
                        write_stdio_response(&mut stdout, &response, StdioFrame::LineDelimited)?;
                        continue;
                    }
                };

                let mut body = vec![0_u8; content_length];
                if let Err(err) = reader.read_exact(&mut body) {
                    let response = JsonRpcResponse::error(
                        Value::Null,
                        -32700,
                        format!("invalid stdio frame body: {err}"),
                    );
                    write_stdio_response(&mut stdout, &response, StdioFrame::ContentLength)?;
                    continue;
                }
                (body, StdioFrame::ContentLength)
            } else {
                (trimmed.as_bytes().to_vec(), StdioFrame::LineDelimited)
            };

            let request: JsonRpcRequest = match serde_json::from_slice(&payload) {
                Ok(v) => v,
                Err(err) => {
                    let response =
                        JsonRpcResponse::error(Value::Null, -32700, format!("parse error: {err}"));
                    write_stdio_response(&mut stdout, &response, frame)?;
                    continue;
                }
            };

            if let Some(response) = self.handle_request(request) {
                write_stdio_response(&mut stdout, &response, frame)?;
            }
        }

        Ok(())
    }

    pub fn serve_http(&self, addr: &str) -> io::Result<()> {
        let listener = TcpListener::bind(addr)?;
        eprintln!(
            "prx-memory-mcp http listening on {}",
            listener.local_addr()?
        );
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if let Err(err) = self.handle_http_connection(stream) {
                        eprintln!("prx-memory-mcp http request error: {err}");
                    }
                }
                Err(err) => {
                    eprintln!("prx-memory-mcp http accept error: {err}");
                }
            }
        }
        Ok(())
    }

    fn handle_http_connection(&self, mut stream: TcpStream) -> io::Result<()> {
        let Some(req) = read_http_request(&stream)? else {
            return Ok(());
        };
        if self.is_sse_stream_request(&req) {
            return self.handle_http_stream_sse(&mut stream, req);
        }
        let response = self.dispatch_http_request(req);
        write_http_response(&mut stream, response)
    }

    fn is_sse_stream_request(&self, req: &HttpRequest) -> bool {
        if req.method != "GET" || req.path != "/mcp/stream" {
            return false;
        }
        if req
            .query
            .get("mode")
            .map(|v| v.eq_ignore_ascii_case("sse"))
            .unwrap_or(false)
        {
            return true;
        }
        req.headers
            .get("accept")
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false)
    }

    fn handle_http_stream_sse(&self, stream: &mut TcpStream, req: HttpRequest) -> io::Result<()> {
        let Some(session_id) = req.query.get("session").cloned() else {
            return write_http_response(
                stream,
                HttpResponse::json(
                    400,
                    json!({"error":"invalid_request","message":"missing query param: session"}),
                ),
            );
        };
        let from = req
            .query
            .get("from")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        let limit = req
            .query
            .get("limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(50)
            .clamp(1, 500);
        let mut ack = req.query.get("ack").and_then(|v| v.parse::<u64>().ok());
        let wait_ms = req
            .query
            .get("wait_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(15_000)
            .clamp(0, 60_000);
        let heartbeat_ms = req
            .query
            .get("heartbeat_ms")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(3_000)
            .clamp(100, 10_000);

        let mut page = match self.collect_session_events(&session_id, from, limit, ack) {
            Ok(v) => v,
            Err(err) => {
                self.record_session_access_error(err);
                return write_http_response(stream, session_error_response(err));
            }
        };
        ack = None;

        write_sse_headers(stream)?;

        let deadline = Instant::now() + Duration::from_millis(wait_ms);
        let mut remaining = limit.saturating_sub(page.events.len());
        let mut next_from = page.next_from;
        let mut last_heartbeat = Instant::now();

        write_sse_events(stream, &page.events)?;
        stream.flush()?;

        while remaining > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            page = match self.collect_session_events(&session_id, next_from, remaining, ack) {
                Ok(v) => v,
                Err(err) => {
                    self.record_session_access_error(err);
                    write_sse_event(stream, "error", &json!({"error": session_error_code(err)}))?;
                    stream.flush()?;
                    return Ok(());
                }
            };
            if !page.events.is_empty() {
                write_sse_events(stream, &page.events)?;
                stream.flush()?;
                remaining = remaining.saturating_sub(page.events.len());
                next_from = page.next_from;
                continue;
            }
            if last_heartbeat.elapsed() >= Duration::from_millis(heartbeat_ms) {
                stream.write_all(b": keep-alive\n\n")?;
                stream.flush()?;
                last_heartbeat = Instant::now();
            }
        }

        write_sse_event(
            stream,
            "cursor",
            &json!({
                "session_id": session_id,
                "next_from": next_from,
                "effective_from": page.effective_from,
                "ack_applied": page.ack_applied,
                "lease_expires_ms": page.lease_expires_ms
            }),
        )?;
        stream.flush()
    }

    fn dispatch_http_request(&self, req: HttpRequest) -> HttpResponse {
        if req.method == "GET" && req.path == "/health" {
            return HttpResponse::json(200, json!({"status":"ok"}));
        }

        if req.method == "GET" && req.path == "/metrics" {
            return HttpResponse::text(
                200,
                "text/plain; version=0.0.4; charset=utf-8",
                self.render_metrics_text(),
            );
        }

        if req.method == "GET" && req.path == "/metrics/summary" {
            return HttpResponse::json(200, self.render_metrics_summary());
        }

        if req.method == "POST" && req.path == "/mcp/session/start" {
            let (session_id, lease_expires_ms) = self.create_session();
            return HttpResponse::json(
                200,
                json!({
                    "session_id": session_id,
                    "lease_ttl_ms": Self::session_ttl_ms(),
                    "lease_expires_ms": lease_expires_ms
                }),
            );
        }

        if req.method == "POST" && req.path == "/mcp/session/renew" {
            let Some(session_id) = req.query.get("session").cloned() else {
                return HttpResponse::json(
                    400,
                    json!({"error":"invalid_request","message":"missing query param: session"}),
                );
            };
            return match self.renew_session_lease(&session_id) {
                Ok(expires) => HttpResponse::json(
                    200,
                    json!({
                        "session_id": session_id,
                        "lease_ttl_ms": Self::session_ttl_ms(),
                        "lease_expires_ms": expires
                    }),
                ),
                Err(err) => {
                    self.record_session_access_error(err);
                    session_error_response(err)
                }
            };
        }

        if req.method == "POST" && req.path == "/mcp/stream" {
            let Some(session_id) = req.query.get("session").cloned() else {
                return HttpResponse::json(
                    400,
                    json!({"error":"invalid_request","message":"missing query param: session"}),
                );
            };
            let rpc: JsonRpcRequest = match serde_json::from_slice(&req.body) {
                Ok(v) => v,
                Err(err) => {
                    return HttpResponse::json(
                        400,
                        json!({"jsonrpc":"2.0","id": Value::Null, "error":{"code":-32700,"message": format!("parse error: {err}")}}),
                    )
                }
            };
            let payload = match self.handle_request(rpc) {
                Some(v) => match serde_json::to_value(v) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return HttpResponse::json(
                            500,
                            json!({"error":"internal_error","message":"failed to serialize rpc response"}),
                        )
                    }
                },
                None => json!({"jsonrpc":"2.0","id": Value::Null, "result": null}),
            };
            match self.append_session_event(&session_id, payload) {
                Ok((seq, lease_expires_ms)) => {
                    return HttpResponse::json(
                        202,
                        json!({
                            "accepted": true,
                            "session_id": session_id,
                            "seq": seq,
                            "lease_expires_ms": lease_expires_ms
                        }),
                    )
                }
                Err(err) => {
                    self.record_session_access_error(err);
                    return session_error_response(err);
                }
            }
        }

        if req.method == "GET" && req.path == "/mcp/stream" {
            let Some(session_id) = req.query.get("session").cloned() else {
                return HttpResponse::json(
                    400,
                    json!({"error":"invalid_request","message":"missing query param: session"}),
                );
            };
            let from = req
                .query
                .get("from")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1);
            let limit = req
                .query
                .get("limit")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(50)
                .clamp(1, 500);
            let ack = req.query.get("ack").and_then(|v| v.parse::<u64>().ok());
            match self.collect_session_events(&session_id, from, limit, ack) {
                Ok(page) => {
                    return HttpResponse::json(
                        200,
                        json!({
                            "session_id": session_id,
                            "from": from,
                            "effective_from": page.effective_from,
                            "next_from": page.next_from,
                            "ack_applied": page.ack_applied,
                            "lease_expires_ms": page.lease_expires_ms,
                            "count": page.events.len(),
                            "events": page.events.into_iter().map(|e| {
                                json!({
                                    "seq": e.seq,
                                    "created_ms": e.created_ms,
                                    "payload": e.payload
                                })
                            }).collect::<Vec<_>>()
                        }),
                    );
                }
                Err(err) => {
                    self.record_session_access_error(err);
                    return session_error_response(err);
                }
            }
        }

        if req.method != "POST" {
            return HttpResponse::json(
                405,
                json!({"error":"method_not_allowed","message":"supported endpoints: GET /health, GET /metrics, GET /metrics/summary, POST /mcp, POST /mcp/session/start, POST /mcp/session/renew, POST/GET /mcp/stream"}),
            );
        }

        if req.path != "/mcp" && req.path != "/" {
            return HttpResponse::json(404, json!({"error":"not_found","message":"use POST /mcp"}));
        }

        let rpc: JsonRpcRequest = match serde_json::from_slice(&req.body) {
            Ok(v) => v,
            Err(err) => {
                return HttpResponse::json(
                    400,
                    json!({"jsonrpc":"2.0","id": Value::Null, "error":{"code":-32700,"message": format!("parse error: {err}")}}),
                )
            }
        };
        match self.handle_request(rpc) {
            Some(v) => match serde_json::to_value(v) {
                Ok(payload) => HttpResponse::json(200, payload),
                Err(_) => HttpResponse::json(
                    500,
                    json!({"error":"internal_error","message":"failed to serialize rpc response"}),
                ),
            },
            None => HttpResponse::json(
                204,
                json!({"jsonrpc":"2.0","id": Value::Null, "result": null}),
            ),
        }
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

fn with_id(mut response: JsonRpcResponse, id: Value) -> JsonRpcResponse {
    response.id = id;
    response
}

fn session_error_code(err: SessionAccessError) -> &'static str {
    match err {
        SessionAccessError::NotFound => "session_not_found",
        SessionAccessError::Expired => "session_expired",
        SessionAccessError::Poisoned => "session_internal_error",
    }
}

fn session_error_response(err: SessionAccessError) -> HttpResponse {
    match err {
        SessionAccessError::NotFound => HttpResponse::json(
            404,
            json!({"error":"session_not_found","message":"unknown session id"}),
        ),
        SessionAccessError::Expired => HttpResponse::json(
            410,
            json!({"error":"session_expired","message":"session lease expired"}),
        ),
        SessionAccessError::Poisoned => HttpResponse::json(
            500,
            json!({"error":"session_internal_error","message":"session lock poisoned"}),
        ),
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, value: Value) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        Self {
            status,
            content_type: "application/json",
            body,
        }
    }

    fn text(status: u16, content_type: &'static str, body: String) -> Self {
        Self {
            status,
            content_type,
            body: body.into_bytes(),
        }
    }
}

fn read_http_request(stream: &TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut reader = io::BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let first = line.trim_end_matches(['\r', '\n']);
    if first.is_empty() {
        return Ok(None);
    }

    let mut parts = first.split_whitespace();
    let Some(method) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid http request line (missing method)",
        ));
    };
    let Some(path_with_query) = parts.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid http request line (missing path)",
        ));
    };
    let (path, query) = parse_path_query(path_with_query);

    let mut content_length = 0usize;
    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0_u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    Ok(Some(HttpRequest {
        method: method.to_string(),
        path,
        query,
        headers,
        body,
    }))
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    let reason = http_reason_phrase(response.status);
    let headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn write_sse_headers(stream: &mut TcpStream) -> io::Result<()> {
    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nX-Accel-Buffering: no\r\n\r\n";
    stream.write_all(headers.as_bytes())
}

fn write_sse_events(stream: &mut TcpStream, events: &[StreamEvent]) -> io::Result<()> {
    for event in events {
        write_sse_event(
            stream,
            "message",
            &json!({
                "seq": event.seq,
                "created_ms": event.created_ms,
                "payload": event.payload
            }),
        )?;
    }
    Ok(())
}

fn write_sse_event(stream: &mut TcpStream, kind: &str, payload: &Value) -> io::Result<()> {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    let frame = format!("event: {kind}\ndata: {data}\n\n");
    stream.write_all(frame.as_bytes())
}

fn http_reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn parse_path_query(raw: &str) -> (String, HashMap<String, String>) {
    let (path, query_str) = match raw.split_once('?') {
        Some((p, q)) => (p.to_string(), q),
        None => (raw.to_string(), ""),
    };
    let mut query = HashMap::new();
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            query.insert(k.to_string(), v.to_string());
        } else {
            query.insert(pair.to_string(), String::new());
        }
    }
    (path, query)
}

fn sorted_counter(map: &HashMap<String, u64>) -> Vec<(String, u64)> {
    let mut pairs = map.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

fn prom_label_value(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

fn sanitize_label_value(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.' | '*') {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
        if out.len() >= 64 {
            break;
        }
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

#[derive(Clone, Copy)]
enum StdioFrame {
    LineDelimited,
    ContentLength,
}

fn write_stdio_response(
    stdout: &mut io::Stdout,
    response: &JsonRpcResponse,
    frame: StdioFrame,
) -> io::Result<()> {
    match frame {
        StdioFrame::LineDelimited => {
            let serialized = serde_json::to_string(response)?;
            writeln!(stdout, "{serialized}")?;
        }
        StdioFrame::ContentLength => {
            let serialized = serde_json::to_vec(response)?;
            write!(stdout, "Content-Length: {}\r\n\r\n", serialized.len())?;
            stdout.write_all(&serialized)?;
        }
    }
    stdout.flush()
}

fn is_stdio_header_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.starts_with("content-length:") || lower.starts_with("content-type:")
}

fn read_stdio_content_length<R: BufRead>(reader: &mut R, first_line: &str) -> io::Result<usize> {
    let mut content_length = parse_content_length(first_line);
    let mut header_line = String::new();
    loop {
        header_line.clear();
        if reader.read_line(&mut header_line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected eof while reading frame headers",
            ));
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = parse_content_length(trimmed) {
            content_length = Some(v);
        }
    }
    content_length
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content-length header"))
}

fn parse_content_length(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

fn env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn env_f64(name: &str, default: f64, min: f64, max: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}

fn alert_level(value: f64, warn: f64, crit: f64) -> u8 {
    if value >= crit {
        2
    } else if value >= warn {
        1
    } else {
        0
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(
    arguments: Option<Value>,
) -> Result<T, JsonRpcResponse> {
    let args = match arguments {
        Some(v) => v,
        None => {
            return Err(JsonRpcResponse::error(
                Value::Null,
                -32602,
                "missing tool arguments",
            ))
        }
    };

    serde_json::from_value(args).map_err(|err| {
        JsonRpcResponse::error(
            Value::Null,
            -32602,
            format!("invalid tool arguments: {err}"),
        )
    })
}

fn parse_args_optional<T: for<'de> Deserialize<'de> + Default>(
    arguments: Option<Value>,
) -> Result<T, JsonRpcResponse> {
    match arguments {
        Some(v) => serde_json::from_value(v).map_err(|err| {
            JsonRpcResponse::error(
                Value::Null,
                -32602,
                format!("invalid tool arguments: {err}"),
            )
        }),
        None => Ok(T::default()),
    }
}

#[derive(Debug, Deserialize)]
struct ToolsCallParams {
    name: String,
    arguments: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ResourceReadParams {
    uri: String,
}

#[derive(Debug, Clone, Copy)]
struct ResourceTemplateDef {
    uri_template: &'static str,
    name: &'static str,
    description: &'static str,
    mime_type: &'static str,
}

#[derive(Debug, Clone)]
struct RenderedResource {
    mime_type: &'static str,
    text: String,
}

fn resource_templates() -> &'static [ResourceTemplateDef] {
    &[
        ResourceTemplateDef {
            uri_template: "prx://templates/memory-store{?text,category,scope,importance_level}",
            name: "template:memory-store",
            description:
                "Standardized memory_store payload template for zero-config and long-term usage.",
            mime_type: "application/json",
        },
        ResourceTemplateDef {
            uri_template: "prx://templates/memory-recall{?query,scope,category,limit}",
            name: "template:memory-recall",
            description: "Standardized memory_recall payload template.",
            mime_type: "application/json",
        },
        ResourceTemplateDef {
            uri_template: "prx://templates/memory-store-dual{?symptom,cause,fix,prevention,scope}",
            name: "template:memory-store-dual",
            description: "Standardized dual-layer write payload template.",
            mime_type: "application/json",
        },
    ]
}

#[derive(Debug, Deserialize)]
struct MemoryStoreInput {
    text: String,
    category: Option<String>,
    scope: Option<String>,
    importance: Option<f32>,
    importance_level: Option<String>,
    governed: Option<bool>,
    use_vector: Option<bool>,
    tags: Option<Vec<String>>,
    project_tag: Option<String>,
    tool_tag: Option<String>,
    domain_tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryRecallInput {
    query: String,
    scope: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
    use_vector: Option<bool>,
    use_remote: Option<bool>,
    provider: Option<String>,
    rerank_provider: Option<String>,
    vector_weight: Option<f32>,
    lexical_weight: Option<f32>,
    candidate_pool: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MemoryForgetInput {
    id: String,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateInput {
    id: String,
    text: Option<String>,
    category: Option<String>,
    scope: Option<String>,
    importance: Option<f32>,
    importance_level: Option<String>,
    tags: Option<Vec<String>>,
    project_tag: Option<String>,
    tool_tag: Option<String>,
    domain_tag: Option<String>,
    governed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryStoreDualInput {
    symptom: String,
    cause: String,
    fix: String,
    prevention: String,
    include_principle: Option<bool>,
    principle_tag: Option<String>,
    principle_rule: Option<String>,
    trigger: Option<String>,
    action: Option<String>,
    scope: Option<String>,
    tags: Option<Vec<String>>,
    project_tag: Option<String>,
    tool_tag: Option<String>,
    domain_tag: Option<String>,
    governed: Option<bool>,
    use_vector: Option<bool>,
    tech_importance_level: Option<String>,
    principle_importance_level: Option<String>,
}

#[derive(Debug, Clone)]
struct StoreLayerRequest {
    text: String,
    category: String,
    scope: String,
    importance: f32,
    importance_level: &'static str,
    tags: Vec<String>,
    governed: bool,
    use_vector: bool,
    enforce_verify: bool,
    allow_auto_maintenance: bool,
}

#[derive(Debug, Clone)]
struct StoreLayerOutcome {
    entry: MemoryEntry,
    auto_maintenance: Option<AutoMaintenanceReport>,
}

#[derive(Debug, Clone, Serialize)]
struct AutoMaintenanceReport {
    trigger_every: usize,
    total_before: usize,
    total_after: usize,
    merged_groups: usize,
    duplicate_deleted: usize,
    rebalance_deleted: usize,
    rebalance_scopes: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryExportInput {
    scope: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
    include_embeddings: Option<bool>,
    output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryImportInput {
    entries: Vec<ImportedMemoryEntry>,
    governed: Option<bool>,
    use_vector: Option<bool>,
    skip_duplicates: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryMigrateInput {
    source_path: String,
    governed: Option<bool>,
    use_vector: Option<bool>,
    skip_duplicates: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryReembedInput {
    scope: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryCompactInput {
    scope: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
    dry_run: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
struct ImportedMemoryEntry {
    text: String,
    category: Option<String>,
    scope: Option<String>,
    importance: Option<f32>,
    importance_level: Option<String>,
    tags: Option<Vec<String>>,
    project_tag: Option<String>,
    tool_tag: Option<String>,
    domain_tag: Option<String>,
    embedding: Option<Vec<f32>>,
}

#[derive(Debug)]
struct ImportOptions {
    governed: bool,
    use_vector: bool,
    skip_duplicates: bool,
}

#[derive(Debug)]
struct ImportSummary {
    created: usize,
    skipped: usize,
    failed: usize,
    errors: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryStatsInput {
    scope: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryListInput {
    scope: Option<String>,
    category: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MemoryEvolveInput {
    parent_score: f32,
    lambda: Option<f32>,
    mu: Option<f32>,
    candidates: Vec<MemoryEvolveCandidate>,
}

#[derive(Debug, Deserialize)]
struct MemoryEvolveCandidate {
    id: String,
    score_train: f32,
    score_holdout: f32,
    cost_penalty: f32,
    risk_penalty: f32,
    constraints_satisfied: bool,
}

#[derive(Debug, Deserialize, Default)]
struct MemorySkillManifestInput {
    include_content: Option<bool>,
}

impl ScopeManager {
    fn from_env() -> Self {
        let agent_id = std::env::var("PRX_MEMORY_AGENT_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "default-agent".to_string());
        let default_scope = std::env::var("PRX_MEMORY_DEFAULT_SCOPE")
            .ok()
            .map(|s| s.trim().replace("{agent_id}", &agent_id))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "global".to_string());

        let raw = std::env::var("PRX_MEMORY_ALLOWED_SCOPES").ok();
        let allowed_scope_rules = match raw {
            Some(v) => {
                let items = v
                    .split(',')
                    .map(|s| s.trim().replace("{agent_id}", &agent_id))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                if items.is_empty() {
                    vec!["global".to_string(), format!("agent:{agent_id}")]
                } else {
                    items
                }
            }
            None => vec!["global".to_string(), format!("agent:{agent_id}")],
        };
        let agent_access = std::env::var("PRX_MEMORY_AGENT_ACCESS")
            .ok()
            .and_then(|raw| serde_json::from_str::<HashMap<String, Vec<String>>>(&raw).ok())
            .unwrap_or_default();

        Self {
            agent_id,
            default_scope,
            allowed_scope_rules,
            agent_access,
        }
    }

    fn default_scope(&self) -> String {
        let preferred = self.default_scope.clone();
        if self.can_access_scope(&preferred) {
            return preferred;
        }
        let own = format!("agent:{}", self.agent_id);
        if self.can_access_scope(&own) {
            return own;
        }
        "global".to_string()
    }

    fn accessible_scope_rules(&self) -> Vec<String> {
        if let Some(scopes) = self.agent_access.get(&self.agent_id) {
            let expanded = scopes
                .iter()
                .map(|s| s.replace("{agent_id}", &self.agent_id))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            if !expanded.is_empty() {
                return expanded;
            }
        }
        self.allowed_scope_rules.clone()
    }

    fn accessible_scopes(&self) -> Vec<String> {
        self.accessible_scope_rules()
    }

    fn has_pattern_rule(&self) -> bool {
        self.accessible_scope_rules()
            .iter()
            .any(|r| r == "*" || r.ends_with('*'))
    }

    fn is_valid_scope(scope: &str) -> bool {
        scope == "global"
            || scope.starts_with("agent:")
            || scope.starts_with("custom:")
            || scope.starts_with("project:")
            || scope.starts_with("user:")
    }

    fn rule_matches_scope(rule: &str, scope: &str) -> bool {
        if rule == "*" {
            return true;
        }
        if let Some(prefix) = rule.strip_suffix('*') {
            return scope.starts_with(prefix);
        }
        rule == scope
    }

    fn can_access_scope(&self, scope: &str) -> bool {
        if !Self::is_valid_scope(scope) {
            return false;
        }
        self.accessible_scope_rules()
            .iter()
            .any(|rule| Self::rule_matches_scope(rule, scope))
    }

    fn validate_scope_write(&self, scope: &str, tags: &[String]) -> Option<String> {
        if scope.starts_with("agent:") {
            let own = format!("agent:{}", self.agent_id);
            if scope != own {
                let cross_domain = tags.iter().any(|t| {
                    let lower = t.to_lowercase();
                    lower == "cross-domain" || lower == "root-cause:cross-domain"
                });
                if !cross_domain {
                    return Some(
                        "agent cannot write to other agent scope without cross-domain tag"
                            .to_string(),
                    );
                }
            }
        }
        None
    }
}

fn store_layer_with_rules(
    scopes: &ScopeManager,
    auto_store_counter: &Mutex<usize>,
    store: &mut dyn StorageBackend,
    req: StoreLayerRequest,
) -> Result<StoreLayerOutcome, String> {
    if !scopes.can_access_scope(&req.scope) {
        return Err(format!("scope access denied: {}", req.scope));
    }
    if let Some(msg) = scopes.validate_scope_write(&req.scope, &req.tags) {
        return Err(msg);
    }
    if req.governed {
        validate_governed_input(&req.text, &req.category, &req.tags, req.importance_level)?;
    }

    if req.governed {
        let maybe_dup = store.recall(RecallQuery {
            query: compact_query(&req.text, 10),
            query_embedding: None,
            scope: Some(req.scope.clone()),
            category: Some(req.category.clone()),
            limit: 3,
            vector_weight: None,
            lexical_weight: None,
        });
        if let Some(top) = maybe_dup.first() {
            if top.score > 0.93 {
                return Err(format!("duplicate memory likely exists: {}", top.entry.id));
            }
        }
        if req.category == "decision" {
            let ratio = decision_ratio_in_scope(store, &req.scope);
            if ratio > 0.30 {
                return Err("decision memory ratio exceeds 30% in current scope".to_string());
            }
        }
    }

    let embedding = if req.use_vector {
        Some(embed_one(&req.text, EmbeddingTask::Passage)?)
    } else {
        None
    };

    let entry = store
        .store(NewMemoryEntry {
            text: req.text,
            category: req.category,
            scope: req.scope,
            importance: req.importance,
            tags: req.tags,
            embedding,
        })
        .map_err(|e| e.to_string())?;

    if req.enforce_verify || (req.governed && req.importance_level == "critical") {
        let verify = store.recall(RecallQuery {
            query: compact_query(&entry.text, 8),
            query_embedding: None,
            scope: Some(entry.scope.clone()),
            category: Some(entry.category.clone()),
            limit: 5,
            vector_weight: None,
            lexical_weight: None,
        });
        let found = verify.iter().any(|r| r.entry.id == entry.id);
        if !found {
            let _ = store.forget_by_id(&entry.id);
            return Err("post-store recall verification failed".to_string());
        }
    }

    let should_trigger = {
        let mut counter = auto_store_counter
            .lock()
            .map_err(|_| "auto maintenance lock poisoned".to_string())?;
        *counter = counter.saturating_add(1);
        req.allow_auto_maintenance && (*counter % 100 == 0)
    };
    let auto_maintenance = if should_trigger {
        Some(run_periodic_maintenance(scopes, store)?)
    } else {
        None
    };

    Ok(StoreLayerOutcome {
        entry,
        auto_maintenance,
    })
}

fn run_periodic_maintenance(
    scopes: &ScopeManager,
    store: &mut dyn StorageBackend,
) -> Result<AutoMaintenanceReport, String> {
    let before_rows = filter_entries_by_acl(store.list(200_000), scopes, None, None);
    let total_before = before_rows.len();
    let mut duplicate_deleted = 0usize;
    let mut merged_groups = 0usize;

    if total_before > 1 {
        let mut groups: HashMap<String, Vec<MemoryEntry>> = HashMap::new();
        for row in before_rows {
            let key = format!(
                "{}|{}|{}",
                row.scope,
                row.category,
                compact_query(&row.text, 16)
            );
            groups.entry(key).or_default().push(row);
        }

        for group in groups.into_values() {
            if group.len() <= 1 {
                continue;
            }
            merged_groups += 1;
            let mut ranked = group;
            ranked.sort_by(|a, b| {
                b.importance
                    .total_cmp(&a.importance)
                    .then_with(|| b.timestamp_ms.cmp(&a.timestamp_ms))
            });
            let keep_id = ranked[0].id.clone();
            for item in ranked.into_iter().skip(1) {
                if item.id == keep_id {
                    continue;
                }
                if matches!(store.forget_by_id(&item.id), Ok(true)) {
                    duplicate_deleted += 1;
                }
            }
        }
    }

    let after_dedup_rows = filter_entries_by_acl(store.list(200_000), scopes, None, None);
    let mut by_scope: HashMap<String, Vec<MemoryEntry>> = HashMap::new();
    for row in after_dedup_rows {
        by_scope.entry(row.scope.clone()).or_default().push(row);
    }

    let mut rebalance_deleted = 0usize;
    let mut rebalance_scopes = Vec::new();
    let mut notes = Vec::new();
    for (scope, rows) in by_scope {
        let mut total = rows.len() as isize;
        if total <= 0 {
            continue;
        }
        let mut decisions = rows
            .iter()
            .filter(|e| e.category == "decision")
            .cloned()
            .collect::<Vec<_>>();
        let mut decision_count = decisions.len() as isize;
        if decision_count <= 0 {
            continue;
        }
        if (decision_count as f32) / (total as f32) <= 0.30 {
            continue;
        }

        decisions.sort_by(|a, b| {
            a.importance
                .total_cmp(&b.importance)
                .then_with(|| a.timestamp_ms.cmp(&b.timestamp_ms))
        });

        let mut scope_deleted = 0usize;
        for item in decisions {
            if (decision_count as f32) / (total as f32) <= 0.30 {
                break;
            }
            if item.importance >= 1.0 {
                continue;
            }
            if matches!(store.forget_by_id(&item.id), Ok(true)) {
                scope_deleted += 1;
                rebalance_deleted += 1;
                decision_count -= 1;
                total -= 1;
            }
        }

        if scope_deleted > 0 {
            rebalance_scopes.push(scope.clone());
        }
        if (decision_count as f32) / (total.max(1) as f32) > 0.30 {
            notes.push(format!(
                "scope {} still above decision ratio after trimming non-critical decision entries",
                scope
            ));
        }
    }

    let total_after = filter_entries_by_acl(store.list(200_000), scopes, None, None).len();
    Ok(AutoMaintenanceReport {
        trigger_every: 100,
        total_before,
        total_after,
        merged_groups,
        duplicate_deleted,
        rebalance_deleted,
        rebalance_scopes,
        notes,
    })
}

fn render_template_resource(
    uri: &str,
    standards: &StandardizationConfig,
) -> Option<RenderedResource> {
    let params = parse_uri_query(uri);
    if uri.starts_with("prx://templates/memory-store") {
        let text = params
            .get("text")
            .cloned()
            .unwrap_or_else(|| "Pitfall: .... Cause: .... Fix: .... Prevention: ....".to_string());
        let category = params
            .get("category")
            .cloned()
            .unwrap_or_else(|| "fact".to_string());
        let scope = params
            .get("scope")
            .cloned()
            .unwrap_or_else(|| "global".to_string());
        let importance_level = params
            .get("importance_level")
            .cloned()
            .unwrap_or_else(|| "medium".to_string());
        return Some(RenderedResource {
            mime_type: "application/json",
            text: json!({
                "jsonrpc":"2.0",
                "id":1,
                "method":"tools/call",
                "params":{
                    "name":"memory_store",
                    "arguments":{
                        "text": text,
                        "category": category,
                        "scope": scope,
                        "importance_level": importance_level,
                        "governed": standards.default_governed_for_store(),
                        "tags":[
                            format!("project:{}", standards.default_project_tag.to_ascii_lowercase()),
                            format!("tool:{}", standards.default_tool_tag.to_ascii_lowercase()),
                            format!("domain:{}", standards.default_domain_tag.to_ascii_lowercase())
                        ]
                    }
                }
            })
            .to_string(),
        });
    }
    if uri.starts_with("prx://templates/memory-recall") {
        let query = params
            .get("query")
            .cloned()
            .unwrap_or_else(|| "tool + error + symptom keywords".to_string());
        let scope = params
            .get("scope")
            .cloned()
            .unwrap_or_else(|| "global".to_string());
        let category = params
            .get("category")
            .cloned()
            .unwrap_or_else(|| "fact".to_string());
        let limit = params
            .get("limit")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);
        return Some(RenderedResource {
            mime_type: "application/json",
            text: json!({
                "jsonrpc":"2.0",
                "id":1,
                "method":"tools/call",
                "params":{
                    "name":"memory_recall",
                    "arguments":{
                        "query": query,
                        "scope": scope,
                        "category": category,
                        "limit": limit
                    }
                }
            })
            .to_string(),
        });
    }
    if uri.starts_with("prx://templates/memory-store-dual") {
        let symptom = params
            .get("symptom")
            .cloned()
            .unwrap_or_else(|| "symptom".to_string());
        let cause = params
            .get("cause")
            .cloned()
            .unwrap_or_else(|| "cause".to_string());
        let fix = params
            .get("fix")
            .cloned()
            .unwrap_or_else(|| "fix".to_string());
        let prevention = params
            .get("prevention")
            .cloned()
            .unwrap_or_else(|| "prevention".to_string());
        let scope = params
            .get("scope")
            .cloned()
            .unwrap_or_else(|| "global".to_string());
        return Some(RenderedResource {
            mime_type: "application/json",
            text: json!({
                "jsonrpc":"2.0",
                "id":1,
                "method":"tools/call",
                "params":{
                    "name":"memory_store_dual",
                    "arguments":{
                        "symptom": symptom,
                        "cause": cause,
                        "fix": fix,
                        "prevention": prevention,
                        "principle_tag":"general-principle",
                        "principle_rule":"prefer standardized and reusable memory entries",
                        "trigger":"new recurring issue appears",
                        "action":"store dual-layer memory and verify by recall",
                        "scope": scope,
                        "governed": true,
                        "tags":[
                            format!("project:{}", standards.default_project_tag.to_ascii_lowercase()),
                            format!("tool:{}", standards.default_tool_tag.to_ascii_lowercase()),
                            format!("domain:{}", standards.default_domain_tag.to_ascii_lowercase())
                        ]
                    }
                }
            })
            .to_string(),
        });
    }
    None
}

fn parse_uri_query(uri: &str) -> HashMap<String, String> {
    let (_, query) = match uri.split_once('?') {
        Some(v) => v,
        None => return HashMap::new(),
    };
    let mut out = HashMap::new();
    for pair in query.split('&') {
        if pair.trim().is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => (pair.trim(), ""),
        };
        if k.is_empty() {
            continue;
        }
        out.insert(k.to_string(), v.to_string());
    }
    out
}

fn enforce_dual_layer() -> bool {
    match std::env::var("PRX_MEMORY_ENFORCE_DUAL_LAYER") {
        Ok(v) => {
            let lowered = v.trim().to_ascii_lowercase();
            !(lowered == "0" || lowered == "false" || lowered == "off" || lowered == "no")
        }
        Err(_) => true,
    }
}

fn infer_default_category(text: &str) -> &'static str {
    let lower = text.to_ascii_lowercase();
    if lower.contains("pitfall:")
        && lower.contains("cause:")
        && lower.contains("fix:")
        && lower.contains("prevention:")
    {
        return "fact";
    }
    if lower.contains("decision principle") {
        return "decision";
    }
    "other"
}

fn normalize_tags_with_defaults(
    raw_tags: Vec<String>,
    project_tag: Option<&str>,
    tool_tag: Option<&str>,
    domain_tag: Option<&str>,
    standards: &StandardizationConfig,
) -> Vec<String> {
    let mut tags = normalize_tags(raw_tags, project_tag, tool_tag, domain_tag);
    if !tags.iter().any(|t| t.starts_with("project:")) {
        tags.push(format!(
            "project:{}",
            standards.default_project_tag.to_ascii_lowercase()
        ));
    }
    if !tags.iter().any(|t| t.starts_with("tool:")) {
        tags.push(format!(
            "tool:{}",
            standards.default_tool_tag.to_ascii_lowercase()
        ));
    }
    if !tags.iter().any(|t| t.starts_with("domain:")) {
        tags.push(format!(
            "domain:{}",
            standards.default_domain_tag.to_ascii_lowercase()
        ));
    }
    tags
}

fn normalize_tags(
    raw_tags: Vec<String>,
    project_tag: Option<&str>,
    tool_tag: Option<&str>,
    domain_tag: Option<&str>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for tag in raw_tags {
        let normalized = canonicalize_tag(&tag);
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    for explicit in [
        prefixed_tag("project", project_tag),
        prefixed_tag("tool", tool_tag),
        prefixed_tag("domain", domain_tag),
    ]
    .into_iter()
    .flatten()
    {
        if seen.insert(explicit.clone()) {
            out.push(explicit);
        }
    }

    out
}

fn prefixed_tag(prefix: &str, value: Option<&str>) -> Option<String> {
    let raw = value?.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with(&format!("{prefix}:")) {
        return Some(raw);
    }
    Some(format!("{prefix}:{raw}"))
}

fn canonicalize_tag(tag: &str) -> String {
    let trimmed = tag.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains(':') {
        return trimmed;
    }
    match trimmed.as_str() {
        "prx-memory" => "project:prx-memory".to_string(),
        "mcp" | "lancedb" | "openai-compatible" | "jina" | "gemini" | "openclaw"
        | "claude-code" | "codex" | "openprx" => format!("tool:{trimmed}"),
        _ => format!("domain:{trimmed}"),
    }
}

fn resolve_importance(
    level: Option<&str>,
    numeric: Option<f32>,
) -> Result<(f32, &'static str), String> {
    if let Some(lv) = level {
        return match lv {
            "low" => Ok((0.25, "low")),
            "medium" => Ok((0.50, "medium")),
            "high" => Ok((0.75, "high")),
            "critical" => Ok((1.0, "critical")),
            _ => Err("importance_level must be low|medium|high|critical".to_string()),
        };
    }

    if let Some(score) = numeric {
        let s = score.clamp(0.0, 1.0);
        if (s - 0.25).abs() < 1e-3 {
            return Ok((0.25, "low"));
        }
        if (s - 0.50).abs() < 1e-3 {
            return Ok((0.50, "medium"));
        }
        if (s - 0.75).abs() < 1e-3 {
            return Ok((0.75, "high"));
        }
        if (s - 1.0).abs() < 1e-3 {
            return Ok((1.0, "critical"));
        }
        return Err(
            "numeric importance must be one of 0.25/0.50/0.75/1.0; prefer importance_level"
                .to_string(),
        );
    }

    Ok((0.50, "medium"))
}

fn importance_level_from_numeric(v: f32) -> &'static str {
    if v >= 0.95 {
        "critical"
    } else if v >= 0.70 {
        "high"
    } else if v >= 0.45 {
        "medium"
    } else {
        "low"
    }
}

fn validate_governed_input(
    text: &str,
    category: &str,
    tags: &[String],
    importance_level: &str,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("text cannot be empty".to_string());
    }
    if text.chars().count() > 500 {
        return Err("entry must be <= 500 chars".to_string());
    }
    if text.contains("```") || text.contains("stacktrace") || text.contains("raw conversation") {
        return Err("log-like or raw content is not allowed".to_string());
    }

    let cat = category.to_lowercase();
    let allowed = ["preference", "fact", "decision", "entity", "other"];
    if !allowed.contains(&cat.as_str()) {
        return Err("category must be one of preference|fact|decision|entity|other".to_string());
    }
    if tags.is_empty() {
        return Err("tags are required in governed mode".to_string());
    }
    let has_project = tags.iter().any(|t| t.starts_with("project:"));
    let has_tool = tags.iter().any(|t| t.starts_with("tool:"));
    let has_domain = tags.iter().any(|t| t.starts_with("domain:"));
    if !(has_project && has_tool && has_domain) {
        return Err("tags must include project:*, tool:*, domain:*".to_string());
    }

    let lower = text.to_lowercase();
    if cat == "fact"
        && !(lower.contains("pitfall:")
            && lower.contains("cause:")
            && lower.contains("fix:")
            && lower.contains("prevention:"))
    {
        return Err("fact entry must follow Pitfall/Cause/Fix/Prevention template".to_string());
    }
    if cat == "decision" && !lower.contains("decision principle") {
        return Err("decision entry must follow Decision principle template".to_string());
    }
    if cat == "decision" && importance_level == "low" {
        return Err("decision importance must be medium/high/critical".to_string());
    }
    Ok(())
}

fn compact_query(text: &str, max_terms: usize) -> String {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for t in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 3)
        .map(|s| s.to_lowercase())
    {
        if seen.insert(t.clone()) {
            out.push(t);
        }
        if out.len() >= max_terms {
            break;
        }
    }
    out.join(" ")
}

fn decision_ratio_in_scope(store: &dyn StorageBackend, scope: &str) -> f32 {
    let rows = store.list(200_000);
    let mut total = 0usize;
    let mut decision = 0usize;
    for e in rows {
        if e.scope != scope {
            continue;
        }
        total += 1;
        if e.category == "decision" {
            decision += 1;
        }
    }
    if total == 0 {
        0.0
    } else {
        decision as f32 / total as f32
    }
}

struct RecallAclRequest {
    query: String,
    query_embedding: Option<Vec<f32>>,
    requested_scope: Option<String>,
    category: Option<String>,
    candidate_pool: usize,
    vector_weight: Option<f32>,
    lexical_weight: Option<f32>,
}

fn recall_with_acl(
    store: &dyn StorageBackend,
    access: &ScopeManager,
    req: RecallAclRequest,
) -> Vec<RecallResult> {
    if let Some(scope) = req.requested_scope {
        if !access.can_access_scope(&scope) {
            return Vec::new();
        }
        return store.recall(RecallQuery {
            query: req.query,
            query_embedding: req.query_embedding,
            scope: Some(scope),
            category: req.category,
            limit: req.candidate_pool,
            vector_weight: req.vector_weight,
            lexical_weight: req.lexical_weight,
        });
    }

    let rules = access.accessible_scope_rules();
    if access.has_pattern_rule() {
        let mut all = store.recall(RecallQuery {
            query: req.query,
            query_embedding: req.query_embedding,
            scope: None,
            category: req.category,
            limit: req.candidate_pool,
            vector_weight: req.vector_weight,
            lexical_weight: req.lexical_weight,
        });
        all.retain(|r| access.can_access_scope(&r.entry.scope));
        all.sort_by(|a, b| b.score.total_cmp(&a.score));
        all.truncate(req.candidate_pool);
        return all;
    }

    let mut merged = Vec::new();
    for scope in &rules {
        let mut one = store.recall(RecallQuery {
            query: req.query.clone(),
            query_embedding: req.query_embedding.clone(),
            scope: Some(scope.clone()),
            category: req.category.clone(),
            limit: req.candidate_pool,
            vector_weight: req.vector_weight,
            lexical_weight: req.lexical_weight,
        });
        merged.append(&mut one);
    }
    merged.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = HashSet::new();
    merged.retain(|r| seen.insert(r.entry.id.clone()));
    merged.truncate(req.candidate_pool);
    merged
}

fn filter_entries_by_acl(
    entries: Vec<prx_memory_storage::MemoryEntry>,
    access: &ScopeManager,
    requested_scope: Option<&str>,
    requested_category: Option<&str>,
) -> Vec<prx_memory_storage::MemoryEntry> {
    entries
        .into_iter()
        .filter(|e| match requested_scope {
            Some(scope) => e.scope == scope,
            None => access.can_access_scope(&e.scope),
        })
        .filter(|e| match requested_category {
            Some(cat) => e.category == cat,
            None => true,
        })
        .collect()
}

fn embed_one(text: &str, task: EmbeddingTask) -> Result<Vec<f32>, String> {
    let provider_hint = std::env::var("PRX_EMBED_PROVIDER")
        .unwrap_or_else(|_| "openai-compatible".to_string())
        .to_ascii_lowercase();
    let key = format!(
        "{}|{:?}|{}",
        provider_hint,
        task,
        text.trim().to_ascii_lowercase()
    );

    if let Ok(mut runtime) = embed_runtime().lock() {
        if let Some(v) = runtime.cache_get(&key, now_ms()) {
            return Ok(v);
        }
    }

    let wait_ms = {
        let mut runtime = embed_runtime()
            .lock()
            .map_err(|_| "embedding runtime lock poisoned".to_string())?;
        runtime.acquire_rate_limit(now_ms())
    };
    if wait_ms > 0 {
        std::thread::sleep(Duration::from_millis(wait_ms));
    }

    let provider = build_embedding_provider_from_env(None)?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("vector runtime initialization failed: {e}"))?;
    let output = rt
        .block_on(async {
            provider
                .embed(EmbeddingRequest {
                    inputs: vec![text.to_string()],
                    task: Some(task),
                    dimensions: None,
                    normalized: Some(true),
                })
                .await
        })
        .map_err(|e| format!("vector embedding failed: {}", provider_error_en_embed(&e)))?;

    let vector = output
        .vectors
        .into_iter()
        .next()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "vector embedding returned empty vector".to_string())?;

    if let Ok(mut runtime) = embed_runtime().lock() {
        runtime.cache_put(key, vector.clone(), now_ms());
    }
    Ok(vector)
}

fn semantic_rerank_with_remote(
    query: &str,
    results: &mut [RecallResult],
    embedding_provider_hint: Option<&str>,
    rerank_provider_hint: Option<&str>,
) -> Result<Option<String>, String> {
    match cross_encoder_rerank_with_remote(query, results, rerank_provider_hint) {
        Ok(()) => Ok(None),
        Err(cross_err) => {
            semantic_rerank_with_embeddings(query, results, embedding_provider_hint)?;
            Ok(Some(format!(
                "Cross-encoder rerank unavailable: {}. Used embedding cosine fallback.",
                cross_err
            )))
        }
    }
}

fn cross_encoder_rerank_with_remote(
    query: &str,
    results: &mut [RecallResult],
    provider_hint: Option<&str>,
) -> Result<(), String> {
    let provider = build_rerank_provider_from_env(provider_hint)?;
    let docs = results
        .iter()
        .map(|r| r.entry.text.clone())
        .collect::<Vec<_>>();
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Cross-encoder runtime initialization failed: {e}"))?;
    let res = rt
        .block_on(async {
            provider
                .rerank(RerankRequest {
                    query: query.to_string(),
                    documents: docs,
                    top_n: Some(results.len()),
                })
                .await
        })
        .map_err(|e| {
            format!(
                "Cross-encoder request failed: {}",
                provider_error_en_rerank(&e)
            )
        })?;

    if res.items.is_empty() {
        return Err("Cross-encoder returned empty results.".to_string());
    }

    let max_local = results
        .iter()
        .map(|r| r.score)
        .fold(0.0_f32, f32::max)
        .max(1e-6);
    let mut cross_scores = vec![None::<f32>; results.len()];
    let mut min_s = f32::INFINITY;
    let mut max_s = f32::NEG_INFINITY;
    for item in res.items {
        if item.index >= cross_scores.len() {
            continue;
        }
        cross_scores[item.index] = Some(item.score);
        min_s = min_s.min(item.score);
        max_s = max_s.max(item.score);
    }

    for (i, row) in results.iter_mut().enumerate() {
        let local = row.score / max_local;
        let cross = if let Some(raw) = cross_scores[i] {
            if max_s > min_s {
                (raw - min_s) / (max_s - min_s)
            } else {
                0.5
            }
        } else {
            0.0
        };
        row.score = 0.4 * local + 0.6 * cross;
    }
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(())
}

fn semantic_rerank_with_embeddings(
    query: &str,
    results: &mut [RecallResult],
    provider_hint: Option<&str>,
) -> Result<(), String> {
    let provider = build_embedding_provider_from_env(provider_hint)?;
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Third-party vector service initialization failed: {e}"))?;

    let query_embedding = rt
        .block_on(async {
            provider
                .embed(EmbeddingRequest {
                    inputs: vec![query.to_string()],
                    task: Some(EmbeddingTask::Query),
                    dimensions: None,
                    normalized: Some(true),
                })
                .await
        })
        .map_err(|e| {
            format!(
                "Third-party vector request failed: {}",
                provider_error_en_embed(&e)
            )
        })?;

    let doc_inputs = results
        .iter()
        .map(|r| r.entry.text.clone())
        .collect::<Vec<_>>();
    let doc_embedding = rt
        .block_on(async {
            provider
                .embed(EmbeddingRequest {
                    inputs: doc_inputs,
                    task: Some(EmbeddingTask::Passage),
                    dimensions: None,
                    normalized: Some(true),
                })
                .await
        })
        .map_err(|e| {
            format!(
                "Third-party vector request failed: {}",
                provider_error_en_embed(&e)
            )
        })?;

    let q = query_embedding.vectors.first().ok_or_else(|| {
        "Third-party vector service returned an empty result. Check model name and account quota."
            .to_string()
    })?;
    if q.is_empty() {
        return Err(
            "Third-party vector dimension is empty. Check your model configuration.".to_string(),
        );
    }

    let max_local = results
        .iter()
        .map(|r| r.score)
        .fold(0.0_f32, f32::max)
        .max(1e-6);

    for (i, item) in results.iter_mut().enumerate() {
        let dv = doc_embedding.vectors.get(i).ok_or_else(|| {
            "Third-party vector service returned inconsistent item count. Please retry.".to_string()
        })?;
        let cos = cosine_similarity(q, dv)?;
        let local = item.score / max_local;
        item.score = 0.4 * local + 0.6 * ((cos + 1.0) / 2.0);
    }

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(())
}

fn build_rerank_provider_from_env(
    provider_hint: Option<&str>,
) -> Result<Arc<dyn prx_memory_rerank::RerankProvider>, String> {
    let provider = provider_hint
        .map(|s| s.to_lowercase())
        .or_else(|| {
            std::env::var("PRX_RERANK_PROVIDER")
                .ok()
                .map(|s| s.to_lowercase())
        })
        .unwrap_or_else(|| "jina".to_string());

    match provider.as_str() {
        "none" => Err("cross-encoder rerank disabled by configuration".to_string()),
        "jina" => {
            let api_key = std::env::var("PRX_RERANK_API_KEY")
                .or_else(|_| std::env::var("JINA_API_KEY"))
                .or_else(|_| std::env::var("PRX_EMBED_API_KEY"))
                .map_err(|_| {
                    "PRX_RERANK_API_KEY/JINA_API_KEY/PRX_EMBED_API_KEY not configured for cross-encoder rerank.".to_string()
                })?;
            let mut cfg = JinaRerankConfig::new(api_key);
            if let Ok(model) = std::env::var("PRX_RERANK_MODEL") {
                cfg.model = model;
            }
            if let Ok(endpoint) = std::env::var("PRX_RERANK_ENDPOINT") {
                cfg.endpoint = endpoint;
            }
            build_rerank_provider(RerankProviderConfig::Jina(cfg)).map_err(|e| {
                format!(
                    "Cross-encoder initialization failed: {}",
                    provider_error_en_rerank(&e)
                )
            })
        }
        "cohere" => {
            let api_key = std::env::var("PRX_RERANK_API_KEY")
                .or_else(|_| std::env::var("COHERE_API_KEY"))
                .map_err(|_| {
                    "PRX_RERANK_API_KEY/COHERE_API_KEY not configured for Cohere rerank."
                        .to_string()
                })?;
            let mut cfg = CohereRerankConfig::new(api_key);
            if let Ok(model) = std::env::var("PRX_RERANK_MODEL") {
                cfg.model = model;
            }
            if let Ok(endpoint) = std::env::var("PRX_RERANK_ENDPOINT") {
                cfg.endpoint = endpoint;
            }
            build_rerank_provider(RerankProviderConfig::Cohere(cfg)).map_err(|e| {
                format!(
                    "Cohere rerank initialization failed: {}",
                    provider_error_en_rerank(&e)
                )
            })
        }
        "pinecone" | "pinecone-compatible" => {
            let api_key = std::env::var("PRX_RERANK_API_KEY")
                .or_else(|_| std::env::var("PINECONE_API_KEY"))
                .map_err(|_| {
                    "PRX_RERANK_API_KEY/PINECONE_API_KEY not configured for Pinecone rerank."
                        .to_string()
                })?;
            let mut cfg = PineconeRerankConfig::new(api_key);
            if let Ok(model) = std::env::var("PRX_RERANK_MODEL") {
                cfg.model = model;
            }
            if let Ok(endpoint) = std::env::var("PRX_RERANK_ENDPOINT") {
                cfg.endpoint = endpoint;
            }
            if let Ok(version) = std::env::var("PRX_RERANK_API_VERSION") {
                cfg.api_version = Some(version);
            }
            build_rerank_provider(RerankProviderConfig::Pinecone(cfg)).map_err(|e| {
                format!(
                    "Pinecone rerank initialization failed: {}",
                    provider_error_en_rerank(&e)
                )
            })
        }
        _ => Err(
            "Unsupported rerank provider. Use jina, cohere, pinecone, pinecone-compatible, or none."
                .to_string(),
        ),
    }
}

fn build_embedding_provider_from_env(
    provider_hint: Option<&str>,
) -> Result<Arc<dyn prx_memory_embed::EmbeddingProvider>, String> {
    let provider = provider_hint
        .map(|s| s.to_lowercase())
        .or_else(|| {
            std::env::var("PRX_EMBED_PROVIDER")
                .ok()
                .map(|s| s.to_lowercase())
        })
        .unwrap_or_else(|| "openai-compatible".to_string());

    match provider.as_str() {
        "openai-compatible" => {
            let api_key = std::env::var("PRX_EMBED_API_KEY").map_err(|_| {
                "PRX_EMBED_API_KEY is not configured. Remote semantic recall is disabled."
                    .to_string()
            })?;
            let model = std::env::var("PRX_EMBED_MODEL")
                .unwrap_or_else(|_| "text-embedding-3-small".to_string());
            let mut cfg = OpenAiCompatibleConfig::new(api_key, model);
            if let Ok(base_url) = std::env::var("PRX_EMBED_BASE_URL") {
                cfg.base_url = base_url;
            }
            build_embedding_provider(EmbeddingProviderConfig::OpenAiCompatible(cfg)).map_err(|e| {
                format!(
                    "Third-party vector service initialization failed: {}",
                    provider_error_en_embed(&e)
                )
            })
        }
        "jina" => {
            let api_key = std::env::var("PRX_EMBED_API_KEY")
                .or_else(|_| std::env::var("JINA_API_KEY"))
                .map_err(|_| {
                    "PRX_EMBED_API_KEY or JINA_API_KEY is not configured. Jina recall is disabled."
                        .to_string()
                })?;
            let model = std::env::var("PRX_EMBED_MODEL")
                .unwrap_or_else(|_| "jina-embeddings-v5-text-small".to_string());
            let mut cfg = OpenAiCompatibleConfig::new(api_key, model);
            cfg.base_url = std::env::var("PRX_EMBED_BASE_URL")
                .unwrap_or_else(|_| "https://api.jina.ai".to_string());
            cfg.task_query = Some("retrieval.query".to_string());
            cfg.task_passage = Some("retrieval.passage".to_string());
            build_embedding_provider(EmbeddingProviderConfig::Jina(cfg)).map_err(|e| {
                format!(
                    "Jina vector service initialization failed: {}",
                    provider_error_en_embed(&e)
                )
            })
        }
        "gemini" => {
            let api_key = std::env::var("PRX_EMBED_API_KEY")
                .or_else(|_| std::env::var("GEMINI_API_KEY"))
                .map_err(|_| {
                    "PRX_EMBED_API_KEY or GEMINI_API_KEY is not configured. Gemini recall is disabled."
                        .to_string()
                })?;
            let model = std::env::var("PRX_EMBED_MODEL")
                .unwrap_or_else(|_| "gemini-embedding-001".to_string());
            let mut cfg = GeminiConfig::new(api_key, model);
            if let Ok(base_url) = std::env::var("PRX_EMBED_BASE_URL") {
                cfg.base_url = base_url;
            }
            build_embedding_provider(EmbeddingProviderConfig::Gemini(cfg)).map_err(|e| {
                format!(
                    "Gemini vector service initialization failed: {}",
                    provider_error_en_embed(&e)
                )
            })
        }
        _ => Err("Unsupported provider. Use openai-compatible, jina, or gemini.".to_string()),
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32, String> {
    if a.len() != b.len() {
        return Err(
            "Third-party vector dimensions do not match. Check that query/document models are aligned."
                .to_string(),
        );
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

fn provider_error_en_embed(err: &EmbeddingProviderError) -> String {
    match err {
        EmbeddingProviderError::Config(msg) => {
            format!("Configuration error: {}", sanitize_sensitive(msg))
        }
        EmbeddingProviderError::Http(msg) => {
            format!("Network error: {}", sanitize_sensitive(&msg.to_string()))
        }
        EmbeddingProviderError::Serde(msg) => format!(
            "Serialization error: {}",
            sanitize_sensitive(&msg.to_string())
        ),
        EmbeddingProviderError::InvalidResponse(msg) => {
            format!("Invalid provider response: {}", sanitize_sensitive(msg))
        }
        EmbeddingProviderError::Api { status, body } => {
            format!(
                "Provider API error (status {status}): {}",
                sanitize_sensitive(body)
            )
        }
    }
}

fn provider_error_en_rerank(err: &RerankProviderError) -> String {
    match err {
        RerankProviderError::Config(msg) => {
            format!("Configuration error: {}", sanitize_sensitive(msg))
        }
        RerankProviderError::Http(msg) => {
            format!("Network error: {}", sanitize_sensitive(&msg.to_string()))
        }
        RerankProviderError::Serde(msg) => format!(
            "Serialization error: {}",
            sanitize_sensitive(&msg.to_string())
        ),
        RerankProviderError::InvalidResponse(msg) => {
            format!("Invalid provider response: {}", sanitize_sensitive(msg))
        }
        RerankProviderError::Api { status, body } => {
            format!(
                "Provider API error (status {status}): {}",
                sanitize_sensitive(body)
            )
        }
    }
}

fn sanitize_sensitive(input: &str) -> String {
    let mut out = input.to_string();

    for key_name in ["PRX_EMBED_API_KEY", "GEMINI_API_KEY", "JINA_API_KEY"] {
        if let Ok(secret) = std::env::var(key_name) {
            if !secret.is_empty() {
                out = out.replace(&secret, "[REDACTED]");
            }
        }
    }

    out = redact_query_param(&out, "key=");
    out = redact_query_param(&out, "api_key=");
    out = redact_query_param(&out, "apikey=");
    out
}

static EMBED_RUNTIME: OnceLock<Mutex<EmbedRuntime>> = OnceLock::new();

fn embed_runtime() -> &'static Mutex<EmbedRuntime> {
    EMBED_RUNTIME.get_or_init(|| Mutex::new(EmbedRuntime::from_env()))
}

fn embed_runtime_stats() -> EmbedRuntimeStats {
    embed_runtime()
        .lock()
        .map(|v| v.stats.clone())
        .unwrap_or_default()
}

impl EmbedRuntime {
    fn from_env() -> Self {
        let capacity = std::env::var("PRX_EMBED_CACHE_CAPACITY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(1024)
            .max(1);
        let ttl_ms = std::env::var("PRX_EMBED_CACHE_TTL_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300_000)
            .max(1_000);
        let rps = std::env::var("PRX_EMBED_RATE_LIMIT_RPS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(20.0)
            .max(0.1);
        let now = now_ms();
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            capacity,
            ttl_ms,
            tokens: rps,
            max_tokens: rps,
            refill_per_sec: rps,
            last_refill_ms: now,
            stats: EmbedRuntimeStats::default(),
        }
    }

    fn refresh_tokens(&mut self, now: u64) {
        if now <= self.last_refill_ms {
            return;
        }
        let elapsed_ms = now - self.last_refill_ms;
        let refill = (elapsed_ms as f64 / 1000.0) * self.refill_per_sec;
        self.tokens = (self.tokens + refill).min(self.max_tokens);
        self.last_refill_ms = now;
    }

    fn acquire_rate_limit(&mut self, now: u64) -> u64 {
        self.refresh_tokens(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return 0;
        }
        let deficit = 1.0 - self.tokens;
        let wait_ms = ((deficit / self.refill_per_sec) * 1000.0).ceil().max(1.0) as u64;
        self.tokens = 0.0;
        self.last_refill_ms = now.saturating_add(wait_ms);
        self.stats.rate_wait_events = self.stats.rate_wait_events.saturating_add(1);
        self.stats.rate_wait_ms_total = self.stats.rate_wait_ms_total.saturating_add(wait_ms);
        wait_ms
    }

    fn cache_get(&mut self, key: &str, now: u64) -> Option<Vec<f32>> {
        if let Some(hit_value) = self.entries.get(key).and_then(|entry| {
            if entry.expire_at_ms >= now {
                Some(entry.value.clone())
            } else {
                None
            }
        }) {
            self.bump_lru(key);
            self.stats.cache_hits = self.stats.cache_hits.saturating_add(1);
            return Some(hit_value);
        }
        if self.entries.remove(key).is_some() {
            self.lru.retain(|k| k != key);
        }
        self.stats.cache_misses = self.stats.cache_misses.saturating_add(1);
        None
    }

    fn cache_put(&mut self, key: String, value: Vec<f32>, now: u64) {
        let expire_at_ms = now.saturating_add(self.ttl_ms);
        self.entries.insert(
            key.clone(),
            EmbedCacheEntry {
                value,
                expire_at_ms,
            },
        );
        self.bump_lru(&key);
        while self.entries.len() > self.capacity {
            if let Some(old) = self.lru.pop_front() {
                if self.entries.remove(&old).is_some() {
                    self.stats.cache_evictions = self.stats.cache_evictions.saturating_add(1);
                }
            } else {
                break;
            }
        }
    }

    fn bump_lru(&mut self, key: &str) {
        self.lru.retain(|k| k != key);
        self.lru.push_back(key.to_string());
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn redact_query_param(input: &str, marker: &str) -> String {
    let mut s = input.to_string();
    let mut start = 0usize;
    while let Some(pos) = s[start..].find(marker) {
        let abs = start + pos + marker.len();
        let tail = &s[abs..];
        let end_rel = tail
            .find(['&', ' ', '"', '\'', ')', '\n'])
            .unwrap_or(tail.len());
        s.replace_range(abs..abs + end_rel, "[REDACTED]");
        start = abs + "[REDACTED]".len();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_for_test(capacity: usize, ttl_ms: u64, rps: f64, now: u64) -> EmbedRuntime {
        EmbedRuntime {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            capacity,
            ttl_ms,
            tokens: rps,
            max_tokens: rps,
            refill_per_sec: rps,
            last_refill_ms: now,
            stats: EmbedRuntimeStats::default(),
        }
    }

    #[test]
    fn embed_cache_lru_and_ttl_work() {
        let mut rt = runtime_for_test(2, 10, 100.0, 0);
        rt.cache_put("a".to_string(), vec![1.0], 0);
        rt.cache_put("b".to_string(), vec![2.0], 0);
        assert_eq!(rt.cache_get("a", 1), Some(vec![1.0]));
        rt.cache_put("c".to_string(), vec![3.0], 2);
        assert_eq!(rt.stats.cache_evictions, 1);
        assert_eq!(rt.cache_get("b", 2), None);
        assert_eq!(rt.cache_get("a", 2), Some(vec![1.0]));
        assert_eq!(rt.cache_get("c", 2), Some(vec![3.0]));

        rt.cache_put("ttl".to_string(), vec![9.0], 0);
        assert_eq!(rt.cache_get("ttl", 20), None);
        assert!(rt.stats.cache_misses >= 2);
    }

    #[test]
    fn embed_rate_limiter_waits_when_tokens_exhausted() {
        let mut rt = runtime_for_test(8, 1000, 1.0, 1000);
        let wait0 = rt.acquire_rate_limit(1000);
        assert_eq!(wait0, 0);
        let wait1 = rt.acquire_rate_limit(1000);
        assert!(wait1 >= 900);
        let wait2 = rt.acquire_rate_limit(3000);
        assert_eq!(wait2, 0);
        assert!(rt.stats.rate_wait_events >= 1);
        assert!(rt.stats.rate_wait_ms_total >= wait1);
    }

    #[test]
    fn bounded_label_counter_applies_overflow_limit() {
        let mut counter = BoundedLabelCounter::new(2);
        counter.record("project:one");
        counter.record("project:two");
        counter.record("project:three");
        counter.record("project:three");
        assert_eq!(counter.counts.len(), 2);
        assert_eq!(counter.overflow, 2);
    }

    #[test]
    fn sanitize_label_value_limits_and_normalizes() {
        let out = sanitize_label_value("Project/ABC?x=1");
        assert_eq!(out, "project_abc_x_1");
        let long = sanitize_label_value(&"A".repeat(200));
        assert_eq!(long.len(), 64);
    }
}
