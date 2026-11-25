# End-to-end test of the phase 2 API (patients, clinical record, consents, prescriptions, schedule).
# Needs a running server with an EMPTY database (it sets up its own clinic, owner, therapist and receptionist).
# Run: pip install requests pyotp && python3 server/tests/e2e_phase2.py   (BASE env var overrides the URL)
import os, sys, time, requests, pyotp
from urllib.parse import quote
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
H = {'X-Device': 'Rececao-PC', 'X-Device-Key': DEVICE_KEY}
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m)
    if not c: fails += 1
def call(method, path, tok=None, json=None):
    h = dict(H)
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + path, json=json, headers=h)

# ── accounts: owner, therapist, receptionist ──
r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    v = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()
    return v['token'], v['staff']['id']
ther, ther_id = join('therapist', 'ana@clinica.pt')
recep, recep_id = join('reception', 'rita@clinica.pt')
owner_id = call('GET', '/me', owner).json()['id']
r = call('GET', '/therapists', recep); ok(r.status_code == 200 and {t['id'] for t in r.json()} == {owner_id, ther_id} and all(set(t) == {'id', 'name', 'role'} for t in r.json()), 'reception lists bookable therapists (names only)')

# ── patients ──
r = call('POST', '/patients', recep, {'name': '  '}); ok(r.status_code == 400, 'patient needs a name')
r = call('POST', '/patients', recep, {'name': 'Maria Santos', 'phone': '912 345 678', 'email': 'maria@example.pt', 'birth_date': '1980-05-02', 'therapist_id': ther_id})
ok(r.status_code == 200 and r.json()['number'] == 1, 'reception registers a patient (n.º 1)'); maria = r.json()['id']
r = call('POST', '/patients', recep, {'name': 'João Pires', 'birth_date': '31/02/1990'}); ok(r.status_code == 400 and 'format' in r.json()['message'], 'bad date → friendly 400')
r = call('POST', '/patients', ther, {'name': 'João Pires', 'phone': '934 000 111'}); joao = r.json()['id']
r = call('POST', '/patients', owner, {'name': 'Rui Costa', 'sex': 'q'}); ok(r.status_code == 400, 'invalid choice refused by the database rules')
r = call('GET', '/patients?q=maria', recep); ok(r.status_code == 200 and [p['name'] for p in r.json()] == ['Maria Santos'], 'search by name')
r = call('GET', '/patients?q=912345', recep); ok(len(r.json()) == 1, 'search by phone ignoring spaces')
r = call('PATCH', f'/patients/{maria}', recep, {'phone': '912 999 999', 'admin_note': 'Prefere manhãs'}); ok(r.status_code == 200, 'reception updates contacts')

# ── clinical record: role enforcement on the server ──
ok(call('PUT', f'/patients/{maria}/anamnesis', recep, {'main_complaint': 'x'}).status_code == 403, 'reception cannot write anamnesis')
r = call('PUT', f'/patients/{maria}/anamnesis', ther, {'main_complaint': 'Lombalgia', 'goals': 'Voltar a correr',
         'regions': [{'region': 'lumbar', 'symptoms': ['dor', 'rigidez'], 'notes': 'pior de manhã'}], 'red_flags': []})
