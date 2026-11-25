# End-to-end test of the phase 3 API (money): prices, packages and plans, what a done appointment uses
# and costs, payments (never deleted, voided with a reason), invoice records, what's owed, expenses with
# receipts, cash close, salaries, month close and the accountant export.
# Needs a running server with an EMPTY database. Optional: PSQL_URL=postgres://… also checks the
# database triggers directly (payments can't be deleted or changed).
# Run: pip install requests pyotp && python3 server/tests/e2e_phase3.py   (BASE env var overrides the URL)
import base64, io, os, subprocess, sys, zipfile, requests, pyotp
from datetime import datetime, timedelta, date

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
H = {'X-Device': 'Rececao-PC', 'X-Device-Key': DEVICE_KEY}
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

# ── accounts ──
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

today = date.today()
this_month = today.strftime('%Y-%m')
last_month_day = (today.replace(day=1) - timedelta(days=1))
last_month = last_month_day.strftime('%Y-%m')

# ── price list ──
r = call('GET', '/products', recep); prods = {p['code']: p for p in r.json()}
ok(r.status_code == 200 and len(prods) == 12, 'reception reads the 12 seeded products')
ok(prods['pack10']['price_cents'] == 17000 and prods['cc4']['price_cents'] == 7200 and prods['cc8']['price_cents'] == 13600, 'suggested ladder prices seeded (owner decision)')
ok(call('PATCH', f"/products/{prods['single']['id']}", recep, {'price_cents': 1}).status_code == 403, 'reception cannot change prices')
ok(call('PATCH', f"/products/{prods['single']['id']}", owner, {'price_cents': 2000, 'name_en': 'Single session (45 min)'}).status_code == 200, 'owner edits a product')
ok(call('POST', '/products', owner, {'kind': 'visit', 'visit_kind': 'session', 'name_pt': 'X', 'name_en': 'X', 'price_cents': 1}).status_code == 409, 'only one active price per appointment kind')
r = call('POST', '/products', owner, {'kind': 'pack', 'name_pt': 'Pacote de 3', 'name_en': 'Pack of 3', 'price_cents': 5400, 'sessions': 3, 'valid_months': 1})
ok(r.status_code == 200, 'owner adds a package'); pack3 = r.json()['id']
ok(call('PATCH', f'/products/{pack3}', owner, {'active': False}).status_code == 200 and len(call('GET', '/products', recep).json()) == 12, 'retired products leave the list')
ok(len(call('GET', '/products?all=true', owner).json()) == 13, '…but stay visible with ?all=true')

# ── patients ──
def patient(name):
    return call('POST', '/patients', recep, {'name': name, 'therapist_id': ther_id, 'nif': valid_nif(next(_nifs))}).json()['id']
maria, joao, ines = patient('Maria Santos'), patient('João Pires'), patient('Inês Costa')
def appt(pid, kind='session', days_ago=1, hour=10, th=None, **kw):
    s = (datetime.now() - timedelta(days=days_ago)).replace(hour=hour, minute=0, second=0, microsecond=0).astimezone()
    body = {'kind': kind, 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(minutes=45)).isoformat(), 'therapist_id': th or ther_id,
            'patient_id': pid, 'allow_overlap': True, **kw}
    if kind == 'home': body['address'] = 'Rua A'
    r = call('POST', '/appointments', recep, body); assert r.status_code == 200, r.text
    return r.json()['ids'][0]
def done(aid, tok=None): return call('PATCH', f'/appointments/{aid}', tok or recep, {'status': 'done'})
def money(pid, tok=None): return call('GET', f'/patients/{pid}/money', tok or recep).json()

