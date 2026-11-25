# End-to-end test of the assistant's schedule jobs and one-press booking of requests (11 Nov 2026).
# Needs a running server with an EMPTY database, started with OLLAMA_URL=http://127.0.0.1:11994 (this test runs a
# stand-in for Ollama there). PSQL_URL adds database checks; LOG = the server's log (no names, no prompts).
# Run: PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_agenda.py
import os, sys, json, re, threading, subprocess, requests, pyotp
from datetime import date, datetime, timedelta
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

# ── a stand-in for Ollama that answers whatever intent the test sets ──
asked = []
mode = {'intent': {'skill': 'answer', 'reply': 'Olá.'}}
class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.reply(200, {'models': [{'name': 'gemma4:12b'}]})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        asked.append(body)
        if self.path == '/api/show': return self.reply(200, {'details': {}})
        # This stand-in plays a model without tools (like gemma): the assistant falls back to its one-step mode (13 Nov 2026).
        if self.path == '/api/chat': return self.reply(400, {'error': 'registry.ollama.ai/library/gemma4:12b does not support tools'})
        self.reply(200, {'model': body.get('model'), 'done': True, 'response': json.dumps(mode['intent'])})
    def reply(self, code, out):
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers()
        self.wfile.write(data)
fake = HTTPServer(('127.0.0.1', 11994), Fake)
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

def newp(name, **kw):
    return call('POST', '/patients', recep, {'name': name, **kw}).json()['id']
maria = newp('Maria Silva', therapist_id=ana, phone='912 345 678')
maria2 = newp('Maria Santos', phone='934 000 111')
joao = newp('João Costa', email='joao@exemplo.pt')

# Days to work with: the next two weekdays at least two days from now (clinic open 08:00–20:00 by default).
def weekday_from(d):
    while d.weekday() >= 5: d += timedelta(days=1)
    return d
D = weekday_from(date.today() + timedelta(days=2))
D2 = weekday_from(D + timedelta(days=1))
def at(d, hm):  # local Lisbon time → ISO with the right offset, worked out by the database
    return sql(f"SELECT to_char(('{d} {hm}'::timestamp AT TIME ZONE 'Europe/Lisbon') AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')")
def appts(d=None):
    d = d or D
    return call('GET', f'/appointments?from={at(d - timedelta(days=1), "00:00")}&to={at(d + timedelta(days=40), "00:00")}&include_cancelled=true', owner).json()
def local(iso):
    return sql(f"SELECT to_char('{iso}'::timestamptz AT TIME ZONE 'Europe/Lisbon', 'YYYY-MM-DD HH24:MI')")
def book(pid, d, hm, mins=45, th=None, kind='session'):
    r = call('POST', '/appointments', owner, {'kind': kind, 'starts_at': at(d, hm), 'ends_at': at(d, f"{int(hm[:2]) + (int(hm[3:]) + mins) // 60:02d}:{(int(hm[3:]) + mins) % 60:02d}"),
                                               'therapist_id': th or ana, 'patient_id': pid, 'allow_overlap': True})
    return r.json()['ids'][0]

# ── who may use it: the AI switch ──
for who, tok in [('therapist without AI', ther), ('reception without AI', recep)]:
    ok(call('POST', '/ai/assistant', tok, {'message': 'olá'}).status_code == 403, f'{who}: assistant refused')
ok(not asked, 'nothing reached the AI for refused requests')
ok(call('PATCH', f'/staff/{rita}', owner, {'ai_enabled': True}).status_code == 200, 'the owner turns the AI switch on for reception')
ok(call('PATCH', f'/staff/{ana}', owner, {'ai_enabled': True}).status_code == 200, 'and for the therapist')
ok(call('GET', '/me', recep).json()['ai_enabled'] is True, '/me says reception now has AI')

def ask(tok, msg, intent, lang='pt', history=None):
    mode['intent'] = intent
    return call('POST', '/ai/assistant', tok, {'message': msg, 'lang': lang, 'history': history or []})

