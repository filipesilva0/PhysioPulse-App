# End-to-end test of patient documents and the patient archive (uploads, signed notes as PDF).
# Needs a running server with an EMPTY database and ARCHIVE_DIR readable from here (set ARCHIVE to it).
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… python3 server/tests/e2e_documents.py
import os, sys, base64, subprocess, requests, pyotp

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
ARCHIVE = os.environ.get('ARCHIVE')
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
    return requests.request(method, B + path, json=json, headers=h)
b64 = lambda b: base64.b64encode(b).decode()

r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    v = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()
    return v['token'], v['staff']['id']
ther, ther_id = join('therapist', 'ana@clinica.pt')
ther2, _ = join('therapist', 'bruno@clinica.pt')
recep, _ = join('reception', 'rita@clinica.pt')
maria = call('POST', '/patients', recep, {'name': 'Maria Santos', 'birth_date': '1980-05-02'}).json()['id']
joao = call('POST', '/patients', recep, {'name': 'João / Pires'}).json()['id']

PDF = b'%PDF-1.4\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n'
PNG = bytes.fromhex('89504e470d0a1a0a0000000d4948445200000001000000010806000000') + b'\x00' * 20
JPG = b'\xff\xd8\xff\xe0' + b'\x00' * 40

# ── upload: types, size, roles ──
r = call('POST', f'/patients/{maria}/documents', ther, {'kind': 'report', 'filename': 'Relatório RM.pdf', 'data': b64(PDF)})
ok(r.status_code == 200 and r.json()['mime'] == 'application/pdf' and r.json()['filename'] == 'Relatório RM.pdf', 'therapist uploads a PDF report'); rep = r.json()['id']
r = call('POST', f'/patients/{maria}/documents', ther, {'kind': 'photo', 'filename': 'postura.pdf', 'data': 'data:image/png;base64,' + b64(PNG)})
ok(r.status_code == 200 and r.json()['mime'] == 'image/png' and r.json()['filename'] == 'postura.png', 'type comes from the bytes; extension corrected; data: prefix accepted'); photo = r.json()['id']
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'photo', 'filename': 'x.exe', 'data': b64(b'MZ\x90\x00 not a picture')}).status_code == 400, 'non-PDF/photo refused')
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'photo', 'filename': 'x.png', 'data': '%%%'}).status_code == 400, 'bad base64 refused')
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'note', 'filename': 'x.pdf', 'data': b64(PDF)}).status_code == 400, 'kind "note" is reserved for signed notes')
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'xray', 'filename': 'x.pdf', 'data': b64(PDF)}).status_code == 400, 'unknown kind refused')
big = b'%PDF' + b'0' * (10 * 1024 * 1024)
ok(call('POST', f'/patients/{maria}/documents', ther, {'kind': 'report', 'filename': 'big.pdf', 'data': b64(big)}).status_code in (400, 413), 'over 10 MB refused')
ok(call('POST', f'/patients/{maria}/documents', recep, {'kind': 'report', 'filename': 'r.pdf', 'data': b64(PDF)}).status_code == 403, 'reception cannot upload clinical documents')
r = call('POST', f'/patients/{maria}/documents', recep, {'kind': 'prescription', 'filename': 'credencial ADSE.jpg', 'data': b64(JPG)})
ok(r.status_code == 200 and r.json()['mime'] == 'image/jpeg', 'reception scans a prescription'); rx = r.json()['id']
ok(call('POST', f'/patients/00000000-0000-0000-0000-000000000000/documents', ther, {'kind': 'report', 'filename': 'r.pdf', 'data': b64(PDF)}).status_code == 404, 'unknown patient → 404')
r = call('POST', f'/patients/{joao}/documents', ther, {'kind': 'letter', 'filename': '../../etc/passwd.pdf', 'data': b64(PDF)})
ok(r.status_code == 200 and '/' not in r.json()['filename'], 'path characters stripped from file names')

# ── the patient file lists them by role ──
f = call('GET', f'/patients/{maria}', ther).json()
ok({d['id'] for d in f['documents']} == {rep, photo, rx} and f['documents'][0]['uploaded_by_name'], 'therapist sees every document with who added it')
f = call('GET', f'/patients/{maria}', recep).json()
ok([d['id'] for d in f['documents']] == [rx], 'reception sees only the scanned prescription')

