# AgentHistoryManager Rust


## 运行

```powershell
cd D:\NJU\AgentHistoryManager-Rust
cargo run -- --port 9000
# 浏览器打开 http://127.0.0.1:9000
```

默认创建本项目 `data/history.sqlite`，启动立即增量同步，此后每 15 秒轮询。

```powershell
cargo run -- --full-ingest       # 全量导入后退出
cargo run -- --sync-once         # 增量导入后退出
cargo run -- --no-sync --port 9001
cargo test
cargo clippy --all-targets -- -D warnings
node tests/test_filter.js
node tests/test_ui.js
```

## 历史目录

从当前进程的 `USERPROFILE` 自动获取 Windows 用户目录，无固定用户名；其他系统回退 `HOME`。

| 来源 | 默认历史目录 | Agent 主目录覆盖 | 直接覆盖历史目录 |
|---|---|---|---|
| Codex CLI | `<用户目录>/.codex/sessions` | `CODEX_HOME` | `AHM_CODEX_SESSIONS_DIR` |
| Claude Code | `<用户目录>/.claude/projects` | `CLAUDE_HOME` | `AHM_CLAUDE_PROJECTS_DIR` |

Web UI 的“历史记录目录”区显示有效路径和可读取、缺失、无权限或无效状态。用户可输入历史目录绝对路径，点击“保存路径”后自动同步。输入 Codex 的 `sessions` 目录或 Claude 的 `projects` 目录。设置保存在本项目 SQLite `source_config` 表，重启继续使用。

启动时优先级：直接历史目录环境变量 > Agent 主目录环境变量 > Web UI 保存的路径 > 用户目录默认值。Web UI 保存会立刻更新本次进程路径；如果仍设置了环境变量，下次启动环境变量重新生效。

目录不存在时给出普通提示，导入返回 0 条记录，服务正常启动。仅凭目录缺失无法判断软件是否安装，因此提示“可能未安装 Agent 或使用了其他路径”。

```powershell
$env:AHM_CODEX_SESSIONS_DIR = 'E:\history\codex\sessions'
$env:AHM_CLAUDE_PROJECTS_DIR = 'E:\history\claude\projects'
cargo run
```

其他环境变量：`AHM_DATA_DIR`、`AHM_DB_PATH`、`AHM_WEB_DIR`、`AHM_SYNC_INTERVAL`、`AHM_HOST`、`AHM_PORT`。Rust 使用重新编译启动，不支持 Python 的 `AHM_RELOAD`。

## API

保留 `/healthz`、`/api/stats`、`/api/sources`、`/api/sessions`、`/api/sessions/{source}/{id}`、`/api/sessions/{source}/{id}/messages`、`POST /api/ingest/full`、`POST /api/ingest/sync`、`/api/ingest/runs`。

新增：

- `GET /api/history-paths`：两种 Agent 的有效路径、状态和提示。
- `POST /api/history-paths`：JSON `{"source":"codex","path":"E:\\history\\sessions"}`，保存绝对路径并返回新状态。

会话列表支持 `source`、`search`、`cwd`、`from`、`to`、`offset`、`limit`。日期使用 `YYYY-MM-DD`，按记录的 `started_at` 日期部分做包含边界的比较。搜索保持原 API 的 preview + message text 语义，cwd 通过独立筛选参数查询。消息默认隐藏事件；`include_events=true` 获取完整解析时间线。原始源记录保存在数据库 `raw_json` 中，保持原 API 消息响应字段不变。

## 迁移与兼容

详细阅读结论、数据库结构、原行为和修正点见 [docs/MIGRATION.md](docs/MIGRATION.md)。Rust 直接保留旧五张表和字段，新增两张独立辅助表。需要旧缓存时，应复制数据库到 Rust 数据目录后启动，不要把 Rust 的 `AHM_DB_PATH` 指向原 Python 数据库。

当前无 FastAPI 自动生成的 OpenAPI/Swagger 页面。搜索仍用 LIKE，删除源文件不会删除已导入会话；同一来源的重复 session ID 仍按最后导入的文件覆盖。增量检测依据 mtime + 文件大小，两者都不变的内容替换需要全量重扫。
