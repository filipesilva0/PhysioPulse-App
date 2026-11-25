# Phase 3 of the PhysioPulse plan: device approval, the owner's device list, recovery code and server console,
# computers trusted before phase 3, rate limits in the database, the real client address behind the proxy,
# two-step secrets encrypted with SECRET_KEY.
# Run against a running server with an EMPTY database, started with SECRET_KEY=<64 hex> and BIND on 127.0.0.1
# (127.0.0.1 is a trusted proxy by default, so this test can send X-Forwarded-For like Nginx Proxy Manager):
#   PSQL_URL=postgres://… SERVER_BIN=server/target/debug/physiopulse-server python3 server/tests/e2e_devices.py
# Takes ~1–2 min (waits for fresh 30-second code windows).
import os, subprocess, time, hashlib, secrets
import requests, pyotp
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
BIN = os.environ.get('SERVER_BIN', os.path.join(os.path.dirname(__file__), '..', 'target', 'debug', 'physiopulse-server'))
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
def key(): return secrets.token_hex(32)
A, Bk, C, D, E, F = key(), key(), key(), key(), key(), key()   # six computers
def req(method, p, j=None, tok=None, dev=None, name='PC', ip=None):
    h = {'X-Device': name}
    if dev: h['X-Device-Key'] = dev
    if tok: h['Authorization'] = 'Bearer ' + tok
    if ip: h['X-Forwarded-For'] = ip
    return requests.request(method, B + p, json=j, headers=h)
def wait_new_step(): time.sleep(30 - time.time() % 30 + 0.5)
def sql(q): return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()
def sha(s): return hashlib.sha256(s.encode()).hexdigest()

