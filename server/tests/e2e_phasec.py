# End-to-end test of audit phase C on the server (server 0.14.0 + migration 0014): access-log filters, paging and
# CSV export; question-by-question test forms scored on the server (NPRS, SPADI, LEFS, PSFS, KOOS); employer Social
# Security on salaries (clinic rate, own rate, amount kept per payment, in the month result and the accountant export).
# Needs a running server with an EMPTY database. PSQL_URL adds the append-only and stored-value checks.
# Run: PSQL_URL=postgres://… python3 server/tests/e2e_phasec.py
import os, sys, io, csv, zipfile, subprocess, requests, pyotp
from datetime import date

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
H = {'X-Device': 'MacBook-Filipe', 'X-Device-Key': DEVICE_KEY}
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m)
    if not c: fails += 1
def call(method, path, tok=None, json=None, lang=None, device=None):
    h = dict(H)
    if device: h['X-Device'] = device
    if tok: h['Authorization'] = 'Bearer ' + tok
    if lang: h['Accept-Language'] = lang
    return requests.request(method, B + path, json=json, headers=h)
def sql(q):
    return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
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
today = date.today().isoformat()

maria = call('POST', '/patients', recep, {'name': 'Maria Santos'}).json()['id']
joao = call('POST', '/patients', recep, {'name': 'João Pires'}).json()['id']

