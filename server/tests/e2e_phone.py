# Phase 5 of the PhysioPulse plan: the staff iPhone app on the server side.
# - A device says it is a phone (X-Device-Kind: phone) when it is first saved; the owner sees "phone" before approving.
# - Sessions on an approved phone: no idle timeout, 30 days, one per person per phone.
# - A computer can't turn itself into a phone later by sending the header.
# - Team: the owner lists anyone's phones and signs one out; everyone sees their own; nobody else.
# Run against a running server with an EMPTY database, started with SECRET_KEY=<64 hex> on 127.0.0.1 and IDLE_MINUTES=15:
#   PSQL_URL=postgres://… python3 server/tests/e2e_phone.py
# Takes ~1–2 min (waits for fresh 30-second code windows).
import os, subprocess, time, secrets
import requests, pyotp
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
def key(): return secrets.token_hex(32)
PC, PHONE, ANA_PHONE, ANA_PC = key(), key(), key(), key()
def req(method, p, j=None, tok=None, dev=None, name='PC', phone=False):
    h = {'X-Device': name}
    if dev: h['X-Device-Key'] = dev
    if phone: h['X-Device-Kind'] = 'phone'
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + p, json=j, headers=h)
def wait_new_step(): time.sleep(30 - time.time() % 30 + 0.5)
def sql(q): return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

PW = 'correct horse battery'
# ── The owner sets up the clinic on the computer ──
r = req('POST', '/setup', {'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW}, dev=PC, name='MacBook-Filipe')
en = r.json(); totp = pyotp.TOTP(en['secret'])
r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': totp.now()}, dev=PC, name='MacBook-Filipe')
owner = r.json().get('token'); me = r.json()['staff']['id']
ok(r.status_code == 200 and owner, 'owner signed in on the computer')

# ── The owner signs in on the iPhone: password, code, then waits for approval like any new device ──
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=PHONE, name='iPhone de Filipe', phone=True)
ok(r.json().get('next') == 'code', 'phone: password, then the 6-digit code')
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()}, dev=PHONE, name='iPhone de Filipe', phone=True)
j = r.json(); ticket = j.get('ticket', '')
ok(j.get('next') == 'pending' and 'device_token' not in j, 'a new phone waits for approval (no trusted-computer token: the phone app never asks for one)')
r = req('GET', '/alerts', tok=owner, dev=PC); d = (r.json().get('devices') or [{}])[0]
ok(d.get('kind') == 'phone' and d.get('name') == 'iPhone de Filipe', "Today's alert says it's a phone")
lst = req('GET', '/devices', tok=owner, dev=PC).json(); ph = [x for x in lst if x['status'] == 'pending'][0]
ok(ph['kind'] == 'phone' and [x for x in lst if x['this_device']][0]['kind'] == 'computer', 'device list: kind phone for the iPhone, computer for the Mac')
req('POST', f"/devices/{ph['id']}/approve", tok=owner, dev=PC)
r = req('POST', '/auth/pending', {'ticket': ticket}, dev=PHONE, name='iPhone de Filipe', phone=True)
phone_tok = r.json().get('token')
ok(r.json().get('next') == 'session' and phone_tok, 'approved: the phone gets its session by itself')
if PSQL:
    ok(sql(f"SELECT phone FROM sessions WHERE device_id = '{ph['id']}'") == 't', 'it is a phone session')
    days = float(sql(f"SELECT extract(epoch FROM expires_at - created_at) / 86400 FROM sessions WHERE device_id = '{ph['id']}'") or 0)
    ok(29.9 < days < 30.1, 'phone session lasts 30 days')
    pc_hours = float(sql(f"SELECT extract(epoch FROM expires_at - created_at) / 3600 FROM sessions WHERE staff_id = '{me}' AND NOT phone LIMIT 1") or 0)
    ok(11.9 < pc_hours < 12.1, 'computer sessions still last 12 hours')

    # No idle timeout on the phone; the computer still locks after 15 minutes.
    sql(f"UPDATE sessions SET last_seen = now() - interval '3 days' WHERE device_id = '{ph['id']}'")
    ok(req('GET', '/me', tok=phone_tok, dev=PHONE, phone=True).status_code == 200, 'phone session unused for 3 days still works (Face ID is the lock)')
    sql(f"UPDATE sessions SET last_seen = now() - interval '16 minutes' WHERE staff_id = '{me}' AND NOT phone")
    ok(req('GET', '/me', tok=owner, dev=PC).status_code == 401, 'computer session unused for 16 minutes ended (idle timeout as before)')
    # The housekeeping query keeps idle phone sessions but drops expired ones.
    sql("DELETE FROM sessions WHERE expires_at < now() OR (NOT phone AND last_seen < now() - make_interval(mins => 15))")
    ok(sql(f"SELECT count(*) FROM sessions WHERE device_id = '{ph['id']}'") == '1', 'housekeeping keeps an idle phone session')