ok(r.status_code == 200, 'therapist writes anamnesis with regions')
ok(call('PUT', f'/patients/{maria}/anamnesis', ther, {'regions': 'lumbar'}).status_code == 400, 'regions must be a list')
r = call('POST', f'/patients/{maria}/notes', ther, {'template': 'lombalgia', 'blocks': [{'type': 'slider', 'label': 'Dor', 'value': 6}], 'text': 'S: dor 6/10'})
note = r.json()['id']; ok(r.status_code == 200, 'therapist starts a progress note')
ok(call('POST', f'/patients/{maria}/notes', recep, {'text': 'x'}).status_code == 403, 'reception cannot write notes')
ok(call('PATCH', f'/notes/{note}', ther, {'text': 'S: dor 5/10'}).status_code == 200, 'draft can be edited')
ok(call('POST', f'/notes/{note}/sign', ther).status_code == 200, 'therapist signs the note')
r = call('PATCH', f'/notes/{note}', owner, {'text': 'changed'}); ok(r.status_code == 409, 'signed note cannot be edited (even by the owner)')
ok(call('DELETE', f'/notes/{note}', owner).status_code == 409, 'signed note cannot be deleted')
r = call('POST', f'/patients/{maria}/tests', ther, {'test_code': 'ODI', 'score': 40, 'max_score': 100, 'taken_on': '2026-10-01'})
ok(r.status_code == 200 and r.json()['worse'] is False, 'first test result')
r = call('POST', f'/patients/{maria}/tests', ther, {'test_code': 'odi', 'score': 48, 'max_score': 100})
ok(r.json()['worse'] is True and r.json()['previous'] == 40, 'higher ODI → flagged as worse')
r = call('POST', f'/patients/{maria}/prescriptions', ther, {'doctor': 'Dr. Lopes', 'specialty': 'Fisiatria', 'issued_on': '2026-10-20', 'sessions_total': 3, 'payer': 'ADSE'})
ok(r.status_code == 200, 'prescription recorded'); presc = r.json()['id']
ok(call('POST', f'/patients/{maria}/prescriptions', ther, {'doctor': 'x', 'issued_on': '2026-10-20', 'sessions_total': 0}).status_code == 400, 'prescription needs ≥ 1 session')

# ── consents ──
ok(call('POST', '/consent-texts', recep, {'kind': 'treatment', 'lang': 'pt', 'title': 'x', 'body': 'x'}).status_code == 403, 'only the owner edits consent texts')
t1 = call('POST', '/consent-texts', owner, {'kind': 'treatment', 'lang': 'pt', 'title': 'Consentimento para tratamento', 'body': 'Texto v1', 'valid_months': 12}).json()
t2 = call('POST', '/consent-texts', owner, {'kind': 'treatment', 'lang': 'pt', 'title': 'Consentimento para tratamento', 'body': 'Texto v2', 'valid_months': 12}).json()
ok(t1['version'] >= 2 and t2['version'] == t1['version'] + 1, 'consent texts are versioned (after the server’s v1 drafts)')
td = call('POST', '/consent-texts', owner, {'kind': 'data', 'lang': 'pt', 'title': 'RGPD', 'body': 'Dados'}).json()
sig = 'data:image/png;base64,iVBORw0KGgo='
ok(call('POST', f'/patients/{maria}/consents', recep, {'text_id': t2['id'], 'granted': True, 'signed_by_name': 'Maria Santos', 'signature_png': 'nope'}).status_code == 400, 'signature required')
r = call('POST', f'/patients/{maria}/consents', recep, {'text_id': t2['id'], 'granted': True, 'signed_by_name': 'Maria Santos', 'signature_png': sig})
ok(r.status_code == 200 and r.json()['expires_on'] is not None, 'reception records a signed consent with expiry')
consent = r.json()['id']
r = call('POST', f'/patients/{maria}/consents', recep, {'text_id': td['id'], 'granted': True, 'signed_by_name': 'Maria Santos', 'signature_png': sig})
ok(r.status_code == 200 and r.json()['expires_on'] is None, 'consent without expiry (data processing)')

