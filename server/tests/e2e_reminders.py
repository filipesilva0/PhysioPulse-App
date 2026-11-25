# End-to-end test of hand-sent reminders: the day's list, marking sent/undo, the message template, roles.
# Needs a running server with an EMPTY database. PSQL_URL adds checks in the database; LOG = server log file.
# Run: PSQL_URL=postgres://… python3 server/tests/e2e_reminders.py
import os, sys, subprocess, requests, pyotp
from datetime import datetime, timedelta

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Rececao-1', 'X-Device-Key': DEVICE_KEY}
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m)
    if not c: fails += 1
def call(method, path, tok=None, json=None):
    h = dict(H)
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + path, json=json, headers=h)
def sql(q):
    return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    v = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()
    return v['token'], v['staff']['id']
ther, ther_id = join('therapist', 'ana@clinica.pt')
recep, _ = join('reception', 'rita@clinica.pt')

def patient(name, **kw):
    return call('POST', '/patients', recep, {'name': name, **kw}).json()['id']
maria = patient('Maria Santos', phone='912 345 678', lang='pt')
john = patient('John Smith', phone='+44 7700 900123', lang='en', email='john@example.com', reminders='email')
kid = patient('Tomás Lopes', guardian_name='Ana Lopes', guardian_phone='936 111 222', birth_date='2015-02-01')
none = patient('Rui Alves', phone='961 000 000', reminders='none')

