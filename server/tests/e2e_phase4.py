# End-to-end test of phase 4: the local AI's jobs (note rewrite, receipt reading, monthly report, admin assistant)
# and opening hours / closures. Needs a running server with an EMPTY database, started with
# OLLAMA_URL=http://127.0.0.1:11997 (this test runs a stand-in for Ollama there).
# ARCHIVE = the server's ARCHIVE_DIR; PSQL_URL adds database checks; LOG = the server's log (no names, no prompts).
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_phase4.py
import os, sys, json, glob, base64, shutil, threading, subprocess, zlib, struct, requests, pyotp
from datetime import datetime, timedelta, date
from http.server import BaseHTTPRequestHandler, HTTPServer

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
    return requests.request(method, B + path, json=json, headers=h, timeout=60)
def sql(q):
    return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

# ── a stand-in for Ollama: answers depend on the job (told apart by the system prompt) ──
asked = []
mode = {'fail': None, 'soap': 0, 'intent': {'skill': 'answer', 'reply': 'Posso ajudar com preços.'}}
RECEIPT = {'supplier': 'Fisiomed, Lda.', 'supplier_nif': 'PT509876543', 'date': '', 'total': 86.4, 'vat_shown': True, 'vat': 16.15,
           'document_number': 'FT A/1234', 'category': 'supplies', 'description': 'Ligaduras e gel', 'payment': 'card'}
class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.reply(200, {'models': [{'name': 'gemma4:12b'}]})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        asked.append((self.path, body))
        if self.path == '/api/show': return self.reply(200, {'details': {}})
        # This stand-in plays a model without tools (like gemma): the assistant falls back to its one-step mode (13 Nov 2026).
        if self.path == '/api/chat': return self.reply(400, {'error': 'registry.ollama.ai/library/gemma4:12b does not support tools'})
        sysp = body.get('system', '')
        if mode['fail'] == 'vision' and body.get('images'):
            return self.reply(500, {'error': 'llama runner: this model does not support images'})
        if mode['fail'] == 'garbage':
            return self.reply(200, {'done': True, 'response': 'não sei'})
        if 'nota de evolução' in sysp or 'progress note' in sysp:
            # mode['soap'] = how many structured answers to give before a plain one (-1 = always structured)
            if mode['soap']:
                mode['soap'] -= 1 if mode['soap'] > 0 else 0
                out = '**S (subjetivo)** Refere dor 6/10.\nO (objetivo) [a completar]\nA (avaliação) Melhor.\nP (plano) Manter exercício.'
            else:
                out = '**A utente** referiu dor de 6/10, melhor do que na semana passada.\n\n\n- Fez-se mobilização cervical. Mantém-se o plano.'
        elif 'receipts' in sysp:
            out = json.dumps(dict(RECEIPT, date=date.today().isoformat()))
        elif 'admin assistant' in sysp:
            out = json.dumps(mode['intent'])
        elif 'relatório mensal' in sysp or 'monthly report' in sysp:
            out = 'Em **novembro** a receita foi a indicada.\n\nAs despesas foram as indicadas.'
        else:
            out = 'Resumo curto do mês.'
        self.reply(200, {'model': body.get('model'), 'done': True, 'response': out})
    def reply(self, code, out):
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers()
        self.wfile.write(data)
fake = HTTPServer(('127.0.0.1', 11997), Fake)
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
ana_id = next(s['id'] for s in call('GET', '/therapists', recep).json() if s['name'] == 'Ana Rocha')
pid = call('POST', '/patients', recep, {'name': 'Joana Ribeiro', 'therapist_id': ana_id}).json()['id']
today = date.today()