# ── the patient file: full vs reduced, and the access log ──
f = call('GET', f'/patients/{maria}', ther).json()
ok(f['view'] == 'full' and f['anamnesis']['main_complaint'] == 'Lombalgia' and len(f['notes']) == 1 and len(f['tests']) == 2, 'therapist sees the full file')
ok(f['prescriptions'][0]['remaining'] == 3, 'prescription shows 3 sessions remaining')
f = call('GET', f'/patients/{maria}', recep).json()
ok(f['view'] == 'reduced' and all(k not in f for k in ('anamnesis', 'notes', 'tests')), 'reception gets no clinical fields (server-side)')
ok(all(d['kind'] in ('prescription', 'consent') for d in f.get('documents', [])), 'reception sees scanned prescriptions and consent PDFs only (no signed-note PDFs)')
ok(f['prescriptions'][0]['remaining'] == 3 and f['prescriptions'][0]['payer'] == 'ADSE', 'reception sees prescriptions (owner decision)')
ok(f['patient']['phone'] == '912 999 999' and len(f['consents']) == 2 and f['consents'][0]['state'] == 'valid', 'reception sees contacts and consents')
ok('signature_png' not in str(f), 'file does not carry signature images')
ok(call('GET', f'/consents/{consent}', recep).json()['body'] == 'Texto v2', 'signed consent keeps the text version it was signed on')
ok(call('GET', '/patients/00000000-0000-0000-0000-000000000000', ther).status_code == 404, 'unknown patient → 404')
log = call('GET', f'/access-log?subject_type=patient&subject_id={maria}&limit=100', owner).json()
views = [(l['staff_name'], l['detail']) for l in log if l['action'] == 'patient.viewed']
ok(('Ana', 'full') in views and ('Rita', 'reduced') in views and ('Rita', 'consent') in views, 'every file read is in the access log: ' + str(sorted(set(views))))
acts = {l['action'] for l in log}
ok({'patient.created', 'patient.updated', 'anamnesis.updated', 'note.signed', 'test.recorded', 'prescription.created', 'consent.signed'} <= acts, 'clinical writes are logged')
ok(not any('Lombalgia' in l['detail'] or '912' in l['detail'] for l in log), 'access log details hold no clinical content or contacts')

