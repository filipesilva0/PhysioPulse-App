# End-to-end test of the assistant with tools (13 Nov 2026): it looks things up through the app's own endpoints as
# the person asking (same reach as their role), every change waits for Apply, dates are worked out by the server.
# Needs a running server with an EMPTY database, started with OLLAMA_URL=http://127.0.0.1:11993 (this test runs a
# stand-in for Ollama there that plays a model with tools, following a script). PSQL_URL adds database checks;
# LOG = the server's log (no names, no prompts).
# Run: PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_agent.py
import os, sys, json, re, threading, subprocess, requests, pyotp
from datetime import date, timedelta
from http.server import BaseHTTPRequestHandler, HTTPServer

DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Rececao', 'X-Device-Key': DEVICE_KEY}
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

# ── a stand-in for Ollama with tools: answers each /api/chat from a script ──
chats = []          # every /api/chat request body
script = []         # what to answer next: {'calls': [(name, args)]} or {'text': '…'} or {'raw': '…'}
class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.reply(200, {'models': [{'name': 'qwen3:8b'}, {'name': 'gemma4:12b'}]})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/api/show': return self.reply(200 if body['model'] in ('qwen3:8b', 'gemma4:12b') else 404, {'details': {}})
        if self.path == '/api/chat':
            chats.append(body)
            step = script.pop(0) if script else {'text': 'Pronto.'}
            if 'calls' in step:
                msg = {'role': 'assistant', 'content': '', 'tool_calls': [{'function': {'name': n, 'arguments': a}} for n, a in step['calls']]}
            elif 'raw' in step:
                msg = {'role': 'assistant', 'content': step['raw']}
            else:
                msg = {'role': 'assistant', 'content': step['text']}
            # Streamed, like Ollama: the text in two pieces, then the end.
            lines = [{'message': {'role': 'assistant', 'content': msg['content'][:3]}, 'done': False},
                     {'message': {**msg, 'content': msg['content'][3:]}, 'done': False},
                     {'message': {'role': 'assistant', 'content': ''}, 'done': True}]
            data = ''.join(json.dumps(l) + '\n' for l in lines).encode()
            self.send_response(200); self.send_header('Content-Type', 'application/x-ndjson'); self.send_header('Content-Length', str(len(data))); self.end_headers()
            return self.wfile.write(data)
        self.reply(200, {'done': True, 'response': 'Resumo do mês.'})
    def reply(self, code, out):
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers()
        self.wfile.write(data)
fake = HTTPServer(('127.0.0.1', 11993), Fake)
threading.Thread(target=fake.serve_forever, daemon=True).start()

# ── people ──
r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Sousa', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email, name):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': name, 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt', 'Ana Rocha')
recep = join('reception', 'rita@clinica.pt', 'Rita Lopes')
staff = {s['name']: s for s in call('GET', '/staff', owner).json()}
ana, rita, filipe = staff['Ana Rocha']['id'], staff['Rita Lopes']['id'], staff['Filipe Sousa']['id']
for who in (ana, rita):
    call('PATCH', f'/staff/{who}', owner, {'ai_enabled': True})

def newp(name, **kw):
    return call('POST', '/patients', recep, {'name': name, **kw}).json()['id']
celes = newp('Celestina Marques', therapist_id=ana, phone='912 345 678', email='celestina@exemplo.pt')
maria = newp('Maria Silva', phone='934 000 111')
maria2 = newp('Maria Santos')
num = {p['id']: p['number'] for p in call('GET', '/patients', owner).json()}
call('PUT', f'/patients/{celes}/anamnesis', ther, {'main_complaint': 'Dor lombar há 3 semanas', 'goals': 'Voltar a caminhar 30 min'})
n = call('POST', f'/patients/{celes}/notes', ther, {'text': 'Sessão com terapia manual lombar; dor desceu de 6 para 4.', 'blocks': [{'type': 'slider', 'label': 'Dor', 'value': 4, 'max': 10}]}).json()['id']
call('POST', f'/notes/{n}/sign', ther)