tomorrow = (datetime.now() + timedelta(days=1)).replace(minute=0, second=0, microsecond=0)
def at(h): return tomorrow.replace(hour=h).astimezone()
def book(pid, h, kind='session', **kw):
    s = at(h)
    body = {'kind': kind, 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': ther_id, 'allow_overlap': True, **kw}
    if pid: body['patient_id'] = pid
    r = call('POST', '/appointments', recep, body); assert r.status_code == 200, r.text
    return r.json()['ids'][0]
a_maria = book(maria, 9)
a_john = book(john, 10, 'assessment')
a_kid = book(kid, 11)
a_none = book(none, 12)
cls = book(None, 18, 'class', title='Pilates clínico', capacity=6)
call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': maria})
cancelled = book(maria, 15)
call('PATCH', f'/appointments/{cancelled}', recep, {'status': 'cancelled'})
today_appt = book(maria, 8)  # moved to today below
call('PATCH', f'/appointments/{today_appt}', recep, {'starts_at': (at(8) - timedelta(days=1)).isoformat(), 'ends_at': (at(8) - timedelta(days=1, minutes=-45)).isoformat()})

# ── the list ──
r = call('GET', '/reminders', recep)
ok(r.status_code == 200, 'reception lists tomorrow’s reminders')
d = r.json()
items = d['items']
ok(d['day'] == tomorrow.strftime('%Y-%m-%d'), 'default day is tomorrow: ' + d['day'])
ok(d['clinic'] == 'Clínica Teste' and '{nome}' in d['template_pt'] and '{name}' in d['template_en'], 'clinic name and default messages')
ok([i['name'] for i in items] == ['Maria Santos', 'John Smith', 'Tomás Lopes', 'Rui Alves', 'Maria Santos'], 'booked sessions and class attendees, in time order: ' + str([i['name'] for i in items]))
ok(not any(i['appointment_id'] in (cancelled, today_appt) for i in items), 'cancelled and other days left out')
kid_i = next(i for i in items if i['patient_id'] == kid)
ok(kid_i['phone'] == '936 111 222', 'minor without a phone → guardian’s phone')
john_i = next(i for i in items if i['patient_id'] == john)
ok(john_i['lang'] == 'en' and john_i['prefers'] == 'email' and john_i['email'] == 'john@example.com', 'language, preferred channel and email come along')
ok(next(i for i in items if i['patient_id'] == none)['prefers'] == 'none', 'patients who don’t want reminders are marked')
c = next(i for i in items if i['appointment_id'] == cls)
ok(c['kind'] == 'class' and c['title'] == 'Pilates clínico', 'class reminder carries the class name')
ok(all(i['sent_at'] is None for i in items), 'nothing sent yet')
ok('anamnesis' not in str(d) and 'main_complaint' not in str(d), 'nothing clinical in the list')
ok(call('GET', '/reminders?day=2026-13', recep).status_code == 400, 'bad day → 400')
ok(len(call('GET', f"/reminders?day={(tomorrow - timedelta(days=1)).strftime('%Y-%m-%d')}", ther).json()['items']) == 1, 'therapist can list another day (today: 1)')

# ── marking ──
ok(call('POST', '/reminders/sent', recep, {'appointment_id': a_maria, 'patient_id': maria, 'channel': 'whatsapp', 'sent': True}).status_code == 200, 'mark sent (WhatsApp)')
ok(call('POST', '/reminders/sent', recep, {'appointment_id': cls, 'patient_id': maria, 'channel': 'sms', 'sent': True}).status_code == 200, 'mark a class attendee sent (SMS)')
ok(call('POST', '/reminders/sent', recep, {'appointment_id': a_maria, 'patient_id': john, 'channel': 'sms', 'sent': True}).status_code == 404, 'patient not on that appointment → 404')
ok(call('POST', '/reminders/sent', recep, {'appointment_id': a_john, 'patient_id': john, 'channel': 'pigeon', 'sent': True}).status_code == 400, 'unknown channel → 400')
items = call('GET', '/reminders', recep).json()['items']
m = next(i for i in items if i['appointment_id'] == a_maria)
ok(m['sent_at'] and m['sent_channel'] == 'whatsapp' and m['sent_by_name'] == 'Rita', 'list shows sent, channel and who')
ok(next(i for i in items if i['appointment_id'] == cls)['sent_channel'] == 'sms', 'class attendee marked separately')
ok(call('POST', '/reminders/sent', ther, {'appointment_id': a_maria, 'patient_id': maria, 'sent': False}).status_code == 200, 'undo')
ok(next(i for i in call('GET', '/reminders', recep).json()['items'] if i['appointment_id'] == a_maria)['sent_at'] is None, 'undone')

# ── template ──
ok(call('PUT', '/reminders/template', recep, {'pt': 'x', 'en': 'y'}).status_code == 403, 'reception can’t change the message')
ok(call('PUT', '/reminders/template', ther, {'pt': 'x', 'en': 'y'}).status_code == 403, 'therapist can’t change the message')
ok(call('PUT', '/reminders/template', owner, {'pt': '', 'en': 'y'}).status_code == 400, 'both languages required')
ok(call('PUT', '/reminders/template', owner, {'pt': 'Olá {nome}, até {dia} às {hora}!', 'en': 'Hi {name}, see you {day} at {time}!'}).status_code == 200, 'owner saves the message')
d = call('GET', '/reminders', recep).json()
ok(d['template_pt'] == 'Olá {nome}, até {dia} às {hora}!', 'new message in the list')

if PSQL:
    acts = sql("SELECT string_agg(DISTINCT action, ',') FROM access_log")
    ok(all(a in acts for a in ('reminders.listed', 'appointment.reminded', 'appointment.reminder_undone', 'reminder_text.updated')), 'logged: ' + acts)
    ok(sql("SELECT count(*) FROM access_log WHERE detail ILIKE '%Maria%' OR detail ILIKE '%912%'") == '0', 'no names or phones in the access log')
    ok(sql("SELECT column_name FROM information_schema.columns WHERE table_name = 'reminders_sent' AND column_name ILIKE '%text%'") == '', 'message text is never stored')
if LOG:
    log = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Maria' not in log and '912 345' not in log, 'server log has no names or phones')

print('\nALL PASS' if fails == 0 else f'\n{fails} FAILED')
sys.exit(1 if fails else 0)
