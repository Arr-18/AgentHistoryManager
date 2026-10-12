use crate::{db, ingest::Ingestor};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
pub type AppState = Arc<Ingestor>;
pub struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({"detail":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        eprintln!("API error: {e:#}");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error".into(),
        )
    }
}
type ApiResult = Result<Json<Value>, ApiError>;
async fn blocking<F>(f: F) -> ApiResult
where
    F: FnOnce() -> Result<Value, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
}
pub fn router(state: AppState, web: std::path::PathBuf) -> Router {
    Router::new()
        .route("/healthz", get(|| async { Json(json!({"ok":true})) }))
        .route(
            "/api/history-paths",
            get(history_paths).post(set_history_path),
        )
        .route("/api/stats", get(stats))
        .route("/api/sources", get(sources))
        .route("/api/sessions", get(sessions))
        .route("/api/sessions/{source}/{sid}", get(detail))
        .route("/api/sessions/{source}/{sid}/messages", get(messages))
        .route("/api/ingest/full", post(full))
        .route("/api/ingest/sync", post(sync))
        .route("/api/ingest/runs", get(runs))
        .nest_service("/static", tower_http::services::ServeDir::new(&web))
        .route_service(
            "/",
            tower_http::services::ServeFile::new(web.join("index.html")),
        )
        .with_state(state)
}
const SOURCES: &str = "SELECT s.name, COUNT(se.id) AS session_count, COALESCE(SUM(se.message_count),0) AS message_count FROM sources s LEFT JOIN sessions se ON se.source=s.name GROUP BY s.name ORDER BY s.name";
async fn sources(State(s): State<AppState>) -> ApiResult {
    blocking(move || {
        let c = db::open(&s.db)?;
        Ok(json!(db::query(&c, SOURCES, &[])?))
    })
    .await
}
async fn stats(State(s): State<AppState>) -> ApiResult {
    blocking(move||{let c=db::open(&s.db)?;let totals=db::query(&c,"SELECT COUNT(*) AS s,COALESCE(SUM(message_count),0) AS m FROM sessions",&[])?;let last=db::query(&c,"SELECT finished_at FROM ingest_runs WHERE finished_at IS NOT NULL ORDER BY id DESC LIMIT 1",&[])?;Ok(json!({"sources":db::query(&c,SOURCES,&[])?,"total_sessions":totals[0]["s"],"total_messages":totals[0]["m"],"last_sync_at":last.first().map(|r|r["finished_at"].clone())}))}).await
}
#[derive(Deserialize, Default)]
struct SessionQuery {
    source: Option<String>,
    search: Option<String>,
    cwd: Option<String>,
    #[serde(rename = "from")]
    from: Option<String>,
    to: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}