def weekday_from(d):
    while d.weekday() >= 5: d += timedelta(days=1)
    return d
TODAY = date.fromisoformat(sql("SELECT ((now() AT TIME ZONE 'Europe/Lisbon')::date)::text"))
TOMORROW = TODAY + timedelta(days=1)
D = weekday_from(TODAY + timedelta(days=2))
def at(d, hm):
    return sql(f"SELECT to_char(('{d} {hm}'::timestamp AT TIME ZONE 'Europe/Lisbon') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')")
a1 = call('POST', '/appointments', owner, {'kind': 'session', 'starts_at': at(D, '09:00'), 'ends_at': at(D, '09:45'), 'therapist_id': ana, 'patient_id': maria}).json()['ids'][0]

def ask(tok, msg, steps, lang='pt', history=None):
    chats.clear(); script.clear(); script.extend(steps)
    return call('POST', '/ai/assistant', tok, {'message': msg, 'lang': lang, 'history': history or []})
def tool_names(body):
    return {t['function']['name'] for t in body['tools']}
def tool_results(body):
    return [m['content'] for m in body['messages'] if m['role'] == 'tool']
def dm(d):
    return d.strftime('%d/%m')

# ── what each person's assistant can use (same reach as their role in the app) ──
r = ask(owner, 'olá', [{'text': 'Olá! Em que posso ajudar?'}])
j = r.json()
ok(r.status_code == 200 and j['reply'] == 'Olá! Em que posso ajudar?' and j['change'] is None, 'a plain answer comes back as the reply')
body = chats[0]
ok(body['model'] == 'gemma4:12b' or body['model'], 'the request names a model')
sysmsg = body['messages'][0]['content']
ok(body['messages'][0]['role'] == 'system' and 'Filipe Sousa' in sysmsg and "owner" in sysmsg, 'the brief says who is asking and their role')
ok(f'{TODAY.isoformat()}' in sysmsg and 'today/hoje' in sysmsg and f'{TOMORROW.isoformat()}' in sysmsg and 'tomorrow/amanhã' in sysmsg, 'the brief has the calendar with today and tomorrow worked out')
ok('Ana Rocha' in sysmsg and 'European Portuguese' in sysmsg, 'the brief lists the therapists and the language')
ok('912' not in json.dumps(body) and 'Celestina' not in json.dumps(body), 'nothing about patients is sent until a tool looks it up')
own = tool_names(body)
ok({'search_patients', 'open_patient', 'agenda', 'book', 'update_anamnesis', 'write_note', 'price', 'month_summary'} <= own, 'the owner gets every tool ' + str(sorted(own)))
ask(ther, 'olá', [{'text': 'Olá.'}])
th = tool_names(chats[0])
ok('update_anamnesis' in th and 'write_note' in th and 'price' not in th and 'month_summary' not in th and 'staff_change' not in th, 'a therapist: clinical tools, no money or staff')
ask(recep, 'olá', [{'text': 'Olá.'}])
rc = tool_names(chats[0])
ok('book' in rc and 'open_patient' in rc and 'update_anamnesis' not in rc and 'write_note' not in rc and 'price' not in rc, 'reception: schedule and contacts, no clinical tools')
ok('NEVER clinical information' in chats[0]['messages'][0]['content'], 'reception’s brief says no clinical information')

# ── the chat that went wrong (owner, 13 Nov 2026) now works ──
r = ask(owner, 'marca a utente celestina amanhã às 2 da tarde para o fisioterapeuta filipe', [
    {'calls': [('search_patients', {'query': 'celestina'})]},
    {'calls': [('book', {'patient': 'Celestina Marques', 'date': 'amanhã', 'time': '14:00', 'therapist': 'Filipe'})]},
])
j = r.json()
res = tool_results(chats[1])
ok(len(chats) == 2 and res and f"nº {num[celes]} · Celestina Marques" in res[0] and 'Ana Rocha' in res[0], 'search_patients finds her (number, therapist) ' + (res[0][:120] if res else ''))
c = j.get('change') or {}
ok(c.get('skill') == 'book' and c['rows'][0][0].startswith('Celestina Marques (nº') and dm(TOMORROW) in c['rows'][0][2] and '14:00' in c['rows'][0][2] and 'Filipe Sousa' in c['rows'][0][2],
   '“amanhã às 2 da tarde com o Filipe” → a preview for tomorrow 14:00 with Filipe ' + str(c.get('rows')))
