# 验证结果

验证日期：2026-10-08。Windows x86_64-pc-windows-msvc，rustc/cargo 1.98.1。

## 已通过

- `cargo test --all-targets`：11 项集成回归，全部通过。涵盖 Codex 原行/未知记录/损坏 JSON/token/world_state；Claude 主链、分支、空行、损坏记录、循环和缺少 ID；会话消息替换、新文件旧时间戳、失败重试、全量导入、旧 schema、并发扫描；API 筛选/分页/事件/详情/静态资源；缺失 Agent、持久化路径和环境变量优先级。
- `cargo clippy --all-targets -- -D warnings`：通过，无警告。
- `cargo build --bins --examples`：通过，生成服务可执行文件及解析对照工具。
- `node --check web/app.js`：通过。
- `node tests/test_filter.js`：56 项检查通过。
- `node tests/test_ui.js`：实际前端控制器在轻量 DOM/fetch 环境执行；分页、详情摘要、聊天/内部记录切换、缺失历史目录、编辑中保留输入、保存路径后同步全部通过。
- `python -B tests/test_http.py`：启动真实 Rust 服务并使用本机 HTTP 请求，验证启动导入、每秒轮询发现新文件、手动增量/全量、手动路径更新、未安装 Agent 的提示、页面和静态资源。使用临时用户目录和临时 SQLite；结束后停止测试服务。
- `python -B tests/compare_parsers.py`：只读原缓存引用的 10 个真实历史文件，Claude 52 条 / Codex 1462 条，共 1514 条。开始结束时间、cwd、模型、CLI、token 汇总，逐消息 ordinal/时间/角色/类型/model/stop_reason/token/raw_json/content 对照无差异；函数参数文本的 JSON 空格允许不同。结果见 parser-parity.json。
- 原项目完整文件 SHA-256 核对：32 个文件，修改 0、新增 0。原 SQLite 只用 URI `mode=ro&immutable=1` 读取；原模块导入禁用 pyc；没有运行会修改原 data 目录的 Python 测试。

## 实用边界

前端验证包括控制器执行及真实 HTTP 静态资源响应，未做真实浏览器视觉截图检查。

真实历史对照只覆盖读取时原缓存引用的 10 个文件，其他格式边界通过人工 fixture 验证。修复 Claude 空行/损坏记录/无可用主链时，结果有意优于原错误行为。

Python 原 API 的核心 JSON 路由已迁移，FastAPI 自动 OpenAPI/Swagger 文档页未迁移。运行 Rust 后端不需要 Python；Python 仅用于本仓库的开发验证脚本。

本次未复制原会话数据库到新项目，也未改写原数据库。首次正常运行会自动读取当前用户的历史目录，创建独立 Rust 数据库。未找到目录可以在 UI 输入绝对路径。

## 重现

```powershell
cd D:\NJU\AgentHistoryManager-Rust
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo build --bins --examples
node --check web/app.js
node tests/test_filter.js
node tests/test_ui.js
python -B tests/test_http.py
python -B tests/compare_parsers.py
```
