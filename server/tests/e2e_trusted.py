# Trusted computers: the 6-digit code only on the first sign-in on a computer, then password only for 30 days.
# Run against a running server with an EMPTY database: pip install requests pyotp && python3 server/tests/e2e_trusted.py
# PSQL_URL (optional) adds the expiry check. Takes ~1–2 min (waits for fresh 30-second code windows).
import os, subprocess, time
import requests, pyotp
# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
PSQL = os.environ.get('PSQL_URL')
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
def req(method, p, j=None, tok=None, dev='MacBook-Filipe'):
    h = {'X-Device': dev, 'X-Device-Key': DEVICE_KEY}
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + p, json=j, headers=h)
def wait_new_step(): time.sleep(30 - time.time() % 30 + 0.5)
def sql(q): return subprocess.run(['psql', PSQL, '-tAc', q], capture_output=True, text=True).stdout.strip()

PW = 'correct horse battery'
r = req('POST', '/setup', {'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW})
en = r.json(); totp = pyotp.TOTP(en['secret'])
# Enrol WITHOUT asking for trust: no token comes back (older desktop apps don't ask, and must never see one).
r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': totp.now()})
owner = r.json()['token']; ok(r.status_code == 200 and 'device_token' not in r.json(), 'no device token unless the app asks for one')

# First sign-in on this computer: password → code, and the app asks to trust it.
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}); ok(r.json().get('next') == 'code', 'first sign-in on a computer asks for the code')
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now(), 'trust_device': True})
dt = r.json().get('device_token', ''); ok(r.status_code == 200 and len(dt) == 64, 'after the code, the computer gets a device token')

# Next sign-ins: password only.
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': dt})
ok(r.status_code == 200 and r.json().get('next') == 'session' and r.json().get('token'), 'trusted computer: password only, session straight away')
ok(r.json().get('staff', {}).get('name') == 'Filipe Silva' and 'password_hash' not in r.json()['staff'] and 'totp_secret' not in r.json()['staff'], 'session answer has the name, no secrets')
s2 = r.json()['token']; ok(req('GET', '/me', tok=s2).status_code == 200, 'that session works')
# Password still required.
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': 'wrong one', 'device_token': dt}); ok(r.status_code == 401, 'trusted computer still needs the right password')
# Token belongs to one person: someone else can't use it.
r = req('POST', '/invites', {'role': 'reception', 'email': 'marta@clinica.pt'}, owner); code = r.json()['code']
r = req('POST', '/auth/signup', {'invite': code, 'name': 'Marta Costa', 'email': 'marta@clinica.pt', 'password': 'another long password'})
men = r.json(); mt = pyotp.TOTP(men['secret'])
r = req('POST', '/auth/verify', {'challenge': men['challenge'], 'code': mt.now(), 'trust_device': True}); mdt = r.json()['device_token']
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password', 'device_token': dt}); ok(r.json().get('next') == 'code', "another person's device token doesn't skip the code")
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': 'f' * 64}); ok(r.json().get('next') == 'code', 'made-up token: code asked')
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': 'short'}); ok(r.json().get('next') == 'code', 'malformed token: code asked')

# Access log.
log = req('GET', '/access-log', tok=owner).json()
ok(any(x['action'] == 'login.computer_trusted' for x in log), 'access log: computer trusted')
ok(any(x['action'] == 'login.ok' and x['detail'] == 'trusted_computer' for x in log), 'access log: sign-in on a trusted computer')

# Re-trusting the same computer replaces the old token.
wait_new_step()
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW})
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now(), 'trust_device': True}); dt2 = r.json()['device_token']
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': dt}); ok(r.json().get('next') == 'code', 'old token of the same computer stops working')
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': dt2}); ok(r.json().get('next') == 'session', 'new token works')

# Forget this computer.
ok(req('POST', '/auth/forget-device', {'device_token': dt2}).status_code == 200, 'forget this computer')
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': dt2}); ok(r.json().get('next') == 'code', 'forgotten computer asks for the code again')
ok(req('POST', '/auth/forget-device', {'device_token': 'nope'}).status_code == 200, 'forget answers ok for unknown tokens (no token probing)')

# Owner ends someone's sessions everywhere → their trusted computers are forgotten too.
mid = [x for x in req('GET', '/staff', tok=owner).json() if x['email'] == 'marta@clinica.pt'][0]['id']
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password', 'device_token': mdt}); ok(r.json().get('next') == 'session', 'reception trusted computer works')
req('DELETE', f'/staff/{mid}/sessions', tok=owner)
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password', 'device_token': mdt}); ok(r.json().get('next') == 'code', '"sign out everywhere" also forgets trusted computers')
# Deactivation: no sign-in at all, and trust is gone.
wait_new_step()
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password'})
r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': mt.now(), 'trust_device': True}); mdt = r.json()['device_token']
req('PATCH', f'/staff/{mid}', {'active': False}, owner)
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password', 'device_token': mdt}); ok(r.status_code == 401, 'deactivated account: refused even on a trusted computer')
if PSQL:
    ok(sql(f"SELECT count(*) FROM trusted_devices WHERE staff_id = '{mid}'") == '0', 'deactivation deletes trusted computers')

# 30 days.
if PSQL:
    wait_new_step()
    r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW})
    r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now(), 'trust_device': True}); dt3 = r.json()['device_token']
    days = sql("SELECT round(extract(epoch FROM expires_at - created_at) / 86400) FROM trusted_devices ORDER BY created_at DESC LIMIT 1")
    ok(days == '30', f'trust lasts 30 days ({days})')
    sql("UPDATE trusted_devices SET expires_at = now() - interval '1 second'")
    r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW, 'device_token': dt3}); ok(r.json().get('next') == 'code', 'after 30 days the code is asked again')

print(f'\n{"ALL PASSED" if not fails else f"{fails} FAILED"}')
raise SystemExit(1 if fails else 0)