# ── selling a package; done sessions use its credits ──
r = call('POST', f'/patients/{maria}/plans', recep, {'product_id': prods['pack5']['id'], 'starts_on': (today - timedelta(days=7)).isoformat()})
ok(r.status_code == 200, 'reception sells a pack of 5 (from last week)'); m_pack = r.json()['id']
ok(call('POST', f'/patients/{maria}/plans', recep, {'product_id': prods['single']['id']}).status_code == 400, 'a visit price is not a package')
ok(call('POST', f'/patients/{maria}/plans', recep, {'product_id': prods['pack5']['id'], 'shared_with': joao}).status_code == 400, 'pack of 5 is not shareable')
mm = money(maria); pl = mm['plans'][0]
ok(pl['remaining'] == 5 and pl['expires_on'] is not None and pl['paid_cents'] == 0, 'new pack: 5 left, expiry set, nothing paid')
ok([o['type'] for o in mm['owed']] == ['plan'] and mm['due_cents'] == 8000, 'unpaid pack shows as owed (€80)')
a1 = appt(maria, days_ago=3); ok(done(a1).status_code == 200, 'session marked done')
r = call('GET', f"/appointments?from={(datetime.now()-timedelta(days=5)).astimezone().isoformat()}&to={datetime.now().astimezone().isoformat()}".replace('+', '%2B'), recep)
row = next(x for x in r.json() if x['id'] == a1)
ok(row['plan_id'] == m_pack and row['charge_cents'] is None, 'done session used a pack credit, no charge')
ok(money(maria)['plans'][0]['remaining'] == 4, 'pack: 4 left')
a2 = appt(maria, 'assessment', days_ago=4); done(a2)
mm = money(maria)
ok(any(o['appointment_id'] == a2 and o['due_cents'] == 3000 for o in mm['owed']) and mm['plans'][0]['remaining'] == 4, 'assessment is not covered by a plain pack → €30 owed')
a3 = appt(maria, 'home', days_ago=2); done(a3)
ok(any(o['appointment_id'] == a3 and o['due_cents'] == 4000 for o in money(maria)['owed']), 'home visit has its own price (€40)')
r = call('PATCH', f'/appointments/{a1}', recep, {'status': 'booked'})
ok(r.status_code == 200 and money(maria)['plans'][0]['remaining'] == 5, 'Undo done → the credit comes back')
done(a1)
ok(call('PATCH', f'/appointments/{a3}', recep, {'charge_waived': True}).status_code == 403, 'reception cannot waive a charge')
ok(call('PATCH', f'/appointments/{a3}', owner, {'charge_waived': True}).status_code == 200 and not any(o['appointment_id'] == a3 for o in money(maria)['owed']), 'owner waives the home visit')
ok(call('PATCH', f'/appointments/{a2}', recep, {'plan_id': m_pack}).status_code == 400, 'choosing a pack that can’t cover an assessment is refused')

# starter covers the assessment
r = call('POST', f'/patients/{ines}/plans', recep, {'product_id': prods['starter']['id'], 'starts_on': (today - timedelta(days=7)).isoformat()}); starter = r.json()['id']
a4 = appt(ines, 'assessment', days_ago=2); done(a4)
mi = money(ines)
ok(mi['plans'][0]['remaining'] == 4 and not any(o.get('appointment_id') == a4 for o in mi['owed']), 'starter pack covers the first assessment')
a5 = appt(ines, 'assessment', days_ago=1); done(a5)
ok(any(o.get('appointment_id') == a5 for o in money(ines)['owed']), '…but only one assessment')

# backdated sale covers unpaid done sessions
tiago = patient('Tiago Reis')
b1 = appt(tiago, days_ago=5); b2 = appt(tiago, days_ago=2); done(b1); done(b2)
ok(money(tiago)['due_cents'] == 4000, 'two single sessions owed (€40)')
r = call('POST', f'/patients/{tiago}/plans', recep, {'product_id': prods['pack5']['id'], 'starts_on': (today - timedelta(days=3)).isoformat()})
mt = money(tiago)
ok(r.json()['attached'] == 1 and mt['plans'][0]['remaining'] == 4 and mt['due_cents'] == 2000 + 8000, 'pack sold from 3 days ago takes the session after that date only')

# shared pack of 10
r = call('POST', f'/patients/{maria}/plans', recep, {'product_id': prods['pack10']['id'], 'shared_with': joao, 'starts_on': (today - timedelta(days=7)).isoformat()}); p10 = r.json()['id']
ok(r.status_code == 200, 'pack of 10 shared with a family member')
mj = money(joao); ok(any(p['id'] == p10 and p['shared_with_name'] == 'João Pires' for p in mj['plans']), 'shared pack appears in the family member’s file')
ok(not any(o.get('plan_id') == p10 for o in mj['owed']), '…but it’s owed by the buyer, not by them')
a6 = appt(joao, days_ago=1); done(a6)
ok(next(p for p in money(joao)['plans'] if p['id'] == p10)['remaining'] == 9, 'family member’s session used the shared pack')

