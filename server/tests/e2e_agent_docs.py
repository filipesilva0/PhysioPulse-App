# End-to-end test of the assistant, stage 3 (13 Nov 2026): documents, letters, legal tools, settings. Same rules as
# stages 1 and 2 — look-ups and Apply through the app's own endpoints as the person (role checks unchanged), every
# change waits for Apply and can be undone while nothing changed it (a consent withdrawal can't). The assistant
# never approves, signs, deletes, anonymises or exports. Needs a running server with an EMPTY database, started with
# OLLAMA_URL=http://127.0.0.1:11991 (this test runs a stand-in for Ollama there that plays a model with tools).
# Run: ARCHIVE=/tmp/arch PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_agent_docs.py
import os, sys, json, base64, struct, zlib, threading, subprocess, requests, pyotp
from datetime import date, datetime, timedelta
from http.server import BaseHTTPRequestHandler, HTTPServer

DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Rececao', 'X-Device-Key': DEVICE_KEY}
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
b64 = lambda b: base64.b64encode(b).decode()
PDF = b'%PDF-1.4\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n'
def png_rgba(w, h):
    raw = b''.join(b'\x00' + b''.join(b'\x10\x10\x30\xff' if abs(x * h // w - y) < 2 else b'\x00\x00\x00\x00' for x in range(w)) for y in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
SIG = 'data:image/png;base64,' + b64(png_rgba(120, 40))

chats, script = [], []
class Fake(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.reply(200, {'models': [{'name': 'qwen3:8b'}, {'name': 'gemma4:12b'}]})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/api/show': return self.reply(200, {'details': {}})
        if self.path == '/api/chat':
            chats.append(body)
            step = script.pop(0) if script else {'text': 'Pronto.'}
            if 'calls' in step:
                msg = {'role': 'assistant', 'content': '', 'tool_calls': [{'function': {'name': n, 'arguments': a}} for n, a in step['calls']]}
            else:
                msg = {'role': 'assistant', 'content': step['text']}
            lines = [{'message': msg, 'done': False}, {'message': {'role': 'assistant', 'content': ''}, 'done': True}]
            data = ''.join(json.dumps(l) + '\n' for l in lines).encode()
            self.send_response(200); self.send_header('Content-Type', 'application/x-ndjson'); self.send_header('Content-Length', str(len(data))); self.end_headers()
            return self.wfile.write(data)
        self.reply(200, {'done': True, 'response': 'Resumo.'})
    def reply(self, code, out):
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers()
        self.wfile.write(data)
fake = HTTPServer(('127.0.0.1', 11991), Fake)
threading.Thread(target=fake.serve_forever, daemon=True).start()

# ── people and a patient file with something in it ──
r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Sousa', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email, name):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': name, 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt', 'Ana Rocha')
recep = join('reception', 'rita@clinica.pt', 'Rita Lopes')
staff = {s['name']: s for s in call('GET', '/staff', owner).json()}
ana, rita = staff['Ana Rocha']['id'], staff['Rita Lopes']['id']
for who in (ana, rita):
    call('PATCH', f'/staff/{who}', owner, {'ai_enabled': True})

maria = call('POST', '/patients', recep, {'name': 'Maria Silva', 'therapist_id': ana, 'birth_date': '1980-05-02'}).json()['id']
joao = call('POST', '/patients', recep, {'name': 'João Pires', 'therapist_id': ana, 'phone': '912345678'}).json()['id']
call('PUT', f'/patients/{maria}/anamnesis', ther, {'main_complaint': 'Dor lombar há 3 semanas', 'goals': 'Voltar a correr'})
rep = call('POST', f'/patients/{maria}/documents', ther, {'kind': 'report', 'filename': 'Relatório RM.pdf', 'data': b64(PDF)}).json()['id']
scan = call('POST', f'/patients/{maria}/documents', recep, {'kind': 'prescription', 'filename': 'prescricao.pdf', 'data': b64(PDF)}).json()['id']
issued = (date.today() - timedelta(days=20)).isoformat()
rx = call('POST', f'/patients/{maria}/prescriptions', ther, {'doctor': 'Dr. Lopes', 'specialty': 'Medicina Geral', 'issued_on': issued, 'sessions_total': 10, 'payer': 'ADSE'}).json()['id']
s = (datetime.now() - timedelta(days=3)).replace(hour=10, minute=0, second=0, microsecond=0).astimezone()
a1 = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': ana, 'patient_id': maria, 'prescription_id': rx, 'allow_overlap': True}).json()['ids'][0]
call('PATCH', f'/appointments/{a1}', recep, {'status': 'done'})
call('POST', '/payments', recep, {'patient_id': maria, 'method': 'cash', 'items': [{'appointment_id': a1, 'amount_cents': 2000}]})
draft_text = next(t for t in call('GET', '/consent-texts', owner).json() if t['draft'] and t['lang'] == 'pt')
call('POST', f"/consent-texts/{draft_text['id']}/approve", owner)
consent = call('POST', f'/patients/{maria}/consents', recep, {'text_id': draft_text['id'], 'granted': True, 'signed_by_name': 'Maria Silva', 'signature_png': SIG}).json()['id']
t = (datetime.now() + timedelta(days=1)).replace(hour=11, minute=0, second=0, microsecond=0).astimezone()
a2 = call('POST', '/appointments', recep, {'kind': 'session', 'starts_at': t.isoformat(), 'ends_at': (t + timedelta(minutes=45)).isoformat(), 'therapist_id': ana, 'patient_id': joao, 'allow_overlap': True}).json()['ids'][0]
TODAY = date.fromisoformat(sql("SELECT ((now() AT TIME ZONE 'Europe/Lisbon')::date)::text")) if PSQL else date.today()
ref = lambda i: i.replace('-', '')[:8]

def ask(tok, msg, steps, lang='pt'):
    chats.clear(); script.clear(); script.extend(steps)
    return call('POST', '/ai/assistant', tok, {'message': msg, 'lang': lang, 'history': []})
def tool_names(body): return {t['function']['name'] for t in body['tools']}
def tool_results(body): return [m['content'] for m in body['messages'] if m['role'] == 'tool']
def last_tool(): return tool_results(chats[-1])[-1] if len(chats) > 1 and tool_results(chats[-1]) else ''
def apply(c, tok): return call('POST', f"/assistant/changes/{c['id']}/apply", tok)
def undo(c, tok): return call('POST', f"/assistant/changes/{c['id']}/undo", tok)
def change(r): return (r.json() or {}).get('change') or {}

# ── who gets which tools (same reach as the app) ──
ask(owner, 'olá', [{'text': 'Olá.'}]); own = tool_names(chats[0])
ok({'patient_documents', 'reminders', 'rename_document', 'withdraw_consent', 'mark_reminded', 'draft_letter', 'edit_letter', 'reimbursement_pack',
    'complaints', 'legal_check', 'clinic_settings', 'log_complaint', 'update_complaint', 'clinic_details', 'reminder_text', 'ai_switch', 'website'} <= own, 'the owner gets every stage-3 tool')
ok(not ({'approve_letter', 'anonymise_patient', 'delete_patient', 'export_patient', 'delete_document', 'delete_day'} & own), 'nobody gets approve, anonymise, delete or export')
ok('You never approve or sign a letter' in chats[0]['messages'][0]['content'], 'the brief says what it never does')
ask(ther, 'olá', [{'text': 'Olá.'}]); th = tool_names(chats[0])
ok({'patient_documents', 'draft_letter', 'edit_letter', 'reimbursement_pack', 'withdraw_consent'} <= th and not ({'complaints', 'legal_check', 'clinic_settings', 'clinic_details', 'website', 'ai_switch', 'log_complaint'} & th),
   'a therapist: documents, letters, packs; no complaints, legal check or settings')
ask(recep, 'olá', [{'text': 'Olá.'}]); rc = tool_names(chats[0])
ok({'patient_documents', 'rename_document', 'withdraw_consent', 'reminders', 'mark_reminded'} <= rc and not ({'draft_letter', 'edit_letter', 'reimbursement_pack', 'complaints', 'clinic_details'} & rc),
   'reception: documents they handle, consents, reminders; no letters, packs or settings')
r = ask(recep, 'escreve uma carta', [{'calls': [('draft_letter', {'patient': 'Maria Silva'})]}, {'text': 'Não posso.'}])
ok(r.status_code == 200 and "There's no tool called" in last_tool(), 'a clinical tool called by reception’s model is refused')
r = ask(owner, 'anonimiza a maria', [{'calls': [('anonymise_patient', {'patient': 'Maria Silva'})]}, {'text': 'Isso faz-se na ficha.'}])
ok("There's no tool called" in last_tool() and sql(f"SELECT anonymised_at IS NULL FROM patients WHERE id = '{maria}'") in ('t', ''), 'anonymising through the assistant is impossible')

# ── documents: what each role sees, through the app (access log) ──
before = sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.viewed' AND subject_id = '{maria}'")
r = ask(recep, 'que documentos tem a maria?', [{'calls': [('patient_documents', {'patient': 'Maria Silva'})]}, {'text': 'Tem uma prescrição.'}])
res = last_tool()
ok(f'ref {ref(scan)}' in res and 'prescricao.pdf' in res and 'Relatório RM' not in res and 'Discharge letters' not in res, 'reception sees the scanned prescription and consents, not the report or letters ' + res[:160])
ok(f'ref {ref(consent)}' in res and 'valid' in res, 'consents listed with ref and state')
ok(r.json()['steps'] and 'documentos' in r.json()['steps'][0], 'the step says what was looked up ' + str(r.json()['steps']))
after = sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.viewed' AND subject_id = '{maria}'")
ok(PSQL is None or int(after) == int(before) + 1, 'the look-up is in the access log like opening the file')
r = ask(ther, 'documentos da maria', [{'calls': [('patient_documents', {'patient': 'Maria Silva'})]}, {'text': 'Ok.'}])
res = last_tool()
ok('Relatório RM.pdf' in res and 'Discharge letters:' in res, 'a therapist sees every document and the letters')

# ── renaming a document ──
r = ask(recep, 'muda o nome do relatório', [{'calls': [('rename_document', {'patient': 'Maria Silva', 'ref': ref(rep), 'name': 'RM lombar'})]}, {'text': 'Não encontro.'}])
ok("isn't one of this patient's documents" in last_tool(), 'reception can’t reach a clinical document by its ref')
r = ask(ther, 'chama-lhe RM lombar', [{'calls': [('rename_document', {'patient': 'Maria Silva', 'ref': ref(rep), 'name': 'RM lombar', 'kind': 'imaging'})]}])
c = change(r)
ok(c.get('skill') == 'document_update' and c['rows'][0][1] == 'Relatório RM.pdf' and c['rows'][0][2] == 'RM lombar.pdf' and c['rows'][1][2] == 'Exame de imagem', 'preview: the name keeps .pdf, the kind in words ' + str(c.get('rows')))
ok(apply(c, ther).status_code == 200 and sql(f"SELECT filename || '|' || kind FROM documents WHERE id = '{rep}'") == 'RM lombar.pdf|imaging', 'Apply renames it through the app')
ok(undo(c, ther).status_code == 200 and sql(f"SELECT filename || '|' || kind FROM documents WHERE id = '{rep}'") == 'Relatório RM.pdf|report', 'Undo puts the name and kind back')
r = ask(ther, 'renomeia', [{'calls': [('rename_document', {'patient': 'Maria Silva', 'ref': ref(rep), 'name': 'Outro nome'})]}])
c = change(r)
call('PATCH', f'/documents/{rep}', ther, {'filename': 'Mudado à mão.pdf'})
ok(apply(c, ther).status_code == 409, 'renamed by hand meanwhile → Apply refused (stale)')

# ── discharge letters ──
r = ask(ther, 'prepara a carta de alta da maria', [{'calls': [('draft_letter', {'patient': 'Maria Silva'})]}])
c = change(r)
ok(c.get('skill') == 'letter_draft' and 'Modelo da ficha' in c['rows'][0][2] and any('Só o proprietário aprova' in w for w in c['warnings']), 'no text → a draft from the file’s template; only the owner approves ' + str(c.get('rows')))
ok(any('[a completar]' in w for w in c['warnings']), 'warns about the parts to complete')
ok(sql(f"SELECT count(*) FROM discharge_letters WHERE patient_id = '{maria}'") == '0', 'nothing saved before Apply')
ok(apply(c, ther).status_code == 200 and sql(f"SELECT count(*) || '|' || bool_or(ai_drafted) || '|' || bool_and(approved_at IS NULL) FROM discharge_letters WHERE patient_id = '{maria}'") == '1|false|true',
   'Apply saves one draft (not AI-written, not approved)')
ok(undo(c, ther).status_code == 200 and sql(f"SELECT count(*) FROM discharge_letters WHERE patient_id = '{maria}'") == '0', 'Undo throws the draft away')
body = 'A utente realizou sessões de fisioterapia por lombalgia, com melhoria da dor e regresso à corrida ligeira. Recomenda-se manter os exercícios em casa.'
r = ask(ther, 'escreve tu a carta', [{'calls': [('draft_letter', {'patient': 'Maria Silva', 'text': body, 'addressee': 'Exmo. Sr. Dr. Lopes'})]}])
c = change(r)
ok(c.get('skill') == 'letter_draft' and 'Escrita pela IA' in c['rows'][0][2] and 'Dr. Lopes' in c['rows'][0][2], 'the model’s own text, marked as AI-written')
ok(apply(c, ther).status_code == 200, 'Apply saves it')
letter = sql(f"SELECT id FROM discharge_letters WHERE patient_id = '{maria}'")
ok(sql(f"SELECT ai_drafted::text || '|' || addressee FROM discharge_letters WHERE id = '{letter}'") == 'true|Exmo. Sr. Dr. Lopes', 'saved as AI-drafted with the addressee')
r = ask(ther, 'muda a carta', [{'calls': [('edit_letter', {'patient': 'Maria Silva', 'ref': ref(letter), 'text': body + ' Alta a ' + TODAY.isoformat() + '.'})]}])
c2 = change(r)
ok(c2.get('skill') == 'letter_update' and c2['rows'][0][0].endswith('Texto'), 'edit a draft: before/after of the text')
ok(apply(c2, ther).status_code == 200 and 'Alta a' in sql(f"SELECT body FROM discharge_letters WHERE id = '{letter}'"), 'Apply changes the draft')
ok(undo(c, ther).status_code == 409, 'Undo of the draft after it was edited → refused (changed since)')
ok(undo(c2, ther).status_code == 200 and 'Alta a' not in sql(f"SELECT body FROM discharge_letters WHERE id = '{letter}'"), 'Undo of the edit puts the text back')
ok(call('POST', f'/letters/{letter}/approve', owner, {'discharge': False, 'professional_id': '12345'}).status_code == 200, '(the owner approves it by hand, in the app)')
r = ask(ther, 'muda a carta', [{'calls': [('edit_letter', {'patient': 'Maria Silva', 'ref': ref(letter), 'text': 'Outro texto qualquer para a carta.'})]}, {'text': 'Já está aprovada.'}])
ok('approved and can\'t change' in last_tool(), 'an approved letter can’t be edited by the assistant')
ok(undo(c, ther).status_code in (404, 409), 'nor removed by an old Undo')

# ── reimbursement pack ──
r = ask(ther, 'faz o pacote de reembolso da maria para a ADSE', [{'calls': [('reimbursement_pack', {'patient': 'Maria Silva'})]}])
c = change(r)
ok(c.get('skill') == 'pack' and 'ADSE' in c['rows'][0][2] and '1 sessões' in c['rows'][0][2] and '1 pagamentos' in c['rows'][0][2] and 'Dr. Lopes' in c['rows'][0][2],
   'preview: period, latest prescription, payer, sessions and payments ' + str(c.get('rows')))
ok(len(c['rows']) == 2 and 'Dor lombar' in c['rows'][1][2], 'the clinical report from the file is shown before Apply')
ok(any('sem número de fatura' in w for w in c['warnings']), 'warns about payments without an invoice number')
n0 = sql(f"SELECT count(*) FROM documents WHERE patient_id = '{maria}' AND kind = 'report'")
ok(apply(c, ther).status_code == 200 and int(sql(f"SELECT count(*) FROM documents WHERE patient_id = '{maria}' AND kind = 'report'")) == int(n0) + 1, 'Apply makes the PDF in the patient’s documents')
ok(sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.reimbursement_pack' AND subject_id = '{maria}'") == '1', 'logged as the app logs it')
ok(undo(c, ther).status_code == 200 and sql(f"SELECT count(*) FROM documents WHERE patient_id = '{maria}' AND kind = 'report'") == n0, 'Undo removes the pack')
r = ask(ther, 'pacote', [{'calls': [('reimbursement_pack', {'patient': 'Maria Silva', 'prescription': 'Dr. Inexistente'})]}, {'text': 'Qual?'}])
ok('No prescription matches' in last_tool() and 'Dr. Lopes' in last_tool(), 'an unknown prescription: the model is told which there are')

# ── consent withdrawal (can't be undone) ──
r = ask(recep, 'a maria retirou o consentimento', [{'calls': [('withdraw_consent', {'patient': 'Maria Silva', 'ref': ref(consent)})]}])
c = change(r)
ok(c.get('skill') == 'consent_revoke' and c.get('undoable') is False and any('Não dá para desfazer' in w for w in c['warnings']), 'withdrawal: preview, no Undo, warned')
ok(sql(f"SELECT revoked_at IS NULL FROM consents WHERE id = '{consent}'") in ('t', ''), 'nothing changed before Apply')
ok(apply(c, recep).status_code == 200 and sql(f"SELECT revoked_at IS NOT NULL FROM consents WHERE id = '{consent}'") in ('t', ''), 'Apply records the withdrawal')
ok(PSQL is None or sql(f"SELECT count(*) FROM documents WHERE consent_id = '{consent}' AND consent_event = 'revoked'") == '1', 'with its withdrawal PDF (like the app)')
ok(undo(c, recep).status_code == 409, 'Undo is refused')
r = ask(recep, 'outra vez', [{'calls': [('withdraw_consent', {'patient': 'Maria Silva', 'ref': ref(consent)})]}, {'text': 'Já estava.'}])
ok('already withdrawn' in last_tool(), 'a withdrawn consent can’t be withdrawn again')

# ── reminders ──
r = ask(recep, 'quem tenho de lembrar amanhã?', [{'calls': [('reminders', {})]}, {'text': 'O João.'}])
res = last_tool()
ok(f'ref {ref(a2)}' in res and 'João Pires' in res and 'not reminded yet' in res and 'has a phone' in res, 'tomorrow’s reminders with refs ' + res[:200])
ok('912345678' not in res, 'the phone number itself isn’t sent to the model')
r = ask(recep, 'já mandei sms ao joão', [{'calls': [('mark_reminded', {'refs': [ref(a2)], 'channel': 'sms'})]}])
c = change(r)
ok(c.get('skill') == 'reminder_mark' and 'Por lembrar' == c['rows'][0][1] and 'SMS' in c['rows'][0][2], 'preview: not reminded → reminded (SMS)')
ok(apply(c, recep).status_code == 200 and sql(f"SELECT channel FROM reminders_sent WHERE appointment_id = '{a2}'") == 'sms', 'Apply records it')
ok(undo(c, recep).status_code == 200 and sql(f"SELECT count(*) FROM reminders_sent WHERE appointment_id = '{a2}'") == '0', 'Undo unmarks it')
r = ask(recep, 'lembrei', [{'calls': [('mark_reminded', {'refs': [ref(a2)]})]}, {'text': 'Como?'}])
ok('how the reminders were sent' in last_tool(), 'no channel → the model must ask')

# ── the owner: complaints log ──
r = ask(ther, 'regista uma reclamação', [{'calls': [('log_complaint', {'source': 'paper', 'summary': 'Atraso'})]}, {'text': 'Não posso.'}])
ok("There's no tool called" in last_tool(), 'a therapist can’t log a complaint through the assistant')
r = ask(owner, 'recebemos uma reclamação no livro hoje', [{'calls': [('log_complaint', {'source': 'paper', 'summary': 'Esperou 40 minutos pela sessão', 'complainant': 'Maria Silva', 'book_ref': 'Folha 12'})]}])
c = change(r)
ok(c.get('skill') == 'complaint_create' and 'Livro em papel' in c['rows'][0][2] and any('10 dias úteis' in w and 'ERS' in w for w in c['warnings']), 'preview with the ERS deadline ' + str(c.get('warnings')))
ok(apply(c, ther).status_code == 403, 'a therapist can’t apply an owner change')
ok(apply(c, owner).status_code == 200 and sql("SELECT count(*) FROM complaints") == '1', 'Apply logs the complaint')
comp = sql("SELECT id FROM complaints")
r = ask(owner, 'reclamações?', [{'calls': [('complaints', {})]}, {'text': 'Uma.'}])
ok(f'ref {ref(comp)}' in last_tool() and 'working days left' in last_tool() and 'open' in last_tool(), 'complaints: ref, deadline, open ' + last_tool()[:200])
r = ask(owner, 'respondi hoje', [{'calls': [('update_complaint', {'ref': ref(comp), 'reply': 'Pedimos desculpa pelo atraso.', 'replied_on': 'hoje', 'sent_to_ers_on': 'hoje'})]}])
c2 = change(r)
ok(c2.get('skill') == 'complaint_update' and len(c2['rows']) == 3, 'reply, day answered and day sent to the ERS')
ok(apply(c2, owner).status_code == 200 and sql(f"SELECT sent_to_ers_on::text FROM complaints WHERE id = '{comp}'") == TODAY.isoformat(), 'Apply saves them')
ok(undo(c, owner).status_code == 409, 'Undo of the new complaint after it was answered → refused')
ok(undo(c2, owner).status_code == 200 and sql(f"SELECT COALESCE(sent_to_ers_on::text, 'null') || '|' || reply FROM complaints WHERE id = '{comp}'") == 'null|', 'Undo of the answer puts it back')
ok(undo(c, owner).status_code == 200 and sql("SELECT count(*) FROM complaints") == '0', 'then Undo of the complaint removes it')
r = ask(owner, 'reclamação amanhã', [{'calls': [('log_complaint', {'source': 'paper', 'summary': 'x', 'received': (TODAY + timedelta(days=1)).isoformat()})]}, {'text': 'Não.'}])
ok('in the future' in last_tool(), 'a complaint can’t be received in the future')

# ── the owner: legal check (look only) ──
r = ask(owner, 'posso apagar a maria?', [{'calls': [('legal_check', {'patient': 'Maria Silva'})]}, {'text': 'Tem de guardar.'}])
ok('20 years' in last_tool() and 'by hand' in last_tool() and 'payments' in last_tool(), 'legal check: what the file holds, the keeping period, done by hand ' + last_tool()[:160])

# ── the owner: settings ──
r = ask(owner, 'definições?', [{'calls': [('clinic_settings', {})]}, {'text': 'Aqui.'}])
ok('Clinic: name Clínica Teste' in last_tool() and 'Ana Rocha · therapist · AI on' in last_tool() and 'Website:' in last_tool(), 'clinic_settings reads details, AI switches and the website')
r = ask(owner, 'nif 123456780', [{'calls': [('clinic_details', {'nif': '123456780'})]}, {'text': 'NIF inválido.'}])
ok("NIF isn't valid" in last_tool(), 'a bad NIF is refused before the preview')
r = ask(owner, 'muda o nif e o telefone', [{'calls': [('clinic_details', {'nif': '123456789', 'phone': '213456789'})]}])
c = change(r)
ok(c.get('skill') == 'clinic_details' and len(c['rows']) == 2 and any('PDFs' in w for w in c['warnings']), 'clinic details: only what changes, warned about PDFs')
ok(apply(c, ther).status_code == 403, 'only the owner applies it')
ok(apply(c, owner).status_code == 200 and sql("SELECT nif || '|' || phone || '|' || name FROM clinic") == '123456789|213456789|Clínica Teste', 'Apply changes them, keeps the rest')
ok(undo(c, owner).status_code == 200 and sql("SELECT nif || '|' || phone FROM clinic") == '|', 'Undo puts them back')
r = ask(owner, 'telefone', [{'calls': [('clinic_details', {'phone': '213456789'})]}])
c = change(r)
call('PUT', '/clinic', owner, {'name': 'Clínica Teste', 'phone': '219999999'})
ok(apply(c, owner).status_code == 409, 'changed by hand meanwhile → stale')

r = ask(owner, 'muda a mensagem', [{'calls': [('reminder_text', {'pt': 'Olá! Lembramos a sua sessão amanhã.'})]}, {'text': 'Faltam os campos.'}])
ok('lost the placeholder {nome}' in last_tool(), 'a reminder message without {nome} is refused')
newpt = 'Olá {nome}, até {dia} às {hora} na {clinica}!'
r = ask(owner, 'muda a mensagem', [{'calls': [('reminder_text', {'pt': newpt})]}])
c = change(r)
ok(c.get('skill') == 'reminder_text' and c['rows'][0][2] == newpt, 'reminder message preview')
old = sql("SELECT reminder_pt FROM clinic")
ok(apply(c, owner).status_code == 200 and sql("SELECT reminder_pt FROM clinic") == newpt and sql("SELECT reminder_en FROM clinic").startswith('Hi {name}'), 'Apply changes Portuguese only')
ok(undo(c, owner).status_code == 200 and sql("SELECT reminder_pt FROM clinic") == old, 'Undo puts it back')

r = ask(owner, 'desliga a IA da Ana', [{'calls': [('ai_switch', {'name': 'Ana', 'on': False})]}])
c = change(r)
ok(c.get('skill') == 'staff_ai' and c['rows'][0] == ['Ana Rocha — Interruptor de IA', 'Ligado', 'Desligado'], 'AI switch preview ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql(f"SELECT ai_enabled FROM staff WHERE id = '{ana}'") == 'f', 'Apply turns it off')
ok(undo(c, owner).status_code == 200 and sql(f"SELECT ai_enabled FROM staff WHERE id = '{ana}'") == 't', 'Undo turns it on again')
r = ask(owner, 'desliga a minha', [{'calls': [('ai_switch', {'name': 'Filipe Sousa', 'on': False})]}, {'text': 'Não dá.'}])
ok('always on' in last_tool(), 'the owner’s own switch stays on')

r = ask(owner, 'esconde os preços do site', [{'calls': [('website', {'show_prices': False, 'tagline_pt': 'Fisioterapia perto de si'})]}])
c = change(r)
ok(c.get('skill') == 'website' and ['Site — Mostrar preços', 'Sim', 'Não'] in c['rows'] and any('site público' in w for w in c['warnings']), 'website preview ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql("SELECT show_prices::text || '|' || tagline_pt FROM website") == 'false|Fisioterapia perto de si', 'Apply changes the site')
ok(undo(c, owner).status_code == 200 and sql("SELECT show_prices::text || '|' || tagline_pt FROM website") == 'true|Fisioterapia com tempo para si', 'Undo puts it back')
r = ask(owner, 'publica o site', [{'calls': [('website', {'published': True})]}])
c = change(r)
ok(apply(c, owner).status_code == 400 and sql("SELECT published FROM website") == 'f', 'publishing without the clinic’s NIF: the app’s own check refuses it at Apply')

# ── English ──
r = ask(owner, 'hide prices', [{'calls': [('website', {'show_prices': False})]}], lang='en')
c = change(r)
ok(c['rows'][0] == ['Website — Show Prices', 'Yes', 'No'], 'English labels ' + str(c.get('rows')))

# ── nothing about patients in the server log ──
if LOG:
    text = open(LOG, encoding='utf-8', errors='replace').read()
    ok('Maria Silva' not in text and 'João Pires' not in text and 'Esperou 40 minutos' not in text, 'no patient names or complaint text in the server log')

print(f'\n{"ALL PASSED" if fails == 0 else f"{fails} FAILED"}')
sys.exit(1 if fails else 0)
