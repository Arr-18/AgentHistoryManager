use agent_history_manager::{api, db, ingest::Ingestor, parsers};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;
use tower::ServiceExt;
fn write(path: &std::path::Path, records: &[Value]) {
    std::fs::write(
        path,
        records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
}
fn codex(id: &str) -> Vec<Value> {
    vec![
        json!({"timestamp":"2026-10-01T00:00:00Z","type":"session_meta","payload":{"id":id,"cwd":"D:\\workspace","cli_version":"1","base_instructions":{"text":"中文规则"}}}),
        json!({"timestamp":"2026-10-01T00:01:00Z","type":"turn_context","payload":{"model":"test-model","cwd":"D:\\workspace"}}),
        json!({"timestamp":"2026-10-01T00:02:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello 中文"}]}}),
        json!({"timestamp":"2026-10-01T00:03:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"world"}]}}),
    ]
}
fn setup() -> (TempDir, Arc<Ingestor>) {
    let dir = TempDir::new().unwrap();
    let roots = [
        ("codex".into(), dir.path().join("codex")),
        ("claude_code".into(), dir.path().join("claude")),
    ];
    for (_, p) in &roots {
        std::fs::create_dir_all(p).unwrap();
    }
    let database = dir.path().join("history.sqlite");
    db::init(&database).unwrap();
    let s = Arc::new(Ingestor::new(database, roots));
    (dir, s)
}
#[test]
fn codex_raw_unknown_malformed_and_tokens() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("rollout-a.jsonl");
    let mut rows = codex("a");
    rows.extend([json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"summary"}],"content":[{"text":"detail"}]}}),json!({"type":"token_usage_record","payload":{"usage":{"input_tokens":"10","output_tokens":4,"reasoning_output_tokens":2,"cached_input_tokens":3}}}),json!({"type":"future","payload":{"secret":1}})]);
    write(&p, &rows);
    let mut data = std::fs::read_to_string(&p).unwrap();
    data.push_str("\n\n{broken\n[1]\n");
    std::fs::write(&p, &data).unwrap();
    let parsed = parsers::parse(&p, "codex").unwrap();
    assert_eq!(parsed.session["input_tokens"], 10);
    assert_eq!(parsed.session["preview"], "hello 中文");
    assert_eq!(parsed.messages[4]["text"], "summary\ndetail");
    assert_eq!(parsed.messages[6]["kind"], "unknown:future");
    assert_eq!(parsed.messages[7]["raw_json"], "{broken");
    assert_eq!(parsed.messages[8]["kind"], "malformed");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), data);
}
#[test]
fn codex_world_state_once_and_turn_override() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("rollout-a.jsonl");
    write(
        &p,
        &[
            json!({"type":"session_meta","payload":{"id":"a"}}),
            json!({"type":"world_state","payload":{"state":{"collaboration_mode":{"model":"first"},"environments":{"local":{"cwd":"D:\\first"}}}}}),
            json!({"type":"world_state","payload":{"state":{}}}),
            json!({"type":"turn_context","payload":{"model":"last","cwd":"D:\\last"}}),
        ],
    );
    let parsed = parsers::parse(&p, "codex").unwrap();
    assert_eq!(parsed.messages.len(), 3);
    assert_eq!(parsed.messages[2]["ordinal"], 3);
    assert_eq!(parsed.session["model"], "last");
    assert_eq!(parsed.session["cwd"], "D:\\last");
}
#[test]
fn claude_tree_branches_blank_lines_malformed() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("fallback.jsonl");
    let rows = [
        json!({"type":"user","uuid":"root","sessionId":"c","timestamp":"2026-10-01T00:00:00Z","message":{"content":"question"}}),
        json!({"type":"assistant","uuid":"branch","parentUuid":"root","message":{"content":[{"type":"text","text":"other branch"}]}}),
        json!({"type":"assistant","uuid":"chosen","parentUuid":"root","message":{"model":"claude","usage":{"input_tokens":9,"output_tokens":4,"cache_read_input_tokens":2},"content":[{"type":"thinking","thinking":"thinking"},{"type":"text","text":"reply"}]}}),
        json!({"type":"last-prompt","leafUuid":"chosen"}),
    ];
    std::fs::write(
        &p,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n\n")
            + "\n{broken\n",
    )
    .unwrap();
    let parsed = parsers::parse(&p, "claude_code").unwrap();
    assert_eq!(parsed.messages.len(), 5);
    assert_eq!(parsed.messages[1]["text"], "thinking\nreply");
    assert_eq!(parsed.messages[2]["text"], "other branch");
    assert_eq!(parsed.messages[4]["kind"], "malformed");
    assert_eq!(parsed.session["input_tokens"], 0);
    assert_eq!(parsed.session["output_tokens"], 4);
    assert_eq!(parsed.session["cached_input_tokens"], 2);
}
#[test]
fn claude_cycle_and_missing_id() {
    let d = TempDir::new().unwrap();
    let p = d.path().join("fallback.jsonl");
    write(
        &p,
        &[
            json!({"type":"user","uuid":"a","parentUuid":"b","message":{"content":"a"}}),
            json!({"type":"assistant","uuid":"b","parentUuid":"a","message":{"content":[{"type":"tool_use","name":"shell"}]}}),
        ],
    );
    let parsed = parsers::parse(&p, "claude_code").unwrap();
    assert_eq!(parsed.session["id"], "fallback");
    assert_eq!(parsed.messages[0]["text"], "a");
    assert_eq!(parsed.messages[1]["kind"], "tool_use");
}
#[test]
fn incremental_replacement_late_files_and_retry() {
    let (d, s) = setup();
    let p = d.path().join("codex/rollout-one.jsonl");
    write(&p, &codex("a"));
    assert_eq!(s.run(false).unwrap()["codex"]["added"], 1);
    assert_eq!(s.run(false).unwrap()["codex"]["files"], 0);
    let mut rows = codex("a");
    rows.pop();
    write(&p, &rows);
    assert_eq!(s.run(false).unwrap()["codex"]["updated"], 1);
    let c = db::open(&s.db).unwrap();
    assert_eq!(
        db::query(&c, "SELECT COUNT(*) AS n FROM messages", &[]).unwrap()[0]["n"],
        3
    );
    let late = d.path().join("codex/rollout-late.jsonl");
    write(&late, &codex("late"));
    let f = std::fs::File::options().write(true).open(&late).unwrap();
    f.set_times(
        std::fs::FileTimes::new()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
    )
    .unwrap();
    assert_eq!(s.run(false).unwrap()["codex"]["added"], 1);
    let bad = d.path().join("codex/rollout-bad.jsonl");
    std::fs::write(&bad, "{broken").unwrap();
    assert_eq!(
        s.run(false).unwrap()["codex"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.run(false).unwrap()["codex"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    write(&bad, &codex("fixed"));
    assert_eq!(s.run(false).unwrap()["codex"]["added"], 1);
    assert_eq!(s.run(true).unwrap()["codex"]["updated"], 3);
    assert!(
        db::query(&c, "PRAGMA foreign_key_check", &[])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn missing_agents_and_saved_paths() {
    let (d, s) = setup();
    let missing = d.path().join("not-installed");
    s.set_path("codex", &missing).unwrap();
    assert_eq!(s.path_status().unwrap()[0]["status"], "missing");
    assert_eq!(s.run(false).unwrap()["codex"]["files"], 0);
    assert!(
        s.set_path("codex", std::path::Path::new("relative"))
            .is_err()
    );
    let s2 = Ingestor::new(
        s.db.clone(),
        [
            ("codex".into(), PathBuf::from("unused")),
            ("claude_code".into(), PathBuf::from("unused")),
        ],
    );
    s2.load_saved_paths().unwrap();
    if std::env::var_os("CODEX_HOME").is_none()
        && std::env::var_os("AHM_CODEX_SESSIONS_DIR").is_none()
    {
        assert_eq!(
            s2.path_status().unwrap()[0]["path"],
            json!(missing.to_string_lossy())
        );
    }
}
async fn request(app: axum::Router, uri: &str) -> (StatusCode, Value) {
    let r = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = to_bytes(r.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn api_contract_filters_events_and_validation() {
    let (d, s) = setup();
    write(&d.path().join("codex/rollout-a.jsonl"), &codex("a"));
    s.run(false).unwrap();
    let app = api::router(s, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"));
    let (status, v) = request(app.clone(), "/api/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["total_sessions"], 1);
    assert_eq!(v["total_messages"], 4);
    let (_, v) = request(
        app.clone(),
        "/api/sessions?search=hello&limit=1&from=2026-10-01&to=2026-10-01",
    )
    .await;
    assert_eq!(v["total"], 1);
    assert_eq!(v["items"][0]["id"], "a");
    let (_, v) = request(app.clone(), "/api/sessions?from=2026-10-02").await;
    assert_eq!(v["total"], 0);
    let (_, v) = request(app.clone(), "/api/sessions/codex/a/messages").await;
    assert_eq!(v["count"], 2);
    assert!(v["items"][0]["content"].is_object());
    let (_, v) = request(
        app.clone(),
        "/api/sessions/codex/a/messages?include_events=true",
    )
    .await;
    assert_eq!(v["count"], 4);
    let (_, v) = request(app.clone(), "/api/sessions/codex/a").await;
    assert_eq!(v["extra"]["base_instructions_chars"], 4);
    assert!(v.get("extra_json").is_none());
    assert_eq!(
        request(app.clone(), "/api/sessions/codex/missing").await.0,
        StatusCode::NOT_FOUND
    );
    for url in [
        "/api/sessions?limit=0",
        "/api/sessions?offset=-1",
        "/api/sessions?from=bad",
        "/api/ingest/runs?limit=201",
    ] {
        assert_eq!(
            request(app.clone(), url).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
}
#[tokio::test]
async fn history_path_api_and_static_ui() {
    let (d, s) = setup();
    let app = api::router(s, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"));
    let (_, v) = request(app.clone(), "/api/history-paths").await;
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[0]["status"], "available");
    let path = d.path().join("missing");
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/history-paths")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    json!({"source":"claude_code","path":path}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value =
        serde_json::from_slice(&to_bytes(r.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(v[1]["status"], "missing");
    for url in [
        "/",
        "/static/app.js",
        "/static/filter.js",
        "/static/styles.css",
    ] {
        let r = app
            .clone()
            .oneshot(Request::builder().uri(url).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }
}
#[test]
fn old_sqlite_schema_remains_compatible() {
    let d = TempDir::new().unwrap();
    let path = d.path().join("old.sqlite");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(include_str!("../src/schema.sql")).unwrap();
        c.execute_batch("INSERT INTO sources(name) VALUES ('codex'); INSERT INTO sessions(source,id,raw_path,file_mtime,ingested_at,preview) VALUES ('codex','old','old.jsonl',1,'2026-10-01','keep me');").unwrap();
    }
    db::init(&path).unwrap();
    let c = db::open(&path).unwrap();
    assert_eq!(
        db::query(&c, "SELECT preview FROM sessions WHERE id='old'", &[]).unwrap()[0]["preview"],
        "keep me"
    );
    assert_eq!(
        db::query(&c, "SELECT COUNT(*) AS n FROM sources", &[]).unwrap()[0]["n"],
        2
    );
    assert!(
        db::query(&c, "PRAGMA foreign_key_check", &[])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn manual_and_background_scans_are_serialized() {
    let (d, s) = setup();
    write(&d.path().join("codex/rollout-a.jsonl"), &codex("a"));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let s = s.clone();
            std::thread::spawn(move || s.run(false).unwrap())
        })
        .collect();
    let added: u64 = handles
        .into_iter()
        .map(|h| h.join().unwrap()["codex"]["added"].as_u64().unwrap())
        .sum();
    assert_eq!(added, 1);
    let c = db::open(&s.db).unwrap();
    assert_eq!(
        db::query(&c, "SELECT COUNT(*) AS n FROM messages", &[]).unwrap()[0]["n"],
        4
    );
}
#[test]
fn user_directory_and_environment_overrides() {
    let d = TempDir::new().unwrap();
    let user = d.path().join("dynamic-user");
    let default_codex = user.join(".codex/sessions");
    let default_claude = user.join(".claude/projects");
    std::fs::create_dir_all(&default_codex).unwrap();
    std::fs::create_dir_all(&default_claude).unwrap();
    write(&default_codex.join("rollout-a.jsonl"), &codex("a"));
    let custom_codex = d.path().join("custom-codex/sessions");
    let direct_codex = d.path().join("direct-sessions");
    let custom_claude = d.path().join("custom-claude/projects");
    std::fs::create_dir_all(&custom_codex).unwrap();
    std::fs::create_dir_all(&direct_codex).unwrap();
    std::fs::create_dir_all(&custom_claude).unwrap();
    write(&custom_codex.join("rollout-b.jsonl"), &codex("b"));
    write(&direct_codex.join("rollout-c.jsonl"), &codex("c"));
    for (i, overrides, expected) in [
        (0, vec![], "a"),
        (
            1,
            vec![
                ("CODEX_HOME", d.path().join("custom-codex")),
                ("CLAUDE_HOME", d.path().join("custom-claude")),
            ],
            "b",
        ),
        (
            2,
            vec![
                ("CODEX_HOME", d.path().join("custom-codex")),
                ("AHM_CODEX_SESSIONS_DIR", direct_codex.clone()),
                ("AHM_CLAUDE_PROJECTS_DIR", custom_claude.clone()),
            ],
            "c",
        ),
    ] {
        let db_path = d.path().join(format!("env-{i}.sqlite"));
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_agent-history-manager"));
        command
            .args(["--sync-once", "--db-path"])
            .arg(&db_path)
            .env("USERPROFILE", &user);
        for key in [
            "CODEX_HOME",
            "CLAUDE_HOME",
            "AHM_CODEX_SESSIONS_DIR",
            "AHM_CLAUDE_PROJECTS_DIR",
            "AHM_DB_PATH",
        ] {
            command.env_remove(key);
        }
        for (key, value) in overrides {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let c = db::open(&db_path).unwrap();
        assert_eq!(
            db::query(&c, "SELECT id FROM sessions WHERE source='codex'", &[]).unwrap()[0]["id"],
            expected
        );
    }
}