# ── payments ──
r = call('POST', '/payments', recep, {'patient_id': maria, 'method': 'cash', 'items': [{'plan_id': m_pack, 'amount_cents': 8000}, {'appointment_id': a2, 'amount_cents': 3000}]})
ok(r.status_code == 200 and len(r.json()['ids']) == 2, 'reception records a payment for the pack and the assessment'); pay_pack, pay_ass = r.json()['ids']
mm = money(maria)
ok(not any(o.get('plan_id') == m_pack or o.get('appointment_id') == a2 for o in mm['owed']), 'paid items leave the owed list')
ok(mm['payments'][0]['description'] in ('Pacote de 5', 'Avaliação inicial ' + (today - timedelta(days=4)).strftime('%d/%m/%Y')), 'automatic Portuguese description')
ok(call('POST', '/payments', recep, {'patient_id': maria, 'method': 'stripe', 'items': [{'amount_cents': 100, 'description': 'x'}]}).status_code == 400, 'Stripe can’t be recorded by hand')
ok(call('POST', '/payments', recep, {'patient_id': maria, 'method': 'cash', 'items': [{'amount_cents': 0, 'description': 'x'}]}).status_code == 400, 'amount must be positive')
ok(call('POST', '/payments', recep, {'patient_id': maria, 'method': 'cash', 'received_on': (today + timedelta(days=1)).isoformat(), 'items': [{'amount_cents': 100, 'description': 'x'}]}).status_code == 400, 'no payments dated in the future')
ok(call('POST', '/payments', recep, {'patient_id': joao, 'method': 'cash', 'items': [{'plan_id': p10, 'amount_cents': 100}]}).status_code == 400, 'can’t pay someone else’s package')
ok(call('POST', '/payments', recep, {'patient_id': joao, 'method': 'cash', 'items': [{'appointment_id': a2, 'amount_cents': 100}]}).status_code == 400, 'can’t pay someone else’s appointment')
ok(call('POST', '/payments', recep, {'method': 'cash', 'items': [{'amount_cents': 1000}]}).status_code == 400, 'a payment needs a description')
r = call('POST', '/payments', recep, {'method': 'mbway', 'items': [{'amount_cents': 1000, 'description': 'Aula avulsa (sem ficha)'}]})
ok(r.status_code == 200, 'walk-in payment without a patient'); walkin = r.json()['ids'][0]
# instalments
r = call('POST', '/payments', recep, {'patient_id': maria, 'method': 'transfer', 'items': [{'plan_id': p10, 'amount_cents': 8500}]})
ok(next(o for o in money(maria)['owed'] if o.get('plan_id') == p10)['due_cents'] == 8500, 'instalment: half paid, half still owed')

# invoice number + PDF, from the certified invoicing software
pdf = base64.b64encode(b'%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF').decode()
ok(call('POST', '/finance/files', recep, {'kind': 'invoice', 'filename': 'x.exe', 'data': base64.b64encode(b'MZ\x90\x00 not a pdf').decode()}).status_code == 400, 'only PDFs or photos are accepted')
ok(call('POST', '/finance/files', recep, {'kind': 'expense', 'filename': 'r.pdf', 'data': pdf}).status_code == 403, 'reception cannot upload expense receipts')
r = call('POST', '/finance/files', recep, {'kind': 'invoice', 'filename': 'FR 2026-14.pdf', 'data': 'data:application/pdf;base64,' + pdf})
ok(r.status_code == 200 and r.json()['mime'] == 'application/pdf', 'reception uploads the invoice PDF'); inv_file = r.json()['id']
ok(call('PATCH', f'/payments/{pay_pack}', recep, {'invoice_number': 'FR 2026/14', 'invoice_file_id': inv_file}).status_code == 200, 'invoice number and PDF linked to the payment')
p = next(x for x in money(maria)['payments'] if x['id'] == pay_pack)
ok(p['invoice_number'] == 'FR 2026/14' and p['invoiced_at'] and p['invoice_file_id'] == inv_file, 'payment shows as invoiced')
r = requests.get(B + f'/finance/files/{inv_file}/raw', headers={**H, 'Authorization': 'Bearer ' + recep})
ok(r.status_code == 200 and r.content.startswith(b'%PDF') and 'attachment' in r.headers.get('content-disposition', ''), 'invoice PDF can be downloaded')

