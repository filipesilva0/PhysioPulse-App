# End-to-end test of the assistant, stage 2 (13 Nov 2026): money and staff. Same rules as stage 1 — look-ups and
# Apply through the app's own endpoints as the person (role checks unchanged), every change waits for Apply and can
# be undone (a void can't). Needs a running server with an EMPTY database, started with
# OLLAMA_URL=http://127.0.0.1:11992 (this test runs a stand-in for Ollama there that plays a model with tools).
# Run: PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_agent_money.py
import os, sys, json, re, threading, subprocess, requests, pyotp
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
fake = HTTPServer(('127.0.0.1', 11992), Fake)
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
staff = {s['name']: s for s in call('GET', '/staff', owner).json()}
ana, rita = staff['Ana Rocha']['id'], staff['Rita Lopes']['id']
for who in (ana, rita):
    call('PATCH', f'/staff/{who}', owner, {'ai_enabled': True})

def newp(name, **kw):
    return call('POST', '/patients', recep, {'name': name, 'therapist_id': ana, **kw}).json()['id']
maria, joao, ines = newp('Maria Silva'), newp('João Pires'), newp('Inês Costa')
num = {p['id']: p['number'] for p in call('GET', '/patients', owner).json()}
def appt(pid, kind='session', days_ago=1, hour=10):
    s = (datetime.now() - timedelta(days=days_ago)).replace(hour=hour, minute=0, second=0, microsecond=0).astimezone()
    body = {'kind': kind, 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': ana, 'patient_id': pid, 'allow_overlap': True}
    if kind == 'home': body['address'] = 'Rua A'
    r = call('POST', '/appointments', recep, body); assert r.status_code == 200, r.text
    aid = r.json()['ids'][0]
    assert call('PATCH', f'/appointments/{aid}', recep, {'status': 'done'}).status_code == 200
    return aid
a1 = appt(maria, days_ago=2)          # €20 session
a2 = appt(maria, 'home', days_ago=1)   # €40 home visit
a3 = appt(joao, days_ago=1)
TODAY = date.fromisoformat(sql("SELECT ((now() AT TIME ZONE 'Europe/Lisbon')::date)::text"))
def money(pid, tok=None): return call('GET', f'/patients/{pid}/money', tok or owner).json()

def ask(tok, msg, steps, lang='pt'):
    chats.clear(); script.clear(); script.extend(steps)
    return call('POST', '/ai/assistant', tok, {'message': msg, 'lang': lang, 'history': []})
def tool_names(body): return {t['function']['name'] for t in body['tools']}
def tool_results(body): return [m['content'] for m in body['messages'] if m['role'] == 'tool']
def apply(c, tok): return call('POST', f"/assistant/changes/{c['id']}/apply", tok)
def undo(c, tok): return call('POST', f"/assistant/changes/{c['id']}/undo", tok)

# ── who gets which money tools (same reach as the app) ──
ask(owner, 'olá', [{'text': 'Olá.'}]); own = tool_names(chats[0])
ok({'patient_money', 'price_list', 'record_payment', 'sell_package', 'package_status', 'invoice_number', 'void_payment',
    'owed_list', 'payments', 'expenses', 'salaries', 'finance_overview', 'add_expense', 'pay_salary', 'waive_charge', 'close_month'} <= own, 'the owner gets every money tool')
ok('Money: amounts are euros' in chats[0]['messages'][0]['content'], 'the brief has the money rule')
ask(recep, 'olá', [{'text': 'Olá.'}]); rc = tool_names(chats[0])
ok({'patient_money', 'record_payment', 'sell_package', 'invoice_number'} <= rc and not ({'owed_list', 'payments', 'expenses', 'salaries', 'add_expense', 'pay_salary', 'waive_charge', 'close_month', 'finance_overview'} & rc),
   'reception: takes payments and sells packages, no clinic finances ' + str(sorted(rc & {'owed_list', 'payments', 'expenses'})))
ask(ther, 'olá', [{'text': 'Olá.'}]); th = tool_names(chats[0])
ok('record_payment' in th and 'expenses' not in th and 'pay_salary' not in th, 'a therapist: payments at the desk, no finances')
r = ask(recep, 'quanto faturámos?', [{'calls': [('payments', {})]}, {'text': 'Não posso.'}])
ok(r.status_code == 200 and "There's no tool called" in tool_results(chats[1])[0], 'an owner tool called by reception’s model is refused')

# ── looking up a patient's money (through the app, logged) ──
before = sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.viewed' AND subject_id = '{maria}' AND detail = 'money'")
r = ask(recep, 'quanto deve a maria silva?', [{'calls': [('patient_money', {'patient': 'Maria Silva'})]}, {'text': 'Deve 60 euros.'}])
res = tool_results(chats[1])[0] if len(chats) > 1 else ''
ok('owes €60.00 in total' in res and f'ref {a1.replace("-", "")[:8]}' in res and 'Home Visit' in res, 'patient_money: total and each owed item with its ref ' + res[:200])
ok(r.json()['steps'] and 'pagamentos' in r.json()['steps'][0], 'the step says what was looked up ' + str(r.json()['steps']))
after = sql(f"SELECT count(*) FROM access_log WHERE action = 'patient.viewed' AND subject_id = '{maria}' AND detail = 'money'")
ok(PSQL is None or int(after) == int(before) + 1, 'the look-up is in the access log like a click (patient.viewed · money)')
r = ask(recep, 'preços?', [{'calls': [('price_list', {})]}, {'text': 'Aqui.'}])
ok('Pacote de 10' in tool_results(chats[1])[0] and '€170.00' in tool_results(chats[1])[0], 'price_list reads the price list')

# ── recording a payment: preview, Apply, Undo (= void) ──
r = ask(recep, 'a maria silva pagou tudo em mb way', [{'calls': [('patient_money', {'patient': 'Maria Silva'})]}, {'calls': [('record_payment', {'patient': 'Maria Silva', 'method': 'MB Way'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'payment' and len(c['rows']) == 2 and all('MB WAY' in x[2] for x in c['rows']) and any('20,00 €' in x[2] for x in c['rows']) and any('40,00 €' in x[2] for x in c['rows']),
   'all owed → one row per item, with amount and method ' + str(c.get('rows')))
ok(any('registo fiscal' in w for w in c.get('warnings', [])) and c.get('undoable') is True, 'warns it’s a fiscal record; can be undone')
ok(sql(f"SELECT count(*) FROM payments WHERE patient_id = '{maria}'") == '0', 'nothing recorded before Apply')
ok(apply(c, ther).status_code == 403, 'someone else can’t apply it')
ok(apply(c, recep).status_code == 200 and money(maria)['due_cents'] == 0 and sql(f"SELECT string_agg(method, ',') FROM payments WHERE patient_id = '{maria}'") == 'mbway,mbway', 'Apply records both payments: nothing owed')
ok(sql(f"SELECT count(*) FROM access_log WHERE action = 'payment.recorded' AND subject_id = '{maria}'") == '1', 'logged as the app logs it (payment.recorded)')
ok(undo(c, recep).status_code == 200 and money(maria)['due_cents'] == 6000 and sql(f"SELECT count(*) FROM payments WHERE patient_id = '{maria}' AND voided_at IS NOT NULL AND void_reason = 'Desfeito no assistente'") == '2',
   'Undo voids them (payments are never deleted): owed again')

r = ask(recep, 'a maria pagou 10 euros da sessão em numerário', [{'calls': [('record_payment', {'patient': 'Maria Silva', 'method': 'cash', 'pay_for': 'item', 'ref': a1.replace('-', '')[:8], 'amount_eur': 10})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'payment' and len(c['rows']) == 1 and '10,00 €' in c['rows'][0][2] and any('fica em dívida 10,00 €' in w for w in c['warnings']), 'one item by ref, part payment warned ' + str(c.get('warnings')))
ok(apply(c, recep).status_code == 200 and money(maria)['due_cents'] == 5000, 'Apply: €10 paid, €50 still owed')
r = ask(recep, 'a maria pagou 25', [{'calls': [('record_payment', {'patient': 'Maria Silva', 'method': 'cash', 'amount_eur': 25})]}, {'text': 'Qual?'}])
ok('over 2 items, not €25.00' in tool_results(chats[1])[0], 'an amount that doesn’t match what’s owed: the model is asked which item')
r = ask(recep, 'pagou', [{'calls': [('record_payment', {'patient': 'Maria Silva'})]}, {'text': 'Como pagou?'}])
ok('how it was paid' in tool_results(chats[1])[0], 'no method → the model must ask how it was paid')
r = ask(recep, 'o joão pagou amanhã', [{'calls': [('record_payment', {'patient': 'João Pires', 'method': 'cash', 'date': (TODAY + timedelta(days=1)).isoformat()})]}, {'text': 'Não dá.'}])
ok("can't be dated in the future" in tool_results(chats[1])[0], 'a payment dated in the future is refused')
r = ask(recep, 'aula avulsa paga em dinheiro, 10 euros, sem ficha', [{'calls': [('record_payment', {'patient': '', 'pay_for': 'other', 'method': 'dinheiro', 'amount_eur': 10, 'description': 'Aula avulsa'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'payment' and c['rows'][0][0].startswith('Sem utente') and apply(c, recep).status_code == 200, 'a walk-in payment without a file')

# ── invoice number, void ──
pn = sql(f"SELECT number FROM payments WHERE patient_id = '{maria}' AND voided_at IS NULL")
r = ask(recep, f'fatura do pagamento {pn} é FR 2026/7', [{'calls': [('invoice_number', {'payment': f'nº {pn}', 'invoice_number': 'FR 2026/7'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'invoice_number' and c['rows'][0][1] == '—' and c['rows'][0][2] == 'FR 2026/7', 'invoice number preview ' + str(c.get('rows')))
ok(apply(c, recep).status_code == 200 and sql(f"SELECT invoice_number FROM payments WHERE number = {pn}") == 'FR 2026/7', 'Apply adds the invoice number')
ok(undo(c, recep).status_code == 200 and sql(f"SELECT invoice_number FROM payments WHERE number = {pn}") == '', 'Undo takes it off')
r = ask(owner, f'anula o pagamento {pn}, valor errado', [{'calls': [('void_payment', {'payment': str(pn), 'reason': 'Valor errado'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'payment_void' and c.get('undoable') is False and 'Anulado — Valor errado' in c['rows'][0][2], 'void preview: can’t be undone ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql(f"SELECT void_reason FROM payments WHERE number = {pn}") == 'Valor errado', 'Apply voids it')
ok(undo(c, owner).status_code == 409, 'Undo refused: a void stays')
r = ask(ther, 'anula o pagamento da aula', [{'calls': [('void_payment', {'payment': sql("SELECT number FROM payments WHERE patient_id IS NULL"), 'reason': 'x'})]}])
c = r.json().get('change') or {}
ok(c and apply(c, ther).status_code == 403 and sql("SELECT count(*) FROM payments WHERE patient_id IS NULL AND voided_at IS NULL") == '1', 'a therapist can’t void someone else’s payment (the app’s rule decides at Apply)')

# ── packages ──
r = ask(recep, 'vende o pacote de 10 à inês', [{'calls': [('sell_package', {'patient': 'Inês Costa', 'product': 'pacote de 10'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'plan_sell' and 'Pacote de 10' in c['rows'][0][2] and '170,00 €' in c['rows'][0][2], 'sell preview ' + str(c.get('rows')))
ok(apply(c, recep).status_code == 200 and sql(f"SELECT count(*) FROM patient_plans WHERE patient_id = '{ines}' AND status = 'active'") == '1' and money(ines)['due_cents'] == 17000, 'Apply sells it: €170 owed')
ok(undo(c, recep).status_code == 200 and sql(f"SELECT status FROM patient_plans WHERE patient_id = '{ines}'") == 'cancelled' and money(ines)['due_cents'] == 0, 'Undo cancels the unused package')
r = ask(recep, 'vende o pacote', [{'calls': [('sell_package', {'patient': 'Inês Costa', 'product': 'pacote'})]}, {'text': 'Qual?'}])
ok('Several match' in tool_results(chats[1])[0], 'an unclear product: the model asks which')
r = ask(recep, 'o joão quer o acompanhamento 4 por mês', [{'calls': [('sell_package', {'patient': 'João Pires', 'product': 'Acompanhamento, 4 por mês'})]}])
ok(apply(r.json()['change'], recep).status_code == 200, 'monthly plan sold')
r = ask(recep, 'põe em pausa o plano do joão', [{'calls': [('package_status', {'patient': 'João Pires', 'status': 'paused'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'plan_status' and c['rows'][0][1] == 'Ativo' and c['rows'][0][2] == 'Em pausa', 'pause preview ' + str(c.get('rows')))
ok(apply(c, recep).status_code == 200 and sql(f"SELECT status FROM patient_plans WHERE patient_id = '{joao}'") == 'paused', 'Apply pauses it')
ok(undo(c, recep).status_code == 200 and sql(f"SELECT status FROM patient_plans WHERE patient_id = '{joao}'") == 'active', 'Undo resumes it')
r = ask(recep, 'paga o mês do joão por transferência', [{'calls': [('patient_money', {'patient': 'João Pires'})]}, {'calls': [('record_payment', {'patient': 'João Pires', 'method': 'transfer', 'pay_for': 'month'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'payment' and len(c['rows']) == 1 and '72,00 €' in c['rows'][0][2], 'a month of a monthly plan ' + str(c.get('rows')))
ok(apply(c, recep).status_code == 200 and sql(f"SELECT covers_month::text FROM payments WHERE patient_id = '{joao}'").startswith(TODAY.strftime('%Y-%m')), 'Apply pays this month')

# ── the owner's money ──
r = ask(owner, 'quem me deve dinheiro?', [{'calls': [('owed_list', {})]}, {'text': 'A Maria e o João.'}])
res = tool_results(chats[1])[0]
ok('Maria Silva' in res and 'João Pires' in res and 'Owed in total' in res, 'owed_list: who owes and how much')
r = ask(owner, 'pagamentos deste mês?', [{'calls': [('payments', {})]}, {'text': 'Ok.'}])
ok('payment nº' in tool_results(chats[1])[0] and 'VOID' in tool_results(chats[1])[0], 'payments: this month’s, voids marked')
r = ask(owner, 'números do mês', [{'calls': [('finance_overview', {'month': 'este mês'})]}, {'text': 'Ok.'}])
ok('revenue €' in tool_results(chats[1])[0], 'finance_overview reads the month')

r = ask(owner, 'paguei a renda, 650 euros por transferência', [{'calls': [('add_expense', {'amount_eur': 650, 'category': 'renda', 'method': 'transferência', 'supplier': 'Senhorio'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'expense' and 'Renda' in c['rows'][0][0] and '650,00 €' in c['rows'][0][2], 'expense preview ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql("SELECT amount_cents || category || method FROM expenses") == '65000renttransfer', 'Apply records the expense')
ok(undo(c, owner).status_code == 200 and sql("SELECT count(*) FROM expenses") == '0', 'Undo deletes it')
r = ask(owner, 'despesa de 30', [{'calls': [('add_expense', {'amount_eur': 30, 'category': 'xyz'})]}, {'text': 'Que categoria?'}])
ok('Say the category' in tool_results(chats[1])[0], 'an unknown category: the model asks')

call('PUT', f'/staff/{ana}/pay', owner, {'model': 'fixed', 'fixed_cents': 145000})
r = ask(owner, 'paga o salário da ana deste mês', [{'calls': [('pay_salary', {'name': 'Ana'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'salary' and '1450,00 €' in c['rows'][0][2] and '344,38 €' in c['rows'][0][2], 'salary preview: fixed pay and employer SS ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql(f"SELECT gross_cents || '/' || employer_ss_cents FROM salary_payments WHERE staff_id = '{ana}'") == '145000/34438', 'Apply records the salary')
r = ask(owner, 'paga outra vez', [{'calls': [('pay_salary', {'name': 'Ana Rocha'})]}])
ok(any('já tem' in w.lower() or 'já há' in w.lower() for w in r.json()['change']['warnings']), 'a second salary in the same month is warned')
call('POST', f"/assistant/changes/{r.json()['change']['id']}/cancel", owner)
ok(undo(c, owner).status_code == 200 and sql(f"SELECT count(*) FROM salary_payments WHERE staff_id = '{ana}'") == '0', 'Undo deletes the salary')

r = ask(owner, 'não cobres a sessão do joão', [{'calls': [('waive_charge', {'ref': a3.replace('-', '')[:8]})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'waive' and c['rows'][0][1] == '20,00 €' and c['rows'][0][2] == 'Não cobrar', 'waive preview ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and not any(o['appointment_id'] == a3 for o in money(joao)['owed']), 'Apply waives it: not owed')
ok(undo(c, owner).status_code == 200 and any(o['appointment_id'] == a3 for o in money(joao)['owed']), 'Undo charges it again')

last = (TODAY.replace(day=1) - timedelta(days=1)).strftime('%Y-%m')
r = ask(owner, 'fecha o mês passado', [{'calls': [('close_month', {'month': 'mês passado'})]}])
c = r.json().get('change') or {}
ok(c.get('skill') == 'month_close' and c['rows'][0][1] == 'Aberto' and c['rows'][0][2] == 'Fechado', 'close preview ' + str(c.get('rows')))
ok(apply(c, owner).status_code == 200 and sql(f"SELECT count(*) FROM month_closes WHERE to_char(month, 'YYYY-MM') = '{last}'") == '1', 'Apply closes it')
ok(undo(c, owner).status_code == 200 and sql("SELECT count(*) FROM month_closes") == '0', 'Undo reopens it')
r = ask(owner, 'fecha este mês', [{'calls': [('close_month', {'month': TODAY.strftime('%Y-%m')})]}, {'text': 'Ainda não acabou.'}])
ok("hasn't ended" in tool_results(chats[1])[0], 'the current month can’t be closed')

# ── staleness and roles at Apply ──
r = ask(recep, 'fatura', [{'calls': [('invoice_number', {'payment': sql(f"SELECT number FROM payments WHERE patient_id = '{joao}'"), 'invoice_number': 'FR 2026/9'})]}])
c = r.json()['change']
call('PATCH', f"/payments/{sql(f'SELECT id FROM payments WHERE patient_id = ' + chr(39) + joao + chr(39))}", owner, {'invoice_number': 'FR 2026/8'})
ok(apply(c, recep).status_code == 409, 'changed by hand meanwhile: Apply refused')
ok(sql("SELECT count(*) FROM assistant_changes WHERE skill = 'expense' AND created_by <> (SELECT id FROM staff WHERE role = 'owner')") == '0', 'only the owner prepares expenses')

# ── English, and nothing in the log ──
r = ask(owner, 'Maria paid the rest in cash', [{'calls': [('record_payment', {'patient': 'Maria Silva', 'method': 'cash'})]}], lang='en')
c = r.json()['change']
ok(any('€' in x[2] and 'Cash' in x[2] for x in c['rows']) and any('fiscal record' in w for w in c['warnings']), 'English preview ' + str(c['rows']))
if LOG:
    text = open(LOG).read()
    ok(not re.search(r'Maria Silva|João Pires|Inês Costa|Senhorio|FR 2026', text), 'server log has no names or invoice numbers')

print('\n' + ('ALL PASSED' if not fails else f'{fails} FAILED'))
sys.exit(1 if fails else 0)