PW = 'correct horse battery'
# ── Setup: the computer the clinic is set up on is the first approved device ──
r = req('POST', '/setup', {'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW})
ok(r.status_code == 403 and r.json().get('error') == 'app_update', 'an app without a device key is asked to update (setup)')
r = req('POST', '/setup', {'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW}, dev=A, name='MacBook-Filipe')
en = r.json(); totp = pyotp.TOTP(en['secret'])
ok(len(en['secret']) >= 16 and not en['secret'].startswith('v1:'), 'the setup screen still shows the plain secret to type in')
r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': totp.now(), 'trust_device': True}, dev=A, name='MacBook-Filipe')
j = r.json(); owner = j.get('token'); codes = j.get('recovery_codes') or []
ok(r.status_code == 200 and j.get('next') == 'session' and owner and len(codes) == 10, 'owner on the setup computer: session straight away, ten recovery codes')
owner_dt = j.get('device_token', '')
ok(req('GET', '/me', tok=owner, dev=A).status_code == 200, 'owner session works from its device')
ok(req('GET', '/me', tok=owner).status_code == 401, 'the same session without the device key is refused')
ok(req('GET', '/me', tok=owner, dev=Bk).status_code == 401, 'the same session from another device is refused')
owner = None
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': owner_dt}, dev=A, name='MacBook-Filipe')
owner = r.json().get('token'); ok(r.json().get('next') == 'session' and req('GET', '/me', tok=owner, dev=A).status_code == 200, 'trusted, approved computer: password only')

# ── Two-step secrets are encrypted in the database ──
if PSQL:
    ok(sql("SELECT count(*) FROM staff WHERE totp_secret NOT LIKE 'v1:%'") == '0', 'no two-step secret in plain text in the database')
r = req('GET', '/system/status', tok=owner, dev=A); ok(r.json().get('secrets_encrypted') is True, 'Settings › Server says the secrets are encrypted')

# ── A therapist joins from her own laptop: password + code, then she waits for the owner ──
code = req('POST', '/invites', {'role': 'therapist', 'email': 'ana@clinica.pt'}, owner, dev=A).json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Ana Reis', 'email': 'ana@clinica.pt', 'password': 'another long password'}, dev=Bk, name='Portatil-Ana', ip='203.0.113.7')
ana_en = r.json(); ana_totp = pyotp.TOTP(ana_en['secret'])
r = req('POST', '/auth/verify', {'challenge': ana_en['challenge'], 'code': ana_totp.now(), 'trust_device': True}, dev=Bk, name='Portatil-Ana', ip='203.0.113.7')
j = r.json(); ticket = j.get('ticket', ''); dev_code = (j.get('device') or {}).get('code', '')
ok(r.status_code == 200 and j.get('next') == 'pending' and 'token' not in j and len(ticket) == 64, 'new device: no session, a ticket to wait with')
ok(len(dev_code) == 7 and dev_code[3] == '-' and j.get('owner') is False, 'the waiting screen gets a short code (and is told she is not the owner)')
r = req('POST', '/auth/pending', {'ticket': ticket}, dev=Bk); ok(r.json().get('next') == 'pending', 'still waiting')
ok(req('POST', '/auth/pending', {'ticket': ticket}, dev=C).status_code == 401, 'the ticket only works on its own device')
ok(req('POST', '/auth/pending', {'ticket': 'f' * 64}, dev=Bk).status_code == 401, 'a made-up ticket is refused')
r = req('POST', '/auth/pending/recover', {'ticket': ticket, 'recovery_code': codes[0]}, dev=Bk); ok(r.status_code == 403, "a therapist can't approve her own device with the owner's recovery code")

# ── The owner sees it and approves ──
r = req('GET', '/devices', tok=owner, dev=A); lst = r.json()
ok(r.status_code == 200 and lst[0]['status'] == 'pending' and lst[0]['code'] == dev_code and lst[0]['requested_by_name'] == 'Ana Reis', 'device list: the waiting device first, with its code and who asked')
ok(lst[0]['last_ip'] == '203.0.113.7', 'it shows the address the proxy saw (X-Forwarded-For from the proxy)')
mine = [d for d in lst if d['this_device']]; ok(len(mine) == 1 and mine[0]['status'] == 'approved' and mine[0]['how'] == 'setup', 'the owner sees their own computer marked "this device"')
r = req('GET', '/alerts', tok=owner, dev=A); ok(len(r.json().get('devices') or []) == 1, 'Today: the owner has a "device waiting" alert')
pend_id = lst[0]['id']
r = req('POST', f'/devices/{pend_id}/approve', tok=owner, dev=A); ok(r.json().get('status') == 'approved', 'owner approves it')
r = req('POST', '/auth/pending', {'ticket': ticket}, dev=Bk, name='Portatil-Ana'); j = r.json(); ana = j.get('token')
ok(j.get('next') == 'session' and ana and len(j.get('device_token', '')) == 64, 'the waiting app gets its session by itself (and its trusted-computer token)')
ana_dt = j.get('device_token')
ok(req('POST', '/auth/pending', {'ticket': ticket}, dev=Bk).status_code == 401, 'the ticket works once')
ok(req('GET', '/me', tok=ana, dev=Bk).status_code == 200, "Ana's session works from her laptop")
ok(req('GET', '/devices', tok=ana, dev=Bk).status_code == 403, 'only the owner sees the device list')
ok(req('POST', f'/devices/{pend_id}/revoke', tok=ana, dev=Bk).status_code == 403, 'only the owner removes devices')
r = req('GET', '/alerts', tok=ana, dev=Bk); ok(r.json().get('devices') is None, 'therapists get no device alert')
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password', 'device_token': ana_dt}, dev=Bk)
ok(r.json().get('next') == 'session', 'next time on her laptop: password only')
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password', 'device_token': ana_dt}, dev=C)
ok(r.json().get('next') == 'code', "her laptop's token doesn't skip the code on another computer")

# ── The approved device is approved for everyone who uses it (a shared reception PC) ──
code = req('POST', '/invites', {'role': 'reception', 'email': 'marta@clinica.pt'}, owner, dev=A).json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Marta Costa', 'email': 'marta@clinica.pt', 'password': 'a third long password'}, dev=Bk)
m_en = r.json(); mtotp = pyotp.TOTP(m_en['secret'])
r = req('POST', '/auth/verify', {'challenge': m_en['challenge'], 'code': mtotp.now()}, dev=Bk)
ok(r.json().get('next') == 'session', 'another person on an approved device: no new approval')

# ── Rename, then remove: sessions end at once, sign-in is refused, trust is forgotten ──
r = req('PATCH', f'/devices/{pend_id}', {'name': '  Portátil   da Ana '}, tok=owner, dev=A); ok(r.json().get('name') == 'Portátil da Ana', 'owner renames a device')
ok(req('PATCH', f'/devices/{pend_id}', {'name': ' '}, tok=owner, dev=A).status_code == 400, 'a device needs a name')
my_id = mine[0]['id']
r = req('POST', f'/devices/{my_id}/revoke', tok=owner, dev=A); ok(r.status_code == 400, "the owner can't remove the device they are using")
r = req('POST', f'/devices/{pend_id}/revoke', tok=owner, dev=A); ok(r.json().get('sessions_ended', 0) >= 2, 'removing a device ends its sessions')
r = req('GET', '/me', tok=ana, dev=Bk); ok(r.status_code == 401, "Ana's open session stops at once")
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password', 'device_token': ana_dt}, dev=Bk)
ok(r.status_code == 403 and r.json().get('error') == 'device_revoked', 'sign-in from a removed device is refused (after the password)')
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'wrong password!!'}, dev=Bk); ok(r.status_code == 401, 'a wrong password still reads as a wrong password')
if PSQL:
    ok(sql(f"SELECT count(*) FROM trusted_devices WHERE device_id = '{pend_id}'") == '0', 'its trusted-computer tokens are gone')
r = req('POST', f'/devices/{pend_id}/approve', tok=owner, dev=A); ok(r.json().get('status') == 'approved', 'the owner can approve a removed device again')
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password', 'device_token': ana_dt}, dev=Bk)
ok(r.json().get('next') == 'code', 'after that, the code is asked again (trust was forgotten)')

# ── Declining a waiting device ──
code = req('POST', '/invites', {'role': 'therapist', 'email': 'rui@clinica.pt'}, owner, dev=A).json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Rui Lopes', 'email': 'rui@clinica.pt', 'password': 'rui long password'}, dev=D, name='Telemovel-Rui')
rui_en = r.json(); rtotp = pyotp.TOTP(rui_en['secret'])
r = req('POST', '/auth/verify', {'challenge': rui_en['challenge'], 'code': rtotp.now()}, dev=D, name='Telemovel-Rui'); rui_ticket = r.json().get('ticket')
rid = [d for d in req('GET', '/devices', tok=owner, dev=A).json() if d['status'] == 'pending'][0]['id']
r = req('POST', f'/devices/{rid}/revoke', tok=owner, dev=A); ok(r.status_code == 200, 'owner declines a waiting device')
r = req('POST', '/auth/pending', {'ticket': rui_ticket}, dev=D); ok(r.status_code == 403 and r.json().get('error') == 'device_revoked', 'the waiting screen learns it was declined')

# ── The owner on a new computer with no approved device at hand: a printed recovery code ──
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=C, name='PC-Casa')
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()}, dev=C, name='PC-Casa'); j = r.json()
ok(j.get('next') == 'pending' and j.get('owner') is True, 'the owner on a new computer waits too (and is offered the recovery code)')
r = req('POST', '/auth/pending/recover', {'ticket': j['ticket'], 'recovery_code': 'AAAA-BBBB-CCCC'}, dev=C); ok(r.status_code == 401, 'a wrong recovery code is refused')
r = req('POST', '/auth/pending/recover', {'ticket': j['ticket'], 'recovery_code': codes[1]}, dev=C); j2 = r.json()
ok(j2.get('next') == 'session' and j2.get('token') and j2.get('codes_left') == 9, 'a recovery code approves the device and signs in (9 codes left)')
ok(req('GET', '/me', tok=j2['token'], dev=C).status_code == 200, 'that session works')