# voiding
ok(call('POST', f'/payments/{pay_ass}/void', owner, {'reason': ''}).status_code == 400, 'voiding needs a reason')
ok(call('POST', f'/payments/{walkin}/void', ther, {'reason': 'x'}).status_code == 403, 'others can’t void someone else’s payment')
ok(call('POST', f'/payments/{walkin}/void', recep, {'reason': 'Engano'}).status_code == 200, 'whoever recorded it can void it the same day')
ok(call('POST', f'/payments/{pay_ass}/void', owner, {'reason': 'Valor errado'}).status_code == 200, 'owner voids a payment')
ok(call('POST', f'/payments/{pay_ass}/void', owner, {'reason': 'again'}).status_code == 409, 'can’t void twice')
ok(call('PATCH', f'/payments/{pay_ass}', owner, {'invoice_number': 'X'}).status_code == 409, 'a voided payment can’t be changed')
ok(any(o.get('appointment_id') == a2 for o in money(maria)['owed']), 'voided payment → the assessment is owed again')
ok(next(x for x in money(maria)['payments'] if x['id'] == pay_ass)['void_reason'] == 'Valor errado', 'voided payment stays in the record with its reason')
psql = os.environ.get('PSQL_URL')
if psql:
    def sql(q): return subprocess.run(['psql', psql, '-v', 'ON_ERROR_STOP=1', '-qtAc', q], capture_output=True, text=True)
    ok(sql(f"DELETE FROM payments WHERE id = '{pay_pack}'").returncode != 0, 'database refuses to delete a payment')
    ok(sql(f"UPDATE payments SET amount_cents = 1 WHERE id = '{pay_pack}'").returncode != 0, 'database refuses to change an amount')
    ok(sql(f"UPDATE payments SET note = 'ok' WHERE id = '{pay_pack}'").returncode == 0, 'database allows a note')

# ── monthly plan ──
r = call('POST', f'/patients/{joao}/plans', recep, {'product_id': prods['cc4']['id']}); cc4 = r.json()['id']
mo = [o for o in money(joao)['owed'] if o.get('plan_id') == cc4]
ok(len(mo) == 1 and mo[0]['type'] == 'month' and mo[0]['month'] == this_month + '-01' and mo[0]['due_cents'] == 7200, 'monthly plan: this month owed (€72)')
r = call('POST', '/payments', recep, {'patient_id': joao, 'method': 'mbway', 'items': [{'plan_id': cc4, 'amount_cents': 7200}]})
ok(r.status_code == 200 and not any(o.get('plan_id') == cc4 for o in money(joao)['owed']), 'paying the month clears it')
ok(this_month[:4] in next(x for x in money(joao)['payments'] if x['plan_id'] == cc4)['description'], 'month named in the description')
r = call('POST', f'/patients/{ines}/plans', recep, {'product_id': prods['cc8']['id']}); cc8 = r.json()['id']
ok(call('PATCH', f'/plans/{cc8}', recep, {'status': 'paused'}).status_code == 200 and not any(o.get('plan_id') == cc8 for o in money(ines)['owed']), 'pausing skips this month')
ok(call('PATCH', f'/plans/{cc8}', recep, {'status': 'active'}).status_code == 200 and any(o.get('plan_id') == cc8 for o in money(ines)['owed']), 'resuming charges it again')
ok(call('PATCH', f'/plans/{m_pack}', recep, {'status': 'paused'}).status_code == 400, 'packages can’t be paused')
ok(call('PATCH', f'/plans/{m_pack}', recep, {'status': 'cancelled'}).status_code == 409, 'a used/paid package can’t be cancelled')
ok(call('PATCH', f'/plans/{cc8}', recep, {'status': 'cancelled'}).status_code == 200 and not any(o.get('plan_id') == cc8 for o in money(ines)['owed']), 'an unused plan sold by mistake can be cancelled')
ok(call('PATCH', f'/plans/{p10}', recep, {'expires_on': '2030-01-01'}).status_code == 403, 'only the owner changes an expiry date')
# monthly credits apply to sessions
a7 = appt(joao, days_ago=0, hour=7); done(a7)
ok(next(p for p in money(joao)['plans'] if p['id'] == cc4)['used_month'] == 1, 'session this month counted against the monthly plan')

