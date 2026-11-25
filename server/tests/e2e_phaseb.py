# End-to-end test of audit phase B on the server (server 0.13.0): patient search picker (?id, paging), NIF check digit
# and no repeated NIF, possible duplicates, moving an appointment to another therapist and time (drag in the Schedule),
# and server messages in the app's language (Accept-Language).
# Needs a running server with an EMPTY database. PSQL_URL adds the access-log checks.
# Run: PSQL_URL=postgres://… python3 server/tests/e2e_phaseb.py
import os, sys, subprocess, requests, pyotp
from datetime import datetime, timedelta

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
H = {'X-Device': 'Rececao-PC', 'X-Device-Key': DEVICE_KEY}
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m)
    if not c: fails += 1
def call(method, path, tok=None, json=None, lang=None):
    h = dict(H)
    if tok: h['Authorization'] = 'Bearer ' + tok
    if lang: h['Accept-Language'] = lang
    return requests.request(method, B + path, json=json, headers=h)
def sql(q):
    return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()
def valid_nif(n):
    d = [2] + [int(c) for c in f'{n:07d}']
    s = sum(x * (9 - i) for i, x in enumerate(d)) % 11
    return ''.join(map(str, d)) + str(0 if s < 2 else 11 - s)

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

# ── NIF: check digit, tidying, foreign numbers, no repeats (M11) ──
r = call('POST', '/patients', recep, {'name': 'Errado', 'nif': '123456780'})
ok(r.status_code == 400 and 'NIF' in r.json()['message'], 'a NIF with a wrong check digit is refused')
r = call('POST', '/patients', recep, {'name': 'Curto', 'nif': '12345'}); ok(r.status_code == 400, 'a NIF that is too short is refused')
n1 = valid_nif(1)
r = call('POST', '/patients', recep, {'name': 'Maria Santos', 'nif': f'PT {n1[:3]} {n1[3:6]} {n1[6:]}', 'phone': '912 345 678', 'birth_date': '1980-05-02'})
ok(r.status_code == 200, 'a valid NIF with spaces and a PT prefix is accepted'); maria = r.json()['id']
ok(call('GET', f'/patients/{maria}', owner).json()['patient']['nif'] == n1, 'the NIF is kept tidy (9 digits only)')
r = call('POST', '/patients', recep, {'name': 'Outra Maria', 'nif': n1})
ok(r.status_code == 409 and r.json()['error'] == 'duplicate_nif' and r.json()['details']['id'] == maria and r.json()['details']['number'] == 1, 'a NIF another patient has is refused, saying whose')
r = call('POST', '/patients', recep, {'name': 'Pablo García', 'nif': 'es12345678z'})
ok(r.status_code == 200, 'a foreign number with its country code is accepted'); pablo = r.json()['id']
ok(call('GET', f'/patients/{pablo}', owner).json()['patient']['nif'] == 'ES12345678Z', 'foreign number kept in capitals')
r = call('PATCH', f'/patients/{pablo}', recep, {'nif': n1}); ok(r.status_code == 409 and r.json()['error'] == 'duplicate_nif', 'changing to a NIF in use is refused')
r = call('PATCH', f'/patients/{maria}', recep, {'nif': n1, 'admin_note': 'x'}); ok(r.status_code == 200, 'saving a patient with their own NIF is fine')
r = call('PATCH', f'/patients/{maria}', recep, {'nif': '999999999'}); ok(r.status_code == 400, 'PATCH checks the NIF too')
r = call('PATCH', f'/patients/{pablo}', recep, {'nif': ''}); ok(r.status_code == 200, 'a NIF can be cleared')

# ── possible duplicates (M11) ──
r = call('GET', '/patients/duplicates?phone=%2B351%20912%20345%20678', recep)
ok(r.status_code == 200 and [x['id'] for x in r.json()] == [maria] and r.json()[0]['reason'] == 'phone', 'same phone, written differently → found (reason phone)')
r = call('GET', '/patients/duplicates?name=%20maria%20santos&birth_date=1980-05-02', recep)
ok([x['reason'] for x in r.json()] == ['name_birth'], 'same name (any case, spaces) and birth date → found')
r = call('GET', '/patients/duplicates?name=Maria%20Santos&birth_date=1980-05-03', recep); ok(r.json() == [], 'other birth date → nothing')
r = call('GET', f'/patients/duplicates?nif={n1}', ther); ok(r.json()[0]['reason'] == 'nif', 'same NIF → reason nif')
r = call('GET', f'/patients/duplicates?phone=912345678&exclude={maria}', recep); ok(r.json() == [], 'exclude leaves out the patient being edited')
r = call('GET', '/patients/duplicates?name=Maria%20Santos', recep); ok(r.json() == [], 'a name alone is not enough to ask')
ok(set(r.json()[0]) == {'id', 'number', 'name', 'birth_date', 'phone', 'reason'} if (r := call('GET', '/patients/duplicates?phone=912345678', recep)).json() else False, 'answers contacts only (no clinical fields)')
ok(call('GET', '/patients/duplicates?phone=912345678').status_code == 401, 'needs a session')