# ── what the AI sees ──
m1 = book(maria, D, '09:00')
r = ask(recep, 'O que temos amanhã?', {'skill': 'answer', 'reply': 'Olá.'})
p = asked[-1]['prompt']
ok(r.status_code == 200, 'reception with AI can ask')
ok('Agenda (' in p and re.search(r'A1 \| ' + str(D) + r' \| \w+ \| 09:00–09:45 \| session \| Maria Silva \(nº \d+\) \| Ana Rocha \| booked', p) is not None, 'the AI sees the agenda with refs, names, times and therapists')
ok('912' not in p and 'joao@exemplo.pt' not in p, 'never phone numbers or e-mails')
ok('Products' not in p and 'price' not in asked[-1]['system'].split('- book')[0], 'reception’s AI gets no price list and no owner jobs')
ok('price' not in json.dumps(asked[-1]['format']['properties']['skill']['enum']), 'the owner’s jobs aren’t even offered to reception’s AI')
r = ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
ok('Products' in asked[-1]['prompt'] and 'Agenda (' in asked[-1]['prompt'] and 'price' in asked[-1]['format']['properties']['skill']['enum'], 'the owner’s AI gets prices, staff and the agenda')
prods = {x['code']: x for x in call('GET', '/products', owner).json()}
r = ask(recep, 'Muda o preço da avaliação para 99 €', {'skill': 'price', 'product': 'assessment', 'price_eur': 99, 'reply': 'Aqui está.'})
ok(r.status_code == 200 and r.json()['change'] is None, 'reception asking for an owner job gets an answer, no change, whatever the AI says')
ok({x['code']: x for x in call('GET', '/products', owner).json()}['assessment']['price_cents'] == prods['assessment']['price_cents'], 'price untouched')

# ── book ──
r = ask(recep, 'Marca a Maria Silva quinta às 10h', {'skill': 'book', 'patient': 'Maria Silva', 'date': str(D), 'time': '10:00', 'reply': 'Aqui está a marcação.'})
c = r.json().get('change') or {}
ok(r.status_code == 200 and c.get('skill') == 'book' and c['rows'][0][0].startswith('Maria Silva (nº') and '10:00–10:45' in c['rows'][0][2] and 'Ana Rocha' in c['rows'][0][2],
   'book: preview with the patient, the time and her own therapist ' + str(c.get('rows')))
before = len(appts())
ok(before == 1, 'nothing booked before Apply')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", ther).status_code == 403, 'someone else can’t apply reception’s change')
r2 = call('POST', f"/assistant/changes/{c['id']}/apply", recep)
ok(r2.status_code == 200 and r2.json()['patient_id'] == maria, 'Apply books it')
new = [x for x in appts() if local(x['starts_at']) == f'{D} 10:00']
ok(len(new) == 1 and new[0]['patient_id'] == maria and new[0]['therapist_id'] == ana and new[0]['kind'] == 'session', 'the appointment is in the schedule')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", recep).status_code == 409, 'can’t apply twice')
log = call('GET', '/access-log?limit=20', owner).json()
ap = next((x for x in log if x['action'] == 'assistant.applied'), None)
ok(ap and ap.get('subject_id') == maria and 'Maria' not in ap['detail'] and 'book' in ap['detail'], 'logged with the patient as subject, no name in the detail')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 200 and not [x for x in appts() if local(x['starts_at']) == f'{D} 10:00'], 'Undo removes it')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 409, 'can’t undo twice')

# which Maria?
r = ask(recep, 'Marca a Maria quinta às 11h', {'skill': 'book', 'patient': 'Maria', 'date': str(D), 'time': '11:00', 'reply': 'Aqui está.'})
j = r.json()
ok(j['change'] is None and j['choices'] and len(j['choices']) == 2 and all('nº' in ch['send'] for ch in j['choices']), 'two Marias: asks which, with choices to tap')
r = ask(recep, j['choices'][1]['send'], {'skill': 'book', 'patient': j['choices'][1]['send'], 'date': str(D), 'time': '11:00', 'reply': 'Aqui está.'})
ok((r.json().get('change') or {}).get('rows', [[''] * 3])[0][0] == j['choices'][1]['send'], 'the tapped choice picks that one (by number)')
call('POST', f"/assistant/changes/{r.json()['change']['id']}/cancel", recep)