# ── classes ──
s = datetime.now().replace(hour=19, minute=0, second=0, microsecond=0).astimezone() - timedelta(days=1)
cls = call('POST', '/appointments', recep, {'kind': 'class', 'starts_at': s.isoformat(), 'ends_at': (s + timedelta(hours=1)).isoformat(), 'therapist_id': ther_id, 'capacity': 6, 'title': 'Pilates', 'allow_overlap': True}).json()['ids'][0]
call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': ines}); call('POST', f'/appointments/{cls}/attendees', recep, {'patient_id': joao})
call('POST', f'/patients/{joao}/plans', recep, {'product_id': prods['class10']['id'], 'starts_on': (today - timedelta(days=7)).isoformat()})
call('PATCH', f'/appointments/{cls}/attendees/{ines}', recep, {'attended': True}); call('PATCH', f'/appointments/{cls}/attendees/{joao}', recep, {'attended': True})
ok(any(o.get('appointment_id') == cls and o['due_cents'] == 1000 for o in money(ines)['owed']), 'class drop-in without a card → €10 owed')
ok(not any(o.get('appointment_id') == cls for o in money(joao)['owed']) and any(p['applies_to'] == 'class' and p['remaining'] == 9 for p in money(joao)['plans']), 'class card credit used')
call('PATCH', f'/appointments/{cls}/attendees/{ines}', recep, {'attended': False})
ok(not any(o.get('appointment_id') == cls for o in money(ines)['owed']), 'not attended → nothing owed')

# ── owner-only Finance ──
for path in ['/finance/overview', '/finance/owed', '/payments', '/expenses', '/finance/cash', '/finance/salaries', '/finance/months']:
    ok(call('GET', path, recep).status_code == 403 and call('GET', path, ther).status_code == 403, f'{path}: owner only')
ok(call('POST', '/finance/export', recep, {'from': this_month, 'to': this_month}).status_code == 403, 'export: owner only')
ow = call('GET', '/finance/owed', owner).json()
ok(len({o['patient_id'] for o in ow}) >= 2 and all(o['due_cents'] > 0 for o in ow), 'owner sees everything owed')
al = call('GET', '/alerts', owner).json(); alr = call('GET', '/alerts', recep).json()
ok(al['money'] and al['money']['owed_cents'] == sum(o['due_cents'] for o in ow) and al['money']['today_cents'] > 0, 'Today: owner gets money owed and today’s takings')
ok(alr['money'] is None and isinstance(alr['plans_low'], list), 'Today: reception gets no money totals')

# ── expenses ──
png = base64.b64encode(bytes.fromhex('89504e470d0a1a0a') + b'\x00' * 32).decode()
r = call('POST', '/finance/files', owner, {'kind': 'expense', 'filename': 'talão.png', 'data': png, 'day': today.isoformat()}); rec_file = r.json()['id']
ok(r.status_code == 200, 'owner uploads a receipt photo')
r = call('POST', '/expenses', owner, {'spent_on': today.isoformat(), 'supplier': 'Fisiomed Lda.', 'category': 'supplies', 'amount_cents': 8640, 'vat_cents': 1615, 'method': 'card', 'file_id': rec_file})
ok(r.status_code == 200, 'owner records an expense with the receipt'); exp1 = r.json()['id']
ok(call('POST', '/expenses', owner, {'spent_on': today.isoformat(), 'category': 'nope', 'amount_cents': 100}).status_code == 400, 'unknown category refused')
ok(call('POST', '/expenses', owner, {'spent_on': today.isoformat(), 'category': 'rent', 'amount_cents': -5}).status_code == 400, 'negative amount refused')
call('POST', '/expenses', owner, {'spent_on': today.isoformat(), 'supplier': 'Senhorio', 'category': 'rent', 'amount_cents': 85000, 'method': 'transfer'})
ex = call('GET', '/expenses', owner).json()
ok(len(ex) == 2 and any(x['filename'] for x in ex), 'expenses listed with their receipt')
ok(call('GET', f'/finance/files/{rec_file}/raw', recep).status_code == 403, 'reception cannot open expense receipts')