# ── roles: every AI job and assistant action is the owner's ──
for m, p, b in [('POST', '/ai/rewrite-note', {'patient_id': pid, 'blocks': [], 'text': 'x'}), ('POST', '/ai/read-receipt', {'file_id': pid}),
                ('POST', '/ai/report', {'month': today.strftime('%Y-%m')}), ('GET', '/finance/reports', None), ('GET', f"/finance/reports/figures?month={today.strftime('%Y-%m')}", None),
                ('POST', '/ai/assistant', {'message': 'olá'}), ('POST', f'/assistant/changes/{pid}/apply', None), ('PUT', '/clinic/hours', {'hours': {}}),
                ('POST', '/closures', {'starts_on': today.isoformat(), 'ends_on': today.isoformat()}), ('GET', f'/closures/check?starts_on={today}&ends_on={today}', None)]:
    codes = (call(m, p, ther, b).status_code, call(m, p, recep, b).status_code)
    ok(codes == (403, 403), f'therapist and reception refused: {m} {p.split("?")[0]} {codes}')
ok(not asked, 'nothing reached the AI for refused requests')

# ── note rewrite ──
blocks = [{'type': 'slider', 'key': 'pain', 'label': 'Dor', 'value': 6, 'min': 0, 'max': 10}, {'type': 'text', 'key': 't', 'label': 'Técnica', 'value': 'Mobilização cervical'},
          {'type': 'checklist', 'key': 'c', 'label': 'Exercícios', 'value': [], 'options': ['A']}]
r = call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'template': 'cervical', 'blocks': blocks, 'text': 'Melhor do que na semana passada.', 'lang': 'pt'})
ok(r.status_code == 200 and r.json()['text'] == 'A utente referiu dor de 6/10, melhor do que na semana passada.\n\nFez-se mobilização cervical. Mantém-se o plano.', 'rewrite returns plain paragraphs: no bold, no bullets, one blank line')
ok(r.json().get('model') == 'gemma4:12b' or r.json().get('model'), 'the draft says which model wrote it')
first_ask = asked[-1][1]; sysp = first_ask['system']
ok('sem formato SOAP' in sysp and 'sem títulos' in sysp and 'Exemplo inventado' in sysp and '«o utente»' in sysp, 'the note prompt carries the plain-text rules and an invented example')
n_before = len(asked); mode['soap'] = 1
r = call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'template': 'cervical', 'blocks': blocks, 'lang': 'pt'})
ok(r.status_code == 200 and not r.json()['text'].startswith('S (') and len(asked) - n_before == 2 and 'sem secções S, O, A, P' in asked[-1][1]['prompt'], 'a SOAP answer is asked again once, with a reminder')
mode['soap'] = -1
r = call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'template': 'cervical', 'blocks': blocks, 'lang': 'pt'})
ok(r.status_code == 409 and r.json()['error'] == 'ai_failed', 'a model that keeps writing SOAP → ai_failed, never a structured draft')
mode['soap'] = 0
last = first_ask
ok('Dor: 6/10' in last['prompt'] and 'Mobilização cervical' in last['prompt'] and 'Exercícios' not in last['prompt'], 'the AI gets only what was filled in')
ok('Joana' not in last['prompt'] and 'Joana' not in last['system'], 'the patient’s name is never sent to the AI')
ok(call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'blocks': [{'type': 'text', 'label': 'x', 'value': ''}], 'text': ' '}).status_code == 400, 'an empty note isn’t sent')
ok(call('POST', '/ai/rewrite-note', owner, {'patient_id': ana_id, 'blocks': blocks}).status_code == 404, 'unknown patient → 404')
r = call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'blocks': blocks, 'lang': 'en'})
ok(r.status_code == 200 and 'progress note' in asked[-1][1]['system'] and 'British English' in asked[-1][1]['system'] and 'no SOAP format' in asked[-1][1]['system'], 'English notes use the English instruction')
mode['fail'] = 'garbage'
# A plain-text job still accepts any text; garbage is only a failure for JSON jobs (below).
mode['fail'] = None
n = call('POST', f'/patients/{pid}/notes', owner, {'template': 'cervical', 'blocks': blocks, 'text': 'A utente referiu dor de 6/10.', 'ai_assisted': True}).json()['id']
f = call('GET', f'/patients/{pid}', owner).json()
ok(any(x['id'] == n and x['ai_assisted'] for x in f['notes']), 'a note started from the AI is marked ai_assisted')
log = call('GET', f'/access-log?subject_type=patient&subject_id={pid}', owner).json()
ok(any(x['action'] == 'ai.note_rewritten' for x in log), 'rewrites are in the patient’s access log')