# no time: offers free times
r = ask(owner, 'Marca o João na quinta', {'skill': 'book', 'patient': 'João Costa', 'date': str(D), 'reply': 'Aqui está.'})
j = r.json()
ok(j['change'] is None and j['choices'] and j['choices'][0]['label'].endswith('Filipe Sousa') and '08:00' in j['choices'][0]['label'], 'no time: the free times as choices (owner first when the owner asks) ' + str(j['choices'][:2]))
ok(not any('09:00' in ch['label'] and 'Ana' in ch['label'] for ch in j['choices']), 'busy times aren’t offered')

# someone new, with a series
r = ask(owner, 'Marca o Rui Novo, 912 000 999, terças e quintas, 4 sessões, a começar quinta às 18h com a Ana',
        {'skill': 'book', 'patient': 'Rui Novo', 'phone': '912 000 999', 'date': str(D), 'time': '18:00', 'kind': 'session', 'therapist': 'Ana', 'repeat_weekdays': [2, 4], 'repeat_count': 4, 'reply': 'Aqui está.'})
c = r.json()['change']
ok(any('ainda não tem ficha' in w for w in c['warnings']) and c['rows'][0][0] == 'Rui Novo' and '4 marcações' in c['rows'][1][2], 'someone new: says a file will be made; the series is shown ' + str(c['rows']))
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200, 'Apply')
rui = [x for x in appts() if x.get('patient_name') == 'Rui Novo']
ok(len(rui) == 4 and len({x['series_id'] for x in rui}) == 1, 'a file and 4 appointments in one series')
if PSQL: ok(sql("SELECT phone FROM patients WHERE name = 'Rui Novo'") == '912 000 999', 'the new file has the phone')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 200 and not [x for x in appts() if x.get('patient_name') == 'Rui Novo'], 'Undo removes the series')
if PSQL: ok(sql("SELECT count(*) FROM patients WHERE name = 'Rui Novo'") == '0', 'and the file it made')

# overlaps and staleness
r = ask(owner, 'Marca o João quinta às 9h com a Ana', {'skill': 'book', 'patient': 'João Costa', 'date': str(D), 'time': '09:00', 'therapist': 'Ana Rocha', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(any('Sobrepõe-se a 1 marcação' in w and 'Maria Silva' in w for w in c['warnings']), 'a clash is a warning on the preview')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200, 'Apply books it anyway (as shown)')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)
r = ask(owner, 'Marca o João quinta às 12h com a Ana', {'skill': 'book', 'patient': 'João Costa', 'date': str(D), 'time': '12:00', 'therapist': 'Ana Rocha', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(not c['warnings'], 'a free time has no warnings')
x = book(maria2, D, '12:00')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 409, 'refused if that time was taken since the preview')
call('PATCH', f'/appointments/{x}', owner, {'status': 'cancelled'})
r = ask(owner, 'Marca o João sábado às 22h', {'skill': 'book', 'patient': 'João Costa', 'date': str(D), 'time': '21:00', 'reply': 'Aqui está.'})
ok(any('Fora do horário' in w for w in r.json()['change']['warnings']), 'outside opening hours: a warning')
r = ask(owner, 'Marca o João ontem', {'skill': 'book', 'patient': 'João Costa', 'date': str(date.today() - timedelta(days=1)), 'time': '10:00', 'reply': 'x'})
ok(r.json()['change'] is None and 'passou' in r.json()['reply'], 'a day that passed: asks again')

# ── understanding the ones you mean ──
def ref_of(name, hm, d=D):
    p = asked[-1]['prompt']
    m = re.search(r'(A\d+) \| ' + str(d) + r' \| \w+ \| ' + hm + r'–[^|]+\| \w+ \| ' + re.escape(name), p)
    return m.group(1) if m else None
j2 = book(joao, D2, '15:00')
ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
ref = ref_of('Maria Silva', '09:00')
ok(ref is not None, f'Maria’s 09:00 has a ref in the agenda ({ref})')
r = ask(owner, 'Passa a sessão da Maria para as 14h30', {'skill': 'move', 'appointment': ref, 'time': '14:30', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c['skill'] == 'move' and '09:00' in c['rows'][0][1] and '14:30–15:15' in c['rows'][0][2], 'move: before and after ' + str(c['rows']))
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200 and local(next(x for x in appts() if x['id'] == m1)['starts_at']) == f'{D} 14:30', 'Apply moves it')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 200 and local(next(x for x in appts() if x['id'] == m1)['starts_at']) == f'{D} 09:00', 'Undo puts it back')
r = ask(owner, 'Muda o João para sexta às 16h', {'skill': 'move', 'patient': 'João', 'date': str(D2), 'time': '16:00', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c and 'João Costa' in c['rows'][0][0] and '16:00' in c['rows'][0][2], 'without a ref: found by the patient’s name (and the day)')
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
call('PATCH', f'/appointments/{j2}', owner, {'starts_at': at(D2, '17:00'), 'ends_at': at(D2, '17:45')})
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 409, 'Undo refused once someone moved it again by hand')
m3 = book(maria, D2, '10:00')
r = ask(owner, 'Cancela a da Maria', {'skill': 'cancel', 'patient': 'Maria Silva', 'reply': 'Aqui está.'})
j = r.json()
ok(j['change'] is None and len(j['choices']) == 2, 'Maria has two: asks which')

