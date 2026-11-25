# End-to-end test of the clinic-wide legal tools and VAT (8 Nov 2026):
# complaints log (ERS deadline in working days), delete everything from one day, the 20-year
# last-contact date on the patient legal check, VAT-exempt price-list items and the IVA column of the
# accountant export. Needs a running server with an EMPTY database.
# Run: ARCHIVE=/tmp/arch LOG=server.log PSQL_URL=postgres://… python3 server/tests/e2e_clinic.py
import os, sys, io, csv, base64, zipfile, subprocess, requests, pyotp
from datetime import datetime, timedelta, date

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
ARCHIVE = os.environ.get('ARCHIVE')
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Gabinete-1', 'X-Device-Key': DEVICE_KEY}
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
b64 = lambda b: base64.b64encode(b).decode()
PDF = b'%PDF-1.4\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n'
PNG = bytes.fromhex('89504e470d0a1a0a0000000d4948445200000001000000010806000000') + b'\x00' * 20

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

def at(days, hour):
    return (datetime.now() + timedelta(days=days)).replace(hour=hour, minute=0, second=0, microsecond=0).astimezone()
def appt(pid, days, hour, kind='session', **kw):
    s = at(days, hour)
    body = {'kind': kind, 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': ther_id, 'allow_overlap': True, **kw}
    if pid: body['patient_id'] = pid
    r = call('POST', '/appointments', recep, body); assert r.status_code == 200, r.text
    return r.json()['ids'][0]
def patient(name):
    return call('POST', '/patients', recep, {'name': name, 'birth_date': '1980-05-02', 'phone': '912 345 678', 'therapist_id': ther_id}).json()['id']

today = date.today()
iso = lambda d: d.isoformat()

# ───────── complaints ─────────
for tok, who in ((ther, 'therapist'), (recep, 'reception')):
    ok(call('GET', '/complaints', tok).status_code == 403, f'{who} can’t see the complaints log')
    ok(call('POST', '/complaints', tok, {'received_on': '2026-11-06', 'source': 'paper'}).status_code == 403, f'{who} can’t log a complaint')
ok(call('POST', '/complaints', owner, {'source': 'paper'}).status_code == 400, 'a complaint needs its date of receipt')
ok(call('POST', '/complaints', owner, {'received_on': '2026-11-06', 'source': 'fax'}).status_code == 400, 'unknown source refused')
ok(call('POST', '/complaints', owner, {'received_on': '2026-02-30', 'source': 'paper'}).status_code == 400, 'impossible date refused')

ana = patient('Ana Matos')
r = call('POST', '/complaints', owner, {'received_on': '2026-11-06', 'source': 'paper', 'book_ref': 'Folha 12', 'complainant': 'Ana Matos', 'patient_id': ana,
                                        'summary': 'Esperou 40 minutos.'})
ok(r.status_code == 200 and r.json()['due_on'] == '2026-11-20', 'paper complaint on Fri 6 Nov is due Fri 20 Nov (10 working days)')
c1 = r.json()['id']
r = call('POST', '/complaints', owner, {'received_on': '2026-11-23', 'source': 'electronic', 'complainant': 'Rui'})
ok(r.json().get('due_on') == '2026-12-09', '23 Nov → 9 Dec: 1 and 8 December are holidays')
c2 = r.json()['id']
lst = call('GET', '/complaints', owner).json()
ok([x['id'] for x in lst] == [c2, c1], 'list is newest first')
x1 = next(x for x in lst if x['id'] == c1)
ok(x1['patient_name'] == 'Ana Matos' and x1['book_ref'] == 'Folha 12' and x1['source'] == 'paper' and x1['sent_to_ers_on'] is None, 'complaint fields come back')
ok(call('PATCH', f'/complaints/{c2}', owner, {'received_on': '2026-11-27'}).status_code == 200, 'owner changes the date of receipt')
ok(next(x for x in call('GET', '/complaints', owner).json() if x['id'] == c2)['due_on'] == '2026-12-15', '…and the deadline moves with it (27 Nov → 15 Dec)')
ok(call('PATCH', f'/complaints/{c2}', ther, {'reply': 'x'}).status_code == 403, 'therapist can’t change a complaint')

# Alerts: an open paper complaint received 20 days ago is late; it shows for the owner only.
late = call('POST', '/complaints', owner, {'received_on': iso(today - timedelta(days=20)), 'source': 'paper', 'complainant': 'Late Person'}).json()['id']
al = call('GET', '/alerts', owner).json()
ok(any(x['id'] == late and x['working_days_left'] < 0 for x in al['complaints'] or []), 'Today: the late complaint is an owner alert')
ok(call('GET', '/alerts', ther).json().get('complaints') is None and call('GET', '/alerts', recep).json().get('complaints') is None, 'no complaint alerts for therapist or reception')
call('PATCH', f'/complaints/{late}', owner, {'reply': 'Pedimos desculpa.', 'replied_on': iso(today)})
ok(any(x['id'] == late for x in call('GET', '/alerts', owner).json()['complaints']), 'a paper complaint answered but not sent to the ERS is still open')
call('PATCH', f'/complaints/{late}', owner, {'sent_to_ers_on': iso(today)})
ok(not any(x['id'] == late for x in call('GET', '/alerts', owner).json()['complaints']), 'sent to the ERS: the alert goes')
ok(call('DELETE', f'/complaints/{late}', ther).status_code == 403, 'therapist can’t delete a complaint')
ok(call('DELETE', f'/complaints/{late}', owner).status_code == 200, 'owner deletes a complaint logged by mistake')
ok(call('DELETE', f'/complaints/{late}', owner).status_code == 404, '…and it’s gone')
lg = call('GET', '/access-log?subject_type=complaint&limit=50', owner).json()
acts = {x['action'] for x in lg}
ok({'complaint.created', 'complaint.updated', 'complaint.deleted'} <= acts, 'complaint changes are in the access log')
ok(not any('Ana' in x['detail'] or 'Late' in x['detail'] for x in lg), 'access log details carry no names')

# ───────── VAT on the price list ─────────
prods = call('GET', '/products?all=true', owner).json()
ok(all('vat_exempt' in p for p in prods), 'products carry vat_exempt')
ok(all(p['vat_exempt'] for p in prods if p['applies_to'] == 'session') and not any(p['vat_exempt'] for p in prods if p['applies_to'] == 'class'),
   'sessions start exempt, classes start with VAT')
class10 = next(p for p in prods if p['code'] == 'class10')
ok(call('PATCH', f"/products/{class10['id']}", recep, {'vat_exempt': True}).status_code == 403, 'reception can’t change VAT')
ok(call('PATCH', f"/products/{class10['id']}", owner, {'vat_exempt': True}).status_code == 200, 'owner marks a class card exempt (clinical Pilates)')
ok(next(p for p in call('GET', '/products?all=true', owner).json() if p['id'] == class10['id'])['vat_exempt'] is True, '…and it sticks')
call('PATCH', f"/products/{class10['id']}", owner, {'vat_exempt': False})
r = call('POST', '/products', owner, {'kind': 'pack', 'name_pt': 'Pilates 8', 'name_en': 'Pilates 8', 'price_cents': 9000, 'sessions': 8, 'applies_to': 'class', 'vat_exempt': True})
ok(r.status_code == 200, 'new product with vat_exempt')
bea = patient('Beatriz Lopes')
plan = call('POST', f'/patients/{bea}/plans', recep, {'product_id': class10['id']}).json()['id']
if PSQL:
    ok(sql(f"SELECT vat_exempt FROM patient_plans WHERE id = '{plan}'") == 'f', 'the sale copies the VAT flag')
    call('PATCH', f"/products/{class10['id']}", owner, {'vat_exempt': True})
    ok(sql(f"SELECT vat_exempt FROM patient_plans WHERE id = '{plan}'") == 'f', 'changing the product later doesn’t rewrite the sale')
    call('PATCH', f"/products/{class10['id']}", owner, {'vat_exempt': False})
ok(call('POST', '/payments', recep, {'patient_id': bea, 'method': 'cash', 'items': [{'plan_id': plan, 'amount_cents': 8000}]}).status_code == 200, 'class card paid')
sess = appt(bea, -1, 9); call('PATCH', f'/appointments/{sess}', recep, {'status': 'done'})
ok(call('POST', '/payments', recep, {'patient_id': bea, 'method': 'mbway', 'items': [{'appointment_id': sess, 'amount_cents': 2000}]}).status_code == 200, 'session paid')
ok(call('POST', '/payments', recep, {'method': 'cash', 'items': [{'description': 'Venda avulsa', 'amount_cents': 500}]}).status_code == 200, 'walk-in paid')
m = today.strftime('%Y-%m'); m0 = (today - timedelta(days=1)).strftime('%Y-%m')
r = call('POST', '/finance/export', owner, {'from': min(m, m0), 'to': m, 'include_files': False})
ok(r.status_code == 200, 'accountant export')
raw = requests.get(B + f"/finance/files/{r.json()['id']}/raw", headers={**H, 'Authorization': 'Bearer ' + owner}).content
z = zipfile.ZipFile(io.BytesIO(raw))
ent = [n for n in z.namelist() if n.endswith('entradas.csv')][0]
rows = list(csv.reader(io.StringIO(z.read(ent).decode('utf-8-sig')), delimiter=';'))
ok('IVA' in rows[0], 'entradas.csv has an IVA column')
iva = {row[5]: row[rows[0].index('IVA')] for row in rows[1:]}
ok(any('Cartão de 10 aulas' in k and v.startswith('Sujeito') for k, v in iva.items()), 'class card: subject to VAT')
ok(any(v.startswith('Isento — artigo 9.º') for k, v in iva.items() if 'Cart' not in k and 'avulsa' not in k), 'session: exempt, with the CIVA article')
ok(any('avulsa' in k and v == '' for k, v in iva.items()), 'walk-in described in words: left empty')
readme = [n for n in z.namelist() if n.endswith('LEIA-ME.txt')][0]
ok('coluna IVA' in z.read(readme).decode('utf-8'), 'LEIA-ME explains the IVA column')

# ───────── last contact (20-year keep period) ─────────
car = patient('Carlos Dias')
ok(call('GET', f'/patients/{car}/legal', owner).json()['last_contact'] is None, 'no contact yet: last_contact is null')
a = appt(car, -40, 10); call('PATCH', f'/appointments/{a}', recep, {'status': 'done'})
appt(car, 3, 10)  # a future booking isn't contact
ok(call('GET', f'/patients/{car}/legal', owner).json()['last_contact'] == iso(today - timedelta(days=40)), 'last_contact = the last visit done (future bookings don’t count)')
call('POST', f'/patients/{car}/tests', ther, {'test_code': 'odi', 'score': 20, 'max_score': 100, 'taken_on': iso(today - timedelta(days=5))})
ok(call('GET', f'/patients/{car}/legal', owner).json()['last_contact'] == iso(today - timedelta(days=5)), 'a later test moves it')

# ───────── delete everything from one day ─────────
text_id = call('POST', '/consent-texts', owner, {'kind': 'treatment', 'lang': 'pt', 'title': 'Consentimento', 'body': 'Texto', 'valid_months': 12}).json()['id']
dan = patient('Daniel Reis'); eva = patient('Eva Nunes')
d1 = appt(dan, 0, 9); call('PATCH', f'/appointments/{d1}', recep, {'status': 'done'})
call('POST', '/payments', recep, {'patient_id': dan, 'method': 'cash', 'items': [{'appointment_id': d1, 'amount_cents': 2000}]}).json()
d2 = appt(eva, 0, 11)
blk = appt(None, 0, 13, kind='block', title='Reunião')
cls = appt(None, 0, 18, kind='class', title='Pilates', capacity=6)
call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': eva})
yest = appt(eva, -1, 10)
n1 = call('POST', f'/patients/{dan}/notes', ther, {'text': 'S: melhor hoje.', 'appointment_id': d1}).json()['id']
ok(call('POST', f'/notes/{n1}/sign', ther).status_code == 200, 'note signed today (PDF in the archive)')
n2 = call('POST', f'/patients/{eva}/notes', ther, {'text': 'rascunho'}).json()['id']
call('POST', f'/patients/{dan}/tests', ther, {'test_code': 'ndi', 'score': 12, 'max_score': 50})
doc = call('POST', f'/patients/{dan}/documents', ther, {'kind': 'report', 'filename': 'RM.pdf', 'data': b64(PDF)}).json()['id']
cons = call('POST', f'/patients/{eva}/consents', recep, {'text_id': text_id, 'granted': True, 'signed_by_name': 'Eva Nunes', 'signature_png': 'data:image/png;base64,' + b64(PNG)}).json()['id']
note_pdf = sql(f"SELECT path FROM documents WHERE note_id = '{n1}'") if PSQL else ''
doc_path = sql(f"SELECT path FROM documents WHERE id = '{doc}'") if PSQL else ''
cons_pdf = sql(f"SELECT path FROM documents WHERE kind = 'consent' AND patient_id = '{eva}'") if PSQL else ''

day = iso(today)
for tok, who in ((ther, 'therapist'), (recep, 'reception')):
    ok(call('GET', f'/days/{day}', tok).status_code == 403, f'{who} can’t see what a day holds')
    ok(call('POST', f'/days/{day}/delete', tok, {'confirm': day}).status_code == 403, f'{who} can’t delete a day')
ok(call('GET', '/days/2026-13-01', owner).status_code == 400, 'bad day refused')
c = call('GET', f'/days/{day}', owner).json()
ok(c['appointments'] >= 3 and c['blocks'] == 1 and c['attendees'] == 1, f"check counts appointments, blocked time and class places ({c['appointments']}, {c['blocks']}, {c['attendees']})")
ok(c['notes_signed'] >= 1 and c['notes_draft'] >= 1 and c['tests'] >= 1 and c['documents'] == 1, 'check counts notes (signed and draft), tests and documents (not consent PDFs)')
ok(c['payments_kept'] >= 1 and c['patients'] >= 2, 'check says payments are kept and how many patients')
ok(call('POST', f'/days/{day}/delete', owner, {'confirm': iso(today - timedelta(days=1))}).status_code == 400, 'the day must be confirmed')
r = call('POST', f'/days/{day}/delete', owner, {'confirm': day})
ok(r.status_code == 200 and r.json()['files_not_removed'] == 0, 'owner deletes the day')
c = call('GET', f'/days/{day}', owner).json()
ok(c['appointments'] == 0 and c['blocks'] == 0 and c['notes_signed'] + c['notes_draft'] == 0 and c['tests'] == 0 and c['documents'] == 0, 'nothing left on that day')
ok(call('GET', f'/patients/{dan}', owner).status_code == 200 and call('GET', f'/patients/{eva}', owner).status_code == 200, 'patients stay')
ok(any(x['id'] == yest for x in call('GET', f"/appointments?from={iso(today - timedelta(days=2))}&to={day}", owner).json()), 'yesterday’s appointment stays')
f = call('GET', f'/patients/{eva}', ther).json()
ok(any(x['id'] == cons for x in f.get('consents', [])), 'consents stay')
if PSQL:
    ok(sql(f"SELECT count(*) FROM payments WHERE patient_id = '{dan}' AND appointment_id IS NULL AND voided_at IS NULL") == '1', 'the payment stays, no longer linked to the visit')
    ok(sql(f"SELECT count(*) FROM notes WHERE id IN ('{n1}', '{n2}')") == '0', 'signed and draft notes are gone')
    ok(sql(f"SELECT count(*) FROM documents WHERE kind = 'consent' AND patient_id = '{eva}'") == '1', 'the consent PDF stays')
    ok(sql(f"SELECT count(*) FROM class_attendees WHERE appointment_id = '{cls}'") == '0', 'class list is gone with the class')
if ARCHIVE and PSQL:
    ok(note_pdf and not os.path.exists(os.path.join(ARCHIVE, note_pdf)), 'the note PDF file is removed from the archive')
    ok(doc_path and not os.path.exists(os.path.join(ARCHIVE, doc_path)), 'the document file is removed from the archive')
    ok(cons_pdf and os.path.exists(os.path.join(ARCHIVE, cons_pdf)), 'the consent PDF file stays in the archive')
lg = call('GET', '/access-log?subject_type=day&limit=10', owner).json()
ok(any(x['action'] == 'day.deleted' and x['subject_id'] == day and 'appointments' in x['detail'] for x in lg), 'day.deleted is in the access log, with counts')

# Deleting a patient keeps their complaint (a legal record), without the link.
r = call('POST', f'/patients/{ana}/delete', owner, {'confirm_name': 'Ana Matos'})
ok(r.status_code == 200, 'patient with a complaint deleted')
x1 = next(x for x in call('GET', '/complaints', owner).json() if x['id'] == c1)
ok(x1['patient_id'] is None and x1['patient_name'] is None, 'the complaint stays, no longer linked to the file')

if LOG:
    txt = open(LOG, encoding='utf-8', errors='replace').read()
    ok(not any(n in txt for n in ('Ana Matos', 'Daniel Reis', 'Eva Nunes', 'Late Person', 'Esperou')), 'server log has no names or complaint text')

print(f'\n{"ALL PASSED" if fails == 0 else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
