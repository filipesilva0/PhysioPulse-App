# PhysioPulse plan, phase 4a: the public website (server/portal) and Settings › Website.
# Needs, on fresh databases:
#   the staff server on BASE (default http://127.0.0.1:8080) started with SECRET_KEY=<64 hex> and PORTAL_DB_PASSWORD=<16+ letters/digits>
#   the website on SITE (default http://127.0.0.1:8090) started with DATABASE_URL=postgres://physiopulse_portal:<that password>@…/physiopulse
#   PSQL_URL = the main database login, PORTAL_URL = the website's login (same as the website's DATABASE_URL)
#   python3 server/tests/e2e_website.py
import os, re, subprocess, time, secrets, datetime
import requests, pyotp
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
SITE = os.environ.get('SITE', 'http://127.0.0.1:8090')
PSQL, PORTAL = os.environ['PSQL_URL'], os.environ['PORTAL_URL']
fails = 0
def ok(c, m):
    global fails
    print(('PASS ' if c else 'FAIL ') + m); fails += 0 if c else 1
DEV = secrets.token_hex(32)
def req(method, p, j=None, tok=None):
    h = {'X-Device': 'PC', 'X-Device-Key': DEV}
    if tok: h['Authorization'] = 'Bearer ' + tok
    return requests.request(method, B + p, json=j, headers=h)
def sql(q, url=PSQL):
    r = subprocess.run(['psql', url, '-v', 'ON_ERROR_STOP=1', '-tAc', q], capture_output=True, text=True)
    return (r.stdout.strip(), r.stderr.strip())
def site(p, **kw): return requests.get(SITE + p, allow_redirects=False, **kw)
def token(html):
    t = re.search(r'name="t" value="(\d+)"', html); s = re.search(r'name="s" value="([0-9a-f]+)"', html)
    return t.group(1), s.group(1)
def book(path='/marcar', ip='198.51.100.1', wait=True, **fields):
    t, s = token(site(path).text)
    if wait: time.sleep(3.2)
    data = {'service': 'assessment', 'name': 'Ana Lopes', 'phone': '912 345 678', 'email': '', 'day': tomorrow, 'part': 'morning',
            'address': '', 'message': '', 'privacy': 'on', 't': t, 's': s, 'website': ''}
    data.update(fields)
    return requests.post(SITE + path, data=data, allow_redirects=False, headers={'X-Forwarded-For': ip})
lisbon_today = datetime.datetime.now(datetime.timezone.utc).date()
tomorrow = (lisbon_today + datetime.timedelta(days=2)).isoformat()

PW = 'correct horse battery'
# ── Owner ──
r = req('POST', '/setup', {'clinic_name': 'PhysioPulse', 'name': 'Filipe Silva', 'email': 'filipe@clinica.pt', 'password': PW})
en = r.json(); totp = pyotp.TOTP(en['secret'])
r = req('POST', '/auth/verify', {'challenge': en['challenge'], 'code': totp.now(), 'trust_device': True}); owner = r.json()['token']
ok(bool(owner), 'owner signed in')

# ── The website's database login sees only the portal views ──
out, _ = sql("SELECT rolcanlogin, rolsuper, rolcreaterole FROM pg_roles WHERE rolname = 'physiopulse_portal'")
ok(out == 't|f|f', 'the website has its own login, not a superuser')
for t in ['patients', 'staff', 'sessions', 'payments', 'booking_requests', 'access_log', 'clinic', 'website', 'products', 'notes']:
    _, err = sql(f'SELECT 1 FROM public.{t} LIMIT 1', PORTAL)
    ok('permission denied' in err, f'the website cannot read {t}')