ok(j['steps'] and 'celestina' in j['steps'][0], 'the answer says what it looked up ' + str(j['steps']))
ok('Celestina' in j['memo'], 'and carries a memo for the next message')
before = sql(f"SELECT count(*) FROM appointments WHERE patient_id = '{celes}'")
ok(before == '0', 'nothing booked before Apply')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200 and sql(f"SELECT count(*) FROM appointments WHERE patient_id = '{celes}' AND therapist_id = '{filipe}'") == '1', 'Apply books it with Filipe')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)

# a day that has passed: the tool says so and the model can ask
r = ask(owner, 'marca a Celestina ontem às 10h', [
    {'calls': [('book', {'patient': 'Celestina Marques', 'date': (TODAY - timedelta(days=1)).isoformat(), 'time': '10:00'})]},
    {'text': 'Esse dia já passou. Para que dia quer?'},
])
res = tool_results(chats[1])
ok(res and 'has passed' in res[0] and TODAY.isoformat() in res[0] and r.json()['change'] is None, 'a past day: the model is told (with today’s date) and nothing is prepared')
# today is NOT past
r = ask(owner, 'marca a Celestina hoje às 23h', [{'calls': [('book', {'patient': 'Celestina Marques', 'date': TODAY.isoformat(), 'time': '23:00'})]}])
ok('has passed' not in json.dumps(chats) and (r.json().get('change') or {}).get('skill') == 'book', 'today is never “a day that has passed”: the booking is prepared')
if r.json().get('change'): call('POST', f"/assistant/changes/{r.json()['change']['id']}/cancel", owner)

# a call written as text (some models do that) still counts
r = ask(owner, 'procura a Maria', [{'raw': '<tool_call>{"name": "search_patients", "arguments": {"query": "Maria"}}</tool_call>'}, {'text': 'Há duas Marias: Maria Silva e Maria Santos.'}])
ok(len(chats) == 2 and 'Maria Santos' in tool_results(chats[1])[0] and r.json()['reply'].startswith('Há duas'), 'a tool call written as text is run too')

# two Marias: a booking asks which, with choices to tap
r = ask(owner, 'marca a Maria quinta às 11h', [{'calls': [('book', {'patient': 'Maria', 'date': D.isoformat(), 'time': '11:00'})]}])
j = r.json()
ok(j['change'] is None and j['choices'] and len(j['choices']) == 2, 'two Marias: asks which, with choices')

# ── reading a patient file: the app’s own endpoint, as the person (access log, role) ──
r = ask(owner, 'como está a Celestina?', [{'calls': [('open_patient', {'patient': 'Celestina', 'notes': 5})]}, {'text': 'Está a melhorar: a dor desceu de 6 para 4.'}])
res = tool_results(chats[1])[0]
ok('Dor lombar há 3 semanas' in res and 'terapia manual lombar' in res and 'Dor: 4.0/10' in res or 'Dor: 4/10' in res, 'owner: the file has the anamnesis and the notes ' + res[:200])
ok('912 345 678' in res and 'Ana Rocha' in res, 'and the contacts and therapist')
ok(r.json()['steps'] == [f'Abriu a ficha de Celestina Marques (nº {num[celes]})'], 'step in Portuguese: opened the file ' + str(r.json()['steps']))
log = call('GET', '/access-log?limit=30', owner).json()
v = [x for x in log if x['action'] == 'patient.viewed' and x.get('subject_id') == celes]
ok(v and v[0]['detail'] == 'full' and v[0]['staff_name'] == 'Filipe Sousa' if 'staff_name' in v[0] else v and v[0]['detail'] == 'full', 'the access log has the owner opening her file (full)')
r = ask(recep, 'qual é o telefone da Celestina?', [{'calls': [('open_patient', {'patient': 'Celestina'})]}, {'text': 'É o 912 345 678.'}])
res = tool_results(chats[1])[0]
ok('912 345 678' in res and 'Dor lombar' not in res and 'terapia manual' not in res and 'Anamnesis' not in res and 'Progress notes' not in res, 'reception: contacts only, no clinical information, whatever the model asks')
log = call('GET', '/access-log?limit=30', owner).json()
ok(any(x['action'] == 'patient.viewed' and x.get('subject_id') == celes and x['detail'] == 'reduced' for x in log), 'logged as a reduced view for reception')
r = ask(recep, 'escreve uma nota', [{'calls': [('write_note', {'patient': 'Celestina', 'text': 'Nota inventada pela receção.'})]}, {'text': 'Não posso escrever notas clínicas.'}])
ok('no tool called' in tool_results(chats[1])[0] and r.json()['change'] is None, 'reception’s model calling a clinical tool anyway: refused, nothing prepared')