# ── Server console: `physiopulse-server approve CODE` ──
code = req('POST', '/invites', {'role': 'therapist', 'email': 'sara@clinica.pt'}, owner, dev=A).json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Sara Dias', 'email': 'sara@clinica.pt', 'password': 'sara long password'}, dev=E, name='PC-Sara')
s_en = r.json(); stotp = pyotp.TOTP(s_en['secret'])
r = req('POST', '/auth/verify', {'challenge': s_en['challenge'], 'code': stotp.now()}, dev=E, name='PC-Sara'); s_ticket = r.json()['ticket']; s_code = r.json()['device']['code']
if PSQL and os.path.exists(BIN):
    env = {**os.environ, 'DATABASE_URL': PSQL}
    out = subprocess.run([BIN, 'devices'], capture_output=True, text=True, env=env)
    ok(out.returncode == 0 and s_code in out.stdout and 'pending' in out.stdout, 'console lists the waiting devices')
    out = subprocess.run([BIN, 'approve', 'ZZZ-ZZZ'], capture_output=True, text=True, env=env); ok(out.returncode == 1, 'console: an unknown code is refused')
    out = subprocess.run([BIN, 'approve', s_code.lower()], capture_output=True, text=True, env=env); ok(out.returncode == 0, 'console approves by the code on the waiting screen')
    r = req('POST', '/auth/pending', {'ticket': s_ticket}, dev=E); ok(r.json().get('next') == 'session', 'the waiting app then signs in by itself')
