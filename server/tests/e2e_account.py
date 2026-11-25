# Audit fixes, phase A (10 Nov 2026): accounts (change password, name, recovery codes, owner resets),
# invites list/withdraw, clinic details on PDFs, unset scales left out of signed notes, server and backup status.
# Run against a running server with an EMPTY database, started with BACKUP_CONTROL=<an empty folder> and
# PUBLIC_ADDRESS=100.68.180.126:8095:
#   pip install requests pyotp pillow && CONTROL=/tmp/ctl python3 server/tests/e2e_account.py
# Uses pdftotext if installed. Takes ~2 min (waits for fresh 30-second code windows).
import io, json, os, shutil, subprocess, time
import requests, pyotp
# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
CONTROL = os.environ.get('CONTROL')
LOG = os.environ.get('LOG')
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
def req(method, p, j=None, tok=None, dev='MacBook-Filipe'):
    h = {'X-Device': dev, 'X-Device-Key': DEVICE_KEY}
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + p, json=j, headers=h)
def wait_new_step(): time.sleep(30 - time.time() % 30 + 0.5)
def sign_in(email, pw, totp):
    wait_new_step()
    r = req('POST', '/auth/login', {'email': email, 'password': pw})
    if r.json().get('next') != 'code': return None
    r = req('POST', '/auth/verify', {'challenge': r.json()['challenge'], 'code': totp.now()})
    return r.json().get('token')
def pdf_text(doc_id, tok):
    if not shutil.which('pdftotext'): return None
    r = requests.get(B + f'/documents/{doc_id}/raw', headers={'Authorization': 'Bearer ' + tok, 'X-Device-Key': DEVICE_KEY})
    return subprocess.run(['pdftotext', '-', '-'], input=r.content, capture_output=True).stdout.decode('utf-8', 'replace')

PW = 'correct horse battery'
# ── Owner setup: recovery codes come with the first two-step setup, once ──
en = req('POST', '/setup', {'clinic_name': 'Clínica Teste', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW}).json()
totp = pyotp.TOTP(en['secret'])
r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': totp.now()})
owner = r.json()['token']; codes = r.json().get('recovery_codes') or []
ok(len(codes) == 10 and all(len(c) == 14 for c in codes) and len(set(codes)) == 10, 'owner gets 10 recovery codes when two-step is first set up')
r = req('GET', '/me/recovery-codes', tok=owner); ok(r.json()['left'] == 10 and r.json()['total'] == 10, 'recovery status: 10 left')

# Staff: a therapist and a receptionist (no recovery codes for them).
def join(role, email, name):
    code = req('POST', '/invites', {'role': role, 'email': email}, owner).json()['code']
    e = req('POST', '/auth/signup', {'invite': code, 'name': name, 'email': email, 'password': 'another long password'}).json()
    t = pyotp.TOTP(e['secret']); r = req('POST', '/auth/verify', {'challenge': e['challenge'], 'code': t.now()})
    return r.json()['token'], r.json()['staff']['id'], t, r.json()
ther, ther_id, ther_totp, rj = join('therapist', 'ana@clinica.pt', 'Ana Rocha')
ok('recovery_codes' not in rj, 'therapists get no recovery codes')
recep, recep_id, recep_totp, _ = join('reception', 'marta@clinica.pt', 'Marta Costa')
ok(req('GET', '/me/recovery-codes', tok=ther).status_code == 403 and req('POST', '/me/recovery-codes', {'password': 'another long password'}, ther).status_code == 403, 'recovery codes are owner only (server)')

# ── Name ──
r = req('PATCH', '/me', {'name': '  Ana   Rocha Silva '}, ther); ok(r.status_code == 200 and r.json()['name'] == 'Ana Rocha Silva' and 'password_hash' not in r.json(), 'anyone changes their own name (spaces tidied)')
ok(req('PATCH', '/me', {'name': '   '}, ther).status_code == 400, 'empty name refused')

