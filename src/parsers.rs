use anyhow::Result;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
#[derive(Debug)]
pub struct Parsed {
    pub session: Value,
    pub messages: Vec<Value>,
}
pub fn int(v: &Value) -> Value {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .or_else(|| v.as_f64().map(|n| n as i64))
        .map(Value::from)
        .unwrap_or(Value::Null)
}
fn text(v: &Value, claude: bool) -> String {
    if let Some(s) = v.as_str() {
        return s.into();
    }
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| match b["type"].as_str().unwrap_or("") {
            "text" if claude => b["text"].as_str(),
            "thinking" if claude => b["thinking"].as_str(),
            "input_text" | "output_text" if !claude => b["text"].as_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn pick(v: &Value, keys: &[&str]) -> Value {
    keys.iter()
        .map(|k| (k.to_string(), v[*k].clone()))
        .collect()
}
fn set(s: &mut Value, k: &str, v: &Value) {
    if v.is_string() {
        s[k] = v.clone();
    }
}
fn build(o: &Value, raw: &str, ordinal: usize, source: &str) -> Value {
    let mut m = json!({"ordinal":ordinal,"timestamp":o["timestamp"],"role":"event","kind":"unknown","text":"","content":o,"raw_json":raw,"model":null,"stop_reason":null,"input_tokens":null,"output_tokens":null,"reasoning_tokens":null,"cached_input_tokens":null});
    if !o.is_object() {
        m["kind"] = json!("malformed");
        m["content"] = json!({"raw":raw.chars().take(500).collect::<String>()});
        return m;
    }
    let ty = o["type"].as_str().unwrap_or("");
    m["kind"] = json!(if ty.is_empty() {
        "unknown".into()
    } else {
        format!("unknown:{ty}")
    });
    if source == "codex" {
        let p = &o["payload"];
        match ty {
            "session_meta" => {
                m["role"] = json!("system");
                m["kind"] = json!(ty);
                m["content"] = p.clone();
                if let Some(c) = m["content"].as_object_mut() {
                    c.remove("base_instructions");
                }
            }
            "world_state" => {
                let w = &p["state"];
                let e = &w["environments"];
                let l = &e["local"];
                m["role"] = json!("system");
                m["kind"] = json!(ty);
                m["content"] = json!({"collaboration_mode":w["collaboration_mode"],"cwd":l["cwd"],"shell":l["shell"],"current_date":e["current_date"],"timezone":e["timezone"],"model":w["collaboration_mode"]["model"]});
            }
            "turn_context" => {
                m["role"] = json!("system");
                m["kind"] = json!(ty);
                m["content"] = pick(
                    p,
                    &[
                        "model",
                        "effort",
                        "approval_policy",
                        "sandbox_policy",
                        "collaboration_mode",
                        "cwd",
                        "workspace_roots",
                    ],
                );
            }
            "token_usage_record" => {
                let u = &p["usage"];
                m["role"] = json!("system");
                m["kind"] = json!("token_usage");
                m["content"] = json!({"turn_id":p["turn_id"],"response_id":p["response_id"],"total_tokens":int(&u["total_tokens"])});
                for (dst, src) in [
                    ("input_tokens", "input_tokens"),
                    ("output_tokens", "output_tokens"),
                    ("reasoning_tokens", "reasoning_output_tokens"),
                    ("cached_input_tokens", "cached_input_tokens"),
                ] {
                    let n = json!(int(&u[src]).as_i64().unwrap_or(0));
                    m[dst] = n.clone();
                    m["content"][src] = n;
                }
            }
            "event_msg" => {
                m["kind"] = json!(
                    p["type"]
                        .as_str()
                        .map(|s| format!("event:{s}"))
                        .unwrap_or("event".into())
                );
                m["content"] = p.clone();
                if !m["content"].is_object() {
                    m["content"] = json!({});
                }
                m["content"].as_object_mut().unwrap().remove("type");
                m["content"]["subtype"] = p["type"].clone();
            }
            "response_item" => match p["type"].as_str().unwrap_or("") {
                "message" => {
                    m["role"] = p["role"].as_str().map(Value::from).unwrap_or(json!("user"));
                    m["kind"] = json!("message");
                    m["text"] = json!(text(&p["content"], false));
                    m["content"] = json!({"role":m["role"],"content":p["content"]});
                }
                "reasoning" => {
                    m["role"] = json!("assistant");
                    m["kind"] = json!("reasoning");
                    let mut parts = Vec::new();
                    if let Some(s) = p["summary"].as_str() {
                        parts.push(s);
                    }
                    for key in ["summary", "content"] {
                        for b in p[key].as_array().into_iter().flatten() {
                            if let Some(t) = b["text"].as_str()
                                && !t.is_empty()
                            {
                                parts.push(t);
                            }
                        }
                    }
                    m["text"] = json!(parts.join("\n"));
                    m["content"] = pick(p, &["summary"]);
                }
                "function_call" => {
                    m["role"] = json!("assistant");
                    m["kind"] = json!("function_call");
                    let a = &p["arguments"];
                    let args = a
                        .as_str()
                        .map(|s| {
                            serde_json::from_str::<Value>(s)
                                .map(|v| v.to_string())
                                .unwrap_or(s.into())
                        })
                        .unwrap_or_else(|| {
                            if a.is_null() {
                                String::new()
                            } else {
                                a.to_string()
                            }
                        });
                    m["text"] = json!(
                        format!("[tool:{}] {}", p["name"].as_str().unwrap_or("tool"), args).trim()
                    );
                    m["content"] = pick(p, &["name", "arguments", "call_id"]);
                }
                "function_call_output" => {
                    m["role"] = json!("tool");
                    m["kind"] = json!("function_call_output");
                    m["text"] =
                        json!(p["output"].as_str().map(str::to_owned).unwrap_or_else(|| {
                            if p["output"].is_null() {
                                String::new()
                            } else {
                                p["output"].to_string()
                            }
                        }));
                    m["content"] = pick(p, &["call_id", "output"]);
                }
                sub => {
                    m["kind"] = json!(if sub.is_empty() {
                        "response_item".into()
                    } else {
                        format!("response_item:{sub}")
                    });
                    m["content"] = p.clone();
                }
            },
            _ => {}
        }
    } else {
        let p = &o["message"];
        match ty {
            "user" | "assistant" => {
                let c = &p["content"];
                m["role"] = json!(ty);
                m["kind"] = json!(if ty == "assistant"
                    && !c
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|b| matches!(b["type"].as_str(), Some("text" | "thinking")))
                {
                    "tool_use"
                } else {
                    "message"
                });
                m["text"] = json!(text(c, true));
                m["content"] = json!({"role":ty,"content":c});
                if ty == "assistant" {
                    m["model"] = p["model"].clone();
                    m["stop_reason"] = p["stop_reason"].clone();
                    m["content"]["model"] = p["model"].clone();
                    m["content"]["stop_reason"] = p["stop_reason"].clone();
                    m["content"]["usage"] = p["usage"].clone();
                    for (dst, src) in [
                        ("input_tokens", "input_tokens"),
                        ("output_tokens", "output_tokens"),
                        ("cached_input_tokens", "cache_read_input_tokens"),
                    ] {
                        m[dst] = int(&p["usage"][src]);
                    }
                }
            }
            "attachment" => {
                m["role"] = json!("user");
                m["kind"] = json!(ty);
                m["text"] = o["attachment"]["file_name"]
                    .as_str()
                    .or(o["attachment"]["file_path"].as_str())
                    .map(Value::from)
                    .unwrap_or(json!(""));
                m["content"] = pick(o, &["attachment"]);
            }
            "system" => {
                m["role"] = json!("system");
                m["kind"] = json!(
                    o["subtype"]
                        .as_str()
                        .map(|s| format!("system:{s}"))
                        .unwrap_or("system".into())
                );
                m["text"] = o["content"]
                    .as_str()
                    .or(o["message"].as_str())
                    .map(Value::from)
                    .unwrap_or(json!(""));
                m["content"] = pick(o, &["subtype", "durationMs", "messageCount", "isMeta"]);
            }
            "ai-title" | "permission-mode" | "last-prompt" | "file-history-snapshot" => {
                m["role"] = json!("system");
                let (kind, keys): (&str, &[&str]) = match ty {
                    "ai-title" => ("ai_title", &["aiTitle"]),
                    "permission-mode" => ("permission_mode", &["permissionMode"]),
                    "last-prompt" => ("last_prompt", &["lastPrompt", "leafUuid"]),
                    _ => ("file_history_snapshot", &["messageId", "isSnapshotUpdate"]),
                };
                m["kind"] = json!(kind);
                m["content"] = pick(o, keys);
                if ty == "file-history-snapshot" {
                    m["content"]["snapshotKeys"] = json!(
                        o["snapshot"]
                            .as_object()
                            .map(|v| v.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default()
                    );
                } else if ty != "last-prompt" {
                    m["text"] = o[keys[0]].as_str().map(Value::from).unwrap_or(json!(""));
                }
            }
            _ => {}
        }
    }
    m
}
pub fn parse(path: &Path, source: &str) -> Result<Parsed> {
    let data = std::fs::read(path)?;
    let records: Vec<(String, Value)> = String::from_utf8_lossy(&data)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            (
                l.to_owned(),
                serde_json::from_str::<Value>(l)
                    .ok()
                    .filter(Value::is_object)
                    .unwrap_or(Value::Null),
            )
        })
        .collect();
    let mut s = json!({"source":source,"id":null,"started_at":null,"ended_at":null,"cwd":null,"model":null,"model_provider":null,"cli_version":null,"runtime_source":null,"originator":null,"git_branch":null,"entrypoint":null,"user_type":null,"extra":{}});
    let mut roots = Vec::<Value>::new();
    let mut by = HashMap::new();
    let mut ids = Vec::new();
    let mut leaf = None;
    for (i, (_, o)) in records.iter().enumerate() {
        set(&mut s, "ended_at", &o["timestamp"]);
        if source == "codex" {
            let p = &o["payload"];
            match o["type"].as_str().unwrap_or("") {
                "session_meta" => {
                    s["id"] = p["session_id"]
                        .as_str()
                        .or(p["id"].as_str())
                        .map(Value::from)
                        .unwrap_or(Value::Null);
                    s["started_at"] = p["timestamp"]
                        .as_str()
                        .or(o["timestamp"].as_str())
                        .map(Value::from)
                        .unwrap_or(Value::Null);
                    for k in ["cwd", "cli_version", "originator", "model_provider"] {
                        set(&mut s, k, &p[k]);
                    }
                    set(&mut s, "runtime_source", &p["source"]);
                    for k in [
                        "model_provider",
                        "originator",
                        "thread_source",
                        "history_mode",
                    ] {
                        if !p[k].is_null() {
                            s["extra"][k] = p[k].clone();
                        }
                    }
                    if let Some(a) = p["runtime_workspace_roots"].as_array() {
                        roots = a.iter().filter(|v| v.is_string()).cloned().collect();
                    }
                    if let Some(b) = p["base_instructions"]
                        .as_str()
                        .or(p["base_instructions"]["text"].as_str())
                    {
                        s["extra"]["base_instructions_chars"] = json!(b.chars().count());
                    }
                }
                "turn_context" => {
                    set(&mut s, "model", &p["model"]);
                    if p["cwd"].as_str().is_some_and(|v| !v.is_empty()) {
                        set(&mut s, "cwd", &p["cwd"]);
                    }
                }
                "world_state" => {
                    if s["model"].is_null() {
                        set(&mut s, "model", &p["state"]["collaboration_mode"]["model"]);
                    }
                    if s["cwd"].is_null() {
                        set(&mut s, "cwd", &p["state"]["environments"]["local"]["cwd"]);
                    }
                }
                _ => {}
            }
        } else {
            if s["id"].is_null() && o["sessionId"].is_string() {
                s["id"] = o["sessionId"].clone();
                s["started_at"] = o["timestamp"].clone();
            }
            for (dst, src) in [
                ("cwd", "cwd"),
                ("cli_version", "version"),
                ("git_branch", "gitBranch"),
                ("entrypoint", "entrypoint"),
                ("user_type", "userType"),
            ] {
                set(&mut s, dst, &o[src]);
            }
            if o["cwd"].is_string() && !roots.contains(&o["cwd"]) {
                roots.push(o["cwd"].clone());
            }
            if let Some(id) = o["uuid"].as_str() {
                if !by.contains_key(id) {
                    ids.push(id.to_owned());
                }
                by.insert(id.to_owned(), i);
            }
            if o["type"] == "last-prompt" {
                leaf = o["leafUuid"].as_str().map(str::to_owned).or(leaf);
            }
        }
    }
    if !roots.is_empty() {
        s["extra"]["workspace_roots"] = json!(roots);
    }
    if source != "codex" {
        for k in ["git_branch", "entrypoint", "user_type"] {
            if !s[k].is_null() {
                s["extra"][k] = s[k].clone();
            }
        }
    }
    let mut order: Vec<usize> = (0..records.len()).collect();
    let mut chain_len = order.len();
    if source == "claude_code" {
        if leaf.is_none() {
            let parents: HashSet<&str> = records
                .iter()
                .filter_map(|(_, o)| o["parentUuid"].as_str())
                .collect();
            leaf = ids
                .iter()
                .rev()
                .find(|id| !parents.contains(id.as_str()))
                .cloned();
        }
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut current = leaf;
        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                break;
            }
            let Some(&i) = by.get(&id) else {
                break;
            };
            chain.push(i);
            current = records[i].1["parentUuid"].as_str().map(str::to_owned);
        }
        if !chain.is_empty() {
            chain.reverse();
        } else {
            chain = ids.iter().map(|id| by[id]).collect();
        }
        chain_len = chain.len();
        let chosen: HashSet<usize> = chain.iter().copied().collect();
        chain.extend((0..records.len()).filter(|i| {
            !chosen.contains(i)
                && records[*i].1["uuid"]
                    .as_str()
                    .is_none_or(|id| by.get(id) == Some(i))
        }));
        order = chain;
        if s["id"].is_null() {
            s["id"] = json!(path.file_stem().unwrap_or_default().to_string_lossy());
        }
    }
    let mut messages = Vec::new();
    let mut world = false;
    let mut totals = [0i64; 4];
    for (position, i) in order.into_iter().enumerate() {
        let (raw, o) = &records[i];
        if source == "codex" && o["type"] == "world_state" {
            if world {
                continue;
            }
            world = true;
        }
        let ordinal = if source == "codex" { i } else { messages.len() };
        let m = build(o, raw, ordinal, source);
        if source == "claude_code"
            && position < chain_len
            && m["role"] == "assistant"
            && s["model"].is_null()
        {
            set(&mut s, "model", &m["model"]);
        }
        for (j, k) in [
            "input_tokens",
            "output_tokens",
            "reasoning_tokens",
            "cached_input_tokens",
        ]
        .iter()
        .enumerate()
        {
            let count = if source == "codex" {
                m["kind"] == "token_usage"
            } else {
                position < chain_len
                    && if j == 0 {
                        m["role"] == "user"
                    } else {
                        m["role"] == "assistant"
                    }
            };
            if count {
                totals[j] += m[*k].as_i64().unwrap_or(0);
            }
        }
        messages.push(m);
    }
    anyhow::ensure!(
        s["id"].as_str().is_some_and(|id| !id.is_empty()),
        "Codex rollout has no session_id: {}",
        path.display()
    );
    for (i, k) in [
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cached_input_tokens",
    ]
    .iter()
    .enumerate()
    {
        s[*k] = json!(totals[i]);
    }
    s["message_count"] = json!(messages.len());
    s["user_message_count"] = json!(messages.iter().filter(|m| m["role"] == "user").count());
    s["assistant_message_count"] = json!(
        messages
            .iter()
            .filter(|m| m["role"] == "assistant"
                && matches!(
                    m["kind"].as_str(),
                    Some("message" | "reasoning" | "function_call" | "tool_use")
                ))
            .count()
    );
    s["preview"] = messages
        .iter()
        .find(|m| m["role"] == "user" && m["text"].as_str().is_some_and(|t| !t.is_empty()))
        .map(|m| {
            json!(
                m["text"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
        .unwrap_or(Value::Null);
    Ok(Parsed {
        session: s,
        messages,
    })
}