# cancel / mark
ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
ra, rb = ref_of('Maria Silva', '09:00'), ref_of('Maria Silva', '10:00', D2)
r = ask(owner, 'Cancela as duas sessões da Maria', {'skill': 'cancel', 'refs': [ra, rb], 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c['skill'] == 'cancel' and len(c['rows']) == 2 and all(x[2] == 'Cancelada' for x in c['rows']), 'cancel two at once')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200 and all(x['status'] == 'cancelled' for x in appts() if x['id'] in (m1, m3)), 'Apply cancels both')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 200 and all(x['status'] == 'booked' for x in appts() if x['id'] in (m1, m3)), 'Undo books them again')
past = book(joao, date.today() - timedelta(days=1), '10:00')
ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
rp = ref_of('João Costa', '10:00', date.today() - timedelta(days=1))
r = ask(ther, 'O João faltou ontem', {'skill': 'mark', 'refs': [rp], 'status': 'no_show', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c and c['rows'][0][2] == 'Faltou', 'mark as missed (therapist with AI)')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", ther).status_code == 200, 'Apply')
st = {x['id']: x for x in call('GET', f'/appointments?from={at(date.today() - timedelta(days=2), "00:00")}&to={at(date.today(), "00:00")}&include_cancelled=true', owner).json()}
ok(st[past]['status'] == 'no_show', 'marked missed')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", ther).status_code == 200, 'Undo')
r = ask(owner, 'O João veio ontem', {'skill': 'mark', 'refs': [rp], 'status': 'done', 'reply': 'Aqui está.'})
c = r.json()['change']
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
st = {x['id']: x for x in call('GET', f'/appointments?from={at(date.today() - timedelta(days=2), "00:00")}&to={at(date.today(), "00:00")}&include_cancelled=true', owner).json()}
ok(st[past]['status'] == 'done' and st[past]['charge_cents'], 'done: charged like by hand')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)
st = {x['id']: x for x in call('GET', f'/appointments?from={at(date.today() - timedelta(days=2), "00:00")}&to={at(date.today(), "00:00")}&include_cancelled=true', owner).json()}
ok(st[past]['status'] == 'booked' and st[past]['charge_cents'] is None, 'Undo takes the charge away again')

