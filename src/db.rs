use anyhow::Result;
use rusqlite::{
    Connection, params_from_iter,
    types::{Value as SqlValue, ValueRef},
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(5))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
    Ok(c)
}
pub fn init(path: &Path) -> Result<()> {
    let c = open(path)?;
    c.execute_batch(include_str!("schema.sql"))?;
    c.execute_batch("INSERT OR IGNORE INTO sources(name) VALUES ('codex'),('claude_code'); CREATE TABLE IF NOT EXISTS source_config (source TEXT PRIMARY KEY, history_path TEXT NOT NULL); CREATE TABLE IF NOT EXISTS file_state (source TEXT NOT NULL, raw_path TEXT NOT NULL, file_mtime REAL NOT NULL, file_size INTEGER NOT NULL, PRIMARY KEY(source,raw_path));")?;
    Ok(())
}
pub fn sql_value(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Number(n) => n
            .as_i64()
            .map(SqlValue::Integer)
            .unwrap_or_else(|| SqlValue::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => SqlValue::Text(s.clone()),
        _ => SqlValue::Text(v.to_string()),
    }
}
pub fn query(c: &Connection, sql: &str, args: &[Value]) -> Result<Vec<Value>> {
    let mut stmt = c.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let args: Vec<_> = args.iter().map(sql_value).collect();
    let rows = stmt.query_map(params_from_iter(args), |r| {
        let mut obj = serde_json::Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match r.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => json!(n),
                ValueRef::Real(n) => json!(n),
                ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                ValueRef::Blob(b) => json!(b),
            };
            obj.insert(name.clone(), v);
        }
        Ok(Value::Object(obj))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
pub fn store(
    c: &mut Connection,
    p: &crate::parsers::Parsed,
    path: &Path,
    mtime: f64,
    size: u64,
) -> Result<bool> {
    let size = i64::try_from(size)?;
    let tx = c.transaction()?;
    let mut s = p.session.clone();
    s["raw_path"] = json!(path.to_string_lossy());
    s["file_mtime"] = json!(mtime);
    s["extra_json"] = if s["extra"].as_object().is_some_and(|e| e.is_empty()) {
        Value::Null
    } else {
        json!(s["extra"].to_string())
    };
    s["ingested_at"] = json!(now());
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE source=?1 AND id=?2)",
        rusqlite::params![s["source"].as_str(), s["id"].as_str()],
        |r| r.get(0),
    )?;
    let cols = [
        "source",
        "id",
        "started_at",
        "ended_at",
        "cwd",
        "model",
        "model_provider",
        "cli_version",
        "runtime_source",
        "originator",
        "git_branch",
        "entrypoint",
        "user_type",
        "raw_path",
        "file_mtime",
        "message_count",
        "user_message_count",
        "assistant_message_count",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cached_input_tokens",
        "preview",
        "extra_json",
        "ingested_at",
    ];
    let updates = cols
        .iter()
        .skip(2)
        .map(|k| {
            if *k == "preview" {
                "preview=COALESCE(excluded.preview,sessions.preview)".into()
            } else {
                format!("{k}=excluded.{k}")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "INSERT INTO sessions ({}) VALUES ({}) ON CONFLICT(source,id) DO UPDATE SET {}",
        cols.join(","),
        vec!["?"; cols.len()].join(","),
        updates
    );
    tx.execute(
        &sql,
        params_from_iter(cols.iter().map(|k| sql_value(&s[*k]))),
    )?;
    tx.execute(
        "DELETE FROM messages WHERE source=?1 AND session_id=?2",
        rusqlite::params![s["source"].as_str(), s["id"].as_str()],
    )?;
    let mcols = [
        "session_id",
        "source",
        "ordinal",
        "timestamp",
        "role",
        "kind",
        "text",
        "content_json",
        "raw_json",
        "model",
        "stop_reason",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cached_input_tokens",
    ];
    {
        let sql = format!(
            "INSERT INTO messages ({}) VALUES ({})",
            mcols.join(","),
            vec!["?"; mcols.len()].join(",")
        );
        let mut stmt = tx.prepare(&sql)?;
        for m in &p.messages {
            let mut row = m.clone();
            row["source"] = s["source"].clone();
            row["session_id"] = s["id"].clone();
            row["content_json"] = json!(m["content"].to_string());
            stmt.execute(params_from_iter(mcols.iter().map(|k| sql_value(&row[*k]))))?;
        }
    }
    tx.execute("INSERT INTO file_state(source,raw_path,file_mtime,file_size) VALUES (?1,?2,?3,?4) ON CONFLICT(source,raw_path) DO UPDATE SET file_mtime=excluded.file_mtime,file_size=excluded.file_size",rusqlite::params![s["source"].as_str(),path.to_string_lossy(),mtime,size])?;
    tx.execute("INSERT INTO ingest_runs(source,started_at,finished_at,sessions_added,sessions_updated,messages_added) VALUES (?1,?2,?2,?3,?4,?5)",rusqlite::params![s["source"].as_str(),now(),i64::from(!exists),i64::from(exists),p.messages.len() as i64])?;
    tx.commit()?;
    Ok(!exists)
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}