# ── schedule ──
LIS = ZoneInfo('Europe/Lisbon')
day = (datetime.now(LIS) + timedelta(days=1)).replace(hour=9, minute=0, second=0, microsecond=0)
iso = lambda d: d.isoformat()
qiso = lambda d: quote(d.isoformat())  # '+' in an offset must be escaped in a query string
r = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(day), 'ends_at': iso(day + timedelta(minutes=45)), 'therapist_id': ther_id, 'patient_id': maria, 'prescription_id': presc})
ok(r.status_code == 200 and len(r.json()['ids']) == 1, 'reception books a session'); a1 = r.json()['ids'][0]
r = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(day + timedelta(minutes=30)), 'ends_at': iso(day + timedelta(minutes=75)), 'therapist_id': ther_id, 'patient_id': joao})
ok(r.status_code == 409 and r.json()['error'] == 'overlap' and len(r.json()['details']) == 1, 'overlap warned (409 with the clash)')
r = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(day + timedelta(minutes=30)), 'ends_at': iso(day + timedelta(minutes=75)), 'therapist_id': ther_id, 'patient_id': joao, 'allow_overlap': True})
ok(r.status_code == 200 and len(r.json()['overlaps']) == 1, '…and allowed with allow_overlap'); a2 = r.json()['ids'][0]
ok(call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(day), 'ends_at': iso(day), 'therapist_id': ther_id, 'patient_id': joao}).status_code == 400, 'end must be after start')
ok(call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(day), 'ends_at': iso(day + timedelta(minutes=45)), 'therapist_id': ther_id}).status_code == 400, 'session needs a patient')
# home visit with travel buffer clashes with a session 20 minutes after it ends
hv = day.replace(hour=14)
call('POST', '/appointments', recep, {'kind': 'home', 'starts_at': iso(hv), 'ends_at': iso(hv + timedelta(hours=1)), 'therapist_id': ther_id, 'patient_id': joao, 'address': 'Rua A, 1', 'travel_minutes': 30})
r = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': iso(hv + timedelta(minutes=80)), 'ends_at': iso(hv + timedelta(minutes=110)), 'therapist_id': ther_id, 'patient_id': maria})
ok(r.status_code == 409, 'home-visit travel time counts as busy')
# recurring series across the end of summer time: weekly on Mon+Thu, 6 sessions, all at 18:00 local
start = datetime(2027, 3, 22, 18, 0, tzinfo=LIS)  # a Monday; Portugal moves to summer time on 28 Mar 2027
r = call('POST', '/appointments', ther, {'kind': 'session', 'starts_at': iso(start), 'ends_at': iso(start + timedelta(minutes=45)), 'therapist_id': ther_id, 'patient_id': maria, 'repeat': {'weekdays': [1, 4], 'count': 6}})
ok(r.status_code == 200 and len(r.json()['ids']) == 6, 'twice-weekly series of 6 created'); series = r.json()['series_id']; sids = r.json()['ids']
lst = call('GET', f'/appointments?from={qiso(start - timedelta(days=1))}&to={qiso(start + timedelta(days=30))}', recep).json()
ser = [a for a in lst if a['series_id'] == series]
loc = [datetime.fromisoformat(a['starts_at']).astimezone(LIS) for a in ser]
ok([d.strftime('%a %H:%M') for d in loc] == ['Mon 18:00', 'Thu 18:00'] * 3, 'series stays at 18:00 local across the DST change: ' + ', '.join(d.strftime('%d %a %H:%M') for d in loc))
# move this and following (from the 3rd) to 17:30, same days
third = ser[2]
newstart = datetime.fromisoformat(third['starts_at']).astimezone(LIS).replace(hour=17, minute=30)
r = call('PATCH', f"/appointments/{third['id']}", recep, {'scope': 'following', 'starts_at': iso(newstart), 'ends_at': iso(newstart + timedelta(minutes=60))})
ok(r.status_code == 200 and len(r.json()['ids']) == 4, 'edit this and following (4)')
lst = call('GET', f'/appointments?from={qiso(start - timedelta(days=1))}&to={qiso(start + timedelta(days=30))}', recep).json()
loc = [(datetime.fromisoformat(a['starts_at']).astimezone(LIS), datetime.fromisoformat(a['ends_at']).astimezone(LIS)) for a in lst if a['series_id'] == series]
ok([s.strftime('%H:%M') for s, _ in loc] == ['18:00', '18:00', '17:30', '17:30', '17:30', '17:30'] and all((e - s) == timedelta(minutes=60) for s, e in loc[2:]), 'first two unchanged, rest moved to 17:30 for 60 min')
r = call('PATCH', f"/appointments/{sids[0]}", recep, {'scope': 'all', 'status': 'done'}); ok(r.status_code == 400, 'done only one at a time')
r = call('PATCH', f"/appointments/{ser[5]['id']}", recep, {'scope': 'one', 'status': 'cancelled'}); ok(r.status_code == 200, 'cancel one of the series')
ok(call('GET', '/appointments?from=2026-01-01T00:00:00Z&to=2026-12-31T00:00:00Z', recep).status_code == 400, 'list limited to two months')

# prescriptions count down when sessions are done
call('PATCH', f'/appointments/{a1}', ther, {'status': 'done'})
f = call('GET', f'/patients/{maria}', ther).json()
ok(f['prescriptions'][0]['used'] == 1 and f['prescriptions'][0]['remaining'] == 2, 'done session counts against the prescription (2 left)')
al = call('GET', '/alerts', ther).json()
ok(any(p['prescription_id'] == presc for p in al['prescriptions']), 'Today alert: prescription with 2 sessions left')
ok(any(w['patient_id'] == maria and w['test_code'] == 'odi' for w in al['worse_scores']), 'Today alert: worsening ODI')
ok(any(c['patient_id'] == joao and set(c['missing']) == {'treatment', 'data'} for c in al['consents_missing']), 'Today alert: João has an appointment but no consents')
al = call('GET', '/alerts', recep).json()
ok(isinstance(al['prescriptions'], list) and al['worse_scores'] is None and isinstance(al['consents_missing'], list), 'reception alerts: prescriptions yes, scores no')

