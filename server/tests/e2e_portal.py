# PhysioPulse plan, phase 4b (23 Nov 2026): patient accounts on the website (/conta).
# Needs, on a fresh database:
#   the staff server on BASE (default http://127.0.0.1:8080) with SECRET_KEY=<64 hex>, PORTAL_DB_PASSWORD=<16+ letters/digits>, ARCHIVE_DIR
#   the website on SITE (default http://127.0.0.1:8090) with DATABASE_URL=postgres://physiopulse_portal:<that password>@…/physiopulse,
#       SERVER_URL=<BASE>, COOKIE_INSECURE=1
#   PSQL_URL = the main database login, PORTAL_URL = the website's login
#   python3 server/tests/e2e_portal.py
import os, re, subprocess, secrets, datetime, base64, time
import requests, pyotp
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
SITE = os.environ.get('SITE', 'http://127.0.0.1:8090')
PSQL, PORTAL = os.environ['PSQL_URL'], os.environ['PORTAL_URL']
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
DEV = secrets.token_hex(32)
def req(method, p, j=None, tok=None, lang=None):
    h = {'X-Device': 'PC', 'X-Device-Key': DEV}
    if tok: h['Authorization'] = 'Bearer ' + tok
    if lang: h['Accept-Language'] = lang
    return requests.request(method, B + p, json=j, headers=h)
def sql(q, url=PSQL):
    r = subprocess.run(['psql', '-v', 'ON_ERROR_STOP=1', '-tAc', q, url], capture_output=True, text=True)  # options first (psql on Windows)
    return (r.stdout.strip(), r.stderr.strip())
def iso(days, h=10, m=0):
    d = datetime.datetime.now(datetime.timezone.utc).replace(hour=h, minute=m, second=0, microsecond=0) + datetime.timedelta(days=days)
    return d.isoformat().replace('+00:00', 'Z')
def ip(n): return {'X-Forwarded-For': f'198.51.100.{n}'}

class Browser:
    """A visitor with a cookie jar, like a browser without scripts."""
    def __init__(self, n): self.s = requests.Session(); self.h = ip(n)
    def get(self, p, **kw): return self.s.get(SITE + p, allow_redirects=False, headers=self.h, **kw)
    def post(self, p, data, **kw): return self.s.post(SITE + p, data=data, allow_redirects=False, headers=self.h, **kw)
    def token(self, p='/conta'):
        m = re.search(r'name="c" value="([0-9a-f]+)"', self.get(p).text); return m.group(1) if m else ''

PW = 'correct horse battery'
r = req('POST', '/setup', {'clinic_name': 'PhysioPulse', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW})
en = r.json(); r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': pyotp.TOTP(en['secret']).now(), 'trust_device': True}); owner = r.json()['token']
ok(bool(owner), 'owner signed in')
req('PUT', '/clinic', {'name': 'PhysioPulse', 'nif': '503504564', 'phone': '912000000', 'email': 'geral@clinica.pt'}, owner)
r = req('PUT', '/website', {'published': True}, owner); ok(r.status_code == 200 and r.json()['site'].get('accounts_open') is False, 'site published; the patient area starts closed')