# ── Change password ──
other = sign_in('ana@clinica.pt', 'another long password', ther_totp)
r = req('POST', '/me/password', {'current': 'wrong password!', 'new': 'a brand new password'}, ther); ok(r.status_code == 400, 'wrong current password refused')
r = req('POST', '/me/password', {'current': 'another long password', 'new': 'short'}, ther); ok(r.status_code == 400, 'new password must be 12+ characters')
r = req('POST', '/me/password', {'current': 'another long password', 'new': 'another long password'}, ther); ok(r.status_code == 400, 'new password must differ')
r = req('POST', '/me/password', {'current': 'another long password', 'new': 'a brand new password'}, ther)
ok(r.status_code == 200 and r.json()['other_sessions_ended'] >= 1, 'password changed; other sessions ended')
ok(req('GET', '/me', tok=ther).status_code == 200 and req('GET', '/me', tok=other).status_code == 401, 'this session stays, the other computer is signed out')
ok(req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'another long password'}).status_code == 401, 'old password no longer works')
ok(req('POST', '/auth/login', {'email': 'ana@clinica.pt', 'password': 'a brand new password'}).json().get('next') == 'code', 'new password works (still asks for the code)')

# ── Owner: reset code for someone who forgot their password ──
ok(req('POST', f'/staff/{ther_id}/reset-code', tok=recep).status_code == 403, 'reception cannot make reset codes')
ok(req('POST', f'/staff/{req("GET", "/me", tok=owner).json()["id"]}/reset-code', tok=owner).status_code == 400, 'owner cannot reset their own account from Team')
r = req('POST', f'/staff/{ther_id}/reset-code', tok=owner); rc = r.json()['code']
ok(r.status_code == 200 and len(rc) == 14 and r.json()['expires_in_hours'] == 24, 'owner makes a 24-hour reset code ' + rc)
r = req('POST', '/auth/reset', {'email': 'marta@clinica.pt', 'code': rc, 'password': 'stolen password!!'}); ok(r.status_code == 400, "a reset code only works for that person's email")
r = req('POST', '/auth/reset', {'email': 'ana@clinica.pt', 'code': rc, 'password': 'short'}); ok(r.status_code == 400, 'reset still needs 12+ characters')
r = req('POST', '/auth/reset', {'email': 'ANA@clinica.pt', 'code': rc.lower().replace('-', ' '), 'password': 'forgot it again ok'}); ok(r.status_code == 200, 'reset with the code (typed loosely) sets a new password')
ok(req('GET', '/me', tok=ther).status_code == 401, 'after a reset the person is signed out everywhere')
r = req('POST', '/auth/reset', {'email': 'ana@clinica.pt', 'code': rc, 'password': 'and once more now'}); ok(r.status_code == 400, 'a reset code works once')
ther = sign_in('ana@clinica.pt', 'forgot it again ok', ther_totp); ok(bool(ther), 'signs in with the new password + code')
rc1 = req('POST', f'/staff/{ther_id}/reset-code', tok=owner).json()['code']
rc2 = req('POST', f'/staff/{ther_id}/reset-code', tok=owner).json()['code']
ok(req('POST', '/auth/reset', {'email': 'ana@clinica.pt', 'code': rc1, 'password': 'not this one ok!'}).status_code == 400, 'a newer code replaces the older one')
for i in range(5): req('POST', '/auth/reset', {'email': 'ana@clinica.pt', 'code': 'AAAA-BBBB-CCCC', 'password': 'whatever whatever'})
ok(req('POST', '/auth/reset', {'email': 'ana@clinica.pt', 'code': rc2, 'password': 'whatever whatever'}).status_code == 429, 'reset attempts are rate-limited (5 per 15 min)')

# ── Owner: someone lost their phone → two-step set up again ──
r = req('POST', f'/staff/{recep_id}/reset-two-step', tok=owner); ok(r.status_code == 200, 'owner resets two-step for a receptionist')
ok(req('GET', '/me', tok=recep).status_code == 401, 'they are signed out')
r = req('POST', '/auth/login', {'email': 'marta@clinica.pt', 'password': 'another long password'})
ok(r.json().get('next') == 'enrol' and r.json()['enrol']['secret'] != recep_totp.secret, 'next sign-in shows a new QR code (new secret)')
staff = req('GET', '/staff', tok=owner).json(); ok(not [s for s in staff if s['id'] == recep_id][0]['totp_enabled'], 'Team shows two-step not set up')