# ── the agenda and free times ──
r = ask(recep, 'o que temos na quinta?', [{'calls': [('agenda', {'from': D.isoformat()})]}, {'text': 'Há uma sessão: Maria Silva às 9:00 com a Ana.'}])
res = tool_results(chats[1])[0]
ref = a1.replace('-', '')[:8]
ok(f'ref {ref}' in res and '09:00–09:45' in res and 'Maria Silva' in res and 'Ana Rocha' in res, 'agenda: each line with its ref ' + res[:160])
ok(r.json()['list'] and r.json()['list'][0]['title'].startswith('Maria Silva'), 'the appointments also come back as a list to tap')
r = ask(recep, 'muda a da Maria para as 11h', [{'calls': [('move_appointment', {'ref': ref, 'time': '11:00'})]}], history=[{'who': 'me', 'text': 'o que temos na quinta?'}, {'who': 'bot', 'text': 'Há uma sessão.', 'memo': res}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'move' and '11:00' in c['rows'][0][2], 'move by the ref the agenda showed ' + str(c.get('rows')))
ok(any(m['role'] == 'system' and 'What you looked up' in m['content'] and ref in m['content'] for m in chats[0]['messages'][1:]), 'the memo of the last answer goes back to the model')
call('POST', f"/assistant/changes/{c['id']}/cancel", recep)
r = ask(recep, 'cancela a A99', [{'calls': [('cancel_appointments', {'refs': ['zz99']})]}, {'text': 'Não encontrei essa marcação.'}])
ok('Use the refs' in tool_results(chats[1])[0] and r.json()['change'] is None, 'an unknown ref: told to look it up, nothing prepared')
r = ask(owner, 'há vagas quinta de manhã?', [{'calls': [('free_times', {'date': D.isoformat(), 'part': 'morning'})]}, {'text': 'Sim, às 8:00.'}])
ok('08:00' in tool_results(chats[1])[0] and r.json()['list'], 'free times: for the model and as a list')

# ── patient details: preview, Apply as the person, Undo ──
r = ask(recep, 'o novo telemóvel da Celestina é 961 222 333', [{'calls': [('update_patient', {'patient': 'Celestina Marques', 'phone': '961 222 333'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'patient_update' and c['rows'][0][1] == '912 345 678' and c['rows'][0][2] == '961 222 333', 'update_patient: before → after ' + str(c.get('rows')))
ok(sql(f"SELECT phone FROM patients WHERE id = '{celes}'") == '912 345 678', 'unchanged before Apply')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", ther).status_code == 403, 'someone else can’t apply it')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", recep).status_code == 200 and sql(f"SELECT phone FROM patients WHERE id = '{celes}'") == '961 222 333', 'Apply changes the phone')
log = call('GET', '/access-log?limit=10', owner).json()
ok(any(x['action'] == 'patient.updated' and x.get('subject_id') == celes for x in log) and any(x['action'] == 'assistant.applied' and x['detail'] == 'patient_update' for x in log), 'logged as the app would (patient.updated) and as an assistant change')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 200 and sql(f"SELECT phone FROM patients WHERE id = '{celes}'") == '912 345 678', 'Undo puts the old phone back')
r = ask(recep, 'muda o email', [{'calls': [('update_patient', {'patient': 'Celestina Marques', 'email': 'nova@exemplo.pt'})]}])
c = r.json()['change']
call('PATCH', f'/patients/{celes}', recep, {'email': 'outra@exemplo.pt'})
ok(call('POST', f"/assistant/changes/{c['id']}/apply", recep).status_code == 409, 'changed by hand meanwhile: Apply refused')
r = ask(recep, 'muda nada', [{'calls': [('update_patient', {'patient': 'Celestina Marques', 'email': 'outra@exemplo.pt'})]}, {'text': 'Já tem esse email.'}])
ok('Nothing would change' in tool_results(chats[1])[0], 'no real change: the model is told')
r = ask(recep, 'a Celestina passa para o Filipe', [{'calls': [('update_patient', {'patient': 'Celestina Marques', 'therapist': 'Filipe'})]}])
c = r.json()['change']
ok(c['rows'][0][1] == 'Ana Rocha' and c['rows'][0][2] == 'Filipe Sousa', 'therapist by name: shown as names')
call('POST', f"/assistant/changes/{c['id']}/cancel", recep)

# new patient
r = ask(recep, 'cria a ficha do Rui Pereira, 965 111 222', [{'calls': [('new_patient', {'name': 'Rui Pereira', 'phone': '965 111 222', 'birth_date': '3/4/1980'})]}])
c = r.json()['change']
ok(c['skill'] == 'patient_create' and c['rows'][0][0] == 'Rui Pereira' and '965 111 222' in c['rows'][0][2] and '1980-04-03' in c['rows'][0][2], 'new_patient: preview (birth date worked out) ' + str(c['rows']))
ok(sql("SELECT count(*) FROM patients WHERE name = 'Rui Pereira'") == '0', 'no file before Apply')
j = call('POST', f"/assistant/changes/{c['id']}/apply", recep).json()
ok(j.get('patient_id') and sql("SELECT phone || ' ' || birth_date FROM patients WHERE name = 'Rui Pereira'") == '965 111 222 1980-04-03', 'Apply creates the file')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 200 and sql("SELECT count(*) FROM patients WHERE name = 'Rui Pereira'") == '0', 'Undo removes the unused file')
r = ask(recep, 'cria a Celestina outra vez', [{'calls': [('new_patient', {'name': 'Celestina Marques', 'phone': '912 345 678'})]}])
ok(any('já tem ficha' in w for w in r.json()['change']['warnings']), 'someone who already has a file: a warning on the preview')
r = ask(recep, 'cria o Rui', [{'calls': [('new_patient', {'name': 'Rui'})]}, {'text': 'Qual é o nome completo?'}])
ok('full name' in tool_results(chats[1])[0], 'a first name only: asks for the full name')

# ── clinical changes (owner and therapists) ──
r = ask(ther, 'acrescenta à medicação: ibuprofeno 400 mg', [{'calls': [('update_anamnesis', {'patient': 'Celestina', 'medication': 'Ibuprofeno 400 mg SOS'})]}])
c = r.json()['change']
ok(c['skill'] == 'anamnesis' and c['rows'][0][1] == '—' and c['rows'][0][2] == 'Ibuprofeno 400 mg SOS', 'anamnesis: preview of the field that changes')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", ther).status_code == 200 and sql(f"SELECT medication || '|' || main_complaint FROM anamnesis WHERE patient_id = '{celes}'") == 'Ibuprofeno 400 mg SOS|Dor lombar há 3 semanas', 'Apply changes that field only')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", ther).status_code == 200 and sql(f"SELECT medication FROM anamnesis WHERE patient_id = '{celes}'") == '', 'Undo puts it back')
r = ask(ther, 'escreve a nota de hoje', [{'calls': [('write_note', {'patient': 'Celestina', 'text': '**Nota**\nSessão de exercício terapêutico; tolerou bem e a dor manteve-se em 4/10.'})]}])
c = r.json()['change']
ok(c['skill'] == 'note_draft' and 'exercício terapêutico' in c['rows'][0][2] and '**' not in c['rows'][0][2], 'write_note: a draft preview, markdown removed')
ok(any('rascunho' in w for w in c['warnings']), 'the preview says it stays an unsigned draft')
before = sql(f"SELECT count(*) FROM notes WHERE patient_id = '{celes}'")
ok(call('POST', f"/assistant/changes/{c['id']}/apply", ther).status_code == 200, 'Apply saves it')
row = sql(f"SELECT signed_at IS NULL, ai_assisted, created_by FROM notes WHERE patient_id = '{celes}' ORDER BY created_at DESC LIMIT 1")
ok(row == f't|t|{ana}', 'saved unsigned, marked AI, by the therapist ' + row)
ok(call('POST', f"/assistant/changes/{c['id']}/undo", ther).status_code == 200 and sql(f"SELECT count(*) FROM notes WHERE patient_id = '{celes}'") == before, 'Undo deletes the draft')