fn invalid(msg: &str) -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, msg.into())
}
async fn sessions(State(s): State<AppState>, Query(q): Query<SessionQuery>) -> ApiResult {
    blocking(move||{
    let offset=q.offset.unwrap_or(0);let limit=q.limit.unwrap_or(50);if offset<0 || !(1..=500).contains(&limit){return Err(invalid("offset must be >=0; limit must be between 1 and 500"));}
    let mut clauses=Vec::new();let mut args=Vec::new();
    if let Some(v)=q.source.filter(|v|!v.is_empty()){clauses.push("s.source=?");args.push(json!(v));}
    if let Some(v)=q.cwd.filter(|v|!v.is_empty()){clauses.push("s.cwd LIKE ?");args.push(json!(format!("%{v}%")));}
    if let Some(v)=q.search.filter(|v|!v.is_empty()){clauses.push("(s.preview LIKE ? OR s.id IN (SELECT session_id FROM messages WHERE text LIKE ? AND source=s.source))");args.extend([json!(format!("%{v}%")),json!(format!("%{v}%"))]);}
    for (v,clause) in [(q.from,"substr(s.started_at,1,10)>=?"),(q.to,"substr(s.started_at,1,10)<=?")]{if let Some(v)=v.filter(|v|!v.is_empty()){chrono::NaiveDate::parse_from_str(&v,"%Y-%m-%d").map_err(|_|invalid("date must use YYYY-MM-DD"))?;clauses.push(clause);args.push(json!(v));}}
    let where_sql=if clauses.is_empty(){String::new()}else{format!("WHERE {}",clauses.join(" AND "))};let c=db::open(&s.db)?;let total=db::query(&c,&format!("SELECT COUNT(*) AS c FROM sessions s {where_sql}"),&args)?[0]["c"].clone();args.extend([json!(limit),json!(offset)]);
    let items=db::query(&c,&format!("SELECT s.source,s.id,s.started_at,s.ended_at,s.cwd,s.model,s.cli_version,s.message_count,s.user_message_count,s.assistant_message_count,s.input_tokens,s.output_tokens,s.reasoning_tokens,s.preview FROM sessions s {where_sql} ORDER BY COALESCE(s.started_at,'') DESC,s.source,s.id LIMIT ? OFFSET ?"),&args)?;
    Ok(json!({"total":total,"offset":offset,"limit":limit,"items":items}))
}).await
}
async fn detail(
    State(s): State<AppState>,
    Path((source, sid)): Path<(String, String)>,
) -> ApiResult {
    blocking(move || {
        let c = db::open(&s.db)?;
        let mut rows = db::query(
            &c,
            "SELECT * FROM sessions WHERE source=? AND id=?",
            &[json!(source), json!(sid)],
        )?;
        let Some(mut row) = rows.pop() else {
            return Err(ApiError(StatusCode::NOT_FOUND, "Session not found".into()));
        };
        if let Some(raw) = row["extra_json"].as_str() {
            row["extra"] = serde_json::from_str(raw).unwrap_or(Value::Null);
        }
        row.as_object_mut().unwrap().remove("extra_json");
        Ok(row)
    })
    .await
}
#[derive(Deserialize, Default)]
struct MessageQuery {
    #[serde(default)]
    include_events: bool,
}
async fn messages(
    State(s): State<AppState>,
    Path((source, sid)): Path<(String, String)>,
    Query(q): Query<MessageQuery>,
) -> ApiResult {
    blocking(move||{let c=db::open(&s.db)?;let mut items=db::query(&c,"SELECT ordinal,timestamp,role,kind,text,content_json,model,stop_reason,input_tokens,output_tokens,reasoning_tokens FROM messages WHERE session_id=? AND source=? ORDER BY ordinal",&[json!(sid),json!(source)])?;items.retain(|r|q.include_events || matches!(r["kind"].as_str(),Some("message"|"reasoning"|"function_call"|"function_call_output"|"tool_use"|"attachment")));for m in &mut items{let raw=m.as_object_mut().unwrap().remove("content_json").unwrap();m["content"]=raw.as_str().and_then(|v|serde_json::from_str(v).ok()).unwrap_or(json!({"_raw":raw}));}Ok(json!({"count":items.len(),"items":items}))}).await
}
async fn full(State(s): State<AppState>) -> ApiResult {
    blocking(move || {
        let summary = s.run(true)?;
        let c = db::open(&s.db)?;
        let last = db::query(
            &c,
            "SELECT finished_at FROM ingest_runs ORDER BY id DESC LIMIT 1",
            &[],
        )?;
        Ok(json!({"summary":summary,"finished_at":last.first().map(|r|r["finished_at"].clone())}))
    })
    .await
}
async fn sync(State(s): State<AppState>) -> ApiResult {
    blocking(move || Ok(json!({"summary":s.run(false)?}))).await
}
#[derive(Deserialize)]
struct RunQuery {
    limit: Option<i64>,
}
async fn runs(State(s): State<AppState>, Query(q): Query<RunQuery>) -> ApiResult {
    blocking(move||{let limit=q.limit.unwrap_or(20);if !(1..=200).contains(&limit){return Err(invalid("limit must be between 1 and 200"));}let c=db::open(&s.db)?;Ok(json!(db::query(&c,"SELECT id,source,started_at,finished_at,sessions_added,sessions_updated,messages_added,messages_updated,error FROM ingest_runs ORDER BY id DESC LIMIT ?",&[json!(limit)])?))}).await
}

async fn history_paths(State(s): State<AppState>) -> ApiResult {
    blocking(move || Ok(s.path_status()?)).await
}
#[derive(Deserialize)]
struct PathUpdate {
    source: String,
    path: String,
}
async fn set_history_path(State(s): State<AppState>, Json(q): Json<PathUpdate>) -> ApiResult {
    blocking(move || {
        if !matches!(q.source.as_str(), "codex" | "claude_code") || q.path.trim().is_empty() {
            return Err(invalid(
                "source must be codex or claude_code; path is required",
            ));
        }
        let path = std::path::PathBuf::from(q.path.trim());
        if !path.is_absolute() {
            return Err(invalid("请输入历史目录的绝对路径"));
        }
        s.set_path(&q.source, &path)?;
        Ok(s.path_status()?)
    })
    .await
}