def staff(role, email, name):
    code = req('POST', '/invites', {'role': role, 'email': email}, owner).json()['code']
    e = req('POST', '/auth/signup', {'invite': code, 'name': name, 'email': email, 'password': 'another long password'}).json()
    t = req('POST', '/auth/verify', {'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json().get('token')
    sid, _ = sql(f"SELECT id FROM staff WHERE email = '{email}'")
    return t, sid
rita, _ = staff('reception', 'rita@clinica.pt', 'Rita Costa')
ana, ana_id = staff('therapist', 'ana@clinica.pt', 'Ana Rocha')
rui, rui_id = staff('therapist', 'rui@clinica.pt', 'Rui Lopes')
owner_id, _ = sql("SELECT id FROM staff WHERE role = 'owner'")

maria = req('POST', '/patients', {'name': 'Maria Silva', 'birth_date': '1980-05-04', 'phone': '912345678', 'therapist_id': ana_id}, owner).json()['id']
tiago = req('POST', '/patients', {'name': 'Tiago Silva', 'birth_date': '2016-02-01', 'therapist_id': ana_id}, owner).json()['id']
other = req('POST', '/patients', {'name': 'Outra Pessoa', 'birth_date': '1970-01-01'}, owner).json()['id']
ok(all([maria, tiago, other]), 'three patients (one under 16)')

# ── Closed: nothing works on the website yet ──
b = Browser(1)
r = b.get('/conta'); ok(r.status_code == 200 and 'Em breve' in r.text and 'Set-Cookie' not in r.headers, 'closed: "em breve", no cookie')
r = b.post('/conta/entrar', {'email': 'x@y.pt', 'password': 'x'}); ok('Em breve' in r.text and 'set-cookie' not in {k.lower() for k in r.headers}, 'closed: no sign-in')
_, err = sql("SELECT * FROM portal.account_by_email('x@y.pt')", PORTAL); ok('closed' in err, 'closed: the database refuses too')

# ── Staff: invites, under-16s, who may do what ──
r = req('POST', f'/patients/{maria}/portal/invite', {'relation': 'self'}, rita)
ok(r.status_code == 200 and re.fullmatch(r'[A-Z2-9]{4}-[A-Z2-9]{4}-[A-Z2-9]{4}', r.json()['code']), 'reception makes an invite code (XXXX-XXXX-XXXX)')
code_maria = r.json()['code']
out, _ = sql(f"SELECT count(*) FROM patient_codes WHERE code_hash = '{code_maria}' OR code_hash LIKE '%{code_maria[:4]}%'"); ok(out == '0', 'only a hash of the code is stored')
r = req('POST', f'/patients/{tiago}/portal/invite', {'relation': 'self'}, ana); ok(r.status_code == 400, 'an under-16 gets no login of their own')
r = req('POST', f'/patients/{tiago}/portal/invite', {'relation': 'guardian'}, ana); ok(r.status_code == 200, 'a guardian invite for the child'); code_tiago = r.json()['code']
r = req('POST', f'/patients/{other}/portal/invite', {}, rui); ok(r.status_code == 200, 'any therapist can make an invite'); code_other = r.json()['code']
g = req('GET', f'/patients/{maria}/portal', tok=rita).json()
ok(len(g['invites']) == 1 and g['accounts'] == [] and g['open'] is False and g['can_manage'] is True, 'the file shows the invite waiting')
ok(req('GET', f'/patients/{maria}/portal', tok=rui).json()['can_manage'] is False, "another therapist can't manage Maria's access")
out, _ = sql("SELECT count(*) FROM access_log WHERE action = 'portal.invite_created'"); ok(out == '3', 'invites are in the access log')

# ── Open the area ──
ok(req('PUT', '/website', {'accounts_open': True}, rita).status_code == 403, 'only the owner opens the area')
ok(req('PUT', '/website', {'accounts_open': True}, owner).status_code == 200, 'owner opens the patient area')
r = b.get('/conta'); ok('Entrar' in r.text and 'name="password"' in r.text, 'open: the sign-in form')
r = b.get('/conta/convite?codigo=' + code_maria); ok(code_maria in r.text, 'the invite link fills in the code')

# Creating the account
bad = b.post('/conta/convite', {'code': 'AAAA-BBBB-CCCC', 'email': 'maria@example.com', 'password': 'a good password', 'password2': 'a good password', 'privacy': 'on'})
ok(bad.status_code == 422 and 'não é válido' in bad.text, 'a wrong code is refused')
short = b.post('/conta/convite', {'code': code_maria, 'email': 'maria@example.com', 'password': 'short', 'password2': 'short', 'privacy': 'on'})
ok(short.status_code == 422 and '10 caracteres' in short.text, 'passwords need 10 characters')
r = b.post('/conta/convite', {'code': code_maria.lower().replace('-', ' '), 'email': 'Maria@Example.com', 'password': 'a good password', 'password2': 'a good password', 'privacy': 'on'})
ok(r.status_code == 200 and 'Conta pronta' in r.text, 'account made (code typed in lower case with spaces)')
out, _ = sql("SELECT email, password_hash LIKE '$argon2id$%', totp_secret IS NULL FROM patient_accounts"); ok(out == 'maria@example.com|t|t', 'email kept in lower case, password as Argon2id, two-step off')
r = b.post('/conta/convite', {'code': code_maria, 'email': 'other@example.com', 'password': 'a good password', 'password2': 'a good password', 'privacy': 'on'})
ok(r.status_code == 422, 'a code works once')
g = req('GET', f'/patients/{maria}/portal', tok=rita).json(); ok(len(g['accounts']) == 1 and g['invites'] == [], 'the file shows the account')
maria_acc = g['accounts'][0]['id']

# ── Signing in ──
r = b.post('/conta/entrar', {'email': 'maria@example.com', 'password': 'wrong password'}); ok(r.status_code == 422 and 'não estão certos' in r.text, 'wrong password')
r2 = b.post('/conta/entrar', {'email': 'nobody@example.com', 'password': 'wrong password'}); ok(r2.status_code == 422 and 'não estão certos' in r2.text, 'unknown email: the same answer')
r = b.post('/conta/entrar', {'email': 'maria@example.com', 'password': 'a good password'})
ck = r.headers.get('set-cookie', '')
ok(r.status_code in (302, 303) and 'pp_conta=' in ck and 'HttpOnly' in ck and 'SameSite=Strict' in ck, 'signed in: HttpOnly, SameSite=Strict cookie')
out, _ = sql("SELECT count(*) FROM patient_sessions WHERE length(token_hash) = 64"); ok(out == '1', 'only a hash of the session is stored')
for p in ['/', '/servicos', '/marcar']:
    ok('set-cookie' not in {k.lower() for k in b.get(p).headers}, f'no cookie on public page {p}')

# Data for Maria (and Tiago, and someone else)
a_next = req('POST', '/appointments', {'kind': 'session', 'starts_at': iso(5), 'ends_at': iso(5, 10, 45), 'therapist_id': ana_id, 'patient_id': maria}, ana).json()['ids'][0]
a_soon = req('POST', '/appointments', {'kind': 'session', 'starts_at': iso(0, 23, 0) if datetime.datetime.now(datetime.timezone.utc).hour < 20 else iso(1, 8), 'ends_at': iso(0, 23, 45) if datetime.datetime.now(datetime.timezone.utc).hour < 20 else iso(1, 8, 45), 'therapist_id': ana_id, 'patient_id': maria}, ana).json()['ids'][0]
a_past = req('POST', '/appointments', {'kind': 'session', 'starts_at': iso(-3), 'ends_at': iso(-3, 10, 45), 'therapist_id': ana_id, 'patient_id': maria}, ana).json()['ids'][0]
req('PATCH', f'/appointments/{a_past}', {'status': 'done'}, ana)
a_other = req('POST', '/appointments', {'kind': 'session', 'starts_at': iso(6), 'ends_at': iso(6, 10, 45), 'therapist_id': rui_id, 'patient_id': other}, rui).json()['ids'][0]
sql(f"INSERT INTO notes (id, patient_id, template, blocks, text, created_by) VALUES (gen_random_uuid(), '{maria}', 'free', '[]', 'SEGREDO CLÍNICO', '{ana_id}')")
pdf = base64.b64encode(b'%PDF-1.4\n% invoice\n%%EOF').decode()
f = req('POST', '/finance/files', {'kind': 'invoice', 'filename': 'FR 2026-1.pdf', 'data': pdf}, rita).json()['id']
pr = req('POST', '/payments', {'patient_id': maria, 'method': 'cash', 'items': [{'amount_cents': 2000, 'description': 'Sessão'}], 'invoice_number': 'FR 2026/1'}, rita)
ok(pr.status_code == 200, 'a payment for Maria'); pay = (pr.json().get('ids') or [pr.json().get('id')])[0]
req('PATCH', f'/payments/{pay}', {'invoice_file_id': f}, rita)
po = req('POST', '/payments', {'patient_id': other, 'method': 'cash', 'items': [{'amount_cents': 2000, 'description': 'Sessão'}]}, rita).json()
pay_other = (po.get('ids') or [po.get('id')])[0]
sql(f"UPDATE appointments SET charge_cents = 2500 WHERE id = '{a_past}'")

r = b.get('/conta'); h = r.text
ok(r.status_code == 200 and 'Próximas sessões' in h and 'maria@example.com' in h, 'the patient area opens')
ok('SEGREDO' not in h, 'no clinical notes in the patient area')
ok('Ana' in h and 'Rui' not in h, "only Maria's sessions (her therapist's first name)")
ok('25,00' in h, 'what is owed (from the clinic’s own calculation)')
ok('Fatura (PDF)' in h and '20,00' in h, 'the payment with its invoice')
ok(h.count('Cancelar…') == 1, 'cancel offered only for the session more than 24 h away')

# ── The internal API: website key + session only ──
I = B + '/internal/portal'
sess = b.s.cookies.get('pp_conta')
ok(requests.get(I + '/me', headers={'x-portal-session': sess}).status_code == 404, 'internal API: refused without the website key')
ok(requests.get(I + '/me', headers={'x-portal-session': sess, 'x-portal-key': '00' * 32}).status_code == 404, 'internal API: refused with a wrong key')
ok(requests.get(I + '/me', headers={'Authorization': 'Bearer ' + owner, 'X-Device-Key': DEV}).status_code == 404, 'internal API: a staff login is not enough')

# Invoices: hers yes, someone else's no
r = b.get(f'/conta/fatura/{pay}'); ok(r.status_code == 200 and r.content.startswith(b'%PDF') and 'attachment' in r.headers.get('content-disposition', ''), 'her invoice PDF downloads')
r = b.get(f'/conta/fatura/{pay_other}'); ok(r.status_code == 404, "someone else's invoice: not found")

# ── Cancel and ask for another time ──
c = b.token()
r = b.post('/conta/cancelar', {'c': 'x' * 32, 'id': a_next}); ok(r.status_code in (302, 303), 'a form without the session token is ignored')
out, _ = sql(f"SELECT status FROM appointments WHERE id = '{a_next}'"); ok(out == 'booked', '…and nothing changed')
r = b.post('/conta/cancelar', {'c': c, 'id': a_soon}); ok('menos de 24 horas' in r.text, 'less than 24 h before: call the clinic')
r = b.post('/conta/cancelar', {'c': c, 'id': a_other}); ok('cancelada' not in r.text.lower() or 'Não foi possível' in r.text, "can't cancel someone else's session")
out, _ = sql(f"SELECT status FROM appointments WHERE id = '{a_other}'"); ok(out == 'booked', "…someone else's session stays booked")
r = b.post('/conta/pedido', {'c': c, 'patient': maria, 'appointment': a_soon, 'day': (datetime.date.today() + datetime.timedelta(days=9)).isoformat(), 'part': 'afternoon', 'message': 'Terças à tarde'})
ok('Pedido enviado' in r.text, 'asks for another time')
r = b.post('/conta/pedido', {'c': c, 'patient': maria, 'appointment': a_soon, 'day': (datetime.date.today() + datetime.timedelta(days=9)).isoformat(), 'part': 'any'})
ok('Já pediu' in r.text, 'only one move request per session')
r = b.post('/conta/pedido', {'c': c, 'patient': other, 'service': 'session', 'day': (datetime.date.today() + datetime.timedelta(days=9)).isoformat(), 'part': 'any'})
ok('Pedido enviado' not in r.text, "can't ask for someone not on the account")
r = b.post('/conta/pedido', {'c': c, 'patient': maria, 'service': 'session', 'day': datetime.date.today().isoformat(), 'part': 'any'})
ok('a partir de amanhã' in r.text, 'not for today')
r = b.post('/conta/pedido', {'c': c, 'patient': maria, 'service': 'assessment', 'day': (datetime.date.today() + datetime.timedelta(days=4)).isoformat(), 'part': 'morning'})
ok('Pedido enviado' in r.text, 'asks for a new appointment')
r = b.post('/conta/cancelar', {'c': c, 'id': a_next}); ok('Sessão cancelada' in r.text, 'cancels a session 5 days away')
out, _ = sql(f"SELECT status FROM appointments WHERE id = '{a_next}'"); ok(out == 'cancelled', '…it is cancelled')

reqs = req('GET', '/booking-requests', tok=rita).json()
kinds = sorted((x['kind'], x['source']) for x in reqs if x.get('patient_id') == maria)
ok(kinds == [('book', 'portal'), ('cancelled', 'portal'), ('move', 'portal')], 'reception sees the cancellation, the move and the new request')
mv = next(x for x in reqs if x.get('kind') == 'move'); cn = next(x for x in reqs if x.get('kind') == 'cancelled')
ok(mv.get('about_starts_at') and mv['day_part'] == 'afternoon', 'the move request says which session and when')
ok(len(req('GET', '/booking-requests', tok=ana).json()) == len(reqs), 'the therapist sees them too')
ok(req('POST', f"/booking-requests/{cn['id']}/book", {'starts_at': iso(8), 'therapist_id': ana_id}, rita).status_code == 409, 'a cancellation notice is not something to book')
r = req('POST', f"/booking-requests/{mv['id']}/book", {'starts_at': iso(9, 14), 'therapist_id': ana_id}, rita)
ok(r.status_code == 200, 'reception books the new time')
out, _ = sql(f"SELECT status FROM appointments WHERE id = '{a_soon}'"); ok(out == 'cancelled', '…and the old session is cancelled')
ok(req('POST', f"/booking-requests/{mv['id']}/reopen", None, rita).status_code == 200, 'Undo')
out, _ = sql(f"SELECT status FROM appointments WHERE id = '{a_soon}'"); ok(out == 'booked', '…puts the old session back')
ok(req('POST', f"/booking-requests/{cn['id']}/decline", None, rita).status_code == 200, '"Seen" clears the cancellation notice')
out, _ = sql("SELECT count(*) FROM access_log WHERE device = 'site' AND action IN ('appointment.cancelled', 'booking_request.received', 'portal.invoice_downloaded')"); ok(int(out) >= 4, 'cancel, requests and invoice downloads are in the access log')

# ── Guardians: one account, two people ──
b2 = Browser(2)
r = b2.post('/conta/convite', {'code': code_tiago, 'email': 'maria@example.com', 'password': 'wrong password!', 'password2': 'wrong password!', 'privacy': 'on'})
ok(r.status_code == 422 and 'não corresponde' in r.text, 'an existing email needs that account’s password')
r = b2.post('/conta/convite', {'code': code_tiago, 'email': 'maria@example.com', 'password': 'a good password', 'password2': '', 'privacy': 'on'})
ok('Acrescentado' in r.text, "Tiago's guardian invite added to Maria's account")
h = b.get('/conta').text; ok('Tiago' in h or 'Pedir marcação' in h, 'the account now covers both')
r = b.get('/conta/pedido'); ok('Tiago Silva' in r.text and 'Maria Silva' in r.text, 'the request form asks for whom')
g = req('GET', f'/patients/{tiago}/portal', tok=ana).json(); ok(g['accounts'][0]['relation'] == 'guardian' and g['minor'] is True, "Tiago's file shows the guardian account")

# ── Two-step codes ──
c = b.token('/conta/seguranca')
r = b.post('/conta/seguranca', {'c': c, 'action': 'totp_start'}); m = re.search(r'<code>([A-Z2-7]+)</code>', r.text); sealed = re.search(r'name="pending" value="([^"]+)"', r.text)
ok(m and sealed and '<svg' in r.text, 'two-step set-up shows a QR code and the key')
r = b.post('/conta/seguranca', {'c': c, 'action': 'totp_on', 'pending': sealed.group(1), 'code': '000000', 'current': 'a good password'}); ok('não está certo' in r.text, 'wrong code: not turned on')
r = b.post('/conta/seguranca', {'c': c, 'action': 'totp_on', 'pending': sealed.group(1), 'code': pyotp.TOTP(m.group(1)).now(), 'current': 'wrong'}); ok('palavra-passe atual' in r.text, 'needs the current password')
r = b.post('/conta/seguranca', {'c': c, 'action': 'totp_on', 'pending': sealed.group(1), 'code': pyotp.TOTP(m.group(1)).now(), 'current': 'a good password'}); ok('ligados' in r.text, 'two-step turned on')
out, _ = sql("SELECT totp_secret IS NOT NULL AND totp_secret NOT LIKE '%" + m.group(1) + "%' FROM patient_accounts"); ok(out == 't', 'the secret is stored encrypted')
b3 = Browser(3)
b3.post('/conta/entrar', {'email': 'maria@example.com', 'password': 'a good password'})
r = b3.get('/conta'); ok('Código de dois passos' in r.text and 'Próximas' not in r.text, 'after the password, the code is asked')
ok(requests.get(I + '/me', headers={'x-portal-session': b3.s.cookies.get('pp_conta'), 'x-portal-key': 'x'}).status_code == 404, 'a half-signed-in session gets nothing')
r = b3.post('/conta/codigo', {'code': '123456'}); ok(r.status_code == 422, 'wrong code refused')
b3.post('/conta/codigo', {'code': pyotp.TOTP(m.group(1)).now()}); ok('Próximas sessões' in b3.get('/conta').text, 'right code: in')

# ── Forgotten password: reception, the patient's therapist or the owner ──
ok(req('POST', f'/patients/{maria}/portal/reset', {'account_id': maria_acc}, rui).status_code == 400, "another therapist can't make a reset code")
ok(req('POST', f'/patients/{other}/portal/reset', {'account_id': maria_acc}, owner).status_code == 400, 'a reset code only for an account linked to that patient')
r = req('POST', f'/patients/{maria}/portal/reset', {'account_id': maria_acc}, ana); ok(r.status_code == 200, "Maria's therapist makes a reset code")
r = req('POST', f'/patients/{maria}/portal/reset', {'account_id': maria_acc}, rita); ok(r.status_code == 200, 'reception too (the older code stops working)'); rc = r.json()['code']
r = b2.post('/conta/repor', {'code': rc, 'password': 'a brand new password', 'password2': 'a brand new password'}); ok('Palavra-passe mudada' in r.text, 'new password chosen with the code')
out, _ = sql("SELECT totp_secret IS NULL, (SELECT count(*) FROM patient_sessions) FROM patient_accounts"); ok(out == 't|0', 'reset turns two-step off and ends every session')
ok('Entrar' in b.get('/conta').text, 'the old session is gone')
r = b2.post('/conta/repor', {'code': rc, 'password': 'another new password', 'password2': 'another new password'}); ok(r.status_code == 422, 'a reset code works once')
b.post('/conta/entrar', {'email': 'maria@example.com', 'password': 'a brand new password'}); ok('Próximas sessões' in b.get('/conta').text, 'signs in with the new password')

# ── Sign-in rate limit ──
b4 = Browser(4)
codes = [b4.post('/conta/entrar', {'email': 'someone@example.com', 'password': 'x' * 12}).status_code for _ in range(11)]
ok(codes[-1] == 422 and 'Demasiadas tentativas' in b4.post('/conta/entrar', {'email': 'someone@example.com', 'password': 'x' * 12}).text, 'ten tries in 15 minutes, then wait')

# ── The website's login still can't read patients ──
for t in ['patients', 'appointments', 'payments', 'notes', 'patient_accounts', 'patient_sessions', 'patient_codes']:
    _, err = sql(f'SELECT 1 FROM public.{t} LIMIT 1', PORTAL); ok('permission denied' in err, f'the website login cannot read {t}')

# ── Taking access away, anonymising ──
ok(req('DELETE', f'/patients/{tiago}/portal/accounts/{maria_acc}', None, rui).status_code == 400, "another therapist can't remove access")
ok(req('DELETE', f'/patients/{tiago}/portal/accounts/{maria_acc}', None, ana).status_code == 200, "Tiago's therapist removes the guardian's access to Tiago")
r = b.get('/conta/pedido'); ok('Tiago' not in r.text, 'the account no longer shows Tiago')
b5 = Browser(5); b5.post('/conta/convite', {'code': code_other, 'email': 'outra@example.com', 'password': 'a good password', 'password2': 'a good password', 'privacy': 'on'})
out, _ = sql("SELECT count(*) FROM patient_accounts WHERE email = 'outra@example.com'"); ok(out == '1', 'another account')
sql(f"UPDATE patients SET anonymised_at = now() WHERE id = '{other}'")
out, _ = sql("SELECT count(*) FROM patient_accounts WHERE email = 'outra@example.com'"); ok(out == '0', 'anonymising the patient removes the account')
ok(req('POST', f'/patients/{other}/portal/invite', {}, owner).status_code == 400, 'no invites for an anonymised file')

# ── Closing again ──
req('PUT', '/website', {'accounts_open': False}, owner)
r = b.get('/conta'); ok('Em breve' in r.text, 'closed again: signed-in people see "em breve"')
ok(requests.get(I + '/me', headers={'x-portal-session': b.s.cookies.get('pp_conta') or 'x'}).status_code == 404, 'and the internal API answers nothing')

# English
req('PUT', '/website', {'accounts_open': True}, owner)
r = b.get('/en/account'); ok('Upcoming sessions' in r.text or 'Sign in' in r.text, 'English pages')

print(f'\n{"ALL PASSED" if not fails else f"{fails} FAILED"}')
raise SystemExit(1 if fails else 0)