# ── open and save ──
r = call('GET', f'/documents/{rep}', ther)
ok(r.status_code == 200 and base64.b64decode(r.json()['data']) == PDF, 'open returns the exact bytes')
ok(call('GET', f'/documents/{rep}', recep).status_code == 404, 'reception cannot open (or detect) a clinical document')
ok(call('GET', f'/documents/{rx}', recep).status_code == 200, 'reception opens the prescription scan')
r = requests.get(B + f'/documents/{photo}/raw', headers={**H, 'Authorization': 'Bearer ' + ther})
ok(r.status_code == 200 and r.content == PNG and r.headers['content-type'] == 'image/png' and 'attachment' in r.headers['content-disposition'], 'raw download for Save a Copy')
ok(requests.get(B + f'/documents/{photo}/raw', headers=H).status_code == 401, 'raw download needs a session')

# ── archive folder on disk ──
if ARCHIVE:
    d = os.path.join(ARCHIVE, 'Utentes')
    folders = sorted(os.listdir(d))
    ok('0001-Maria Santos' in folders and '0002-João _ Pires' in folders, 'one folder per patient: <n.º>-<nome> (safe name)')
    files = os.listdir(os.path.join(d, '0001-Maria Santos', 'Documentos'))
    ok(len(files) == 3 and any(x.endswith('Relatório RM.pdf') for x in files), 'uploads land in Documentos')
    call('PATCH', f'/patients/{maria}', recep, {'name': 'Maria Santos Silva'})
    call('POST', f'/patients/{maria}/documents', ther, {'kind': 'other', 'filename': 'depois.pdf', 'data': b64(PDF)})
    ok(sorted(os.listdir(d)) == folders, 'renamed patient keeps the same folder')

# ── edit and remove ──
ok(call('PATCH', f'/documents/{photo}', ther, {'kind': 'imaging', 'filename': 'Postura frente.png'}).status_code == 200, 'rename and re-classify')
ok(call('PATCH', f'/documents/{rx}', recep, {'kind': 'report'}).status_code == 403, 'reception cannot turn a scan into a clinical document')
ok(call('DELETE', f'/documents/{rep}', ther2).status_code == 409, 'another therapist cannot remove it')
ok(call('DELETE', f'/documents/{rep}', ther).status_code == 200, 'uploader removes their own document the same day')
ok(call('GET', f'/documents/{rep}', ther).status_code == 404, 'removed document is gone')
if ARCHIVE:
    ok(len(os.listdir(os.path.join(ARCHIVE, 'Utentes', '0001-Maria Santos', 'Documentos'))) == 3, 'removed file deleted from the archive')
r = call('POST', f'/patients/{maria}/prescriptions', recep, {'doctor': 'x', 'issued_on': '2026-10-20', 'sessions_total': 5})
ok(r.status_code in (200, 403), 'prescription row for the link test')
if r.status_code != 200:
    r = call('POST', f'/patients/{maria}/prescriptions', ther, {'doctor': 'x', 'issued_on': '2026-10-20', 'sessions_total': 5})
presc = r.json()['id']
ok(call('PATCH', f'/prescriptions/{presc}', ther, {'document_id': rx}).status_code == 200, 'scan linked to the prescription')
ok(call('DELETE', f'/documents/{rx}', owner).status_code == 200, 'owner removes a linked scan')
f = call('GET', f'/patients/{maria}', ther).json()
ok([p['document_id'] for p in f['prescriptions'] if p['id'] == presc] == [None], 'link cleared when the scan is removed')

# ── signed notes become PDFs in the archive ──
note = call('POST', f'/patients/{maria}/notes', ther, {'template': 'lowback', 'text': 'S: dor ao levantar.\n\nO: flexão limitada. ' + 'Exercício com progressão de carga e controlo motor. ' * 12,
            'blocks': [{'type': 'slider', 'key': 'pain', 'label': 'Dor (0–10)', 'value': 6, 'min': 0, 'max': 10},
                       {'type': 'checklist', 'key': 't', 'label': 'Técnicas usadas', 'value': ['Tecido mole', 'Educação'], 'options': []},
                       {'type': 'text', 'key': 'x', 'label': 'Plano', 'value': ''}]}).json()['id']
