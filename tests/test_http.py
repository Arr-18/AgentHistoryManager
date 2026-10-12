"""Exercise the built HTTP server and background polling on isolated fixtures."""
import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

root = Path(__file__).resolve().parents[1]
binary = root / 'target/debug/agent-history-manager.exe'
with tempfile.TemporaryDirectory(prefix='ahm-rust-http-') as temp:
    base = Path(temp)
    home = base / 'user'
    sessions = home / '.codex/sessions'
    sessions.mkdir(parents=True)
    def fixture(path, sid):
        records = [
            {'type':'session_meta','timestamp':'2026-10-01T00:00:00Z','payload':{'id':sid,'cwd':str(base)}},
            {'type':'response_item','payload':{'type':'message','role':'user','content':[{'type':'input_text','text':'http smoke'}]}},
        ]
        path.write_text('\n'.join(map(json.dumps, records)), encoding='utf-8')
    fixture(sessions / 'rollout-http.jsonl', 'http-smoke')
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0))
        port = sock.getsockname()[1]
    env = os.environ.copy()
    for name in ['CODEX_HOME','CLAUDE_HOME','AHM_CODEX_SESSIONS_DIR','AHM_CLAUDE_PROJECTS_DIR','AHM_DB_PATH','AHM_SYNC_INTERVAL']:
        env.pop(name, None)
    env['USERPROFILE'] = str(home)
    proc = subprocess.Popen([str(binary),'--port',str(port),'--sync-interval','1','--db-path',str(base / 'history.sqlite')], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
    origin = f'http://127.0.0.1:{port}'
    def request(path, data=None):
        req = urllib.request.Request(origin + path, data=None if data is None else json.dumps(data).encode(), headers={'Content-Type':'application/json'})
        with urllib.request.urlopen(req,timeout=10) as response:
            return json.load(response)
    try:
        deadline = time.monotonic()+20
        while True:
            if proc.poll() is not None:
                raise RuntimeError(proc.stderr.read().decode(errors='replace'))
            try:
                if request('/api/stats')['total_sessions'] == 1:
                    break
            except (OSError, ValueError):
                pass
            if time.monotonic() > deadline:
                raise TimeoutError('startup sync did not finish')
            time.sleep(.1)
        assert request('/healthz') == {'ok':True}
        paths = request('/api/history-paths')
        assert paths[0]['path'] == str(sessions)
        assert paths[0]['status'] == 'available'
        assert paths[1]['status'] == 'missing'
        assert request('/api/sessions?search=http')['total'] == 1
        assert request('/api/sessions/codex/http-smoke/messages')['count'] == 1
        assert request('/api/sessions/codex/http-smoke/messages?include_events=true')['count'] == 2
        assert request('/api/ingest/sync',{})['summary']['codex']['files'] == 0
        assert request('/api/ingest/full',{})['summary']['codex']['updated'] == 1
        fixture(sessions / 'rollout-poll.jsonl','background-poll')
        deadline = time.monotonic()+10
        while request('/api/stats')['total_sessions'] != 2:
            assert time.monotonic() < deadline
            time.sleep(.1)
        manual = base / 'manual-history'
        manual.mkdir()
        fixture(manual / 'rollout-manual.jsonl','manual-path')
        result = request('/api/history-paths',{'source':'codex','path':str(manual)})
        assert result[0]['path'] == str(manual)
        assert result[0]['status'] == 'available'
        request('/api/ingest/sync',{})
        assert request('/api/sessions/codex/manual-path')['id'] == 'manual-path'
        for path in ['/','/static/app.js','/static/filter.js','/static/styles.css']:
            with urllib.request.urlopen(origin+path,timeout=10) as response:
                assert response.status == 200
                assert response.read()
        print('Live HTTP server: startup import, periodic sync, manual import, path update, missing Agent and static UI passed')
    finally:
        proc.terminate()
        proc.wait(timeout=10)
