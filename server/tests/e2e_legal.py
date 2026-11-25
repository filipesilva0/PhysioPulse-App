# End-to-end test of the legal tools on the patient file: export, anonymise, delete.
# Needs a running server with an EMPTY database. ARCHIVE = the server's ARCHIVE_DIR (checks the folders);
# PSQL_URL adds checks straight in the database.
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… python3 server/tests/e2e_legal.py
import os, sys, io, base64, zipfile, subprocess, requests, pyotp
from datetime import datetime, timedelta

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
ARCHIVE = os.environ.get('ARCHIVE')
PSQL = os.environ.get('PSQL_URL')
H = {'X-Device': 'Gabinete-1', 'X-Device-Key': DEVICE_KEY}
fails = 0
def valid_nif(n):
    """A different valid Portuguese NIF for each n (the server checks the check digit and refuses repeats)."""
    d = [2] + [int(c) for c in f'{n:07d}']
    s = sum(x * (9 - i) for i, x in enumerate(d)) % 11
    return ''.join(map(str, d)) + str(0 if s < 2 else 11 - s)
_nifs = iter(range(1, 10_000))

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

def appt(pid, days=-1, hour=10, **kw):
    s = (datetime.now() + timedelta(days=days)).replace(hour=hour, minute=0, second=0, microsecond=0).astimezone()
    body = {'kind': 'session', 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': ther_id,
            'patient_id': pid, 'allow_overlap': True, **kw}
    r = call('POST', '/appointments', recep, body); assert r.status_code == 200, r.text
    return r.json()['ids'][0]

def full_patient(name):
    pid = call('POST', '/patients', recep, {'name': name, 'birth_date': '1980-05-02', 'nif': valid_nif(next(_nifs)), 'phone': '912 345 678',
                                             'email': 'x@exemplo.pt', 'address': 'Rua das Flores 1', 'therapist_id': ther_id, 'admin_note': 'paga por MB WAY'}).json()['id']
    call('PUT', f'/patients/{pid}/anamnesis', ther, {'main_complaint': 'Lombalgia mecânica', 'goals': 'Voltar a correr',
                                                     'regions': [{'region': 'lumbar', 'symptoms': ['pain', 'stiffness'], 'notes': 'pior de manhã'}], 'red_flags': ['night_pain']})
    call('POST', f'/patients/{pid}/tests', ther, {'test_code': 'odi', 'score': 32, 'max_score': 100})
    a1 = appt(pid, -3); call('PATCH', f'/appointments/{a1}', recep, {'status': 'done'})
    n = call('POST', f'/patients/{pid}/notes', ther, {'template': 'lowback', 'text': 'S: melhor.\nO: flexão a 20 cm.',
             'blocks': [{'type': 'slider', 'key': 'pain', 'label': 'Dor (0–10)', 'value': 4, 'min': 0, 'max': 10}], 'appointment_id': a1}).json()['id']
    call('POST', f'/notes/{n}/sign', ther)
    call('POST', f'/patients/{pid}/notes', ther, {'text': 'rascunho por assinar'})
    call('POST', f'/patients/{pid}/documents', ther, {'kind': 'report', 'filename': 'Relatório RM.pdf', 'data': b64(PDF)})
    call('POST', f'/patients/{pid}/documents', ther, {'kind': 'photo', 'filename': 'postura.png', 'data': b64(PNG)})
    call('POST', f'/patients/{pid}/prescriptions', ther, {'doctor': 'Dr. Costa', 'issued_on': '2026-10-20', 'sessions_total': 10, 'payer': 'ADSE'})
    call('POST', f'/patients/{pid}/consents', recep, {'text_id': text_id, 'granted': True, 'signed_by_name': name, 'signature_png': 'data:image/png;base64,' + b64(PNG)})
    future = appt(pid, 5)
    return pid, a1, n, future

text_id = call('POST', '/consent-texts', owner, {'kind': 'treatment', 'lang': 'pt', 'title': 'Consentimento para tratamento', 'body': 'Texto', 'valid_months': 12}).json()['id']
maria, m_a1, m_note, m_future = full_patient('Maria Santos')
joao, j_a1, j_note, j_future = full_patient('João Pires')
rui, r_a1, r_note, r_future = full_patient('Rui Lopes')
# João pays for his session (a fiscal record); Rui never paid.
r = call('POST', '/payments', recep, {'patient_id': joao, 'method': 'cash', 'items': [{'appointment_id': j_a1, 'amount_cents': 3500}]})
ok(r.status_code == 200, 'João pays for his session')
call('POST', '/waitlist', recep, {'patient_id': rui})

