# 阅读结论与分阶段迁移

## 原项目完整阅读范围

已阅读 README、requirements、run.py、所有 backend 模块及 parsers、全部三份 tests，以及 web/index.html、app.js、filter.js、styles.css 和 .gitignore。两个 SQLite 文件以 mode=ro&immutable=1 检查结构和汇总，不运行会改写原项目 data 目录的 Python ingest 测试。

原 history.sqlite：Codex 6 个会话 / 1462 条记录，Claude Code 4 个会话 / 52 条记录。原 _test_history.sqlite：Codex 5 个会话 / 1282 条记录，Claude Code 4 个会话 / 52 条记录。这里是读取时的缓存数量，不代表实时源文件数量。

原项目：FastAPI + uvicorn + SQLite + 原生 JS。默认 127.0.0.1:9000；配置通过环境变量；CLI 提供 host/port/no-reload。运行路径 run.py → config → API lifespan → init_db → SyncWorker。

## 数据库

- sources：name 主键，种子 codex / claude_code。
- sessions：复合主键 (source,id)。开始结束时间、cwd、模型和提供商、CLI/runtime/originator/git/entrypoint/user_type 元数据、原文件路径和浮点秒 mtime、消息及各角色计数、四种 token 计数、preview、extra_json、ingested_at。
- messages：自增 id；(session_id,source,ordinal) 唯一；时间、角色、类型、可搜索 text、content_json、原行 raw_json、模型、stop_reason、四种 token；指向 sessions 的复合外键。
- ingest_runs：每次文件导入和整个扫描各记录一次，含开始结束、来源、新增/更新会话数和重插消息数、error。
- sync_state：按来源保存扫描时间和全局最大 mtime。

连接启用 WAL / foreign_keys / synchronous=NORMAL。原 Python 用写锁串行事务，会话 upsert 后 DELETE + INSERT 全量替换该会话消息；空 preview 更新保留旧值。Rust schema.sql 直接提取原 schema，沿用字段与索引，不修改原文件；新增 file_state 记录每个成功导入文件 mtime/size，source_config 保存用户路径设置。Rust 用单个扫描锁防止手动和后台扫描交错，每个会话事务写入元数据、消息、file_state 和单文件 ingest_run，失败自动回滚。

## Codex 解析

递归找 rollout-*.jsonl，只读 UTF-8，坏字符替换；跳过空行，ordinal 按非空记录位置。session_meta 抽 ID、路径、CLI、来源、workspace_roots、instructions 信息；world_state 提供模型和 cwd 回退，时间线仅保留首个 world_state；turn_context 更新模型和 cwd。

response_item 分 message/reasoning/function_call/function_call_output，文本按对应块类型提取，结构化 content 保存子字段。event_msg 保存 subtype；token_usage_record 按每条 usage 累加，不把 event:token_count 再累加。未知类型保存为事件；损坏 JSON/non-object 保存 malformed。raw_json 保存去行尾后的原行。preview 为首个有文字用户记录的前 200 字符。assistant_count 包括 message、reasoning、function_call。

Rust 保持这些基本语义；function_call 参数重新序列化时 JSON 空格可能和 Python 不同，但 raw_json 保留源行。

## Claude Code 解析

递归找 *.jsonl，排除 history.jsonl（包含子 Agent 目录）。首个 sessionId 决定会话 ID/started_at；最后 timestamp 决定 ended_at；cwd/version/gitBranch/entrypoint/userType 按文件更新，cwd 去重收集为 workspace_roots。

按 uuid 建索引，优先 last-prompt.leafUuid，否则最后一个无子节点的 UUID；沿 parentUuid 倒推并反转，循环时停止；非选中分支与无 UUID 元记录在主链后按文件顺序追加。assistant 内容 text/thinking 合并，没有这类块时标记 tool_use。user 保留字符串或块内容；attachment/system/ai-title/permission-mode/file-history-snapshot/last-prompt 分别保存。没有 sessionId 时使用文件 stem。

原实现 token 仅在主链阶段聚合：assistant 的 output/cache/reasoning，加 user 的 input；assistant input 虽保存逐条但不计入 session input，其他分支不计入 token。Rust 暂时保持该统计口径，避免静默改变现有数字。decode_project_dir 只是 Python 测试显示辅助函数，实际解析以 JSON cwd 为准，Rust 不用文件夹解码猜测 cwd。

修正：Python 用物理行号索引删掉空行后的 raw_records 会错位；malformed 无 UUID 被丢弃；无可用链时回退被无条件反转。Rust 用有效记录索引、保留 malformed、回退保持文件顺序。重复 UUID 使用最后记录，不复制同一逻辑消息。

## 导入与同步

原 full_ingest 扫全部文件并记录错误列表；sync_once 只处理 mtime 大于来源全局 watermark 的文件。各文件失败不终止全局，扫描后记录 run 和水位。后台线程启动即同步，每 15 秒再同步；其锁只限制后台，原手动 API 和后台仍可交错。

Rust 保留全量/增量入口、按会话替换和失败隔离，但按每个成功文件 mtime + size 判断变化；新旧 mtime 的新文件均会发现，失败文件不会被标记成功，下一次重试；解析期间发现文件变化时拒绝确认并重试。sync_state 水位保留为兼容信息，不用于过滤。没有 Agent 目录时 0 条结果；目录权限与遍历错误保留在状态/错误摘要。

## API 和 Web UI

原 API 返回 SQLite 字典，models.py 的 Pydantic 输出模型未应用到 route response_model；读取详情解码 extra_json，消息解码 content_json。消息 API 并未返回原始 raw_json，UI 的 Raw JSON 是 API 消息对象的 JSON，不是源行。Rust 保持响应字段。原搜索实现实际上只查 preview/message text，不查 cwd；README 的 from/to 在旧 API 未实现。Rust 添加日期筛选，保持独立 cwd 参数和其他响应结构，分页边界校验。

Web 使用代理导航、会话分页、详情聊天、状态条。请求消息时始终 include_events=true，在浏览器用 filter.js 过滤 internal。environment_context 在聊天文字中剥离，reasoning/tool/system/event/developer 默认隐藏，附件保留；原生 DOM 输出和 escaping 避免把消息文字作为 HTML 执行。搜索防抖 250ms，stats 每 30s 更新。样式 VS Code 浅色、消息气泡和 raw 卡片。

新项目复制全部 UI 文件，修复摘要引用作用域外 visible 导致 ReferenceError、分页 end 插值遗漏；引擎标签改为 Rust。新增路径状态区和绝对路径输入/保存按钮，30 秒重测状态，编辑输入框时避免自动刷新清空输入。不提供浏览器文件夹选择，因为浏览器选择器不返回可供服务器读取的 Windows 绝对路径；路径输入满足本次手动指定需求。

## 实施阶段

1. 已完成：完整阅读及独立项目、配置、原 schema、双解析器。
2. 已完成：事务导入、按文件增量同步、启动轮询、CLI 单次导入。
3. 已完成：HTTP API 和复制 UI、路径检测/持久化/状态展示。
4. 已完成验证：固定 fixture 的 Rust 回归、旧缓存 schema 兼容、真实历史只读对照、HTTP/static 与 Node 检查；具体结果见 VALIDATION.md。

后续可单独开展 FTS、分页消息、完整 OpenAPI、可选 Claude token 统计口径变更。它们不作为本次基础后端迁移的前置条件。