# ── cash close ──
c = call('GET', '/finance/cash', owner).json()
ok(c['expected'].get('cash') == 8000 and c['expected'].get('mbway') == 7200 and 'stripe' not in c['expected'], 'cash close: expected by method (voided left out)')
r = call('POST', '/finance/cash', owner, {'day': c['day'], 'counted': {'cash': 7500, 'mbway': 7200, 'transfer': 8500}, 'note': 'faltam 5 €'})
ok(r.status_code == 200 and r.json()['difference_cents'] == -500, 'day closed with a €5 shortfall')
ok(call('GET', '/finance/cash', owner).json()['close']['note'] == 'faltam 5 €', 'close saved')

# ── salaries ──
ok(call('PUT', f'/staff/{ther_id}/pay', owner, {'model': 'commission', 'fixed_cents': 0}).status_code == 400, 'commission not available yet')
ok(call('PUT', f'/staff/{ther_id}/pay', owner, {'model': 'fixed', 'fixed_cents': 145000}).status_code == 200, 'fixed pay set')
r = call('POST', '/finance/salaries', owner, {'staff_id': ther_id, 'month': this_month, 'gross_cents': 145000})
ok(r.status_code == 200, 'salary recorded'); sal = r.json()['id']
sl = call('GET', '/finance/salaries', owner).json()
ok(next(x for x in sl['staff'] if x['staff_id'] == ther_id)['fixed_cents'] == 145000 and len(next(x for x in sl['staff'] if x['staff_id'] == ther_id)['paid']) == 1, 'salaries list shows pay and payment')

# ── overview ──
ov = call('GET', '/finance/overview', owner).json()
rev = 8000 + 8500 + 7200
ok(ov['revenue_cents'] == rev and ov['expenses_cents'] == 8640 + 85000 and ov['salaries_cents'] == 145000, 'overview totals (voided payments left out)')
ok(ov['employer_ss_cents'] == 34438, 'employer Social Security at the clinic rate (23,75 % of 1450,00 € = 344,38 €; audit phase C)')
ok(ov['result_cents'] == rev - 8640 - 85000 - 145000 - 34438 and len(ov['series']) == 6 and ov['uninvoiced'] == 2, 'result (with Social Security), 6-month series and uninvoiced count')

# ── month close (last month) ──
r = call('POST', '/payments', recep, {'patient_id': maria, 'method': 'cash', 'received_on': last_month_day.isoformat(), 'items': [{'amount_cents': 2000, 'description': 'Sessão (mês passado)'}]})
old_pay = r.json()['ids'][0]
call('POST', '/expenses', owner, {'spent_on': last_month_day.isoformat(), 'supplier': 'EDP', 'category': 'utilities', 'amount_cents': 6412})
ok(call('POST', f'/finance/months/{this_month}/close', owner).status_code == 400, 'the current month can’t be closed yet')
r = call('POST', f'/finance/months/{last_month}/close', owner)
ok(r.status_code == 200 and r.json()['totals']['revenue_cents'] == 2000, 'owner closes last month')
ok(call('POST', f'/finance/months/{last_month}/close', owner).status_code == 409, 'already closed')
ok(call('POST', '/payments', recep, {'method': 'cash', 'received_on': last_month_day.isoformat(), 'items': [{'amount_cents': 100, 'description': 'x'}]}).status_code == 409, 'no new payments in a closed month')
ok(call('POST', f'/payments/{old_pay}/void', owner, {'reason': 'x'}).status_code == 409, 'no voiding in a closed month')
ok(call('POST', '/expenses', owner, {'spent_on': last_month_day.isoformat(), 'category': 'other', 'amount_cents': 100}).status_code == 409, 'no new expenses in a closed month')
ok(call('POST', '/finance/salaries', owner, {'staff_id': ther_id, 'month': last_month, 'gross_cents': 100}).status_code == 409, 'no salaries in a closed month')
ok(call('POST', '/finance/cash', owner, {'day': last_month_day.isoformat(), 'counted': {}}).status_code == 409, 'no cash close in a closed month')
ok(call('GET', '/finance/months', owner).json()[0]['month'] == last_month, 'closed months listed')