# ── test forms: scored on the server ──
T = lambda pid, body, tok=ther, lang=None: call('POST', f'/patients/{pid}/tests', tok, body, lang)
r = T(maria, {'test_code': 'nprs', 'answers': {'items': {'now': 7}}, 'score': 1})
ok(r.status_code == 200 and r.json()['score'] == 7, 'NPRS form: the server uses the answer, not the score the app sent')
spadi = {**{f'p{i}': 6 for i in range(1, 6)}, **{f'd{i}': 3 for i in range(1, 9)}}
r = T(maria, {'test_code': 'spadi', 'answers': {'items': spadi}})
ok(r.status_code == 200 and r.json()['score'] == 41.5, 'SPADI form → 41.5 % (54 of 130)')
f = call('GET', f'/patients/{maria}', ther).json()
sp = [t for t in f['tests'] if t['test_code'] == 'spadi'][0]
ok(sp['answers']['sub'] == {'pain': 60.0, 'disability': 30.0} and float(sp['max_score']) == 100 and sp['higher_is_worse'] is True, 'SPADI keeps pain and disability subscales, max 100, higher is worse')
ok(sp['answers']['items']['p1'] == 6 and sp['answers']['form'] == 1, 'each answer is stored')
del spadi['p1']
r = T(maria, {'test_code': 'spadi', 'answers': {'items': spadi}}); ok(r.status_code == 200 and r.json()['score'] == 40.0, 'one SPADI question blank → maximum shrinks (48 of 120)')
r = T(maria, {'test_code': 'spadi', 'answers': {'items': {'p1': 6}}}); ok(r.status_code == 400, 'too many SPADI blanks → refused')
r = T(maria, {'test_code': 'spadi', 'answers': {'items': {'p1': 6}}}, lang='pt-PT'); ok(r.status_code == 400 and 'em branco' in r.json()['message'], '… in Portuguese when asked')
r = T(maria, {'test_code': 'spadi', 'answers': {'items': {**spadi, 'p1': 11}}}); ok(r.status_code == 400, 'an answer out of range is refused')
r = T(maria, {'test_code': 'spadi', 'answers': {'items': {**spadi, 'p1': 2.5}}}); ok(r.status_code == 400, 'a fractional answer is refused')
r = T(maria, {'test_code': 'spadi', 'answers': {'items': {**spadi, 'zz': 1}}}); ok(r.status_code == 400, 'an unknown question is refused')
lefs = {f'i{i}': 3 for i in range(1, 21)}
r = T(joao, {'test_code': 'lefs', 'answers': {'items': lefs}}); ok(r.status_code == 200 and r.json()['score'] == 60, 'LEFS form → 60/80')
r = T(joao, {'test_code': 'lefs', 'answers': {'items': {**lefs, 'i1': 1}}})
ok(r.status_code == 200 and r.json()['worse'] is True and r.json()['previous'] == 60, 'LEFS lower than last time → worse (higher is better)')
del lefs['i20']; r = T(joao, {'test_code': 'lefs', 'answers': {'items': lefs}}); ok(r.status_code == 400, 'LEFS needs every answer')
koos = {k: 1 for k in [f's{i}' for i in range(1, 8)] + [f'p{i}' for i in range(1, 10)] + [f'a{i}' for i in range(1, 18)] + [f'sp{i}' for i in range(1, 6)] + [f'q{i}' for i in range(1, 5)]}
r = T(joao, {'test_code': 'koos', 'answers': {'items': koos}}); ok(r.status_code == 200 and r.json()['score'] == 75, 'KOOS all "1" → 75 (each subscale 75)')
koos2 = {**koos, 'q1': 4, 'q2': 4}
r = T(joao, {'test_code': 'koos', 'answers': {'items': koos2}}); ok(r.status_code == 200 and r.json()['score'] == 67.5 and r.json()['worse'] is True, 'KOOS quality of life 37.5 → mean 67.5, worse')
for q in ['q1', 'q2', 'q3']: koos2.pop(q)
r = T(joao, {'test_code': 'koos', 'answers': {'items': koos2}}); ok(r.status_code == 400, 'three blanks in one KOOS subscale → refused')
r = T(joao, {'test_code': 'psfs', 'answers': {'items': {'a1': 4, 'a2': 7}, 'activities': [' Subir escadas ', 'Correr']}})
ok(r.status_code == 200 and r.json()['score'] == 5.5, 'PSFS → mean of the activities (5.5)')
ps = [t for t in call('GET', f'/patients/{joao}', ther).json()['tests'] if t['test_code'] == 'psfs'][0]
ok(ps['answers']['activities'] == ['Subir escadas', 'Correr'], 'PSFS keeps the activity names (tidied)')
r = T(joao, {'test_code': 'psfs', 'answers': {'items': {'a1': 4}, 'activities': ['']}}); ok(r.status_code == 400, 'PSFS without an activity name → refused')
r = T(joao, {'test_code': 'psfs', 'answers': {'items': {'a1': 4, 'a2': 5}, 'activities': ['Correr']}}); ok(r.status_code == 400, 'PSFS score for an activity that isn\'t there → refused')
r = T(joao, {'test_code': 'odi', 'score': 34, 'max_score': 100, 'higher_is_worse': True}); ok(r.status_code == 200 and r.json()['score'] == 34, 'ODI (licensed) keeps the typed total')
r = T(joao, {'test_code': 'ndi', 'score': 60, 'max_score': 50}); ok(r.status_code == 400, 'a typed total above the maximum is refused')
r = T(joao, {'test_code': 'spadi', 'score': 30, 'max_score': 100, 'higher_is_worse': True}); ok(r.status_code == 200 and r.json()['score'] == 30, 'a form test can still be recorded as a total (paper)')
r = T(joao, {'test_code': 'nprs', 'answers': {'items': {'now': 3}}}, tok=recep); ok(r.status_code == 403, 'reception can\'t record tests')
r = call('GET', f'/patients/{joao}/export/full', ther)
ok(r.status_code == 200, 'clinical record export still works with form answers')

