/* Execute the real browser controller with a small DOM/fetch harness. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const Filter = require('../web/filter.js');
class Element {
    constructor(tag = 'div') { this.tag = tag; this.children = []; this.events = {}; this.style = {}; this.dataset = {}; this.value = ''; this.textContent = ''; this.innerHTML = ''; this.classList = {add() {}}; }
    set innerHTML(value) { this._html = value; this.children = []; }
    get innerHTML() { return this._html; }
    appendChild(e) { this.children.push(e); return e; }
    append(...items) { this.children.push(...items); }
    replaceChildren(...items) { this.children = items; }
    addEventListener(name, fn) { this.events[name] = fn; }
    setAttribute(name, value) { this[name] = value; }
    contains(e) { return this === e || this.children.some(c => c.contains && c.contains(e)); }
    blur() { document.activeElement = null; }
    remove() {}
}
const nodes = new Map();
const document = {activeElement: null, getElementById(id) { if (!nodes.has(id)) nodes.set(id, new Element()); return nodes.get(id); }, createElement(tag) { return new Element(tag); }, createTextNode(text) { const n = new Element('text'); n.textContent = text; return n; }};
const session = {source:'codex',id:'sample',cwd:'D:\\workspace',started_at:'2026-10-01',message_count:3,user_message_count:1,assistant_message_count:1,preview:'hello'};
let paths = [{source:'codex',path:'D:\\user\\.codex\\sessions',status:'available',message:'History directory available'}, {source:'claude_code',path:'D:\\user\\.claude\\projects',status:'missing',message:'Agent history directory not found'}];
let saves = 0;
const intervals = [];
async function fetch(url, opts = {}) {
    let data;
    if (url === '/api/history-paths') {
        if (opts.method === 'POST') { const body = JSON.parse(opts.body); saves++; paths = paths.map(p => p.source === body.source ? {...p,path:body.path,status:'available',message:'History directory available'} : p); }
        data = paths;
    } else if (url === '/api/stats') { data = {sources:[{name:'codex',session_count:1,message_count:3},{name:'claude_code',session_count:0,message_count:0}]}; }
    else if (url.startsWith('/api/sessions?')) { data = {total:1,items:[session]}; }
    else if (url.endsWith('/messages?include_events=true')) { data = {count:3,items:[{ordinal:0,role:'system',kind:'session_meta',text:'',content:{}},{ordinal:1,role:'user',kind:'message',text:'hello',content:{}},{ordinal:2,role:'assistant',kind:'message',text:'reply',content:{}}]}; }
    else if (url === '/api/sessions/codex/sample') { data = session; }
    else if (url === '/api/ingest/sync') { data = {summary:{codex:{added:0,updated:1}}}; }
    else { throw new Error('Unexpected request: ' + url); }
    return {ok:true,json:async()=>JSON.parse(JSON.stringify(data))};
}
const sandbox = {document,window:{AHMFilter:Filter},console,fetch,URLSearchParams,setTimeout:()=>1,clearTimeout(){},setInterval(fn) {intervals.push(fn);return 1;}};
vm.runInNewContext(fs.readFileSync(path.join(__dirname,'../web/app.js'),'utf8'),sandbox);
async function settle() {for(let i=0;i<10;i++) await new Promise(resolve=>setImmediate(resolve));}
(async () => {
    await settle();
    assert.match(nodes.get('pagerInfo').textContent,/1.*1 of 1/);
    assert.equal(nodes.get('historyPaths').children.length,2);
    const missing = nodes.get('historyPaths').children[1];
    assert.equal(missing.children[1].value,paths[1].path);
    assert.match(missing.children[2].className,/missing/);
    nodes.get('sessionList').children[0].events.click();
    await settle();
    assert.match(nodes.get('detailSummary').innerHTML,/1.*internal records hidden/);
    assert.equal(nodes.get('detailBody').children[0].children.length,2);
    nodes.get('showInternal').checked=true;
    nodes.get('showInternal').events.change();
    assert.equal(nodes.get('detailBody').children[0].children.length,3);
    const input=missing.children[1];
    input.value='D:\\manual\\projects';
    document.activeElement=input;
    intervals[0]();
    await settle();
    assert.equal(nodes.get('historyPaths').children[1].children[1],input);
    await missing.children[3].events.click();
    await settle();
    assert.equal(saves,1);
    assert.equal(nodes.get('historyPaths').children[1].children[1].value,'D:\\manual\\projects');
    assert.match(nodes.get('historyPaths').children[1].children[2].className,/available/);
    console.log('UI controller: pagination, chat/raw rendering, missing paths, edit preservation and save/sync passed');
})().catch(e=>{console.error(e);process.exitCode=1;});
