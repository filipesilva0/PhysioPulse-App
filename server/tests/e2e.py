# End-to-end test of the phase 1 API against a running server with an EMPTY database.
# Run: pip install requests pyotp && python3 server/tests/e2e.py   (BASE env var overrides the URL)
# Takes ~40 s because it waits for a fresh 30-second code window to test replay protection.
import requests, pyotp, time
import os
# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B=os.environ.get('BASE','http://127.0.0.1:8080')+'/api'; H={'X-Device': 'MacBook-Filipe', 'X-Device-Key': DEVICE_KEY}
ok=lambda c,m: print(('PASS ' if c else 'FAIL ')+m)
def post(p,j,tok=None): 
    h=dict(H); 
    if tok: h['Authorization']='Bearer '+tok
    return requests.post(B+p,json=j,headers=h)
def get(p,tok=None):
    h=dict(H)
    if tok: h['Authorization']='Bearer '+tok
    return requests.get(B+p,headers=h)
def wait_new_step():
    # each code can only be used once per 30 s step
    time.sleep(30 - time.time()%30 + 0.5)

r=post('/setup',{'clinic_name':'Clínica Teste','name':'Filipe Silva','email':'Filipe@Clinica.pt','password':'short'}); ok(r.status_code==400,'setup rejects short password')
r=post('/setup',{'clinic_name':'Clínica Teste','name':'Filipe Silva','email':'Filipe@Clinica.pt','password':'correct horse battery'}); ok(r.status_code==200 and 'qr_svg' in r.json(),'owner setup returns QR + secret')
enrol=r.json(); totp=pyotp.TOTP(enrol['secret'])
ok(enrol['otpauth_uri'].startswith('otpauth://totp/'),'otpauth URI '+enrol['otpauth_uri'][:40]+'…')
r=post('/setup',{'clinic_name':'X','name':'Evil','email':'evil@x.pt','password':'correct horse battery'}); ok(r.status_code==409,'second setup refused')
r=post('/auth/verify',{'challenge':enrol['challenge'],'code':'000000'}); ok(r.status_code==401,'wrong code refused')
r=post('/auth/verify',{'challenge':enrol['challenge'],'code':totp.now()}); ok(r.status_code==200,'enrol confirmed with real code')
owner=r.json()['token']; ok(r.json()['staff']['role']=='owner' and 'password_hash' not in r.json()['staff'],'session issued, no secrets leaked')
r=get('/me',owner); ok(r.status_code==200 and r.json()['email']=='filipe@clinica.pt','/me works, email normalised')
r=get('/me'); ok(r.status_code==401,'/me without token refused')
r=get('/me','deadbeef'); ok(r.status_code==401,'/me with fake token refused')
# invite a therapist
r=post('/invites',{'role':'therapist','email':'ana@clinica.pt'},owner); code=r.json()['code']; ok(r.status_code==200 and len(code)==14,'owner creates invite '+code)
r=post('/auth/signup',{'invite':'AAAA-BBBB-CCCC','name':'Ana','email':'ana@clinica.pt','password':'another long password'}); ok(r.status_code==400,'bad invite refused')
r=post('/auth/signup',{'invite':code.lower(),'name':'Ana','email':'other@clinica.pt','password':'another long password'}); ok(r.status_code==400,'invite locked to its email')
r=post('/auth/signup',{'invite':code.lower(),'name':'Ana Rocha','email':'ana@clinica.pt','password':'another long password'}); ok(r.status_code==200,'therapist signs up with invite')
ana_en=r.json(); ana_totp=pyotp.TOTP(ana_en['secret'])
r=post('/auth/signup',{'invite':code,'name':'Ana2','email':'ana2@clinica.pt','password':'another long password'}); ok(r.status_code==400,'invite cannot be reused')
r=post('/auth/verify',{'challenge':ana_en['challenge'],'code':ana_totp.now()}); ana=r.json()['token']; ok(r.status_code==200,'therapist two-step set up')
# roles
ok(get('/staff',ana).status_code==403,'therapist cannot list staff (role enforced on server)')
ok(post('/invites',{'role':'owner'},ana).status_code==403,'therapist cannot create invites')
ok(get('/access-log',ana).status_code==403,'therapist cannot read access log')
ok(get('/staff',owner).status_code==200 and len(get('/staff',owner).json())==2,'owner lists 2 staff')
# login flow with code + replay protection
r=post('/auth/login',{'email':'filipe@clinica.pt','password':'wrong'}); ok(r.status_code==401 and 'wrong' in r.json()['message'],'wrong password: generic message')
r=post('/auth/login',{'email':'nobody@clinica.pt','password':'wrong'}); ok(r.status_code==401 and r.json()['message']=='The email or password is wrong.','unknown email: same message')
wait_new_step()
r=post('/auth/login',{'email':'filipe@clinica.pt','password':'correct horse battery'}); ch=r.json()['challenge']; ok(r.json()['next']=='code','password ok → asks for code')
c=totp.now(); r=post('/auth/verify',{'challenge':ch,'code':c}); ok(r.status_code==200,'sign-in with code'); tok2=r.json()['token']
r=post('/auth/login',{'email':'filipe@clinica.pt','password':'correct horse battery'}); ch=r.json()['challenge']
r=post('/auth/verify',{'challenge':ch,'code':c}); ok(r.status_code==401,'same code cannot be reused (replay blocked)')
# logout
ok(post('/auth/logout',{},tok2).status_code==200 and get('/me',tok2).status_code==401,'sign out ends the session')
# deactivate therapist -> sessions gone
tid=[s for s in get('/staff',owner).json() if s['role']=='therapist'][0]['id']
ok(requests.patch(B+'/staff/'+tid,json={'active':False},headers={**H,'Authorization':'Bearer '+owner}).status_code==200 and get('/me',ana).status_code==401,'deactivating signs therapist out everywhere')
r=post('/auth/login',{'email':'ana@clinica.pt','password':'another long password'}); ok(r.status_code==401,'deactivated account cannot sign in')
# rate limit
for i in range(5): post('/auth/login',{'email':'filipe@clinica.pt','password':'bad'})
r=post('/auth/login',{'email':'filipe@clinica.pt','password':'correct horse battery'}); ok(r.status_code==429,'locked after 5 failures (429)')
# access log
log=get('/access-log?limit=50',owner).json(); acts=[l['action'] for l in log]
ok('login.failed' in acts and 'staff.deactivated' in acts and 'invite.created' in acts,'access log records: '+', '.join(sorted(set(acts))))