# questions and free times
ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
ra = ref_of('Maria Silva', '09:00')
r = ask(owner, 'Quando é a próxima sessão da Maria?', {'skill': 'agenda', 'refs': [ra, 'A999'], 'reply': 'A Maria Silva vem na quinta às 9h.'})
j = r.json()
ok(j['reply'] == 'A Maria Silva vem na quinta às 9h.' and len(j['list']) == 1 and j['list'][0]['id'] == m1 and j['list'][0]['title'].startswith('Maria Silva'), 'a question: the AI’s answer, with the appointment checked by the server (unknown refs dropped)')
r = ask(owner, 'Há vagas quinta de manhã?', {'skill': 'free_slots', 'date': str(D), 'part': 'morning', 'reply': 'x'})
j = r.json()
ok(j['list'] and all(x['when'].split(', ')[1] < '13:00' for x in j['list']) and not any(x['when'].endswith('09:00') and x['title'] == 'Ana Rocha' for x in j['list']), 'free times in the morning, busy ones left out')

# ── booking requests ──
rq = call('POST', '/booking-requests', recep, {'name': 'Carla Nova', 'phone': '961 222 333', 'service': 'assessment', 'preferred_at': at(D2, '11:00'), 'message': 'Dor lombar'}).json()['id']
ask(owner, 'olá', {'skill': 'answer', 'reply': 'Olá.'})
ok(re.search(r'R1 \| Carla Nova \| assessment \| ' + str(D2) + r' \| 11:00 \| staff', asked[-1]['prompt']) is not None and 'Dor lombar' not in asked[-1]['prompt'] and '961' not in asked[-1]['prompt'],
   'the AI sees waiting requests (no message, no contacts)')