# ── employer Social Security ──
m = today[:7]
s = call('GET', f'/finance/salaries?month={m}', owner).json()
ok(s['employer_ss_bp'] == 2375, 'clinic rate starts at 23,75 %')
ana = [x for x in s['staff'] if x['staff_id'] == ther_id][0]
ok(ana['employer_ss_bp'] is None and ana['effective_ss_bp'] == 2375, 'staff use the clinic rate unless set')
r = call('POST', '/finance/salaries', owner, {'staff_id': ther_id, 'month': m, 'gross_cents': 100000})
ok(r.status_code == 200 and r.json()['employer_ss_cents'] == 23750, '1000,00 € gross → 237,50 € Social Security')
ov = call('GET', f'/finance/overview?month={m}', owner).json()
ok(ov['salaries_cents'] == 100000 and ov['employer_ss_cents'] == 23750 and ov['result_cents'] == ov['revenue_cents'] - ov['expenses_cents'] - 123750, 'the month result subtracts gross + Social Security')
r = call('PUT', f'/staff/{recep_id}/pay', owner, {'model': 'fixed', 'fixed_cents': 80000, 'employer_ss_bp': 0}); ok(r.status_code == 200, 'someone can have their own rate (0 %)')
r = call('POST', '/finance/salaries', owner, {'staff_id': recep_id, 'month': m, 'gross_cents': 80000}); ok(r.json()['employer_ss_cents'] == 0, '… and it is used')
r = call('PUT', '/finance/salaries/rate', owner, {'employer_ss_bp': 2000}); ok(r.status_code == 200, 'owner changes the clinic rate (law changed)')
r = call('POST', '/finance/salaries', owner, {'staff_id': owner_id, 'month': m, 'gross_cents': 150000}); ok(r.json()['employer_ss_cents'] == 30000, 'new salaries use the new rate')
s = call('GET', f'/finance/salaries?month={m}', owner).json()
paid_ana = [x for x in s['staff'] if x['staff_id'] == ther_id][0]['paid'][0]
ok(paid_ana['employer_ss_cents'] == 23750, 'a salary already recorded keeps its amount')
r = call('POST', '/finance/salaries', owner, {'staff_id': ther_id, 'month': m, 'gross_cents': 5000, 'employer_ss_cents': 1234}); ok(r.json()['employer_ss_cents'] == 1234, 'the amount can be typed (from the accountant)')
r = call('PUT', '/finance/salaries/rate', owner, {'employer_ss_bp': 10001}); ok(r.status_code == 400, 'a rate over 100 % is refused')
r = call('PUT', '/finance/salaries/rate', owner, {'employer_ss_bp': -1}, lang='pt'); ok(r.status_code == 400 and 'taxa' in r.json()['message'], '… and a negative one, in Portuguese')
r = call('POST', '/finance/salaries', owner, {'staff_id': ther_id, 'month': m, 'gross_cents': 5000, 'employer_ss_cents': -5}); ok(r.status_code == 400, 'a negative Social Security amount is refused')
r = call('PUT', f'/staff/{ther_id}/pay', owner, {'model': 'fixed', 'fixed_cents': 0, 'employer_ss_bp': 20000}); ok(r.status_code == 400, 'a staff rate over 100 % is refused')
r = call('PUT', '/finance/salaries/rate', ther, {'employer_ss_bp': 2375}); ok(r.status_code == 403, 'therapists can\'t change the rate')
r = call('PUT', f'/staff/{recep_id}/pay', owner, {'model': 'fixed', 'fixed_cents': 80000}); ok(r.status_code == 200, 'leaving the rate out goes back to the clinic rate')
ok([x for x in call('GET', f'/finance/salaries?month={m}', owner).json()['staff'] if x['staff_id'] == recep_id][0]['employer_ss_bp'] is None, '… (null again)')
r = requests.post(B + '/finance/export', headers={**H, 'Authorization': 'Bearer ' + owner}, json={'from': m, 'to': m, 'include_files': False, 'include_pdf': True})
if r.status_code == 200 and r.headers.get('content-type', '').startswith('application/json'):
    fid = r.json().get('file_id') or r.json().get('id')
    r = requests.get(B + f'/finance/files/{fid}/raw', headers={**H, 'Authorization': 'Bearer ' + owner})
ok(r.status_code == 200, 'accountant export still works')
try:
    z = zipfile.ZipFile(io.BytesIO(r.content))
    sal = [n for n in z.namelist() if n.endswith('salarios.csv')][0]
    rows = list(csv.reader(io.StringIO(z.read(sal).decode('utf-8-sig')), delimiter=';'))
    ok('Segurança Social, entidade patronal (€)' in rows[0] and any('237,50' in r for r in rows), 'salaries CSV has the Social Security column')
    res = [n for n in z.namelist() if n.endswith('resumo.csv')][0]
    rows = list(csv.reader(io.StringIO(z.read(res).decode('utf-8-sig')), delimiter=';'))
    ok(rows[0][4].startswith('Segurança Social') and rows[1][4] == '549,84', 'summary CSV has the month\'s Social Security (237,50 + 0 + 300,00 + 12,34)')