# ── receipt reading ──
def png_of(w=4, h=4):
    raw = b''.join(b'\x00' + b'\xff\xff\xff' * w for _ in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
def pdf_with(text):
    content = f'BT /F1 12 Tf 50 700 Td ({text}) Tj ET'.encode()
    objs = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
            b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
            b'<< /Length ' + str(len(content)).encode() + b' >>\nstream\n' + content + b'\nendstream',
            b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>']
    out = b'%PDF-1.4\n'; offs = []
    for i, o in enumerate(objs):
        offs.append(len(out)); out += f'{i+1} 0 obj\n'.encode() + o + b'\nendobj\n'
    x = len(out)
    out += f'xref\n0 {len(objs)+1}\n0000000000 65535 f \n'.encode() + b''.join(f'{o:010d} 00000 n \n'.encode() for o in offs)
    out += f'trailer << /Size {len(objs)+1} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF'.encode()
    return out
up = lambda name, b: call('POST', '/finance/files', owner, {'kind': 'expense', 'filename': name, 'data': base64.b64encode(b).decode(), 'day': today.isoformat()}).json()['id']
photo = up('talão.png', png_of())
r = call('POST', '/ai/read-receipt', owner, {'file_id': photo})
fl = r.json().get('fields', {})
ok(r.status_code == 200 and fl.get('supplier') == 'Fisiomed, Lda.' and fl.get('supplier_nif') == '509876543', 'photo → supplier and NIF (PT prefix removed)')
ok(fl.get('amount_cents') == 8640 and fl.get('vat_cents') == 1615 and fl.get('spent_on') == today.isoformat() and fl.get('category') == 'supplies' and fl.get('method') == 'card', 'photo → amount, VAT, date, category, method')
ok(r.json().get('missing') == [], 'nothing missing')
last = asked[-1][1]
ok(len(last.get('images', [])) == 1 and base64.b64decode(last['images'][0]).startswith(b'\x89PNG'), 'the photo is sent to the local AI as an image')
ok(isinstance(last.get('format'), dict) and 'supplier_nif' in last['format']['properties'], 'the AI must answer in the receipt JSON shape')
text_pdf = up('fatura.pdf', pdf_with('Fisiomed Lda NIF 509876543 Fatura FT A/1234 Total 86,40 EUR IVA 16,15'))
r = call('POST', '/ai/read-receipt', owner, {'file_id': text_pdf})
ok(r.status_code == 200 and not asked[-1][1].get('images') and 'Total 86,40' in asked[-1][1]['prompt'], 'a PDF invoice is read as text (no image sent)')
scan = up('scan.pdf', pdf_with(''))
r = call('POST', '/ai/read-receipt', owner, {'file_id': scan})
ok(r.status_code == 409 and r.json()['error'] == 'ai_scan_pdf', 'a scanned PDF with no text says to use a photo')
heic = up('foto.heic', b'\x00\x00\x00\x18ftypheic' + b'\x00' * 40)
ok(call('POST', '/ai/read-receipt', owner, {'file_id': heic}).json().get('error') == 'ai_heic', 'HEIC photos: asks for JPEG or PDF')
mode['fail'] = 'vision'
r = call('POST', '/ai/read-receipt', owner, {'file_id': photo})
ok(r.status_code == 409 and r.json()['error'] == 'ai_no_vision', 'a model that can’t read pictures → ai_no_vision')
mode['fail'] = 'garbage'
r = call('POST', '/ai/read-receipt', owner, {'file_id': photo})
ok(r.status_code == 409 and r.json()['error'] == 'ai_failed', 'an answer that isn’t JSON → ai_failed')
mode['fail'] = None
inv = call('POST', '/finance/files', recep, {'kind': 'invoice', 'filename': 'f.pdf', 'data': base64.b64encode(pdf_with('x y z w v')).decode()}).json()['id']
ok(call('POST', '/ai/read-receipt', owner, {'file_id': inv}).status_code == 404, 'only expense receipts can be read')
r = call('POST', '/expenses', owner, {**{k: v for k, v in fl.items() if v is not None}, 'file_id': photo, 'ai_read': True})
ok(r.status_code == 200, 'the confirmed details save as an expense')
ex = call('GET', '/expenses', owner).json()
ok(any(x['ai_read'] and x['supplier'] == 'Fisiomed, Lda.' for x in ex), 'the expense is marked as read by the AI')

# ── monthly report ──
call('POST', '/payments', recep, {'patient_id': pid, 'method': 'cash', 'items': [{'amount_cents': 2000, 'description': 'Sessão'}]})
ym = today.strftime('%Y-%m')
r = call('GET', f'/finance/reports/figures?month={ym}&lang=pt', owner)
rows = {k: a for k, a, b in r.json()['rows']}
ok(r.status_code == 200 and rows['Receita'] == '20,00 €' and rows['Despesas'] == '86,40 €', 'figures are worked out by the server')
r = call('POST', '/ai/report', owner, {'month': ym, 'lang': 'pt'})
ok(r.status_code == 200 and '**' not in r.json()['text'] and r.json()['rows'][0][0] == 'Receita', 'report draft: AI text + server figures')
last = asked[-1][1]
ok('Receita: 20,00 €' in last['prompt'] and 'Despesas: 86,40 €' in last['prompt'], 'the AI gets the server’s figures')
ok('Joana' not in last['prompt'], 'no patient names in the report prompt')
ok(call('POST', '/ai/report', owner, {'month': '2026-13'}).status_code == 400, 'bad month refused')
r = call('POST', '/finance/reports', owner, {'month': ym, 'lang': 'pt', 'text': 'Texto revisto pela dona.\n\nSegundo parágrafo.', 'model': 'gemma4:12b'})
ok(r.status_code == 200, 'report saved'); rep = r.json()
lst = call('GET', '/finance/reports', owner).json()
ok(len(lst) == 1 and lst[0]['text'].startswith('Texto revisto') and lst[0]['file_id'] == rep['file_id'], 'saved report listed with its PDF')
raw = requests.get(B + f"/finance/files/{rep['file_id']}/raw", headers={**H, 'Authorization': 'Bearer ' + owner})
ok(raw.status_code == 200 and raw.content.startswith(b'%PDF'), 'the report PDF opens')
if shutil.which('pdftotext'):
    t = subprocess.run(['pdftotext', '-', '-'], input=raw.content, capture_output=True).stdout.decode()
    ok('Comentário' in t and 'Texto revisto' in t and 'Receita' in t and 'IA local' in t, 'the PDF has the figures, the commentary and says the AI drafted it')
ok(call('GET', f"/finance/files/{rep['file_id']}/raw", recep).status_code == 403, 'reception can’t open reports')
if ARCHIVE:
    ok(len(glob.glob(f'{ARCHIVE}/Financas/{today.year}/Relatorios/*Relatorio {ym}.pdf')) == 1, 'the PDF is in Financas/<year>/Relatorios')
ok(call('POST', '/finance/reports', owner, {'month': ym, 'text': '  '}).status_code == 400, 'empty report refused')
ok(call('DELETE', f"/finance/reports/{rep['id']}", owner).status_code == 200 and call('GET', '/finance/reports', owner).json() == [], 'report deleted')
if ARCHIVE:
    ok(not glob.glob(f'{ARCHIVE}/Financas/{today.year}/Relatorios/*.pdf'), '…with its PDF')

# ── opening hours and closures ──
h = call('GET', '/clinic/hours', recep).json()
ok(h['hours']['mon'] == ['08:00', '20:00'] and h['hours']['sun'] is None and h['closures'] == [], 'everyone reads the default hours')
bad = dict(h['hours'], tue=['20:00', '08:00'])
ok(call('PUT', '/clinic/hours', owner, {'hours': bad}).status_code == 400, 'closing before opening refused')
ok(call('PUT', '/clinic/hours', owner, {'hours': {'mon': ['08:00', '20:00']}}).status_code == 400, 'all seven days needed')
good = dict(h['hours'], sat=['09:00', '14:00'])
ok(call('PUT', '/clinic/hours', owner, {'hours': good}).status_code == 200 and call('GET', '/clinic/hours', recep).json()['hours']['sat'] == ['09:00', '14:00'], 'owner changes Saturday')
# a booked session on a future weekday, then close that day and the next
d = today + timedelta(days=1)
while d.isoweekday() > 5: d += timedelta(days=1)
st = datetime(d.year, d.month, d.day, 10).astimezone()
aid = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': st.isoformat(), 'ends_at': (st + timedelta(minutes=45)).isoformat(), 'therapist_id': ana_id, 'patient_id': pid}).json()['ids'][0]
d2 = d + timedelta(days=1)
r = call('GET', f'/closures/check?starts_on={d}&ends_on={d2}', owner).json()
ok(r['booked'] == 1, 'check: one appointment is already booked then')
ok(call('POST', '/closures', owner, {'starts_on': (today - timedelta(days=1)).isoformat(), 'ends_on': today.isoformat()}).status_code == 400, 'can’t close days that passed')
ok(call('POST', '/closures', owner, {'starts_on': d2.isoformat(), 'ends_on': d.isoformat()}).status_code == 400, 'last day before first refused')
r = call('POST', '/closures', owner, {'starts_on': d.isoformat(), 'ends_on': d2.isoformat(), 'label': 'Formação'})
open_days = sum(1 for x in [d, d2] if good[['mon','tue','wed','thu','fri','sat','sun'][x.isoweekday()-1]])
ok(r.status_code == 200 and r.json()['days'] == open_days and r.json()['booked'] == 1, 'closure blocks each open day and reports the booked one')
cid = r.json()['id']
ap = call('GET', f'/appointments?from={d}&to={d2 + timedelta(days=1)}', recep).json()
blocks_ = [x for x in ap if x['kind'] == 'block']
ok(len(blocks_) == open_days and all(x['title'] == 'Formação' for x in blocks_), 'the schedule shows the closure as blocked time')
ok(any(x['id'] == aid and x['status'] == 'booked' for x in ap), 'the booked session stays booked')
ok(call('GET', '/clinic/hours', recep).json()['closures'][0]['label'] == 'Formação', 'upcoming closures listed')
ok(call('DELETE', f'/closures/{cid}', owner).status_code == 200 and not [x for x in call('GET', f'/appointments?from={d}&to={d2 + timedelta(days=1)}', recep).json() if x['kind'] == 'block'], 'removing the closure removes its blocked time')

# ── admin assistant ──
def ask(msg, intent, lang='pt'):
    mode['intent'] = intent
    return call('POST', '/ai/assistant', owner, {'message': msg, 'lang': lang, 'history': [{'who': 'me', 'text': 'olá'}, {'who': 'bot', 'text': 'Olá!'}]})
prods = {p['code']: p for p in call('GET', '/products', owner).json()}
r = ask('Muda o preço da avaliação para 35 €', {'skill': 'price', 'product': 'assessment', 'price_eur': 35, 'reply': 'Aqui está o novo preço.'})
c = r.json().get('change') or {}
ok(r.status_code == 200 and c.get('rows') == [['Avaliação inicial (60 min)', '30,00 €', '35,00 €']] and c.get('status') == 'pending', 'price change previewed before/after')
last = asked[-1][1]
ok('assessment | Avaliação inicial (60 min)' in last['prompt'] and 'Ana Rocha | therapist' in last['prompt'] and 'User: olá' in last['prompt'], 'the AI gets the price list, staff and recent conversation')
ok('Agenda (' in last['prompt'] and 'Mobilização' not in last['prompt'] and 'Fisiomed' not in last['prompt'], 'the assistant sees the agenda (11 Nov 2026), never notes or receipts')
ok(call('GET', '/products', owner).json()[0]['price_cents'] in (3000, 2000, 4000, 1000) and {p['code']: p for p in call('GET', '/products', owner).json()}['assessment']['price_cents'] == 3000, 'nothing changes before Apply')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 200, 'Apply')
ok({p['code']: p for p in call('GET', '/products', owner).json()}['assessment']['price_cents'] == 3500, 'price is now €35')
ok(call('POST', f"/assistant/changes/{c['id']}/apply", owner).status_code == 409, 'can’t apply twice')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 200 and {p['code']: p for p in call('GET', '/products', owner).json()}['assessment']['price_cents'] == 3000, 'Undo puts €30 back')
ok(call('POST', f"/assistant/changes/{c['id']}/undo", owner).status_code == 409, 'can’t undo twice')
log = call('GET', '/access-log', owner).json()
ok(any(x['action'] == 'assistant.applied' and '35,00' in x['detail'] for x in log) and any(x['action'] == 'assistant.undone' for x in log), 'apply and undo are logged')
r = ask('Pacote de 5 a 75', {'skill': 'price', 'product': 'Pacote de 5', 'price_eur': 75, 'reply': 'ok'})
ok((r.json().get('change') or {}).get('rows', [[None]])[0][0] == 'Pacote de 5', 'finds a product by its name too')
stale = r.json()['change']['id']
call('PATCH', f"/products/{prods['pack5']['id']}", owner, {'price_cents': 7000})
ok(call('POST', f'/assistant/changes/{stale}/apply', owner).status_code == 409, 'refused if the price changed since the preview')
r = ask('Muda o preço do pacote de 99', {'skill': 'price', 'product': 'pack99', 'price_eur': 10, 'reply': 'x'})
ok(r.json()['change'] is None and 'Não encontrei' in r.json()['reply'], 'unknown product: says so, no change')
r = ask('preço avaliação', {'skill': 'price', 'product': 'assessment', 'reply': 'x'})
ok(r.json()['change'] is None and 'novo preço' in r.json()['reply'], 'missing price: asks for it')
r = ask('Deixa de vender o pacote de 10', {'skill': 'product_active', 'product': 'pack10', 'active': False, 'reply': 'ok'})
cc = r.json()['change']['id']
ok(call('POST', f'/assistant/changes/{cc}/cancel', owner).status_code == 200 and call('POST', f'/assistant/changes/{cc}/apply', owner).status_code == 409, 'Cancel; a cancelled change can’t be applied')
ok({p['code']: p for p in call('GET', '/products?all=true', owner).json()}['pack10']['active'], 'pack still on sale after Cancel')
r = ask('Ao sábado fechamos', {'skill': 'hours', 'day': 'sat', 'closed': True, 'reply': 'ok'})
c = r.json()['change']
ok(c['rows'] == [['Sábado', '09:00–14:00', 'Fechado']], 'hours change previewed')
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
ok(call('GET', '/clinic/hours', recep).json()['hours']['sat'] is None, 'Saturday closed')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)
ok(call('GET', '/clinic/hours', recep).json()['hours']['sat'] == ['09:00', '14:00'], 'undo reopens Saturday')
r = ask('quarta das 9h às 13h', {'skill': 'hours', 'day': 'wed', 'open': '9h', 'close': '13h', 'reply': 'ok'})
ok(r.json()['change']['rows'][0][2] == '09:00–13:00', 'times like 9h are understood')
r = ask('Fecha amanhã e depois', {'skill': 'closure', 'from': d.isoformat(), 'to': d2.isoformat(), 'label': 'Natal', 'reply': 'ok'})
c = r.json()['change']
ok(c['rows'][0][2] == 'Fechada — Natal' and any('1 marcação' in w for w in c['warnings']), 'closure previewed with the booked appointment warning')
ok(not [x for x in call('GET', f'/appointments?from={d}&to={d2 + timedelta(days=1)}', recep).json() if x['kind'] == 'block'], 'no blocks before Apply')
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
ok(len([x for x in call('GET', f'/appointments?from={d}&to={d2 + timedelta(days=1)}', recep).json() if x['kind'] == 'block']) == open_days, 'Apply blocks the days')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)
ok(not [x for x in call('GET', f'/appointments?from={d}&to={d2 + timedelta(days=1)}', recep).json() if x['kind'] == 'block'] and call('GET', '/clinic/hours', owner).json()['closures'] == [], 'Undo reopens')
r = ask('fecha ontem', {'skill': 'closure', 'from': (today - timedelta(days=3)).isoformat(), 'to': (today - timedelta(days=3)).isoformat(), 'reply': 'ok'})
ok(r.json()['change'] is None, 'past days: no change proposed')
r = ask('A Ana passa a ganhar 1200', {'skill': 'staff_pay', 'staff': 'Ana', 'amount_eur': 1200, 'reply': 'ok'})
c = r.json()['change']
ok(c['rows'][0][1:] == ['0,00 €', '1200,00 €'], 'staff pay previewed (first name is enough)')
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
sal = call('GET', '/finance/salaries', owner).json()['staff']
ok(any(x['name'] == 'Ana Rocha' and x['fixed_cents'] == 120000 for x in sal), 'pay set')
r = ask('Desativa a Ana', {'skill': 'staff_active', 'staff': 'ana rocha', 'active': False, 'reply': 'ok'})
c = r.json()['change']
ok(c['rows'] == [['Ana Rocha', 'Ativa', 'Desativada']], 'deactivation previewed')
call('POST', f"/assistant/changes/{c['id']}/apply", owner)
ok(call('GET', '/me', ther).status_code == 401, 'deactivated: signed out everywhere')
call('POST', f"/assistant/changes/{c['id']}/undo", owner)
ok(any(x['name'] == 'Ana Rocha' and x['active'] for x in call('GET', '/staff', owner).json()), 'undo reactivates')
r = ask('desativa o Filipe', {'skill': 'staff_active', 'staff': 'Filipe Sousa', 'active': False, 'reply': 'ok'})
ok(r.json()['change'] is None, 'can’t deactivate the owner')
r = ask('Desativa a Maria', {'skill': 'staff_active', 'staff': 'Maria', 'active': False, 'reply': 'ok'})
ok(r.json()['change'] is None and 'Não encontrei' in r.json()['reply'], 'unknown person: says so')
r = ask('Resume este mês', {'skill': 'report', 'month': ym, 'reply': 'ok'})
ok(r.status_code == 200 and r.json()['change'] is None and r.json()['rows'][0][0] == 'Receita' and r.json()['reply'] == 'Resumo curto do mês.', 'report skill: figures + short AI summary')
r = ask('Qual o tempo amanhã?', {'skill': 'answer', 'reply': 'Só posso ajudar com preços, horários, equipa e resumos.'})
ok(r.json()['change'] is None and r.json()['reply'].startswith('Só posso'), 'other questions get a plain answer')
r = ask('Set the assessment price to €40', {'skill': 'price', 'product': 'assessment', 'price_eur': 40, 'reply': 'Here it is.'}, lang='en')
ok(r.json()['change']['rows'] == [['First assessment (60 min)', '€30.00', '€40.00']], 'English rows')
if PSQL:
    sql(f"UPDATE assistant_changes SET created_at = now() - interval '2 days' WHERE id = '{r.json()['change']['id']}'")
    ok(call('POST', f"/assistant/changes/{r.json()['change']['id']}/apply", owner).status_code == 409, 'proposals older than a day can’t be applied')
mode['fail'] = 'garbage'
r = call('POST', '/ai/assistant', owner, {'message': 'x'})
ok(r.status_code == 409 and r.json()['error'] == 'ai_failed', 'unreadable AI answer → ai_failed')
mode['fail'] = None
ok(call('POST', '/ai/assistant', owner, {'message': '  '}).status_code == 400, 'empty message refused')

# ── AI offline ──
fake.shutdown(); fake.server_close()
r = call('POST', '/ai/rewrite-note', owner, {'patient_id': pid, 'blocks': blocks})
ok(r.status_code == 409 and r.json()['error'] == 'ai_offline' and 'url' in r.json()['details'], 'AI computer off → ai_offline with the address')

# ── the log never has names or prompts ──
if LOG:
    text = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Joana' not in text and 'Mobilização' not in text and 'Fisiomed' not in text, 'server log has no patient names, note text or receipt details')

print(f'\n{"ALL PASS" if fails == 0 else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