r = ask(recep, 'Aceita o pedido da Carla', {'skill': 'accept_request', 'request': 'R1', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c['skill'] == 'accept_request' and '11:00–12:00' in c['rows'][0][2] and any('ficha nova' in w for w in c['warnings']), 'accept: the time asked for, a new file, 60 min for an assessment ' + str(c['rows']))
ok(call('POST', f"/assistant/changes/{c['id']}/apply", recep).status_code == 200, 'Apply')
req = next(x for x in call('GET', '/booking-requests?status=accepted', recep).json() if x['id'] == rq)
carla = req['patient_id']
ok(req['appointment_id'] and carla and req['created_patient'] is True, 'request accepted, booked, file made')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 200 and any(x['id'] == rq for x in call('GET', '/booking-requests', recep).json()), 'Undo: the request waits again')
if PSQL: ok(sql(f"SELECT count(*) FROM patients WHERE id = '{carla}'") == '0', 'and the file it made is gone')
r = ask(recep, 'Recusa o pedido da Carla', {'skill': 'decline_request', 'patient': 'Carla', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c and c['rows'][0][2] == 'Recusado' and call('POST', f"/assistant/changes/{c['id']}/apply", recep).status_code == 200, 'decline (found by name)')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", recep).status_code == 200, 'Undo')
r = ask(recep, 'Põe a Maria Santos na lista de espera, segundas e quartas de manhã', {'skill': 'waitlist_add', 'patient': 'Maria Santos', 'repeat_weekdays': [1, 3], 'part': 'morning', 'reply': 'Aqui está.'})
c = r.json()['change']
ok(c and 'seg, qua, 08:00–13:00' in c['rows'][0][2], 'waitlist: preview ' + str(c and c['rows']))
call('POST', f"/assistant/changes/{c['id']}/apply", recep)
w = call('GET', '/waitlist', recep).json()
ok(len(w) == 1 and w[0]['patient_id'] == maria2 and w[0]['weekdays'] == [1, 3], 'on the waitlist')
call('POST', f"/assistant/changes/{c['id']}/undo", recep)
ok(call('GET', '/waitlist', recep).json() == [], 'Undo takes them off')

# ── one press ──
s = call('GET', f'/booking-requests/{rq}/suggest', recep).json()
ok(s['slot'] and local(s['slot']['starts_at']) == f'{D2} 11:00' and s['minutes'] == 60 and s['kind'] == 'assessment' and s['patient'] is None and s['lookalikes'] == [] and len(s['alternatives']) >= 2,
   'suggest: the time asked for, 60 min, no file yet ' + str(s['slot']))
r = call('POST', f'/booking-requests/{rq}/book', recep, {'starts_at': s['slot']['starts_at'], 'therapist_id': s['slot']['therapist_id']})
j = r.json()
ok(r.status_code == 200 and j['patient_created'] is True, 'one press: books and makes the file')
a1 = next(x for x in appts(D2) if x['id'] == j['appointment_id'])
ok(a1['patient_name'] == 'Carla Nova' and a1['kind'] == 'assessment' and a1['admin_note'] == 'Dor lombar', 'the appointment has the patient, the type and the message as a note')
if PSQL: ok(sql(f"SELECT phone FROM patients WHERE id = '{j['patient_id']}'") == '961 222 333', 'the file has the request’s phone')
ok(call('POST', f'/booking-requests/{rq}/book', recep, {'starts_at': s['slot']['starts_at'], 'therapist_id': s['slot']['therapist_id']}).status_code == 409, 'can’t book twice')
r = call('POST', f'/booking-requests/{rq}/reopen', recep)
ok(r.status_code == 200 and r.json()['patient_removed'] is True, 'Undo (Reopen) removes the appointment and the new file')
# looks like someone we know
rq2 = call('POST', '/booking-requests', recep, {'name': 'Maria S.', 'phone': '+351 912345678', 'preferred_at': at(D, '09:00')}).json()['id']
s = call('GET', f'/booking-requests/{rq2}/suggest', recep).json()
ok([l['id'] for l in s['lookalikes']] == [maria] and s['lookalikes'][0]['reason'] == 'phone', 'a known phone: shown as a lookalike')
ok(s['slot'] and s['slot']['therapist_id'] != ana or local(s['slot']['starts_at']) != f'{D} 09:00', 'the suggestion avoids Ana’s busy 09:00')
r = call('POST', f'/booking-requests/{rq2}/book', recep, {'starts_at': s['slot']['starts_at'], 'therapist_id': s['slot']['therapist_id'], 'patient_id': maria})
ok(r.status_code == 200 and r.json()['patient_created'] is False and r.json()['patient_id'] == maria, 'book with the existing file: no new one')
r = call('POST', f'/booking-requests/{rq2}/reopen', recep)
ok(r.status_code == 200 and r.json()['patient_removed'] is False and call('GET', f'/patients/{maria}', owner).status_code == 200, 'Undo keeps a file it didn’t make')
# website request: morning, closure moves it on
if PSQL:
    sql(f"INSERT INTO booking_requests (id, name, phone, service, preferred_at, source, day_part) VALUES ('11111111-1111-4111-8111-111111111111', 'Site Pessoa', '930000000', 'session', '{D2} 12:00', 'website', 'afternoon')")
    s = call('GET', '/booking-requests/11111111-1111-4111-8111-111111111111/suggest', recep).json()
    ok(s['slot'] and local(s['slot']['starts_at']) >= f'{D2} 13:00' and local(s['slot']['starts_at']).startswith(str(D2)), 'website "afternoon": from 13:00 that day ' + str(s['slot'] and local(s['slot']['starts_at'])))
    ok(call('POST', '/closures', owner, {'starts_on': str(D2), 'ends_on': str(D2), 'label': 'Formação'}).status_code == 200, 'close the clinic that day')
    s = call('GET', '/booking-requests/11111111-1111-4111-8111-111111111111/suggest', recep).json()
    ok(s['slot'] and local(s['slot']['starts_at']) > f'{D2} 23:59' and local(s['slot']['starts_at'])[11:] >= '13:00', 'closed that day: the next afternoon that fits ' + str(s['slot'] and local(s['slot']['starts_at'])))
ok(call('GET', f'/booking-requests/{rq}/suggest', ther).status_code == 200, 'everyone who books can use one press (therapist)')

# ── safety ──
ok(call('PATCH', f'/staff/{rita}', owner, {'ai_enabled': False}).status_code == 200 and call('POST', '/ai/assistant', recep, {'message': 'olá'}).status_code == 403, 'switch off: assistant refused again')
if LOG:
    text = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Maria' not in text and 'Carla' not in text and 'Agenda (' not in text, 'server log has no names and no prompts')
print(f'\n{fails} failed' if fails else '\nall passed')