else:
    print('SKIP console checks (needs PSQL_URL and SERVER_BIN)')

# ── Computers trusted before phase 3 keep working (owner decision, 11 Nov 2026) ──
if PSQL:
    old = secrets.token_hex(32)
    sid = sql("SELECT id FROM staff WHERE email = 'marta@clinica.pt'")
    sql(f"INSERT INTO trusted_devices (id, staff_id, token_hash, device, expires_at, carried_over) VALUES (gen_random_uuid(), '{sid}', '{sha(old)}', 'Rececao-1', now() + interval '20 days', TRUE)")
    r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'a third long password', 'device_token': old}, dev=F, name='Rececao-1')
    ok(r.json().get('next') == 'session', 'a computer trusted before the update: password only, no approval wait')
    ok(sql(f"SELECT status || ' ' || how FROM devices WHERE key_hash = '{sha(F)}'") == 'approved carried_over', 'its device is approved as "carried over"')
    r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'a third long password', 'device_token': old}, dev=key(), name='Outro')
    ok(r.json().get('next') == 'code', 'the old token approves one device only')

# ── Rate limits live in the database, per email and per internet address ──
for i in range(5):
    req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'nope nope nope'}, dev=Bk, ip='198.51.100.1')
r = req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password'}, dev=Bk, ip='198.51.100.2'); ok(r.status_code == 429, 'five wrong passwords lock that email for 15 minutes')
if PSQL:
    ok(sql("SELECT fails FROM rate_limits WHERE key = 'login:ana@clinica.pt'") == '5', 'the count is kept in the database (survives a restart)')
for i in range(30):
    req('POST', '/auth/login', {'email': f'nobody{i}@x.pt', 'password': 'nope nope nope'}, dev=Bk, ip='198.51.100.9')
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=A, ip='198.51.100.9'); ok(r.status_code == 429, 'thirty failures from one address lock that address')
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': owner_dt}, dev=A, ip='198.51.100.10'); ok(r.json().get('next') == 'session', 'other addresses are not affected')

# ── Access log ──
log = req('GET', '/access-log?limit=500', tok=owner, dev=A).json()
rows = log.get('rows', log) if isinstance(log, dict) else log
acts = {x.get('action') for x in rows}
for a in ['device.requested', 'device.approved', 'device.revoked', 'device.declined', 'device.renamed', 'device.bad_recovery_code', 'login.device_refused']:
    ok(a in acts, f'access log has {a}')

print('ALL PASSED' if fails == 0 else f'{fails} FAILED')
raise SystemExit(1 if fails else 0)