# ── Owner lost their phone: recovery code at the code step ──
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}); ch = r.json()['challenge']
r = req('POST', '/auth/recover', {'challenge': ch, 'recovery_code': 'AAAA-BBBB-CCCC'}); ok(r.status_code == 401, 'wrong recovery code refused')
r = req('POST', '/auth/recover', {'challenge': ch, 'recovery_code': codes[0].lower()})
ok(r.status_code == 200 and r.json()['next'] == 'enrol' and r.json()['codes_left'] == 9, 'recovery code → set up two-step on a new phone (9 left)')
en2 = r.json()['enrol']; totp2 = pyotp.TOTP(en2['secret'])
ok(en2['secret'] != totp.secret, 'new two-step secret')
r = req('POST', '/auth/verify', {'challenge': en2['challenge'], 'code': totp2.now()})
ok(r.status_code == 200 and 'recovery_codes' not in r.json(), 'new phone confirmed; codes kept (no new set while some are left)')
owner = r.json()['token']
r = req('POST', '/auth/login', {'email': 'filipe@clinica.pt', 'password': PW}); ch = r.json()['challenge']
ok(req('POST', '/auth/recover', {'challenge': ch, 'recovery_code': codes[0]}).status_code == 401, 'a recovery code works once')
# The old phone's codes no longer work.
ok(req('POST', '/auth/verify', {'challenge': ch, 'code': totp.now()}).status_code == 401, "the lost phone's codes stop working")
ok(req('POST', '/auth/recover', {'challenge': str(en2['challenge']), 'recovery_code': codes[1]}).status_code == 401, 'recovery only from the sign-in code step')

# Owner forgot their password: one recovery code resets it.
r = req('POST', '/auth/reset', {'email': 'filipe@clinica.pt', 'code': codes[2], 'password': 'owner new password'}); ok(r.status_code == 200, 'owner resets a forgotten password with a recovery code')
ok(req('GET', '/me/recovery-codes', tok=owner).status_code == 401, 'and is signed out everywhere')
owner = sign_in('filipe@clinica.pt', 'owner new password', totp2); ok(bool(owner), 'owner signs in with the new password')
r = req('GET', '/me/recovery-codes', tok=owner); ok(r.json()['left'] == 8, '8 codes left')
ok(req('POST', '/me/recovery-codes', {'password': 'wrong'}, owner).status_code == 400, 'new codes need the password')
r = req('POST', '/me/recovery-codes', {'password': 'owner new password'}, owner); new_codes = r.json()['codes']
ok(len(new_codes) == 10 and req('GET', '/me/recovery-codes', tok=owner).json()['left'] == 10, 'owner makes 10 new codes')
r = req('POST', '/auth/reset', {'email': 'filipe@clinica.pt', 'code': codes[3], 'password': 'should not work!!'}); ok(r.status_code == 400, 'old codes stop working')

# ── Invites: list and withdraw ──
r = req('POST', '/invites', {'role': 'therapist', 'email': 'rui@clinica.pt'}, owner); icode = r.json()['code']
inv = req('GET', '/invites', tok=owner).json(); ok(len(inv) == 1 and inv[0]['email'] == 'rui@clinica.pt' and 'code' not in inv[0] and 'code_hash' not in inv[0], 'owner lists unused invites (no codes shown)')
ok(req('GET', '/invites', tok=ther).status_code == 403, 'therapist cannot list invites')
ok(req('DELETE', f"/invites/{inv[0]['id']}", tok=owner).status_code == 200 and req('GET', '/invites', tok=owner).json() == [], 'owner withdraws the invite')
r = req('POST', '/auth/signup', {'invite': icode, 'name': 'Rui', 'email': 'rui@clinica.pt', 'password': 'another long password'}); ok(r.status_code == 400, 'a withdrawn invite code stops working')
ok(req('DELETE', f"/invites/{inv[0]['id']}", tok=owner).status_code == 409, 'withdrawing twice says so')