_, err = sql("INSERT INTO booking_requests (id, name, preferred_at) VALUES (gen_random_uuid(), 'x', now())", PORTAL); ok('permission denied' in err, 'nor write booking requests directly')
_, err = sql("UPDATE portal.site SET published = true", PORTAL); ok(err != '', 'nor change the site settings through its view')
_, err = sql("INSERT INTO access_log (action) VALUES ('x')", PORTAL); ok('permission denied' in err, 'nor write the access log')
_, err = sql("CREATE TABLE portal.x (a int)", PORTAL); ok('permission denied' in err, 'nor create tables')
out, err = sql("SELECT clinic_name FROM portal.site", PORTAL); ok(out == 'PhysioPulse' and not err, 'it reads the clinic name through portal.site')
out, _ = sql("SELECT string_agg(column_name, ',') FROM information_schema.columns WHERE table_schema = 'portal' AND table_name = 'team'")
ok('email' not in out and 'password' not in out, 'the team view has no email or password column')

# ── Not published yet: "coming soon" everywhere, hidden from search engines, no requests ──
r = site('/'); ok(r.status_code == 200 and 'está a ser preparado' in r.text and r.headers.get('x-robots-tag') == 'noindex', 'unpublished: coming-soon page, noindex')
r = site('/en/services'); ok('being prepared' in r.text, 'unpublished, in English too')
ok('Disallow: /\n' in site('/robots.txt').text, 'robots.txt keeps search engines away')
r = site('/privacidade'); ok('90 dias' in r.text and r.headers.get('x-robots-tag') == 'noindex', 'unpublished: the privacy policy can be read (to review it), not indexed')
ok(site('/health').json().get('ok') is True, 'website /health answers')
out, err = sql(f"SELECT portal.request_booking('assessment', 'Ana', '912345678', '', '{tomorrow}', 'any', '', '', 'pt')", PORTAL); ok('closed' in err, 'the database refuses requests while unpublished')

# ── Settings › Website (owner only) ──
r = req('GET', '/website', tok=owner); w = r.json()
ok(r.status_code == 200 and w['site']['published'] is False and w['site']['tagline_pt'] and len(w['products']) >= 10, 'owner reads the website settings (texts drafted, price list)')
ok(len(w['team']) == 1 and w['team'][0]['web_visible'] is False, 'the team starts hidden from the site')
r = req('PUT', '/website', {'published': True}, owner); ok(r.status_code == 400 and 'NIF' in r.json()['message'], "can't publish without the clinic's NIF and a contact")
r = req('PUT', '/clinic', {'name': 'PhysioPulse', 'nif': '123456789', 'ers_number': 'E123456', 'address': 'Rua das Flores 1, Lisboa', 'phone': '213 456 789', 'email': 'geral@physiopulse.pt'}, owner)
ok(r.status_code == 200, 'clinic details filled in')
r = req('PUT', '/website', {'tagline_pt': 'Fisio <b>já</b>'}, owner); ok(r.status_code == 400, 'texts are plain (no < or >)')
r = req('PUT', '/website', {'tagline_pt': 'x' * 91}, owner); ok(r.status_code == 400, 'texts have a length limit')
r = req('PUT', '/website', {'published': True, 'tagline_pt': 'Fisioterapia com tempo para si', 'intro_pt': 'Primeiro parágrafo.\n\nSegundo & último.'}, owner)
ok(r.status_code == 200 and r.json()['site']['published'] is True, 'owner publishes the site')
out, _ = sql("SELECT action, detail FROM access_log WHERE action LIKE 'website.%' ORDER BY id DESC LIMIT 1"); ok(out.startswith('website.published|'), 'publishing is in the access log')

