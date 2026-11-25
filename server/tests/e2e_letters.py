# End-to-end test of the reimbursement pack (one PDF) and discharge letters (local-AI draft, owner approves).
# Needs a running server with an EMPTY database, started with OLLAMA_URL=http://127.0.0.1:11999 (this test
# runs a stand-in for Ollama on that port). ARCHIVE = the server's ARCHIVE_DIR; PSQL_URL adds database
# checks; LOG = the server's log file (checks no names are logged). Uses `qpdf` and `pdftotext` if present.
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_letters.py
import os, sys, io, json, glob, base64, shutil, threading, subprocess, tempfile, requests, pyotp
from datetime import datetime, timedelta, date
from http.server import BaseHTTPRequestHandler, HTTPServer

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
ARCHIVE = os.environ.get('ARCHIVE')
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Gabinete-2', 'X-Device-Key': DEVICE_KEY}
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
def text_of(pdf_bytes):
    if not shutil.which('pdftotext'): return None
    return subprocess.run(['pdftotext', '-layout', '-', '-'], input=pdf_bytes, capture_output=True).stdout.decode('utf-8', 'replace')
def pages_of(pdf_bytes):
    if not shutil.which('qpdf'): return None
    with tempfile.NamedTemporaryFile(suffix='.pdf') as f:
        f.write(pdf_bytes); f.flush()
        return int(subprocess.run(['qpdf', '--show-npages', f.name], capture_output=True, text=True).stdout.strip() or 0)

# ── a stand-in for Ollama: records what it's asked, answers like Ollama does ──
asked = []
mode = {'answer': 'ok'}
class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        asked.append((self.path, body))
        if self.path == '/api/show':
            out, code = ({'details': {}}, 200) if mode['answer'] != 'no_model' else ({'error': 'model not found'}, 404)
        elif mode['answer'] == 'no_model':
            out, code = {'error': "model 'gemma3:12b' not found"}, 404
        else:
            out, code = {'model': body['model'], 'done': True, 'response':
                '<think>planning</think>A utente foi acompanhada em fisioterapia por **cervicalgia**.\n\nApresentou melhoria da dor e da função.\n\nÀ data da alta: [a completar].'}, 200
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers()
        self.wfile.write(data)
fake = HTTPServer(('127.0.0.1', 11999), Fake)
threading.Thread(target=fake.serve_forever, daemon=True).start()

# ── people ──
r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Sousa', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': email.split('@')[0].title(), 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt')
recep = join('reception', 'rita@clinica.pt')
ther_id = next(s['id'] for s in call('GET', '/therapists', recep).json() if s['name'] == 'Ana')

# ── a patient with a record ──
pid = call('POST', '/patients', recep, {'name': 'Joana Ribeiro', 'birth_date': '1984-03-14', 'nif': '234567899', 'sns_number': '123456789', 'phone': '913000111', 'therapist_id': ther_id}).json()['id']
other = call('POST', '/patients', recep, {'name': 'Rui Matos'}).json()['id']
call('PUT', f'/patients/{pid}/anamnesis', ther, {'main_complaint': 'Cervicalgia mecânica', 'goals': 'Trabalhar sem dor',
     'regions': [{'region': 'cervical', 'symptoms': ['pain', 'stiffness'], 'notes': ''}], 'red_flags': []})
issued = (date.today() - timedelta(days=30)).isoformat()
rx = call('POST', f'/patients/{pid}/prescriptions', ther, {'doctor': 'Dr. Lopes', 'specialty': 'Medicina Geral', 'issued_on': issued, 'sessions_total': 10, 'payer': 'ADSE'}).json()['id']
def appt(days_ago, hour=10):
    s = (datetime.now() - timedelta(days=days_ago)).replace(hour=hour, minute=0, second=0, microsecond=0).astimezone()
    r = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(),
             'therapist_id': ther_id, 'patient_id': pid, 'prescription_id': rx, 'allow_overlap': True})
    aid = r.json()['ids'][0]
    call('PATCH', f'/appointments/{aid}', recep, {'status': 'done'})
    return aid