# ── export ──
def export(pid, kind, tok):
    return requests.get(B + f'/patients/{pid}/export/{kind}', headers={**H, 'Authorization': 'Bearer ' + tok})
r = export(maria, 'summary', ther)
ok(r.status_code == 200 and r.headers['content-type'] == 'application/zip' and 'attachment' in r.headers['content-disposition'], 'therapist exports a summary (zip download)')
z = zipfile.ZipFile(io.BytesIO(r.content)); ok(z.testzip() is None, 'summary zip is valid (CRC)')
names = z.namelist()
ok(any(n.endswith('Processo clínico (resumo).pdf') for n in names) and any(n.endswith('LEIA-ME.txt') for n in names) and len(names) == 2, 'summary: one PDF and LEIA-ME only')
pdf = z.read([n for n in names if n.endswith('.pdf')][0])
ok(pdf.startswith(b'%PDF') and b'Maria Santos' in pdf and b'Lombalgia mec' in pdf and b'Oswestry' in pdf and b'Dor noturna' in pdf, 'summary PDF: identity, anamnesis, tests, red flags')
ok(b'flex\xe3o a 20 cm' not in pdf and b'Notas de evolu' in pdf, 'summary lists signed notes without their text')
ok(b'rascunho por assinar' not in pdf and b'1 rascunho' in pdf, 'drafts are not exported, only counted')
ok(b'Rua das Flores' in pdf and b'Dr. Costa' in pdf and b'ADSE' in pdf, 'contacts and prescriptions included')
r = export(maria, 'full', owner); z = zipfile.ZipFile(io.BytesIO(r.content))
names = z.namelist()
ok(r.status_code == 200 and z.testzip() is None, 'owner exports the full record')
ok(sum('/Documentos/' in n for n in names) == 2 and sum('/Notas/' in n for n in names) == 1, 'full: original documents and note PDFs included')
rep = [n for n in names if n.endswith('Relatório RM.pdf')]
ok(rep and z.read(rep[0]) == PDF, 'original files are byte-for-byte the same')
pdf = z.read([n for n in names if n.endswith('Processo clínico.pdf')][0])
ok(b'flex\xe3o a 20 cm' in pdf and b'Dor \\(0' in pdf and b': 4)' in pdf and b'realizada' in pdf, 'full PDF has note text, blocks and every appointment')
ok('Maria' not in r.headers['content-disposition'], 'file name has the number, not the name')
ok(export(maria, 'full', recep).status_code == 403, 'reception cannot export a clinical record')
ok(export(maria, 'everything', ther).status_code == 400, 'unknown export kind refused')
ok(export('00000000-0000-0000-0000-000000000000', 'full', ther).status_code == 404, 'unknown patient → 404')
if PSQL:
    ok(sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.exported' AND subject_id = '{maria}'") == '2', 'each export logged once (patient.exported)')
    ok(sql(f"SELECT string_agg(detail, ',' ORDER BY id) FROM access_log WHERE action = 'patient.exported'") == 'summary,full', 'log says which kind')

# ── what will happen ──
ok(call('GET', f'/patients/{joao}/legal', ther).status_code == 403, 'only the owner sees the legal check')
c = call('GET', f'/patients/{joao}/legal', owner).json()
ok(c['notes_signed'] == 1 and c['notes_draft'] == 1 and c['documents'] == 4 and c['upcoming'] == 1 and c['payments'] == 1, 'check counts notes, documents (incl. the consent PDF), bookings, payments')
ok(c['name'] == 'João Pires' and c['owed_cents'] == 0 and c['anonymised'] is False, 'check: name, nothing owed')
ok(call('GET', f'/patients/{rui}/legal', owner).json()['owed_cents'] > 0, 'check shows money owed (Rui never paid)')

# ── anonymise ──
ok(call('POST', f'/patients/{maria}/anonymise', ther, {'confirm_name': 'Maria Santos'}).status_code == 403, 'therapist cannot anonymise')
ok(call('POST', f'/patients/{maria}/anonymise', recep, {'confirm_name': 'Maria Santos'}).status_code == 403, 'reception cannot anonymise')
ok(call('POST', f'/patients/{maria}/anonymise', owner, {'confirm_name': 'Maria'}).status_code == 400, 'wrong name typed → refused')
ok(call('POST', f'/patients/{maria}/anonymise', owner, {'confirm_name': ''}).status_code == 400, 'empty name → refused')
folders_before = sorted(os.listdir(os.path.join(ARCHIVE, 'Utentes'))) if ARCHIVE else []
r = call('POST', f'/patients/{maria}/anonymise', owner, {'confirm_name': '  maria  santos '})
ok(r.status_code == 200 and r.json()['folder_removed'] is True, 'owner anonymises (case and spaces ignored)')
f = call('GET', f'/patients/{maria}', owner).json()
p = f['patient']
ok(p['name'] == 'Utente anonimizado' and p['nif'] is None and p['phone'] is None and p['email'] is None and p['address'] is None and p['admin_note'] == '', 'name, NIF and contacts removed')
ok(p['birth_date'] == '1980-01-01' and p['active'] is False and p['anonymised_at'], 'birth date keeps the year; file inactive')
ok(f['anamnesis']['main_complaint'] == 'Lombalgia mecânica' and len(f['tests']) == 1 and len([n for n in f['notes'] if n['signed_at']]) == 1, 'clinical content kept')
ok(f['documents'] == [], 'documents (and note PDFs) removed')
ok(all(c['signed_by_name'] == '' for c in f['consents']) and len(f['consents']) == 1, 'consent kept without the signer’s name')
ok([a['status'] for a in f['appointments'] if a['id'] == m_future] == ['cancelled'], 'future booking cancelled')
ok(call('POST', f'/patients/{maria}/anonymise', owner, {'confirm_name': 'Utente anonimizado'}).status_code == 409, 'cannot anonymise twice')
ok(call('PATCH', f'/patients/{maria}', recep, {'phone': '911'}).status_code == 404, 'an anonymised file can’t get contacts back')
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'report', 'filename': 'r.pdf', 'data': b64(PDF)}).status_code == 404, 'no new documents for an anonymised file')
lst = call('GET', '/patients?q=maria', recep).json()
ok(lst == [], 'anonymised patient not found by the old name')
if ARCHIVE:
    after = sorted(os.listdir(os.path.join(ARCHIVE, 'Utentes')))
    ok(not any(x.startswith('0001-') for x in after) and len(after) == len(folders_before) - 1, 'archive folder removed from disk')