ok(req('GET', '/me', tok=phone_tok, dev=PC).status_code == 401, 'the phone token is refused from another device')
ok(req('GET', '/me', tok=phone_tok).status_code == 401, 'the phone token is refused without the device key')

# Sign the owner back in on the computer.
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=PC, name='MacBook-Filipe')
owner = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()}, dev=PC, name='MacBook-Filipe').json().get('token')
ok(req('GET', '/me', tok=owner, dev=PC).status_code == 200, 'owner back on the computer')

# ── A computer can't become a phone by sending the header ──
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=PC, name='MacBook-Filipe', phone=True)
t2 = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()}, dev=PC, name='MacBook-Filipe', phone=True).json().get('token')
if PSQL:
    ok(sql(f"SELECT count(*) FROM sessions WHERE staff_id = '{me}' AND phone") == '1', "a computer sending 'phone' still gets a computer session")

# ── Signing in again on the same phone replaces its session ──
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}, dev=PHONE, name='iPhone de Filipe', phone=True)
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()}, dev=PHONE, name='iPhone de Filipe', phone=True)
new_phone_tok = r.json().get('token')
ok(r.json().get('next') == 'session' and new_phone_tok, 'approved phone: password + code give a session straight away')
ok(req('GET', '/me', tok=phone_tok, dev=PHONE, phone=True).status_code == 401, 'the old phone session ended (one per person per phone)')
phone_tok = new_phone_tok

# ── Team: the person's phones ──
r = req('GET', f'/staff/{me}/phones', tok=owner, dev=PC); rows = r.json()
ok(r.status_code == 200 and len(rows) == 1 and rows[0]['name'] == 'iPhone de Filipe' and rows[0]['this_device'] is False, "owner sees their iPhone in Team (not 'this device' from the Mac)")
ok(set(rows[0]) >= {'id', 'signed_in', 'last_seen', 'expires_at', 'code'}, 'each row says when it signed in, was last used and ends')
r = req('GET', f'/staff/{me}/phones', tok=phone_tok, dev=PHONE, phone=True)
ok(r.json()[0]['this_device'] is True, "from the phone itself it's marked 'this device'")

