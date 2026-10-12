use anyhow::Result;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, RwLock},
    time::UNIX_EPOCH,
};
pub struct Ingestor {
    pub db: PathBuf,
    pub roots: RwLock<[(String, PathBuf); 2]>,
    lock: Mutex<()>,
}
impl Ingestor {
    pub fn new(db: PathBuf, roots: [(String, PathBuf); 2]) -> Self {
        Self {
            db,
            roots: RwLock::new(roots),
            lock: Mutex::new(()),
        }
    }
    pub fn load_saved_paths(&self) -> Result<()> {
        let c = crate::db::open(&self.db)?;
        let saved = crate::db::query(&c, "SELECT source,history_path FROM source_config", &[])?;
        let mut roots = self
            .roots
            .write()
            .map_err(|_| anyhow::anyhow!("path lock poisoned"))?;
        for (source, path) in roots.iter_mut() {
            let overridden = if source == "codex" {
                ["AHM_CODEX_SESSIONS_DIR", "CODEX_HOME"]
            } else {
                ["AHM_CLAUDE_PROJECTS_DIR", "CLAUDE_HOME"]
            }
            .iter()
            .any(|key| std::env::var_os(key).is_some());
            if !overridden
                && let Some(row) = saved
                    .iter()
                    .find(|r| r["source"].as_str() == Some(source.as_str()))
                && let Some(p) = row["history_path"].as_str()
            {
                *path = PathBuf::from(p);
            }
        }
        Ok(())
    }
    pub fn path_status(&self) -> Result<Value> {
        let roots = self
            .roots
            .read()
            .map_err(|_| anyhow::anyhow!("path lock poisoned"))?;
        Ok(json!(roots.iter().map(|(source,path)|{
            let (status,message)=match std::fs::read_dir(path){
                Ok(_)=>("available","历史目录可读取"),
                Err(e) if e.kind()==std::io::ErrorKind::NotFound=>("missing","未检测到历史目录，可能未安装 Agent 或使用了其他路径"),
                Err(e) if e.kind()==std::io::ErrorKind::PermissionDenied=>("unreadable","历史目录不可读取，请检查权限"),
                Err(_)=>("invalid","路径不是可读取的历史目录"),
            };
            json!({"source":source,"path":path.to_string_lossy(),"status":status,"message":message})
        }).collect::<Vec<_>>()))
    }
    pub fn set_path(&self, source: &str, path: &Path) -> Result<()> {
        anyhow::ensure!(matches!(source, "codex" | "claude_code"), "unknown source");
        anyhow::ensure!(path.is_absolute(), "请输入绝对路径");
        // Serialize path changes with ongoing scans so a scan uses one consistent configuration.
        let _guard = self
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("ingest lock poisoned"))?;
        let mut roots = self
            .roots
            .write()
            .map_err(|_| anyhow::anyhow!("path lock poisoned"))?;
        let c = crate::db::open(&self.db)?;
        c.execute("INSERT INTO source_config(source,history_path) VALUES (?1,?2) ON CONFLICT(source) DO UPDATE SET history_path=excluded.history_path",rusqlite::params![source,path.to_string_lossy()])?;
        roots.iter_mut().find(|(s, _)| s == source).unwrap().1 = path.to_path_buf();
        Ok(())
    }
    pub fn run(&self, full: bool) -> Result<Value> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| anyhow::anyhow!("ingest lock poisoned"))?;
        crate::db::init(&self.db)?;
        let mut c = crate::db::open(&self.db)?;
        let roots = self
            .roots
            .read()
            .map_err(|_| anyhow::anyhow!("path lock poisoned"))?
            .clone();
        let started = crate::db::now();
        let mut summary = json!({});
        for (source, root) in &roots {
            let mut counts = json!({"added":0,"updated":0,"messages":0,"files":0,"errors":[]});
            let mut max_mtime = 0.0f64;
            if root.exists() {
                for entry in walkdir::WalkDir::new(root).follow_links(false) {
                    let entry = match entry {
                        Ok(e) => e,
                        Err(e) => {
                            counts["errors"].as_array_mut().unwrap().push(json!({"file":e.path().map(|p|p.to_string_lossy().to_string()),"error":e.to_string()}));
                            continue;
                        }
                    };
                    let path = entry.path();
                    if !entry.file_type().is_file() || !matches_file(path, source) {
                        continue;
                    }
                    let result = (|| -> Result<Option<(bool, usize)>> {
                        let meta = path.metadata()?;
                        let mtime = meta.modified()?.duration_since(UNIX_EPOCH)?.as_secs_f64();
                        max_mtime = max_mtime.max(mtime);
                        if !full {
                            let rows = crate::db::query(
                                &c,
                                "SELECT file_mtime,file_size FROM file_state WHERE source=? AND raw_path=?",
                                &[json!(source), json!(path.to_string_lossy())],
                            )?;
                            if rows.first().is_some_and(|r| {
                                r["file_mtime"].as_f64() == Some(mtime)
                                    && r["file_size"].as_u64() == Some(meta.len())
                            }) {
                                return Ok(None);
                            }
                        }
                        let parsed = crate::parsers::parse(path, source)?;
                        // Refuse to acknowledge a file that changed during parsing; retry next poll.
                        let after = path.metadata()?;
                        anyhow::ensure!(
                            after.len() == meta.len() && after.modified()? == meta.modified()?,
                            "file changed while being parsed; will retry"
                        );
                        let added = crate::db::store(&mut c, &parsed, path, mtime, meta.len())?;
                        Ok(Some((added, parsed.messages.len())))
                    })();
                    match result {
                        Ok(Some((added, n))) => {
                            for (k, amount) in [
                                (if added { "added" } else { "updated" }, 1),
                                ("messages", n),
                                ("files", 1),
                            ] {
                                counts[k] = json!(counts[k].as_u64().unwrap() + amount as u64);
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            counts["errors"]
                                .as_array_mut()
                                .unwrap()
                                .push(json!({"file":path.to_string_lossy(),"error":e.to_string()}));
                        }
                    }
                }
            }
            c.execute("INSERT INTO sync_state(source,last_scan_at,last_watermark_mtime) VALUES (?1,?2,?3) ON CONFLICT(source) DO UPDATE SET last_scan_at=excluded.last_scan_at,last_watermark_mtime=excluded.last_watermark_mtime",rusqlite::params![source,crate::db::now(),max_mtime])?;
            summary[source] = counts;
        }
        let sum = |key: &str| {
            roots
                .iter()
                .map(|(s, _)| summary[s][key].as_i64().unwrap_or(0))
                .sum::<i64>()
        };
        let errors: Vec<_> = roots
            .iter()
            .flat_map(|(s, _)| summary[s]["errors"].as_array().unwrap().iter().cloned())
            .collect();
        c.execute("INSERT INTO ingest_runs(started_at,finished_at,sessions_added,sessions_updated,messages_added,error) VALUES (?1,?2,?3,?4,?5,?6)",rusqlite::params![started,crate::db::now(),sum("added"),sum("updated"),sum("messages"),if errors.is_empty(){None}else{Some(serde_json::to_string(&errors)?)}])?;
        Ok(summary)
    }
}
pub fn matches_file(path: &Path, source: &str) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if source == "codex" {
        name.starts_with("rollout-") && name.ends_with(".jsonl")
    } else {
        name.ends_with(".jsonl") && name != "history.jsonl"
    }
}