# ── search picker: ?id, paging (M7) ──
for i in range(2, 13): call('POST', '/patients', recep, {'name': f'Paciente {i:02d}'})
r = call('GET', f'/patients?id={maria}', recep); ok([x['id'] for x in r.json()] == [maria], '?id answers just that patient (for a picker opened from a file)')
call('PATCH', f'/patients/{pablo}', recep, {'active': False})
r = call('GET', f'/patients?id={pablo}', recep); ok(len(r.json()) == 1, '?id finds an inactive patient too')
a = call('GET', '/patients?limit=5', recep).json(); b2 = call('GET', '/patients?limit=5&offset=5', recep).json()
ok(len(a) == 5 and len(b2) == 5 and not {x['id'] for x in a} & {x['id'] for x in b2}, 'limit/offset pages without overlap')
allp = call('GET', '/patients', recep).json()
ok([x['id'] for x in a + b2] == [x['id'] for x in allp[:10]], 'pages follow the same name order as the full list')
r = call('GET', f'/patients?q={n1[:3]}%20{n1[3:6]}%20{n1[6:]}', recep); ok([x['id'] for x in r.json()] == [maria], 'search by NIF with spaces')

# ── moving an appointment (drag to another time and therapist, M6) ──
day = (datetime.now() + timedelta(days=2)).replace(hour=10, minute=0, second=0, microsecond=0).astimezone()
iso = lambda d: d.isoformat()
x = call('POST', '/appointments', recep, {'kind': 'session', 'patient_id': maria, 'therapist_id': owner_id, 'starts_at': iso(day), 'ends_at': iso(day + timedelta(minutes=45))}).json()['ids'][0]
y = call('POST', '/appointments', recep, {'kind': 'session', 'patient_id': maria, 'therapist_id': ther_id, 'starts_at': iso(day + timedelta(hours=1)), 'ends_at': iso(day + timedelta(hours=1, minutes=45))}).json()['ids'][0]
r = call('PATCH', f'/appointments/{x}', recep, {'scope': 'one', 'starts_at': iso(day + timedelta(hours=1, minutes=15)), 'ends_at': iso(day + timedelta(hours=2)), 'therapist_id': ther_id, 'allow_overlap': False})
ok(r.status_code == 409 and r.json()['error'] == 'overlap', 'dropping on the other therapist’s busy time → overlap')
r = call('PATCH', f'/appointments/{x}', recep, {'scope': 'one', 'starts_at': iso(day + timedelta(hours=3)), 'ends_at': iso(day + timedelta(hours=4)), 'therapist_id': ther_id, 'allow_overlap': False})
got = [a for a in call('GET', f'/appointments?from={requests.utils.quote(iso(day - timedelta(hours=1)))}&to={requests.utils.quote(iso(day + timedelta(hours=8)))}', recep).json() if a['id'] == x][0]
ok(r.status_code == 200 and got['therapist_id'] == ther_id and got['starts_at'].startswith(day.strftime('%Y-%m-%d')) and (datetime.fromisoformat(got['ends_at']) - datetime.fromisoformat(got['starts_at'])).seconds == 3600, 'move + resize + new therapist in one change')

# ── messages in the app's language (M13) ──
r = call('POST', '/patients', recep, {'name': 'X', 'nif': '123456780'}, lang='pt-PT')
ok(r.status_code == 400 and r.json()['message'] == 'Esse NIF não é válido. Confirme os 9 dígitos.', f'Accept-Language pt → Portuguese ({r.json()["message"]})')
r = call('POST', '/patients', recep, {'name': 'X', 'nif': '123456780'}, lang='en')
ok(r.json()['message'] == 'That NIF isn’t valid. Check the 9 digits.', 'Accept-Language en → English')
r = call('POST', '/patients', recep, {'name': 'X', 'nif': '123456780'})
ok(r.json()['message'] == 'That NIF isn’t valid. Check the 9 digits.', 'no language → English (older apps translate it themselves)')
r = call('GET', '/finance/overview?month=2026-11', ther, lang='pt')
ok(r.status_code == 403 and r.json()['message'] == 'A sua função não permite fazer isto.' and r.json()['error'] == 'forbidden', 'forbidden in Portuguese, code kept')
r = call('POST', '/patients', recep, {'name': 'Y', 'nif': n1}, lang='pt')
ok(r.json()['message'].startswith('Já existe um utente com este NIF') and r.json()['details']['id'] == maria, 'a conflict with details: message translated, details kept')
r = call('PATCH', f'/appointments/{y}', recep, {'scope': 'one', 'starts_at': iso(day + timedelta(hours=3)), 'ends_at': iso(day + timedelta(hours=3, minutes=30)), 'allow_overlap': False}, lang='pt')
ok(r.status_code == 409 and r.json()['message'] == 'Sobrepõe-se a outra marcação do mesmo fisioterapeuta.' and isinstance(r.json()['details'], list), 'overlap in Portuguese, clashes still listed')
r = requests.post(B + '/auth/login', json={'email': 'filipe@clinica.pt', 'password': 'wrong one'}, headers={**H, 'Accept-Language': 'pt-PT,pt;q=0.9'})
ok(r.status_code == 401 and r.json()['message'] == 'O email ou a palavra-passe estão errados.', 'sign-in error in Portuguese')
r = call('GET', '/patients/00000000-0000-0000-0000-000000000000', owner, lang='pt')
ok(r.status_code == 404 and r.json()['message'] == 'Não encontrado.', 'not found in Portuguese')
r = call('GET', '/patients', recep, lang='pt'); ok(r.status_code == 200 and isinstance(r.json(), list), 'answers that worked are untouched')

if PSQL:
    acts = sql("SELECT string_agg(DISTINCT action, ',') FROM access_log WHERE action LIKE 'patients.%'")
    ok('patients.duplicates_checked' in acts, f'duplicate checks that found someone are logged ({acts})')
    ok(sql("SELECT count(*) FROM access_log WHERE detail LIKE '%Maria%' OR detail LIKE '%912%'") == '0', 'no names or phone numbers in the access log')

print(f'\n{"ALL PASSED" if not fails else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