visits = [appt(d) for d in (20, 13, 6)]
old_visit = appt(60)  # before the prescription: outside the default period
for d, score in ((20, 32), (6, 12)):
    call('POST', f'/patients/{pid}/tests', ther, {'test_code': 'ndi', 'score': score, 'max_score': 50, 'taken_on': (date.today() - timedelta(days=d)).isoformat()})
for tech in (['Terapia manual', 'Exercício terapêutico'], ['Exercício terapêutico']):
    n = call('POST', f'/patients/{pid}/notes', ther, {'template': 'neck', 'blocks': [{'type': 'checklist', 'label': 'Técnicas', 'value': tech}, {'type': 'slider', 'label': 'Dor', 'value': 3}], 'text': 'Boa evolução.'}).json()['id']
    call('POST', f'/notes/{n}/sign', ther)

# Invoices: a real multi-object PDF (made by qpdf with object streams), a JPEG photo of an invoice, one with no file.
def invoice_pdf(n):
    # A plain PDF whose pages inherit their Resources and MediaBox from the page tree (like many invoicing tools).
    content = f'BT /F 14 Tf 72 760 Td (FATURA FR 2026/{n} - Joana Ribeiro) Tj ET'.encode()
    objs = [b'<< /Type /Catalog /Pages 2 0 R >>',
            b'<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595 842] /Resources << /Font << /F 4 0 R >> >> >>',
            b'<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>',
            b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
            b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream']
    out, offs = b'%PDF-1.5\n', []
    for i, o in enumerate(objs):
        offs.append(len(out)); out += b'%d 0 obj\n' % (i + 1) + o + b'\nendobj\n'
    x = len(out)
    out += b'xref\n0 %d\n0000000000 65535 f \n' % (len(objs) + 1) + b''.join(b'%010d 00000 n \n' % o for o in offs)
    out += b'trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % (len(objs) + 1, x)
    if shutil.which('qpdf'):
        with tempfile.TemporaryDirectory() as d:
            open(f'{d}/a.pdf', 'wb').write(out)
            subprocess.run(['qpdf', '--object-streams=generate', f'{d}/a.pdf', f'{d}/b.pdf'], check=True)
            return open(f'{d}/b.pdf', 'rb').read()
    return out
def tiny_jpeg():
    try:
        from PIL import Image, ImageDraw
        im = Image.new('RGB', (600, 800), 'white'); d = ImageDraw.Draw(im)
        d.rectangle([40, 40, 560, 140], outline='black', width=4); d.text((60, 70), 'FATURA FR 2026/102 (foto)', fill='black')
        b = io.BytesIO(); im.save(b, 'JPEG', quality=80); return b.getvalue()
    except ImportError:
        pass
    # 8×8 grey baseline JPEG (valid, decodable).
    return base64.b64decode('/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAAMCAgICAgMCAgIDAwMDBAYEBAQEBAgGBgUGCQgKCgkICQkKDA8MCgsOCwkJDRENDg8QEBEQCgwSExIQEw8QEBD/wAALCAAIAAgBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAACf/EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AKp//2Q==')
def pay(days_ago, cents, invoice, data, mime_name):
    pid_ = call('POST', '/payments', recep, {'patient_id': pid, 'method': 'mbway', 'received_on': (date.today() - timedelta(days=days_ago)).isoformat(),
                'items': [{'amount_cents': cents, 'description': 'Sessão de fisioterapia'}]}).json()['ids'][0]
    body = {'invoice_number': invoice}
    if data:
        f = call('POST', '/finance/files', recep, {'kind': 'invoice', 'filename': mime_name, 'data': base64.b64encode(data).decode()}).json()['id']
        body['invoice_file_id'] = f
    call('PATCH', f'/payments/{pid_}', recep, body)
    return pid_
p1 = pay(20, 2000, 'FR 2026/101', invoice_pdf(101), 'FR101.pdf')
p2 = pay(13, 2000, 'FR 2026/102', tiny_jpeg(), 'FR102.jpg')
p3 = pay(6, 2000, 'FR 2026/103', None, '')
p_other = call('POST', '/payments', recep, {'patient_id': other, 'method': 'cash', 'items': [{'amount_cents': 500, 'description': 'x'}]}).json()['ids'][0]
# A scanned prescription (PNG) linked to the prescription.
import zlib, struct
def make_png(w, h):
    raw = b''.join(b'\x00' + bytes([200, 200, 200]) * w for _ in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
png = make_png(60, 80)
scan = call('POST', f'/patients/{pid}/documents', recep, {'kind': 'prescription', 'filename': 'credencial.png', 'data': base64.b64encode(png).decode()}).json()['id']
ok(call('PATCH', f'/prescriptions/{rx}', ther, {'document_id': scan}).status_code == 200, 'prescription scan linked')

# ═════════ reimbursement pack ═════════
ok(call('GET', f'/patients/{pid}/reimbursement', recep).status_code == 403, 'reception can’t make a pack (it holds a clinical report)')
ok(call('POST', f'/patients/{pid}/reimbursement', recep, {'from': issued, 'to': date.today().isoformat(), 'report': 'x'}).status_code == 403, '…not even by calling the server directly')
r = call('GET', f'/patients/{pid}/reimbursement?prescription_id={rx}', ther)
ok(r.status_code == 200, 'therapist opens the pack sheet')
pv = r.json()
ok(pv['from'] == issued and pv['to'] == date.today().isoformat(), 'period defaults to the prescription date → today')
ok(len(pv['sessions']) == 3, 'three done sessions in the period (the older one is left out)')
ok({p['id'] for p in pv['payments']} == {p1, p2, p3}, 'this patient’s payments in the period, nobody else’s')
ok(pv['payer'] == 'ADSE' and pv['prescription_id'] == rx, 'payer taken from the prescription')
ok(pv['report'].startswith('Joana Ribeiro realizou 3 sessões') and 'Dr. Lopes' in pv['report'] and 'NDI: de 32/50 para 12/50' in pv['report'], 'report pre-filled from the file: ' + pv['report'][:80])
ok('exercício terapêutico e terapia manual' in pv['report'], 'techniques from signed notes, most used first')
r = call('GET', f'/patients/{pid}/reimbursement?from=2000-01-01', ther)
ok(len(r.json()['sessions']) == 4, 'a wider period brings the older session')
ok(call('GET', f'/patients/{pid}/reimbursement?prescription_id={other}', ther).status_code == 404, 'unknown prescription → 404')

req = {'prescription_id': rx, 'from': pv['from'], 'to': pv['to'], 'payment_ids': [p1, p2, p3], 'payer': 'ADSE',
       'report': pv['report'] + '\n\nMantém exercícios em casa.', 'professional_id': 'C-12345'}
ok(call('POST', f'/patients/{pid}/reimbursement', ther, {**req, 'report': '  '}).status_code == 400, 'the report can’t be empty')
ok(call('POST', f'/patients/{pid}/reimbursement', ther, {**req, 'payment_ids': [p1, p_other]}).status_code == 400, 'someone else’s payment can’t go in')
r = call('POST', f'/patients/{pid}/reimbursement', ther, req)
ok(r.status_code == 200, 'therapist makes the pack: ' + r.text[:200])
pack = r.json()
ok(pack['filename'] == f'{date.today().isoformat()} Pacote de reembolso ADSE.pdf', 'file name: ' + pack['filename'])
ok(pack['attached'] == ['Prescrição médica', f"Fatura FR 2026/101 de {(date.today() - timedelta(days=20)).strftime('%d/%m/%Y')}", f"Fatura FR 2026/102 de {(date.today() - timedelta(days=13)).strftime('%d/%m/%Y')}"],
   'prescription scan, then each invoice, in order')
ok(len(pack['skipped']) == 1 and pack['skipped'][0]['reason'] == 'no_file', 'the payment without an invoice file is listed as “attach separately”')
d = call('GET', f"/documents/{pack['document_id']}", ther).json()
pdf = base64.b64decode(d['data'])
ok(d['kind'] == 'report' and pdf.startswith(b'%PDF'), 'pack saved with the patient’s documents')
if os.environ.get('SAVE_PDFS'): open(os.environ['SAVE_PDFS'] + '/pack.pdf', 'wb').write(pdf)
n = pages_of(pdf)
if n is not None:
    ok(n == pack['pages'] and n >= 4, f'one PDF: {n} pages (report + prescription + 2 invoices)')
    with tempfile.NamedTemporaryFile(suffix='.pdf') as f:
        f.write(pdf); f.flush()
        chk = subprocess.run(['qpdf', '--check', f.name], capture_output=True, text=True)
        ok(chk.returncode in (0, 3), 'qpdf --check: the joined PDF is well-formed')
t = text_of(pdf)
if t is not None:
    for s_ in ['Pedido de reembolso', 'Joana Ribeiro', '234567899', 'Dr. Lopes', 'ADSE', 'FR 2026/101', 'Total', '60,00 €', 'Relatório clínico',
               'Mantém exercícios em casa', 'cédula profissional n.º C-12345', 'FATURA FR 2026/101 - Joana Ribeiro', 'junte-a à parte']:
        ok(s_ in t, f'pack text has “{s_}”')
ok(call('GET', f"/documents/{pack['document_id']}", recep).status_code == 404, 'reception can’t see the pack exists')
ok(call('GET', f'/patients/{pid}/reimbursement', ther).json()['professional_id'] == 'C-12345', 'the licence number is remembered for next time')
if ARCHIVE:
    ok(len(glob.glob(f'{ARCHIVE}/Utentes/0001-*/Documentos/*Pacote de reembolso ADSE.pdf')) == 1, 'pack is in the patient’s Documentos folder')
# A protected invoice (user password) can't be joined: listed, not attached; the pack is still made.
if shutil.which('qpdf'):
    with tempfile.TemporaryDirectory() as dd:
        open(f'{dd}/a.pdf', 'wb').write(invoice_pdf(104))
        subprocess.run(['qpdf', '--encrypt', 'secret', 'owner', '256', '--', f'{dd}/a.pdf', f'{dd}/b.pdf'], check=True)
        locked = open(f'{dd}/b.pdf', 'rb').read()
        subprocess.run(['qpdf', '--encrypt', '', 'owner', '256', '--', f'{dd}/a.pdf', f'{dd}/c.pdf'], check=True)
        owner_only = open(f'{dd}/c.pdf', 'rb').read()
    p4 = pay(2, 2000, 'FR 2026/104', locked, 'FR104.pdf')
    p5 = pay(1, 2000, 'FR 2026/105', owner_only, 'FR105.pdf')
    r = call('POST', f'/patients/{pid}/reimbursement', ther, {**req, 'payment_ids': [p4, p5], 'prescription_id': None})
    ok(r.status_code == 200 and [x['reason'] for x in r.json()['skipped']] == ['unreadable'] and len(r.json()['attached']) == 1,
       'password-protected invoice is listed to attach separately; one with only an edit lock is joined')

# ═════════ discharge letters ═════════
ok(call('GET', f'/patients/{pid}/letters', recep).status_code == 403, 'reception can’t see letters')
r = call('GET', f'/patients/{pid}/letters/template', ther)
tpl = r.json()
ok(r.status_code == 200 and tpl['lang'] == 'pt' and tpl['addressee'] == 'Ao(À) Exmo.(a) Sr.(a) Dr. Lopes', 'template addressed to the prescribing doctor')
ok('Joana Ribeiro, de 42 anos,' in tpl['body'] or 'Joana Ribeiro, de 4' in tpl['body'], 'template names the patient with age')
ok('num total de 4 sessões' in tpl['body'] and '[a completar]' in tpl['body'], 'template counts every done session and marks gaps')
ok('Physiotherapy' not in tpl['body'] and 'Reason for referral' in call('GET', f'/patients/{pid}/letters/template?lang=en', ther).json()['body'], 'English template on request')

# AI: owner only; never saves.
ok(call('POST', f'/patients/{pid}/letters/ai-draft', ther, {'lang': 'pt'}).status_code == 403, 'therapist can’t use the AI')
ok(call('GET', '/ai/status', ther).status_code == 403, 'AI status is owner-only')
st = call('GET', '/ai/status', owner).json()
ok(st['available'] and st['model'], 'owner sees the local AI is available: ' + json.dumps(st))
asked.clear()
before = sql('SELECT count(*) FROM discharge_letters') if PSQL else None
r = call('POST', f'/patients/{pid}/letters/ai-draft', owner, {'lang': 'pt'})
ok(r.status_code == 200, 'owner asks the AI for a draft')
ai_text = r.json()['text']
ok(ai_text.startswith('A utente foi acompanhada') and '<think>' not in ai_text and '**' not in ai_text, 'answer cleaned: no thinking, no markdown')
gen = [b for p_, b in asked if p_ == '/api/generate']
ok(len(gen) == 1 and gen[0]['stream'] is True and gen[0].get('think') is False and 'Joana Ribeiro' in gen[0]['prompt'] and 'NDI: de 32/50 para 12/50' in gen[0]['prompt'], 'the AI got the facts from the file')
ok('[a completar]' in gen[0]['system'] and 'não inventes' in gen[0]['system'], 'the AI is told to use only the facts and mark gaps')
ok('Boa evolução' in gen[0]['prompt'] and '234567899' not in gen[0]['prompt'], 'recent notes go in; NIF does not')
if PSQL:
    ok(sql('SELECT count(*) FROM discharge_letters') == before, 'the AI saved nothing')
mode['answer'] = 'no_model'
r = call('POST', f'/patients/{pid}/letters/ai-draft', owner, {'lang': 'pt'})
ok(r.status_code == 409 and r.json()['error'] == 'ai_no_model', 'model not downloaded → plain message')
ok(not call('GET', '/ai/status', owner).json()['available'], 'status says it isn’t ready')
mode['answer'] = 'ok'

# A therapist starts a draft by hand; only the owner approves.
r = call('POST', f'/patients/{pid}/letters', ther, {'lang': 'pt', 'addressee': tpl['addressee'], 'body': tpl['body']})
ok(r.status_code == 200, 'therapist saves a draft')
tdraft = r.json()['id']
ok(call('POST', f'/letters/{tdraft}/approve', ther, {}).status_code == 403, 'therapist can’t approve')
ok(call('PATCH', f'/letters/{tdraft}', ther, {'body': 'Novo texto [a completar]'}).status_code == 200, 'author edits the draft')
ok(call('DELETE', f'/letters/{tdraft}', ther).status_code == 200, 'author discards the draft')

r = call('POST', f'/patients/{pid}/letters', owner, {'lang': 'pt', 'addressee': tpl['addressee'], 'body': ai_text, 'ai_drafted': True})
letter = r.json()['id']
ok(call('PATCH', f'/letters/{letter}', ther, {'body': 'x'}).status_code == 409, 'a therapist can’t change the owner’s draft')
r = call('POST', f'/letters/{letter}/approve', owner, {})
ok(r.status_code == 400 and 'a completar' in r.json()['message'], 'can’t approve with [a completar] left in')
final = ai_text.replace('[a completar]', 'sem dor em repouso, mobilidade cervical completa') + '\n\nRecomendações: manter o programa de exercícios.'
ok(call('PATCH', f'/letters/{letter}', owner, {'body': final}).status_code == 200, 'owner corrects the text')
r = call('POST', f'/letters/{letter}/approve', owner, {'discharge': True, 'professional_id': 'C-777'})
ok(r.status_code == 200 and r.json()['document_id'] and r.json()['discharged'], 'owner approves and marks the patient discharged')
ldoc = r.json()['document_id']
ok(call('POST', f'/letters/{letter}/approve', owner, {}).status_code == 409, 'approving twice → already approved')
ok(call('PATCH', f'/letters/{letter}', owner, {'body': 'x'}).status_code == 409, 'an approved letter can’t change')
ok(call('DELETE', f'/letters/{letter}', owner).status_code == 409, 'an approved letter is kept')
ok(call('DELETE', f'/documents/{ldoc}', owner).status_code == 409, 'its PDF can’t be removed')
ok(call('PATCH', f'/documents/{ldoc}', owner, {'filename': 'x.pdf'}).status_code == 409, 'its PDF can’t be renamed')
ls = call('GET', f'/patients/{pid}/letters', ther).json()
ok(len(ls['letters']) == 1 and ls['letters'][0]['approved_by_name'] == 'Filipe Sousa' and ls['letters'][0]['ai_drafted'] and ls['letters'][0]['document_id'] == ldoc, 'letter listed as approved, AI-drafted, with its PDF')
f = call('GET', f'/patients/{pid}', ther).json()
ok(f['patient']['active'] is False, 'patient is now inactive (discharged)')
ok(any(d_['id'] == ldoc and d_['letter_id'] == letter for d_ in f['documents']), 'letter PDF shows in Documents')
lpdf = base64.b64decode(call('GET', f'/documents/{ldoc}', ther).json()['data'])
if os.environ.get('SAVE_PDFS'): open(os.environ['SAVE_PDFS'] + '/letter.pdf', 'wb').write(lpdf)
t = text_of(lpdf)
if t is not None:
    for s_ in ['Carta de alta de fisioterapia', 'Dr. Lopes', 'Joana Ribeiro', 'utente n.º 1', 'mobilidade cervical completa',
               'Com os melhores cumprimentos', 'Filipe Sousa', 'cédula profissional n.º C-777']:
        ok(s_ in t, f'letter text has “{s_}”')
    ok('[a completar]' not in t, 'no gaps left in the letter')
if ARCHIVE:
    ok(len(glob.glob(f'{ARCHIVE}/Utentes/0001-*/Cartas/*Carta de alta*.pdf')) == 1, 'letter is in the patient’s Cartas folder')
ok(call('POST', f'/letters/{letter}/pdf', ther).json()['document_id'] == ldoc, 'saving the PDF again keeps one PDF')

# Without discharge: patient stays active.
call('PATCH', f'/patients/{other}', recep, {'active': True})
l2 = call('POST', f'/patients/{other}/letters', owner, {'lang': 'en', 'body': 'Short letter.'}).json()['id']
r = call('POST', f'/letters/{l2}/approve', owner, {'discharge': False})
ok(r.status_code == 200 and call('GET', f'/patients/{other}', owner).json()['patient']['active'] is True, 'approving without discharge leaves the patient active')
if t is not None:
    t2 = text_of(base64.b64decode(call('GET', f"/documents/{r.json()['document_id']}", owner).json()['data']))
    ok('Physiotherapy discharge letter' in t2 and 'Yours sincerely' in t2, 'English letter in English')

# Full export includes the letter; anonymising removes letters.
if PSQL:
    ok(sql("SELECT count(*) FROM discharge_letters WHERE approved_at IS NOT NULL AND patient_id = '%s'" % pid) == '1', 'letter row stored')
    r = subprocess.run(['psql', PSQL, '-tAc', "UPDATE discharge_letters SET body = 'x' WHERE id = '%s'" % letter], capture_output=True, text=True)
    ok('be changed' in r.stderr, 'database trigger blocks changes to an approved letter')
r = call('POST', f'/patients/{pid}/anonymise', owner, {'confirm_name': 'Joana Ribeiro'})
ok(r.status_code == 200, 'owner anonymises the patient')
if PSQL:
    ok(sql("SELECT count(*) FROM discharge_letters WHERE patient_id = '%s'" % pid) == '0', 'anonymising removes the letters (they name the person)')
ok(call('GET', f'/patients/{pid}/letters/template', owner).status_code == 404, 'no new letters for an anonymised patient')
ok(call('GET', f'/patients/{pid}/reimbursement', owner).status_code == 404, 'no packs for an anonymised patient')

# Access log: what was done, never what was written.
log = call('GET', '/access-log?limit=500', owner).json()
entries = log if isinstance(log, list) else log.get('entries', log.get('items', []))
acts = [x.get('action') for x in entries]
for a_ in ['patient.reimbursement_pack', 'ai.letter_drafted', 'letter.created', 'letter.approved', 'letter.draft_deleted']:
    ok(a_ in acts, f'access log has {a_}')
ok(not any('cervical' in json.dumps(x).lower() for x in entries), 'access log holds no clinical text')
if LOG:
    txt = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Joana' not in txt and 'Ribeiro' not in txt and 'Lopes' not in txt, 'server log has no names')

fake.shutdown(); fake.server_close()
r = call('POST', f'/patients/{other}/letters/ai-draft', owner, {'lang': 'pt'})
ok(r.status_code == 409 and r.json()['error'] == 'ai_offline', 'AI not running → plain “isn’t running” message, nothing else breaks')
st = call('GET', '/ai/status', owner).json()
ok(st['available'] is False and st['reason'] == 'offline', 'status says offline')
print(f'\n{"ALL PASSED" if fails == 0 else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