if PSQL:
    ok(sql(f"SELECT signature_png FROM consents WHERE patient_id = '{maria}'") == '', 'signature image removed in the database')
    ok(sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.anonymised' AND subject_id = '{maria}'") == '1', 'anonymise logged')
    ok(sql("SELECT count(*) FROM access_log WHERE detail ILIKE '%maria%' OR detail ILIKE '%santos%'") == '0', 'the access log never holds the name')

# ── delete: with payments (fiscal records kept, no name) ──
ok(call('POST', f'/patients/{joao}/delete', ther, {'confirm_name': 'João Pires'}).status_code == 403, 'therapist cannot delete')
ok(call('POST', f'/patients/{joao}/delete', owner, {'confirm_name': 'Joao Pires'}).status_code == 400, 'name must match exactly (accents too)')
r = call('POST', f'/patients/{joao}/delete', owner, {'confirm_name': 'João Pires'})
ok(r.status_code == 200 and r.json()['payments_kept'] == 1, 'owner deletes João; one payment kept')
ok(call('GET', f'/patients/{joao}', owner).status_code == 404, 'deleted file can’t be opened')
ok(call('GET', f'/patients/{joao}/legal', owner).status_code == 404, 'deleted patient gone from the legal tools')
ok(export(joao, 'full', owner).status_code == 404, 'deleted patient can’t be exported')
ok(all(x['id'] != joao for x in call('GET', '/patients?include_inactive=true', owner).json()), 'not in the list, even with inactive patients')
pays = call('GET', '/payments', owner).json()
pays = pays if isinstance(pays, list) else pays.get('payments', pays.get('items', []))
jp = [x for x in pays if x.get('patient_id') == joao]
ok(len(jp) == 1 and jp[0]['amount_cents'] == 3500 and jp[0].get('patient_name') == 'Utente apagado', 'payment still in Finance, as “Utente apagado”')
if PSQL:
    ok(sql(f"SELECT count(*) FROM notes WHERE patient_id = '{joao}'") == '0' and sql(f"SELECT count(*) FROM anamnesis WHERE patient_id = '{joao}'") == '0'
       and sql(f"SELECT count(*) FROM test_results WHERE patient_id = '{joao}'") == '0' and sql(f"SELECT count(*) FROM consents WHERE patient_id = '{joao}'") == '0'
       and sql(f"SELECT count(*) FROM appointments WHERE patient_id = '{joao}'") == '0' and sql(f"SELECT count(*) FROM prescriptions WHERE patient_id = '{joao}'") == '0',
       'signed notes, anamnesis, tests, consents, appointments, prescriptions all removed')
    ok(sql(f"SELECT name || '|' || coalesce(nif,'-') || '|' || coalesce(birth_date::text,'-') || '|' || (deleted_at IS NOT NULL) FROM patients WHERE id = '{joao}'") == 'Utente apagado|-|-|true', 'placeholder row has no name, NIF or birth date')
    ok(sql(f"SELECT appointment_id IS NULL FROM payments WHERE patient_id = '{joao}'") == 't', 'payment unlinked from the deleted appointment')
    ok(sql(f"SELECT detail FROM access_log WHERE action = 'patient.deleted' AND subject_id = '{joao}'") == '1 payments kept without a name', 'delete logged with what was kept')
if ARCHIVE:
    ok(not any(x.startswith('0002-') for x in os.listdir(os.path.join(ARCHIVE, 'Utentes'))), 'João’s archive folder removed')

# ── delete: no payments (removed outright) ──
r = call('POST', f'/patients/{rui}/delete', owner, {'confirm_name': 'Rui Lopes'})
ok(r.status_code == 200 and r.json()['payments_kept'] == 0, 'owner deletes Rui (no payments)')
if PSQL:
    ok(sql(f"SELECT count(*) FROM patients WHERE id = '{rui}'") == '0', 'row removed outright')
    ok(sql(f"SELECT count(*) FROM waitlist WHERE patient_id = '{rui}'") == '0' and sql(f"SELECT count(*) FROM documents WHERE patient_id = '{rui}'") == '0', 'waitlist entry and documents gone')
    ok(sql("SELECT count(*) FROM access_log WHERE action = 'patient.deleted'") == '2', 'both deletions in the access log')
    ok(sql("SELECT count(*) FROM access_log WHERE detail ILIKE '%rui%' OR detail ILIKE '%pires%'") == '0', 'no names in the log')
    # The locks still hold outside the legal tools.
    ana = call('POST', '/patients', recep, {'name': 'Ana Lima'}).json()['id']
    n2 = call('POST', f'/patients/{ana}/notes', ther, {'text': 'nota'}).json()['id']; call('POST', f'/notes/{n2}/sign', ther)
    out = subprocess.run(['psql', PSQL, '-tAc', f"DELETE FROM notes WHERE id = '{n2}'"], capture_output=True, text=True)
    ok('can''t be changed' in out.stderr or 'signed note' in out.stderr, 'signed notes still locked without the legal-tool flag')
if ARCHIVE:
    ok(not any(x.startswith('0003-') for x in os.listdir(os.path.join(ARCHIVE, 'Utentes'))), 'Rui’s archive folder removed')
ok(call('POST', f'/patients/{rui}/delete', owner, {'confirm_name': 'Rui Lopes'}).status_code == 404, 'deleting again → 404')

if PSQL:
    # 0015: the record locks answer to physiopulse.legal_tool now; the old name opens nothing.
    ok(sql("SELECT count(*) FROM pg_proc WHERE prosrc LIKE '%clinica.legal_tool%'") == '0' and sql("SELECT count(*) FROM pg_proc WHERE prosrc LIKE '%physiopulse.legal_tool%'") == '3', 'the record locks use the new setting name')
    out = subprocess.run(['psql', PSQL, '-tAc', "BEGIN; SET LOCAL clinica.legal_tool = 'on'; DELETE FROM notes WHERE signed_at IS NOT NULL; ROLLBACK;"], capture_output=True, text=True)
    ok('signed note' in out.stderr, 'the old setting name no longer unlocks a signed note')

print(f'\n{"ALL PASS" if fails == 0 else str(fails) + " FAILED"}')
sys.exit(1 if fails else 0)
