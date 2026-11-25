# End-to-end test of consent texts (drafts → owner approval) and signed consents saved as PDF in the archive.
# Needs a running server with an EMPTY database. ARCHIVE = the server's ARCHIVE_DIR (checks the folders);
# PSQL_URL adds checks straight in the database; LOG = the server's log file (checks no names are logged).
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… python3 server/tests/e2e_consents.py
import os, sys, io, zlib, struct, base64, zipfile, subprocess, requests, pyotp

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
ARCHIVE = os.environ.get('ARCHIVE')
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

def png_rgba(w, h):
    """A real RGBA PNG like the signature pad makes: transparent with a dark diagonal stroke."""
    raw = b''
    for y in range(h):
        row = b'\x00'
        for x in range(w):
            row += b'\x10\x10\x30\xff' if abs(x * h // w - y) < 2 else b'\x00\x00\x00\x00'
        raw += row
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
SIG = 'data:image/png;base64,' + base64.b64encode(png_rgba(120, 40)).decode()

r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt')
recep = join('reception', 'rita@clinica.pt')

# ── drafts from the first install ──
texts = call('GET', '/consent-texts', recep).json()
kinds = {(t['kind'], t['lang']) for t in texts}
ok(len(texts) == 12 and len(kinds) == 12, 'six draft texts in PT and EN arrive with the server')
ok(all(t['draft'] and not t['current'] and t['approved_at'] is None for t in texts), 'they are drafts, not current')
draft = next(t for t in texts if t['kind'] == 'treatment' and t['lang'] == 'pt')
ok('Norma' not in draft['body'] and 'retirar este consentimento' in draft['body'] and not draft['body'].startswith('\n'), 'treatment draft text is complete and trimmed')
ok(next(t for t in texts if t['kind'] == 'invasive' and t['lang'] == 'pt')['valid_months'] == 12, 'invasive techniques: review after 12 months')

pid = call('POST', '/patients', recep, {'name': 'Joana Ribeiro', 'birth_date': '1990-03-14', 'nif': '234567899', 'phone': '913 000 111'}).json()['id']
r = call('POST', f'/patients/{pid}/consents', recep, {'text_id': draft['id'], 'granted': True, 'signed_by_name': 'Joana Ribeiro', 'signature_png': SIG})
ok(r.status_code == 400 and 'approved' in r.json().get('message', r.text), 'a draft can’t be signed')

ok(call('POST', f"/consent-texts/{draft['id']}/approve", ther).status_code == 403, 'therapist can’t approve')
ok(call('POST', f"/consent-texts/{draft['id']}/approve", recep).status_code == 403, 'reception can’t approve')
ok(call('POST', f"/consent-texts/{draft['id']}/approve", owner).status_code == 200, 'owner approves the draft')
ok(call('POST', f"/consent-texts/{draft['id']}/approve", owner).status_code == 409, 'approving twice → already approved')
t = next(x for x in call('GET', '/consent-texts', owner).json() if x['id'] == draft['id'])
ok(t['current'] and not t['draft'], 'approved draft is now the current text')

# The owner edits the media text → a new version, approved straight away (the owner wrote it).
media = next(x for x in texts if x['kind'] == 'media' and x['lang'] == 'pt')
m2 = call('POST', '/consent-texts', owner, {'kind': 'media', 'lang': 'pt', 'title': media['title'], 'body': media['body'] + '\n\nTexto revisto.'}).json()
cur = {x['id']: x for x in call('GET', '/consent-texts', owner).json()}
ok(m2['version'] == 2 and cur[m2['id']]['current'] and cur[media['id']]['draft'] and not cur[media['id']]['current'], 'owner’s edit = v2, approved and current; v1 stays a draft')

# ── signing writes a PDF ──
r = call('POST', f'/patients/{pid}/consents', recep, {'text_id': draft['id'], 'granted': True, 'signed_by_name': 'Joana Ribeiro', 'signature_png': SIG})
ok(r.status_code == 200 and r.json()['document_id'], 'reception records a signed consent; PDF saved')
consent, doc = r.json()['id'], r.json()['document_id']
r = call('POST', f'/patients/{pid}/consents', recep, {'text_id': m2['id'], 'granted': False, 'signed_by_name': 'Joana Ribeiro', 'signature_png': SIG})
ok(r.status_code == 200 and r.json()['document_id'], 'a refusal (photos) is saved as PDF too')
refusal_doc = r.json()['document_id']

d = call('GET', f'/documents/{doc}', recep).json()
pdf = base64.b64decode(d.get('data', ''))
ok(d.get('kind') == 'consent' and d['filename'].startswith('20') and ' Consentimento tratamento ' in d['filename'] and pdf.startswith(b'%PDF'), 'reception opens the consent PDF: ' + d.get('filename', ''))
ok(b'/Subtype /Image' in pdf and b'/Im1 Do' in pdf, 'the signature is drawn in the PDF')
ok(b'Joana Ribeiro' in pdf and b'Consentimento informado para tratamento de fisioterapia' in pdf, 'PDF has the patient and the text title')
ok(b'Decis' not in pdf, 'treatment consent has no agree/refuse line')
rp = base64.b64decode(call('GET', f'/documents/{refusal_doc}', owner).json()['data'])
ok(b'Decis\xe3o: n\xe3o autorizo' in rp and b'Texto revisto.' in rp, 'refusal PDF says “não autorizo” and has the owner’s v2 text')
if ARCHIVE:
    folder = [f for f in os.listdir(os.path.join(ARCHIVE, 'Utentes')) if f.startswith('0001-')]
    files = sorted(os.listdir(os.path.join(ARCHIVE, 'Utentes', folder[0], 'Consentimentos'))) if folder else []
    ok(len(files) == 2 and all(f.endswith('.pdf') for f in files), 'two PDFs in Utentes/0001-…/Consentimentos: ' + str(files))

f = call('GET', f'/patients/{pid}', recep).json()
c = next(x for x in f['consents'] if x['id'] == consent)
ok(c['pdf_id'] == doc and c['revoked_pdf_id'] is None, 'the file links each consent to its PDF')
ok({x['kind'] for x in f['documents']} == {'consent'}, 'reception sees the consent PDFs in Documents')

# Part of the record: can't be renamed or removed, not even by the owner.
ok(call('PATCH', f'/documents/{doc}', owner, {'filename': 'outro.pdf'}).status_code == 409, 'consent PDF can’t be renamed')
ok(call('DELETE', f'/documents/{doc}', owner).status_code == 409, 'consent PDF can’t be removed')
if PSQL:
    ok('can' in subprocess.run(['psql', PSQL, '-tAc', f"DELETE FROM documents WHERE id = '{doc}'"], capture_output=True, text=True).stderr, 'database trigger refuses removing it')
ok(call('POST', f'/patients/{pid}/documents', owner, {'kind': 'consent', 'filename': 'x.pdf', 'data': base64.b64encode(b'%PDF-1.4\n%%EOF').decode()}).status_code == 400, 'nobody uploads a fake “consent” document')

# Try again is safe: same documents, no duplicates.
r = call('POST', f'/consents/{consent}/pdf', recep).json()
ok(r['document_id'] == doc and r['revoked_document_id'] is None, 'Try Again returns the existing PDF')

# ── withdrawal ──
r = call('POST', f'/consents/{refusal_doc}/revoke', recep)
ok(r.status_code == 404, 'withdraw needs a consent id, not a document id')
r = call('POST', f'/consents/{consent}/revoke', recep)
ok(r.status_code == 200 and r.json()['document_id'], 'withdrawal saved as its own PDF')
wd = r.json()['document_id']
ok(call('POST', f'/consents/{consent}/revoke', recep).status_code == 409, 'withdrawing twice → already withdrawn')
w = call('GET', f'/documents/{wd}', ther).json()
wp = base64.b64decode(w['data'])
ok(' Retirada consentimento tratamento ' in w['filename'] and b'Retirada de consentimento' in wp and b'Registado por' in wp and b'Rita' in wp, 'withdrawal PDF says who recorded it: ' + w['filename'])
r = call('POST', f'/consents/{consent}/pdf', recep).json()
ok(r['document_id'] == doc and r['revoked_document_id'] == wd, 'Try Again after withdrawal: both PDFs, no duplicates')
c = next(x for x in call('GET', f'/patients/{pid}', ther).json()['consents'] if x['id'] == consent)
ok(c['state'] == 'revoked' and c['revoked_pdf_id'] == wd, 'file shows withdrawn with its PDF')

# ── full export carries them ──
z = zipfile.ZipFile(io.BytesIO(call('GET', f'/patients/{pid}/export/full', ther).content))
names = z.namelist()
cons = [n for n in names if '/Consentimentos/' in n]
ok(len(cons) == 3 and z.testzip() is None, 'full export has a Consentimentos/ folder with the 3 PDFs')
ok('Consentimentos/' in z.read([n for n in names if n.endswith('LEIA-ME.txt')][0]).decode(), 'LEIA-ME explains the folder')
z = zipfile.ZipFile(io.BytesIO(call('GET', f'/patients/{pid}/export/summary', ther).content))
ok(not any('/Consentimentos/' in n for n in z.namelist()), 'summary export stays PDF-only')

# ── access log, privacy ──
if PSQL:
    acts = sql("SELECT string_agg(DISTINCT action, ',') FROM access_log")
    ok(all(a in acts for a in ('consent_text.approved', 'consent_text.created', 'consent.signed', 'consent.revoked', 'consent.pdf_saved', 'document.opened')), 'logged: ' + acts)
    ok(sql("SELECT count(*) FROM access_log WHERE detail ILIKE '%Joana%' OR subject_id ILIKE '%Joana%'") == '0', 'no patient name in the access log details')

# ── anonymise removes them (they carry the signature and name) ──
r = call('POST', f'/patients/{pid}/anonymise', owner, {'confirm_name': 'Joana Ribeiro'})
ok(r.status_code == 200, 'owner anonymises')
f = call('GET', f'/patients/{pid}', owner).json()
ok(f['documents'] == [] and all(x['pdf_id'] is None for x in f['consents']), 'consent PDFs removed with the other documents')
if ARCHIVE:
    ok(not any(x.startswith('0001-') for x in os.listdir(os.path.join(ARCHIVE, 'Utentes'))), 'patient folder gone')

if LOG:
    log = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Joana' not in log and 'Ribeiro' not in log, 'server log has no patient names')

print('\nALL PASS' if fails == 0 else f'\n{fails} FAILED')
sys.exit(1 if fails else 0)
