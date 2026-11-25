# End-to-end test of the local-AI settings (Settings › Local AI): the owner sets Ollama's address and model in
# the app; only local/private addresses; tried before saving; kept in the database; logged without patient data.
# Needs a running server with an EMPTY database, started with OLLAMA_URL=http://127.0.0.1:11999 (this test runs two
# stand-ins for Ollama: 11999 = "the server's", 11998 = "another clinic computer"). PSQL_URL adds database checks.
# Run: PSQL_URL=postgres://… python3 server/tests/e2e_ai.py
import os, sys, json, threading, subprocess, requests, pyotp
from http.server import BaseHTTPRequestHandler, HTTPServer

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
H = {'X-Device': 'Gabinete-1', 'X-Device-Key': DEVICE_KEY}
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m)
    if not c: fails += 1
def call(method, path, tok=None, json=None):
    h = dict(H)
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + path, json=json, headers=h, timeout=60)
def sql(q):
    return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

def fake(port, models, answer):
    seen = []
    class F(BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def reply(self, code, out):
            data = json.dumps(out).encode()
            self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)
        def do_GET(self):
            seen.append(('GET', self.path, None))
            if self.path == '/api/tags': self.reply(200, {'models': [{'name': m} for m in models]})
            else: self.reply(404, {})
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            seen.append(('POST', self.path, body))
            if self.path == '/api/show': self.reply(200 if body['model'] in models else 404, {})
            elif self.path == '/api/generate':
                if body['model'] not in models: self.reply(404, {'error': 'model not found'})
                else: self.reply(200, {'model': body['model'], 'done': True, 'response': answer})
            else: self.reply(404, {})
    srv = HTTPServer(('127.0.0.1', port), F)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return seen
on_server = fake(11999, ['gemma3:12b'], 'Rascunho do servidor.')
on_pc = fake(11998, ['gemma4:12b', 'qwen3:14b'], '<think>a pensar</think>Rascunho do PC da IA.')

r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Sousa', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt')
recep = join('reception', 'rita@clinica.pt')

# ── defaults come from the server's environment ──
s = call('GET', '/ai/settings', owner).json()
ok(s['url'] == 'http://127.0.0.1:11999' and s['model'] == 'gemma3:12b' and not s['url_in_app'] and not s['model_in_app'], f'defaults from OLLAMA_URL/OLLAMA_MODEL: {s}')
st = call('GET', '/ai/status', owner).json()
ok(st['available'] and st['models'] == ['gemma3:12b'], f'status lists the models: {st}')

# ── only the owner ──
for tok, who in ((ther, 'therapist'), (recep, 'reception')):
    ok(call('GET', '/ai/settings', tok).status_code == 403, f'{who} can’t read AI settings')
    ok(call('PUT', '/ai/settings', tok, {'url': '100.86.97.21'}).status_code == 403, f'{who} can’t change AI settings')
    ok(call('POST', '/ai/test', tok, {'url': '127.0.0.1:11998'}).status_code == 403, f'{who} can’t test an address')

# ── never an internet address ──
for bad in ('8.8.8.8', 'https://100.86.97.21', 'api.openai.com', 'http://example.com:11434', '100.128.0.1', '127.0.0.1:11998/api', 'pc-ia'):
    r = call('PUT', '/ai/settings', owner, {'url': bad})
    ok(r.status_code == 400, f'refuses {bad!r} ({r.status_code})')
ok(call('PUT', '/ai/settings', owner, {'url': '127.0.0.1:11998', 'model': 'a b; rm'}).status_code == 400, 'refuses a bad model name')
ok(call('GET', '/ai/settings', owner).json()['url'] == 'http://127.0.0.1:11999', 'nothing changed after refusals')

