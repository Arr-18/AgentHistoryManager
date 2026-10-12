use clap::Parser;
use std::{env, path::PathBuf};
#[derive(Parser, Debug, Clone)]
pub struct Config {
    #[arg(long, env = "AHM_HOST", default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, env = "AHM_PORT", default_value_t = 9000)]
    pub port: u16,
    #[arg(long, env = "AHM_SYNC_INTERVAL", default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..))]
    pub sync_interval: u64,
    #[arg(long)]
    pub no_sync: bool,
    #[arg(long)]
    pub full_ingest: bool,
    #[arg(long)]
    pub sync_once: bool,
    #[arg(long, env = "AHM_DB_PATH")]
    pub db_path: Option<PathBuf>,
    #[arg(long, env = "AHM_WEB_DIR")]
    pub web_dir: Option<PathBuf>,
}
impl Config {
    pub fn db(&self) -> PathBuf {
        self.db_path.clone().unwrap_or_else(|| {
            env::var_os("AHM_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data"))
                .join("history.sqlite")
        })
    }
    pub fn web(&self) -> PathBuf {
        self.web_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web"))
    }
    pub fn roots(&self) -> [(String, PathBuf); 2] {
        let home = env::var_os("USERPROFILE")
            .or_else(|| env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default();
        [
            (
                "codex".into(),
                env::var_os("AHM_CODEX_SESSIONS_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        env::var_os("CODEX_HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| home.join(".codex"))
                            .join("sessions")
                    }),
            ),
            (
                "claude_code".into(),
                env::var_os("AHM_CLAUDE_PROJECTS_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        env::var_os("CLAUDE_HOME")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| home.join(".claude"))
                            .join("projects")
                    }),
            ),
        ]
    }
}