except Exception as ex:
    ok(False, f'export zip readable ({ex})')

# ── access log: filters, paging, export ──
for i in range(5): call('GET', f'/patients/{maria}', ther, device='PC-Sala-2')
call('POST', '/auth/login', json={'email': 'ana@clinica.pt', 'password': 'wrong password!!'})
L = lambda q='', tok=owner: call('GET', '/access-log' + (('?' + q) if q else ''), tok)
r = L(); ok(r.status_code == 200 and len(r.json()) > 20, 'owner sees the log')
ok(L(tok=ther).status_code == 403, 'therapists can\'t see the log')
rows = L(f'actions=patient.viewed&subject_id={maria}').json()
ok(len(rows) >= 5 and all(x['action'] == 'patient.viewed' for x in rows), 'filter by exact action')
ok(rows[0]['subject_label'] == '0001 Maria Santos', 'patient rows say whose file (number and name)')
rows = L('actions=login.').json(); ok(rows and all(x['action'].startswith('login.') for x in rows), 'filter by prefix (login.)')
rows = L('actions=login.failed,patient.viewed').json(); ok({x['action'] for x in rows} == {'login.failed', 'patient.viewed'}, 'several kinds at once')
rows = L(f'staff_id={ther_id}').json(); ok(rows and all(x['staff_name'] == 'Ana' for x in rows), 'filter by person')
rows = L('q=sala-2').json(); ok(rows and all(x['device'] == 'PC-Sala-2' for x in rows), 'search finds the computer')
rows = L('q=maria%20santos').json(); ok(rows and all(x['subject_label'] == '0001 Maria Santos' for x in rows), 'search finds the patient by name')
rows = L('q=100%25').json(); ok(isinstance(rows, list), '% in the search is taken literally')
rows = L(f'from={today}&to={today}').json(); ok(len(rows) > 20, 'today only → everything so far')
rows = L('from=2020-01-01&to=2020-01-31').json(); ok(rows == [], 'a past month → nothing')
ok(L('from=2026-02-30').status_code == 400, 'a wrong date is refused')
ok(L('from=2026-11-10&to=2026-11-01').status_code == 400, 'from after to is refused')
ok(L('actions=DROP%20TABLE').status_code == 400, 'an odd action filter is refused')
p1 = L('limit=10').json(); p2 = L(f'limit=10&before={p1[-1]["id"]}').json()
ok(len(p1) == 10 and len(p2) == 10 and p2[0]['id'] < p1[-1]['id'] and not ({x['id'] for x in p1} & {x['id'] for x in p2}), '"Show more" pages by id with no repeats')
r = call('GET', f'/access-log/export?actions=patient.viewed&q=maria', owner)
ok(r.status_code == 200 and r.headers['content-type'].startswith('text/csv') and 'registo-acessos-' in r.headers.get('content-disposition', ''), 'CSV export with filters')
rows = list(csv.reader(io.StringIO(r.content.decode('utf-8-sig')), delimiter=';'))
ok(rows[0] == ['Data e hora', 'Quem', 'Ação', 'Utente ou assunto', 'Computador', 'Detalhe'] and len(rows) >= 6 and all(x[2] == 'patient.viewed' and x[3] == '0001 Maria Santos' for x in rows[1:]), 'CSV has the filtered rows, with patient names')
ok(len(rows[1][0]) == 19 and rows[1][0][:10] == today, 'CSV time is clinic local time (YYYY-MM-DD HH:MM:SS)')
last = L('limit=1').json()[0]
ok(last['action'] == 'access_log.exported' and 'kinds patient.viewed' in last['detail'] and 'search' in last['detail'] and 'maria' not in last['detail'].lower(), 'the export is logged (filters, never the search words)')
ok(call('GET', '/access-log/export', ther).status_code == 403, 'therapists can\'t export the log')
if PSQL:
    ok('append-only' in subprocess.run(['psql', PSQL, '-tAc', 'DELETE FROM access_log'], capture_output=True, text=True).stderr, 'the log is still append-only')
    ok(sql("SELECT employer_ss_bp FROM clinic") == '2000', 'clinic rate stored')

print(f'\n{"ALL PASSED" if not fails else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