r = call('POST', f'/notes/{note}/sign', ther)
ok(r.status_code == 200 and r.json().get('document_id'), 'signing a note returns its PDF document'); ndoc = r.json()['document_id']
f = call('GET', f'/patients/{maria}', ther).json()
nd = [d for d in f['documents'] if d['id'] == ndoc]
ok(nd and nd[0]['kind'] == 'note' and nd[0]['note_id'] == note and nd[0]['mime'] == 'application/pdf', 'note PDF listed in documents, linked to the note')
pdf = base64.b64decode(call('GET', f'/documents/{ndoc}', ther).json()['data'])
ok(pdf.startswith(b'%PDF') and pdf.rstrip().endswith(b'%%EOF') and b'Nota de evolu' in pdf and b'Lombalgia' in pdf and b'Assinada por Ana' in pdf, 'PDF has title, template and signer')
ok(b'Tecido mole, Educa' in pdf and b'Plano' not in pdf, 'blocks: filled ones listed, empty ones left out')
ok(call('POST', f'/notes/{note}/pdf', ther).json()['document_id'] == ndoc, 'saving the PDF again keeps one PDF per note')
ok(call('DELETE', f'/documents/{ndoc}', owner).status_code == 409, 'signed-note PDF cannot be removed, even by the owner')
ok(call('PATCH', f'/documents/{ndoc}', owner, {'filename': 'x.pdf'}).status_code == 409, 'signed-note PDF cannot be renamed')
ok(call('GET', f'/documents/{ndoc}', recep).status_code == 404, 'reception cannot open a note PDF')
draft = call('POST', f'/patients/{maria}/notes', ther, {'text': 'rascunho'}).json()['id']
ok(call('POST', f'/notes/{draft}/pdf', ther).status_code == 404, 'no PDF for a draft')
if ARCHIVE:
    notas = os.listdir(os.path.join(ARCHIVE, 'Utentes', '0001-Maria Santos', 'Notas'))
    ok(len(notas) == 1 and notas[0].endswith('.pdf'), 'PDF written to Notas')
    try:
        from pypdf import PdfReader
        txt = PdfReader(os.path.join(ARCHIVE, 'Utentes', '0001-Maria Santos', 'Notas', notas[0])).pages[0].extract_text()
        ok('Maria Santos' in txt and 'Assinada por Ana' in txt, 'PDF opens and reads back')
    except ImportError:
        pass
n2 = call('POST', f'/patients/{maria}/notes', ther, {'text': 'Segunda nota no mesmo minuto.'}).json()['id']
d2 = call('POST', f'/notes/{n2}/sign', ther).json().get('document_id')
ok(d2 and d2 != ndoc, 'a second note signed in the same minute gets its own PDF')
if ARCHIVE:
    notas = sorted(os.listdir(os.path.join(ARCHIVE, 'Utentes', '0001-Maria Santos', 'Notas')))
    ok(len(notas) == 2 and all(' Nota de evolução ' in x for x in notas), 'both PDFs kept, named by date and time')
if PSQL:
    out = subprocess.run(['psql', PSQL, '-tAc', f"DELETE FROM documents WHERE id = '{ndoc}'"], capture_output=True, text=True)
    ok(out.returncode != 0 and "can't be removed" in out.stderr.replace("''", "'"), 'database trigger protects note PDFs')

# ── access log ──
log = call('GET', '/access-log?limit=500', owner).json()
acts = [x['action'] for x in (log if isinstance(log, list) else log.get('rows', []))]
ok({'document.uploaded', 'document.opened', 'document.saved', 'document.updated', 'document.removed'} <= set(acts), 'uploads, opens, saves and removals are logged')
ok(all('Maria' not in str(x.get('detail', '')) for x in (log if isinstance(log, list) else [])), 'no patient names in log details')

print(f"\n{'ALL PASS' if not fails else str(fails) + ' FAILED'}")
sys.exit(1 if fails else 0)