# Reception can't touch it
code = req('POST', '/invites', {'role': 'reception', 'email': 'rita@clinica.pt'}, owner).json()['code']
e = req('POST', '/auth/signup', {'invite': code, 'name': 'Rita', 'email': 'rita@clinica.pt', 'password': 'another long password'}).json()
r = req('POST', '/auth/verify', {'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}); rita = r.json().get('token')
ok(rita and req('GET', '/website', tok=rita).status_code == 403, 'reception can’t open Settings › Website')
ok(req('PUT', '/website', {'published': False}, rita).status_code == 403, 'nor change it')
ok(req('PUT', f"/website/team/{w['team'][0]['id']}", {'web_visible': True}, rita).status_code == 403, 'nor the team page')

# ── Published pages ──
r = site('/'); h = r.text
ok(r.status_code == 200 and 'Fisioterapia com tempo para si' in h and 'x-robots-tag' not in r.headers, 'home page published and indexable')
ok('<p>Primeiro parágrafo.</p><p>Segundo &amp; último.</p>' in h, "the owner's text: paragraphs, escaped")
ok('30,00 €' in h and 'Abertura em breve' in h, 'home: the assessment price from the app, "opening soon"')
ok('Rua das Flores' not in h, 'the address stays hidden while "opening soon" is on')
ok('livroreclamacoes.pt' in h and 'NIF 123456789' in h and 'E123456' in h, 'footer: complaints book, NIF, ERS registration')
ok('<script' not in h.lower(), 'no scripts on the page')
ok('set-cookie' not in {k.lower() for k in r.headers}, 'no cookies')
csp = r.headers.get('content-security-policy', ''); ok("default-src 'none'" in csp and "form-action 'self'" in csp, 'strict Content-Security-Policy')
ok(r.headers.get('referrer-policy') == 'no-referrer', 'no referrer sent to other sites')
r = site('/servicos'); ok('Pacote de 10' in r.text and '170,00 €' in r.text and 'isento de IVA' in r.text and 'IVA incluído' in r.text, 'services and prices, VAT marks')
r = site('/en/services'); ok('Pack of 10' in r.text and '€170.00' in r.text and 'VAT exempt' in r.text, 'the same in English')
ok('hreflang="pt"' in r.text and 'href="/servicos"' in r.text, 'language switch points to the same page in Portuguese')
prods = {p['name_pt']: p['id'] for p in w['products']}
req('PATCH', f"/products/{prods['Avaliação inicial (60 min)']}", {'price_cents': 3500}, owner)
ok('35,00 €' in site('/').text, 'a price changed in the app is on the site at once')
req('PUT', f"/website/products/{prods['Aulas, 2 por semana']}", {'web_visible': False}, owner)
r = site('/servicos'); ok('Aulas, 2 por semana' not in r.text and 'Cartão de 10 aulas' in r.text, 'an item hidden from the site disappears from the price list')
req('PUT', '/website', {'show_prices': False}, owner)
r = site('/servicos'); ok('170,00' not in r.text and 'Pacote de 10' in r.text, 'prices can be hidden (services still listed)')
req('PUT', '/website', {'show_prices': True}, owner)

# Team
me = w['team'][0]['id']
r = req('PUT', f'/website/team/{me}', {'web_visible': True, 'web_title_pt': 'Fisioterapeuta e diretor clínico', 'web_bio_pt': 'Especialista em <ombro>'}, owner); ok(r.status_code == 400, 'bios are plain text')
r = req('PUT', f'/website/team/{me}', {'web_visible': True, 'web_title_pt': 'Fisioterapeuta e diretor clínico', 'web_bio_pt': 'Especialista em ombro e coluna.'}, owner); ok(r.status_code == 200, 'owner shows himself on the team page')
r = site('/equipa'); ok('Filipe Silva' in r.text and 'diretor clínico' in r.text and 'ombro e coluna' in r.text and 'filipe@clinica.pt' not in r.text, 'team page: name, title, bio — no email')
r = req('PUT', f'/website/team/{w["team"][0]["id"]}', {'web_visible': False}, owner)
ok('Filipe Silva' not in site('/equipa').text, 'hidden again')
rita_id = [s for s in req('GET', '/staff', tok=owner).json() if s['email'] == 'rita@clinica.pt'][0]['id']
ok(req('PUT', f'/website/team/{rita_id}', {'web_visible': True}, owner).status_code == 404, 'reception never appears on the team page')

# Legal pages
r = site('/privacidade'); ok('90 dias' in r.text and 'não usam cookies' in r.text and 'único cookie' in r.text and 'cnpd.pt' in r.text and '123456789' in r.text, 'privacy policy: retention, no cookies on public pages (one in the patient area), CNPD, who the clinic is')
r = site('/en/terms'); ok('only booked once the clinic confirms' in r.text, 'terms in English')
r = site('/conta'); ok('Em breve' in r.text, 'patient area: coming soon (phase 4b)')
r = site('/nao-existe'); ok(r.status_code == 404 and 'Página não encontrada' in r.text, '404 page')
r = site('/downtime.html'); ok('axolote' in r.text and '213 456 789' in r.text and "style-src 'unsafe-inline'" in r.headers.get('content-security-policy', ''), 'downtime page with the axolotl and the phone')
ok('Disallow: /conta' in site('/robots.txt').text, 'robots.txt open once published')
r = site('/sw.js'); ok(r.status_code == 200 and 'unregister' in r.text and 'javascript' in r.headers.get('content-type', ''), "/sw.js removes the old site's service worker")

# ── Requesting an appointment ──
r = book(wait=False); ok(r.status_code == 422 and 'depressa demais' in r.text, 'a form sent within 3 seconds is refused (robots)')
t, s = token(site('/marcar').text)
r = requests.post(SITE + '/marcar', data={'service': 'assessment', 'name': 'Ana', 'phone': '912345678', 'day': tomorrow, 'part': 'any', 'privacy': 'on', 't': t, 's': 'f' * 64})
ok(r.status_code == 422, 'a forged form token is refused')
r = book(name='', phone='', privacy='')
ok(r.status_code == 422 and 'Escreva o seu nome' in r.text and 'telemóvel ou um email' in r.text and 'política de privacidade' in r.text, 'missing details: one sentence per field')
ok('aria-invalid="true"' in r.text and 'href="#f-name"' in r.text and 'role="alert"' in r.text, 'errors are linked to the fields, announced')
r = book(name='"><script>alert(1)</script>', phone='')
ok('<script>alert' not in r.text and '&quot;&gt;&lt;script&gt;' in r.text, 'what was typed is shown back escaped')
r = book(day=lisbon_today.isoformat()); ok(r.status_code == 422 and 'a partir de amanhã' in r.text, 'not today')
r = book(service='home'); ok(r.status_code == 422 and 'escreva a morada' in r.text, 'a home visit needs the address')
before, _ = sql("SELECT count(*) FROM booking_requests")
r = book(website='http://spam.example'); ok(r.status_code == 303, 'robot that fills the hidden field: looks sent…')
after, _ = sql("SELECT count(*) FROM booking_requests"); ok(before == after, '…but nothing is stored')
r = book(message='Dor no ombro há 2 semanas.', email='Ana@Example.PT')
ok(r.status_code == 303 and r.headers['location'] == '/marcar?enviado=1', 'request sent, redirected to the thank-you page')
ok('Pedido enviado' in site('/marcar?enviado=1').text, 'thank-you page')
r = book('/en/book', ip='198.51.100.2', service='home', name='John Smith', phone='', email='john@example.com', address='Rua A 10, Cascais', part='afternoon')
ok(r.status_code == 303 and r.headers['location'] == '/en/book?sent=1', 'English home-visit request sent')
reqs = req('GET', '/booking-requests', tok=owner).json()
web = [x for x in reqs if x.get('source') == 'website']
ana = [x for x in web if x['name'] == 'Ana Lopes']; john = [x for x in web if x['name'] == 'John Smith']
ok(len(ana) == 1 and ana[0]['service'] == 'assessment' and ana[0]['day_part'] == 'morning' and ana[0]['email'] == 'ana@example.pt' and ana[0]['lang'] == 'pt', 'it is in Schedule › Booking Requests (service, time of day, email tidied)')
ok(ana[0]['preferred_at'][:10] == tomorrow and ana[0]['preferred_at'][11:16] in ('09:00', '08:00'), 'morning = 09:00 Lisbon time')
ok(len(john) == 1 and john[0]['service'] == 'home' and john[0]['address'] == 'Rua A 10, Cascais' and john[0]['lang'] == 'en', 'home visit: address kept, English')
ok(len(req('GET', '/booking-requests', tok=rita).json()) >= 2, 'reception sees the requests too (as before)')
out, _ = sql("SELECT staff_id IS NULL, device, detail FROM access_log WHERE action = 'booking_request.received' ORDER BY id")
rows = out.splitlines(); ok(len(rows) == 2 and rows[0].startswith('t|site|') and 'Ana' not in out, 'each request is in the access log, without the person’s details')
r = req('GET', '/alerts', tok=owner); ok(r.json().get('booking_requests') == 2, 'Today shows the two requests')
r = req('POST', f"/booking-requests/{ana[0]['id']}/decline", tok=owner); ok(r.status_code == 200, 'staff decline a website request')
r = req('POST', f"/booking-requests/{ana[0]['id']}/reopen", tok=owner); ok(r.status_code == 200, '…and undo')

# Rate limit: 5 an hour from one address
t, s = token(site('/marcar').text); time.sleep(3.2)
codes = []
for i in range(6):
    data = {'service': 'session', 'name': f'Pessoa {i}', 'phone': '912345678', 'day': tomorrow, 'part': 'any', 'privacy': 'on', 't': t, 's': s}
    codes.append(requests.post(SITE + '/marcar', data=data, allow_redirects=False, headers={'X-Forwarded-For': '203.0.113.51'}).status_code)
ok(codes[:5] == [303] * 5 and codes[5] == 429, f'six requests in an hour from one address: the sixth is refused ({codes})')
out, _ = sql("SELECT count(*) FROM portal.hits WHERE key LIKE '%203.0.113%'"); ok(out == '0', 'the visitor address itself is never stored (only a hash)')
r = requests.post(SITE + '/marcar', data={'service': 'session', 'name': 'Outra', 'phone': '912345678', 'day': tomorrow, 'part': 'any', 'privacy': 'on', 't': t, 's': s}, allow_redirects=False, headers={'X-Forwarded-For': '203.0.113.52'})
ok(r.status_code == 303, 'another address can still send')

# Switches
req('PUT', '/website', {'home_visits': False}, owner)
r = site('/marcar'); ok('value="home"' not in r.text, 'home visits off: not offered in the form')
ok('Ao domicílio' not in site('/').text and 'Domicílio (60 min)' not in site('/servicos').text, '…nor on the home page or price list')
out, err = sql(f"SELECT portal.request_booking('home', 'Ana', '912345678', '', '{tomorrow}', 'any', 'Rua X 1', '', 'pt')", PORTAL); ok('bad_service' in err, 'the database refuses a home visit too')
req('PUT', '/website', {'requests_open': False}, owner)
r = site('/marcar'); ok('não recebemos pedidos' in r.text and '<form' not in r.text, 'requests closed: no form, contacts instead')
ok('Pedir marcação' not in site('/').text, 'no "Request an Appointment" button anywhere')
req('PUT', '/website', {'requests_open': True, 'home_visits': True, 'opening_soon': False}, owner)
ok('Rua das Flores 1, Lisboa' in site('/').text, '"opening soon" off: the address shows')
out, err = sql(f"SELECT portal.request_booking('assessment', 'A', '912345678', '', '{tomorrow}', 'any', '', '', 'pt')", PORTAL); ok('bad_name' in err, 'the database checks the name again')
out, err = sql(f"SELECT portal.request_booking('assessment', 'Ana', '', '', '{tomorrow}', 'any', '', '', 'pt')", PORTAL); ok('no_contact' in err, 'and that there is a contact')
far = (lisbon_today + datetime.timedelta(days=90)).isoformat()
out, err = sql(f"SELECT portal.request_booking('assessment', 'Ana', '912345678', '', '{far}', 'any', '', '', 'pt')", PORTAL); ok('bad_day' in err, 'and the day')
req('PUT', '/website', {'published': False}, owner)
ok('está a ser preparado' in site('/marcar').text, 'unpublished again: coming soon')
out, _ = sql("SELECT action FROM access_log WHERE action LIKE 'website.%' ORDER BY id DESC LIMIT 1"); ok(out == 'website.unpublished', 'unpublishing is logged')

print(f"\n{'ALL PASS' if fails == 0 else f'{fails} FAILED'}")
raise SystemExit(1 if fails else 0)