# ── accountant export ──
r = call('POST', '/finance/export', owner, {'from': last_month, 'to': this_month})
ok(r.status_code == 200 and r.json()['provisional'] is True and r.json()['filename'] == f'Exportacao_{last_month}_a_{this_month}.zip', 'export built (provisional: this month is open)')
exp = r.json()
raw = requests.get(B + f"/finance/files/{exp['id']}/raw", headers={**H, 'Authorization': 'Bearer ' + owner})
ok(raw.status_code == 200 and raw.headers['content-type'] == 'application/zip', 'zip downloads')
z = zipfile.ZipFile(io.BytesIO(raw.content)); names = z.namelist(); root = exp['filename'][:-4]
ok(z.testzip() is None, 'zip is valid (CRC checks pass)')
need = ['01 Entradas/entradas.csv', '01 Entradas/por-metodo.csv', '02 Saídas/saidas.csv', '03 Salários/salarios.csv', '04 Resumo/resumo.csv', '04 Resumo/resumo.pdf', 'LEIA-ME.txt']
ok(all(f'{root}/{n}' in names for n in need), 'folders and files as specified (01–04 + LEIA-ME)')
ok(any(n.startswith(f'{root}/01 Entradas/Faturas/') and n.endswith('.pdf') for n in names), 'invoice PDFs included')
ok(any(n.startswith(f'{root}/02 Saídas/Consumíveis clínicos/') for n in names), 'receipts sorted by category')
ent = z.read(f'{root}/01 Entradas/entradas.csv').decode('utf-8')
ok(ent.startswith('﻿') and ';' in ent and 'FR 2026/14' in ent and '80,00' in ent and 'Valor errado' in ent, 'entradas.csv: BOM, semicolons, decimal comma, invoice no., voided marked')
ok(z.read(f'{root}/04 Resumo/resumo.pdf').startswith(b'%PDF-1.4'), 'resumo.pdf is a PDF')
ok('provisórios' in z.read(f'{root}/LEIA-ME.txt').decode('utf-8'), 'LEIA-ME warns about open months')
ok(os.path.exists(os.path.join(os.environ.get('ARCHIVE_DIR', '/tmp/claude-0/arch'), 'Financas/Exportacoes', exp['filename'])) or 'ARCHIVE_DIR' not in os.environ, 'zip also saved in the archive folder')

# reopen
ok(call('POST', f'/finance/months/{last_month}/reopen', owner).status_code == 200, 'owner reopens the month')
ok(call('POST', f'/payments/{old_pay}/void', owner, {'reason': 'teste'}).status_code == 200, '…and can fix it again')

# ── access log ──
lg = call('GET', f'/access-log?subject_type=patient&subject_id={maria}&limit=200', owner).json()
acts = {x['action'] + ':' + x['detail'] for x in lg}
ok('patient.viewed:money' in acts and any(a.startswith('payment.recorded') for a in acts) and any(a.startswith('payment.voided') for a in acts) and any(a.startswith('plan.sold') for a in acts),
   'access log: money reads, payments, voids and sales are recorded')
lg = call('GET', '/access-log?limit=500', owner).json(); acts = {x['action'] for x in lg}
ok({'month.closed', 'month.reopened', 'finance.exported', 'cash.closed', 'salary.paid', 'expense.created', 'product.updated'} <= acts, 'access log: Finance actions recorded')

print(f'\n{"ALL PASSED" if fails == 0 else str(fails) + " FAILED"}')
sys.exit(1 if fails else 0)