# A therapist with her own iPhone.
code = req('POST', '/invites', {'role': 'therapist', 'email': 'ana@clinica.pt'}, owner, dev=PC).json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Ana Reis', 'email': 'ana@clinica.pt', 'password': 'another long password'}, dev=ANA_PHONE, name='iPhone de Ana', phone=True)
ana_en = r.json(); atotp = pyotp.TOTP(ana_en['secret'])
r = req('POST', '/auth/verify', {'challenge': ana_en['challenge'], 'code': atotp.now()}, dev=ANA_PHONE, name='iPhone de Ana', phone=True)
aticket = r.json().get('ticket')
ana_dev = [x for x in req('GET', '/devices', tok=owner, dev=PC).json() if x['status'] == 'pending'][0]
ok(ana_dev['kind'] == 'phone', "Ana's iPhone waits as a phone")
req('POST', f"/devices/{ana_dev['id']}/approve", tok=owner, dev=PC)
r = req('POST', '/auth/pending', {'ticket': aticket}, dev=ANA_PHONE, name='iPhone de Ana', phone=True)
ana = r.json().get('token'); ana_id = r.json()['staff']['id']
ok(ana and req('GET', '/me', tok=ana, dev=ANA_PHONE, phone=True).status_code == 200, 'Ana is signed in on her iPhone')
ok(req('GET', f'/staff/{ana_id}/phones', tok=ana, dev=ANA_PHONE, phone=True).status_code == 200, 'Ana sees her own phones')
ok(req('GET', f'/staff/{me}/phones', tok=ana, dev=ANA_PHONE, phone=True).status_code == 403, "Ana can't see the owner's phones")
asid = req('GET', f'/staff/{ana_id}/phones', tok=owner, dev=PC).json()[0]['id']
ok(req('DELETE', f'/staff/{ana_id}/phones/{asid}', tok=ana, dev=ANA_PHONE, phone=True).status_code == 403, 'only the owner signs a phone out from Team')
ok(req('DELETE', f'/staff/{me}/phones/{asid}', tok=owner, dev=PC).status_code == 404, "the session must belong to that person")
# Role rules still apply on the phone.
ok(req('GET', '/finance/overview', tok=ana, dev=ANA_PHONE, phone=True).status_code == 403, 'a therapist on her phone still has no finances')
ok(req('GET', '/devices', tok=ana, dev=ANA_PHONE, phone=True).status_code == 403, 'nor the device list')

# The owner signs Ana's iPhone out.
r = req('DELETE', f'/staff/{ana_id}/phones/{asid}', tok=owner, dev=PC)
ok(r.status_code == 200, 'owner: Sign Out This iPhone')
ok(req('GET', '/me', tok=ana, dev=ANA_PHONE, phone=True).status_code == 401, "Ana's phone session ended at once")
ok(req('GET', f'/staff/{ana_id}/phones', tok=owner, dev=PC).json() == [], 'her phone is no longer listed')
dv = [x for x in req('GET', '/devices', tok=owner, dev=PC).json() if x['id'] == ana_dev['id']][0]
ok(dv['status'] == 'approved', 'the phone itself stays approved (she signs in again with password + code)')
log = req('GET', f'/access-log?subject_type=staff&subject_id={ana_id}', tok=owner, dev=PC).json()
rows = log if isinstance(log, list) else log.get('rows', log.get('items', []))
ok(any(x['action'] == 'staff.phone_signed_out' for x in rows), 'the access log says who signed which phone out')

# ── Expiry after 30 days, and removing the phone in Settings › Devices ──
if PSQL:
    sql(f"UPDATE sessions SET expires_at = now() - interval '1 minute' WHERE device_id = '{ph['id']}'")
    ok(req('GET', '/me', tok=phone_tok, dev=PHONE, phone=True).status_code == 401, 'after 30 days the phone session ends (password + code again)')
    sql(f"UPDATE sessions SET expires_at = now() + interval '30 days' WHERE device_id = '{ph['id']}'")
    ok(req('GET', '/me', tok=phone_tok, dev=PHONE, phone=True).status_code == 200, '(restored for the next check)')
req('POST', f"/devices/{ph['id']}/revoke", tok=owner, dev=PC)
ok(req('GET', '/me', tok=phone_tok, dev=PHONE, phone=True).status_code == 401, 'removing the iPhone in Settings › Devices ends its session')
ok(req('GET', f'/staff/{me}/phones', tok=owner, dev=PC).json() == [], "and it leaves the owner's phone list")

print(f'\n{fails} failed' if fails else '\nall passed')
raise SystemExit(1 if fails else 0)