# ── try before saving: nothing is stored ──
t = call('POST', '/ai/test', owner, {'url': '127.0.0.1:11998', 'model': 'gemma4:12b'}).json()
ok(t['available'] and t['url'] == 'http://127.0.0.1:11998' and set(t['models']) == {'gemma4:12b', 'qwen3:14b'}, f'test reaches the other computer: {t}')
t = call('POST', '/ai/test', owner, {'url': '127.0.0.1:11998', 'model': 'gemma3:12b'}).json()
ok(not t['available'] and t['reason'] == 'no_model' and 'gemma4:12b' in t['models'], 'test says when the model isn’t there, and lists what is')
t = call('POST', '/ai/test', owner, {'url': '127.0.0.1:11997'}).json()
ok(not t['available'] and t['reason'] == 'offline', 'test says when nothing answers')
ok(call('GET', '/ai/settings', owner).json()['url'] == 'http://127.0.0.1:11999', 'testing saved nothing')

# ── save: used straight away, kept in the database, logged ──
r = call('PUT', '/ai/settings', owner, {'url': ' http://127.0.0.1:11998/ ', 'model': 'gemma4:12b'})
ok(r.status_code == 200 and r.json()['available'], f'saved and reachable: {r.text}')
s = call('GET', '/ai/settings', owner).json()
ok(s['url'] == 'http://127.0.0.1:11998' and s['model'] == 'gemma4:12b' and s['url_in_app'] and s['model_in_app'] and s['server_url'] == 'http://127.0.0.1:11999', f'settings show the app’s choice: {s}')
if PSQL:
    ok(sql("SELECT ai_url || ' ' || ai_model FROM clinic") == 'http://127.0.0.1:11998 gemma4:12b', 'kept in the database')
    ok(sql("SELECT count(*) FROM access_log WHERE action = 'ai.settings_changed' AND detail = 'http://127.0.0.1:11998 · gemma4:12b'") == '1', 'the change is in the access log')

pid = call('POST', '/patients', recep, {'name': 'Joana Ribeiro'}).json()['id']
call('PUT', f'/patients/{pid}/anamnesis', ther, {'main_complaint': 'Cervicalgia', 'goals': '', 'regions': [], 'red_flags': []})
n_server, n_pc = len(on_server), len(on_pc)
d = call('POST', f'/patients/{pid}/letters/ai-draft', owner, {'lang': 'pt'})
ok(d.status_code == 200 and d.json()['text'] == 'Rascunho do PC da IA.' and d.json()['model'] == 'gemma4:12b', f'the draft comes from the new computer and model, thinking removed: {d.text}')
ok(len(on_server) == n_server and any(p == '/api/generate' and b['model'] == 'gemma4:12b' for (_, p, b) in on_pc[n_pc:]), 'the old address was not used')

# Model only: keeps the server's address.
r = call('PUT', '/ai/settings', owner, {'url': '', 'model': 'qwen3:14b'})
s = call('GET', '/ai/settings', owner).json()
ok(s['url'] == 'http://127.0.0.1:11999' and not s['url_in_app'] and s['model'] == 'qwen3:14b', f'empty address = the server’s again: {s}')
ok(not r.json()['available'] and r.json()['reason'] == 'no_model', 'and says the server’s Ollama doesn’t have that model')
d = call('POST', f'/patients/{pid}/letters/ai-draft', owner, {'lang': 'pt'})
ok(d.status_code == 409 and d.json()['error'] == 'ai_no_model', 'a draft then says the model is missing')

# Offline: a clear code and the address it tried.
call('PUT', '/ai/settings', owner, {'url': '127.0.0.1:11997', 'model': 'gemma4:12b'})
d = call('POST', f'/patients/{pid}/letters/ai-draft', owner, {'lang': 'pt'})
ok(d.status_code == 409 and d.json()['error'] == 'ai_offline' and d.json()['details']['url'] == 'http://127.0.0.1:11997', f'offline says which address: {d.text}')

# Back to the server's settings.
call('PUT', '/ai/settings', owner, {'url': '', 'model': ''})
s = call('GET', '/ai/settings', owner).json()
ok(s['url'] == 'http://127.0.0.1:11999' and s['model'] == 'gemma3:12b' and not s['url_in_app'] and not s['model_in_app'], 'both empty = the server’s settings')
if PSQL:
    ok(sql("SELECT coalesce(ai_url,'-') || coalesce(ai_model,'-') FROM clinic") == '--', 'cleared in the database')

print(f'\n{fails} failed' if fails else '\nall passed')
sys.exit(1 if fails else 0)