# ── Clinic details ──
r = req('GET', '/clinic', tok=ther); ok(r.status_code == 200 and r.json()['name'] == 'Clínica Teste' and r.json()['logo'] is None, 'everyone can read the clinic details')
base = {'name': 'Clínica Fisio Lisboa', 'nif': '501 442 600', 'ers_number': 'E123456', 'address': 'Rua das Flores, 10\n1200-001 Lisboa', 'phone': '+351 210 000 000', 'email': 'Geral@Fisio.pt'}
ok(req('PUT', '/clinic', base, ther).status_code == 403, 'only the owner changes them (server)')
ok(req('PUT', '/clinic', {**base, 'nif': '501442601'}, owner).status_code == 400, 'wrong NIF check digit refused')
ok(req('PUT', '/clinic', {**base, 'name': ' '}, owner).status_code == 400, 'name required')
ok(req('PUT', '/clinic', {**base, 'email': 'not-an-email'}, owner).status_code == 400, 'bad email refused')
ok(req('PUT', '/clinic', {**base, 'logo': 'data:image/png;base64,iVBORw0KGgo='}, owner).status_code == 400, 'logo must be a real JPEG')
from PIL import Image
buf = io.BytesIO(); Image.new('RGB', (240, 80), (40, 120, 200)).save(buf, 'JPEG'); import base64
logo = 'data:image/jpeg;base64,' + base64.b64encode(buf.getvalue()).decode()
r = req('PUT', '/clinic', {**base, 'logo': logo}, owner)
ok(r.status_code == 200 and r.json()['nif'] == '501442600' and r.json()['email'] == 'geral@fisio.pt' and r.json()['logo'].startswith('data:image/jpeg;base64,/9j/'), 'owner saves details + logo (NIF tidied, email lower-case)')
ok(req('GET', '/status').json()['clinic'] == 'Clínica Fisio Lisboa', 'the new name shows on the sign-in screen')
r = req('PUT', '/clinic', {**base}, owner); ok(r.json()['logo'] is not None, 'saving without "logo" keeps it')

# A signed note PDF carries the details, and leaves out scales nobody set.
pid = req('POST', '/patients', {'name': 'Maria Santos', 'birth_date': '1980-05-02'}, ther).json()['id']
blocks = [{'type': 'slider', 'key': 'pain', 'label': 'Dor (0–10)', 'value': None, 'min': 0, 'max': 10},
          {'type': 'slider', 'key': 'rotation', 'label': 'Rotação cervical', 'value': 45, 'min': 0, 'max': 90, 'unit': '°'},
          {'type': 'slider', 'key': 'function', 'label': 'Função (0–10)', 'value': None, 'min': 0, 'max': 10}]
note = req('POST', f'/patients/{pid}/notes', {'template': 'neck', 'blocks': blocks, 'text': 'Só uma frase.'}, ther).json()['id']
f = req('GET', f'/patients/{pid}', tok=ther).json()
d = [n for n in f['notes'] if n['id'] == note][0]; ok(sum(1 for b in d['blocks'] if b['value'] is None) == 2, 'a draft keeps unset scales (shown as —)')
r = req('POST', f'/notes/{note}/sign', tok=ther); doc = r.json().get('document_id')
f = req('GET', f'/patients/{pid}', tok=ther).json()
s = [n for n in f['notes'] if n['id'] == note][0]
ok([b['key'] for b in s['blocks']] == ['rotation'], 'signed note keeps only the scale that was set (no invented "Dor 5")')
txt = pdf_text(doc, ther)
if txt is not None:
    ok('NIF 501442600' in txt and 'ERS E123456' in txt and 'Rua das Flores, 10, 1200-001 Lisboa' in txt and 'geral@fisio.pt' in txt, 'note PDF header: name, NIF, ERS, address, contacts')
    ok('Dor' not in txt and 'Função' not in txt and 'Rotação cervical' in txt, 'note PDF leaves out the unset scales')
    r = requests.get(B + f'/documents/{doc}/raw', headers={'Authorization': 'Bearer ' + ther, 'X-Device-Key': DEVICE_KEY}); ok(b'/Subtype /Image' in r.content and b'/DCTDecode' in r.content, 'note PDF has the logo')