# group class
cl = day.replace(hour=19)
r = call('POST', '/appointments', ther, {'kind': 'class', 'title': 'Pilates clínico', 'starts_at': iso(cl), 'ends_at': iso(cl + timedelta(hours=1)), 'therapist_id': ther_id, 'capacity': 1})
cls = r.json()['ids'][0]
ok(call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': maria}).status_code == 200, 'add class attendee')
ok(call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': joao}).status_code == 409, 'class full')
ok(call('PATCH', f'/appointments/{cls}/attendees/{maria}', recep, {'attended': True}).status_code == 200, 'mark attendance')
ok(call('POST', f'/appointments/{a1}/attendees', recep, {'patient_id': joao}).status_code == 400, 'sessions have no attendees')
# blocked time never needs allow_overlap
r = call('POST', '/appointments', owner, {'kind': 'block', 'title': 'Formação', 'color': 'grey', 'starts_at': iso(day), 'ends_at': iso(day + timedelta(hours=3)), 'therapist_id': ther_id})
ok(r.status_code == 200 and len(r.json()['overlaps']) >= 1, 'blocked time saved, overlaps reported')

# waitlist + cancellation match
w = call('POST', '/waitlist', recep, {'name': 'Carla Nunes', 'phone': '961 111 222', 'weekdays': [day.isoweekday()], 'from_time': '08:00', 'to_time': '12:00'})
ok(w.status_code == 200, 'waitlist entry (not yet a patient)')
call('POST', '/waitlist', recep, {'name': 'Pedro', 'phone': '962', 'weekdays': [day.isoweekday()], 'from_time': '15:00', 'to_time': '18:00'})
ok(call('POST', '/waitlist', recep, {'note': 'x'}).status_code == 400, 'waitlist needs a patient or a name')
r = call('PATCH', f'/appointments/{a2}', recep, {'status': 'cancelled'})
ok([m['name'] for m in r.json()['waitlist_matches']] == ['Carla Nunes'], 'cancelled 9:30 slot → offers it to Carla (morning), not Pedro (afternoon)')

# booking requests: accept, undo, decline
br = call('POST', '/booking-requests', recep, {'name': 'Sofia', 'phone': '963', 'preferred_at': iso(day.replace(hour=11))}).json()['id']
ok(len(call('GET', '/booking-requests', recep).json()) == 1, 'pending request listed')
r = call('POST', f'/booking-requests/{br}/accept', recep, {'kind': 'assessment', 'starts_at': iso(day.replace(hour=11)), 'ends_at': iso(day.replace(hour=12)), 'therapist_id': ther_id, 'patient_id': joao, 'allow_overlap': True})
ok(r.status_code == 200, 'request accepted → appointment booked'); ap = r.json()['appointment_id']
ok(call('POST', f'/booking-requests/{br}/accept', recep, {'kind': 'assessment', 'starts_at': iso(day), 'ends_at': iso(day + timedelta(hours=1)), 'therapist_id': ther_id, 'patient_id': joao}).status_code == 409, 'cannot accept twice')
ok(call('POST', f'/booking-requests/{br}/reopen', recep).status_code == 200, 'Undo accept')
lst = call('GET', f'/appointments?from={qiso(day - timedelta(hours=1))}&to={qiso(day + timedelta(hours=12))}', recep).json()
ok(all(a['id'] != ap for a in lst), '…removes the appointment it created')
ok(call('POST', f'/booking-requests/{br}/decline', recep).status_code == 200 and len(call('GET', '/booking-requests', recep).json()) == 0, 'decline')

# access log still append-only, and reception still can't read it
ok(call('GET', '/access-log', recep).status_code == 403, 'reception cannot read the access log')
print(f'\n{"ALL PASSED" if fails == 0 else str(fails) + " FAILED"}')
sys.exit(1 if fails else 0)
