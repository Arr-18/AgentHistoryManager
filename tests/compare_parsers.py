"""Read-only parser comparison with the original Python project.

Run after cargo build --example parse_history. No Python imports may write pyc.
Compare cached sessions' source files only; do not print private message data.
"""
import json
import sqlite3
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True
root = Path(__file__).resolve().parents[1]
original = root.parent / 'AgentHistoryManager'
sys.path.insert(0, str(original / 'backend'))
from parsers import codex, claude

binary = root / 'target/debug/examples/parse_history.exe'
if not binary.exists():
    binary = root / 'target/debug/examples/parse_history'
conn = sqlite3.connect((original / 'data/history.sqlite').as_uri() + '?mode=ro&immutable=1', uri=True)
files = conn.execute('SELECT source,raw_path FROM sessions ORDER BY source,id').fetchall()
conn.close()
checked = 0
skipped = 0
counts = {}
differences = []
for source, file in files:
    path = Path(file)
    if not path.is_file():
        skipped += 1
        continue
    try:
        py = codex.parse_rollout(path) if source == 'codex' else claude.parse_session(path)
    except Exception as exc:
        differences.append({'source': source, 'file': path.name, 'original_parser_error': type(exc).__name__})
        continue
    run = subprocess.run([str(binary), source, str(path)], capture_output=True, check=True)
    rs = json.loads(run.stdout)
    checked += 1
    counts[source] = counts.get(source, 0) + len(py.messages)
    for field in ['started_at', 'ended_at', 'cwd', 'model', 'cli_version', 'input_tokens', 'output_tokens', 'reasoning_tokens', 'cached_input_tokens']:
        if getattr(py, field) != rs['session'][field]:
            differences.append({'source': source, 'file': path.name, 'field': field})
    if len(py.messages) != len(rs['messages']):
        differences.append({'source': source, 'file': path.name, 'field': 'message_count'})
        continue
    for pm, rm in zip(py.messages, rs['messages']):
        for field in ['ordinal', 'timestamp', 'role', 'kind', 'model', 'stop_reason', 'input_tokens', 'output_tokens', 'reasoning_tokens', 'cached_input_tokens', 'raw_json']:
            if getattr(pm, field) != rm[field]:
                differences.append({'source': source, 'file': path.name, 'ordinal': pm.ordinal, 'field': field})
        if pm.content_json != rm['content']:
            differences.append({'source': source, 'file': path.name, 'ordinal': pm.ordinal, 'field': 'content'})
        # JSON spacing differences in rendered tool arguments do not affect source preservation.
        if pm.kind != 'function_call' and pm.text != rm['text']:
            differences.append({'source': source, 'file': path.name, 'ordinal': pm.ordinal, 'field': 'text'})
report = {'checked_files': checked, 'missing_files': skipped, 'message_counts': counts, 'differences': differences}
print(json.dumps(report, ensure_ascii=True, indent=2))
(root / 'docs/parser-parity.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
if differences:
    raise SystemExit(1)