r = req('PUT', '/clinic', {**base, 'logo': None}, owner); ok(r.json()['logo'] is None, 'logo removed with null')

# ── Server and backups ──
ok(req('GET', '/system/status', tok=ther).status_code == 403 and req('POST', '/system/backup', tok=ther).status_code == 403, 'server status and Back Up Now are owner only')
st = req('GET', '/system/status', tok=owner).json()
ok(st['version'] and st['database'].startswith('PostgreSQL') and st['backup']['state'] == 'unknown', 'no status file yet → "unknown" (never a made-up time)')
ok(req('GET', '/alerts', tok=owner).json()['backup']['state'] == 'unknown' and req('GET', '/alerts', tok=ther).json()['backup'] is None, 'Today alert for the owner only')
ok(req('POST', '/system/backup', tok=owner).status_code == 409, 'Back Up Now says so when the backup service isn’t there')
if CONTROL:
    def write(st_): open(os.path.join(CONTROL, 'status.json'), 'w').write(json.dumps(st_))
    iso = lambda h: time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime(time.time() - h * 3600))
    write({'last_ok': iso(2), 'db_file': 'db-20261110-030000.dump.age', 'files_file': 'files-20261110-030000.tar.age', 'bytes': 12345, 'offsite': 'off', 'kept_days': 30, 'running': False, 'next_at': iso(-20)})
    st = req('GET', '/system/status', tok=owner).json()
    ok(st['backup']['state'] == 'ok' and st['backup']['db_file'] == 'db-20261110-030000.dump.age' and st['address'] == '100.68.180.126:8095', 'real status: last backup 2 h ago, address from the server')
    ok(req('GET', '/alerts', tok=owner).json()['backup'] is None, 'no alert when the backup is recent')
    write({'last_ok': iso(40), 'db_file': 'db-x.dump.age'}); ok(req('GET', '/system/status', tok=owner).json()['backup']['state'] == 'old' and req('GET', '/alerts', tok=owner).json()['backup']['state'] == 'old', 'older than 36 h → "old" + Today alert')
    write({'last_ok': iso(30), 'last_error': iso(1)}); ok(req('GET', '/system/status', tok=owner).json()['backup']['state'] == 'failed', 'last run failed → "failed"')
    write({'last_ok': iso(1), 'db_file': '../../etc/passwd'}); ok(req('GET', '/system/status', tok=owner).json()['backup']['db_file'] is None, 'odd file names are not passed on')
    r = req('POST', '/system/backup', tok=owner); ok(r.status_code == 200 and os.path.exists(os.path.join(CONTROL, 'request')) and r.json()['backup']['requested'], 'Back Up Now leaves a request for the backup service')
    os.remove(os.path.join(CONTROL, 'request'))

# ── Access log ──
acts = {x['action'] for x in req('GET', '/access-log?limit=500', tok=owner).json()}
want = {'account.recovery_codes_made', 'account.name_changed', 'account.password_change_failed', 'account.password_changed', 'staff.reset_code_made',
        'account.password_reset', 'account.password_reset_failed', 'staff.two_step_reset', 'login.recovery_code_used', 'login.bad_recovery_code',
        'invite.revoked', 'clinic.updated', 'note.signed'}
ok(want <= acts, 'access log has every step: ' + ', '.join(sorted(want - acts) or ['all']))
if CONTROL: ok('system.backup_requested' in acts, 'access log: backup requested')
if LOG:
    text = open(LOG, encoding='utf-8', errors='replace').read()
    ok(not any(c in text for c in codes + new_codes + [rc, rc1, rc2]) and 'Maria Santos' not in text, 'no codes or patient names in the server log')

print(f'\n{"ALL PASSED" if not fails else f"{fails} FAILED"}')
raise SystemExit(1 if fails else 0)