# ── owner jobs still work through tools ──
r = ask(owner, 'fecha a clínica dia 24 de dezembro', [{'calls': [('close_clinic', {'from': '24 de dezembro', 'reason': 'Natal'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'closure' and '24/12' in c['rows'][0][0] and 'Natal' in c['rows'][0][2], 'close_clinic with a spoken date ' + str(c.get('rows')))
call('POST', f"/assistant/changes/{c['id']}/cancel", owner)
r = ask(ther, 'muda o preço', [{'calls': [('price', {'product': 'assessment', 'price_eur': 99})]}, {'text': 'Não posso.'}])
ok('no tool called' in tool_results(chats[1])[0] and r.json()['change'] is None, 'a therapist’s model calling an owner tool: refused')

# ── giving up instead of looping ──
r = ask(owner, 'faz qualquer coisa', [{'calls': [('move_appointment', {'ref': 'ffffff99', 'time': '10:00'})]}] * 6)
ok(r.status_code == 200 and r.json()['change'] is None and len(chats) <= 5, 'a model that keeps failing stops after a few tries with a sentence')

# ── the assistant's own model (Settings › Local AI) ──
s = call('PUT', '/ai/settings', owner, {'assistant_model': 'qwen3:8b'}).json()
ok(s['assistant']['model'] == 'qwen3:8b' and s['assistant']['own'] is True and s['assistant']['available'] is True, 'the owner sets a model for the assistant')
ok(call('GET', '/ai/settings', owner).json()['assistant_model'] == 'qwen3:8b' and call('GET', '/ai/settings', owner).json()['model'] != 'qwen3:8b', 'the main model stays as it was')
ask(owner, 'olá', [{'text': 'Olá.'}])
ok(chats[0]['model'] == 'qwen3:8b', 'the assistant uses it')
ok(call('GET', '/ai/status', recep).json()['assistant']['model'] == 'qwen3:8b', 'status shows the assistant’s model to whoever uses it')
ok(call('PUT', '/ai/settings', ther, {'assistant_model': 'x'}).status_code == 403, 'only the owner changes it')
ok(call('POST', '/ai/test', owner, {'assistant_model': 'nao-existe:1b'}).json()['available'] is False, 'testing a model that isn’t installed says so')
s = call('PUT', '/ai/settings', owner, {'assistant_model': ''}).json()
ok(s['assistant']['own'] is False, 'empty: back to the main model')

# English
r = ask(owner, 'what do we have on Thursday?', [{'calls': [('agenda', {'from': 'Thursday'})]}, {'text': 'One session.'}], lang='en')
ok('British English' in chats[0]['messages'][0]['content'] and r.json()['steps'][0].startswith('Looked at the schedule'), 'English: brief and steps in English')

# ── nothing about patients in the server log ──
if LOG:
    text = open(LOG).read()
    ok(not re.search(r'Celestina|Maria Silva|Dor lombar|961 222|ibuprofeno|exercício', text, re.I), 'server log has no names, notes or phone numbers')

print('\nall passed' if not fails else f'\n{fails} failed')
sys.exit(1 if fails else 0)
