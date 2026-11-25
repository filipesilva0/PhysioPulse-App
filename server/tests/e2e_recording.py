# End-to-end test of the PhysioPulse plan, phase 2: session recording, speech-to-text, the AI report, the
# per-person AI switch, and deletion (signed, by hand, after 30 days, with the patient or the day).
# Needs a running server with an EMPTY database, started with
#   OLLAMA_URL=http://127.0.0.1:11995 WHISPER_URL=http://127.0.0.1:11996 AUDIO_DIR=/tmp/audio RECORDING_POLL_SECS=1
# (this test runs stand-ins for Ollama and the whisper server there).
# AUDIO = the server's AUDIO_DIR; PSQL_URL adds database checks; LOG = the server's log (must hold no speech).
# Run: AUDIO=/tmp/audio PSQL_URL=postgres://… LOG=server.log python3 server/tests/e2e_recording.py
import os, sys, json, math, time, struct, base64, threading, subprocess, requests, pyotp
from datetime import date
from http.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer

# Phase 3: every request carries the app's device key; the clinic's first computer is approved at setup.
DEVICE_KEY = os.environ.get('DEVICE_KEY', 'a1' * 32)
B = os.environ.get('BASE', 'http://127.0.0.1:8080') + '/api'
AUDIO = os.environ.get('AUDIO', '/tmp/audio')
PSQL = os.environ.get('PSQL_URL')
LOG = os.environ.get('LOG')
H = {'X-Device': 'Gabinete-2', 'X-Device-Key': DEVICE_KEY}
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

TRANSCRIPT_WORDS = 'ombro esquerdo dói menos quando penteio o cabelo'
# ── stand-ins ──
state = {'stt': 'up', 'soap': False, 'asked': [], 'heard': []}
class Whisper(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.reply(503 if state['stt'] == 'down' else 200, b'<html>whisper.cpp</html>', 'text/html')
    def do_POST(self):
        n = int(self.headers['Content-Length']); body = self.rfile.read(n)
        lang = body.split(b'name="language"\r\n\r\n')[1].split(b'\r\n')[0].decode()
        fmt = body.split(b'name="response_format"\r\n\r\n')[1].split(b'\r\n')[0].decode()
        wav = body.split(b'Content-Type: audio/wav\r\n\r\n')[1].rsplit(b'\r\n--', 1)[0]
        state['heard'].append({'path': self.path, 'lang': lang, 'fmt': fmt, 'wav': wav, 'ctype': self.headers['Content-Type']})
        out = {'text': 'x', 'segments': [{'text': ' Então, como está esse ombro?'}, {'text': ' O ' + TRANSCRIPT_WORDS + ', mas ainda custa a levantar o braço.'},
               {'text': '[BLANK_AUDIO]'}, {'text': ' A dor agora é uns quatro em dez. Vamos fazer mobilização e ver a rotação.'}]}
        self.reply(200, json.dumps(out).encode(), 'application/json')
    def reply(self, code, data, ctype):
        self.send_response(code); self.send_header('Content-Type', ctype); self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)
class Ollama(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self): self.reply(200, {'models': [{'name': 'gemma4:12b'}]})
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/api/show': return self.reply(200, {'details': {}})
        state['asked'].append(body)
        sysp = body.get('system', '')
        if 'preenches os campos' in sysp or 'fill in the note' in sysp:
            out = json.dumps({'values': {'pain': 4, 'rotation': 72, 'function': 15, 'techniques': ['Mobilização articular', 'Ventosas']},
                              'plan': 'Na próxima sessão progride-se o fortalecimento e reavalia-se a elevação do braço.'})
        elif state['soap']:
            out = 'S: Refere dor 4/10.\nO: rotação limitada.\nA: melhor.\nP: manter.'
        else:
            out = '**O utente** referiu menos dor no ombro esquerdo, agora 4/10, mas ainda dificuldade em elevar o braço.\n\n\n- Fez-se mobilização articular do ombro.'
        self.reply(200, {'model': body.get('model'), 'done': True, 'response': out})
    def reply(self, code, out):
        data = json.dumps(out).encode()
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.send_header('Content-Length', str(len(data))); self.end_headers(); self.wfile.write(data)
for port, h in [(11996, Whisper), (11995, Ollama)]:
    threading.Thread(target=ThreadingHTTPServer(('127.0.0.1', port), h).serve_forever, daemon=True).start()

def pcm(seconds, freq=220):
    return b''.join(struct.pack('<h', int(8000 * math.sin(2 * math.pi * freq * i / 16000))) for i in range(int(16000 * seconds)))
def b64(b): return base64.b64encode(b).decode()
def wait_status(tok, pid, rid, want, secs=20):
    for _ in range(secs * 4):
        recs = call('GET', f'/patients/{pid}/recordings', tok).json().get('recordings', [])
        r = next((x for x in recs if x['id'] == rid), None)
        if (r and r['status'] == want) or (r is None and want is None): return r
        time.sleep(0.25)
    return r
def folder(rid): return os.path.join(AUDIO, rid)

# ── people ──
r = call('POST', '/setup', json={'clinic_name': 'Clínica Teste', 'name': 'Filipe Sousa', 'email': 'filipe@clinica.pt', 'password': 'correct horse battery'})
if r.status_code != 200: sys.exit('needs an EMPTY database: ' + r.text)
e = r.json(); owner = call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
def join(role, email, name):
    code = call('POST', '/invites', owner, {'role': role}).json()['code']
    e = call('POST', '/auth/signup', json={'invite': code, 'name': name, 'email': email, 'password': 'another long password'}).json()
    return call('POST', '/auth/verify', json={'challenge': e['challenge'], 'code': pyotp.TOTP(e['secret']).now()}).json()['token']
ther = join('therapist', 'ana@clinica.pt', 'Ana Rocha')
ther2 = join('therapist', 'bruno@clinica.pt', 'Bruno Dias')
recep = join('reception', 'rita@clinica.pt', 'Rita Lopes')
staff = {s['name']: s for s in call('GET', '/staff', owner).json()}
ana, rita = staff['Ana Rocha']['id'], staff['Rita Lopes']['id']
pid = call('POST', '/patients', recep, {'name': 'Joana Ribeiro', 'therapist_id': ana}).json()['id']
BLOCKS = [{'type': 'slider', 'key': 'pain', 'label': 'Dor (0–10)', 'value': None, 'min': 0, 'max': 10},
          {'type': 'slider', 'key': 'rotation', 'label': 'Rotação cervical', 'value': None, 'min': 0, 'max': 90, 'step': 5, 'unit': '°'},
          {'type': 'slider', 'key': 'function', 'label': 'Função (0–10)', 'value': None, 'min': 0, 'max': 10},
          {'type': 'checklist', 'key': 'techniques', 'label': 'Técnicas usadas', 'value': [], 'options': ['Mobilização articular', 'Tecido mole', 'Taping']}]
START = {'template': 'neck', 'blocks': BLOCKS, 'lang': 'pt'}

# ── the AI switch ──
ok(staff['Filipe Sousa']['ai_enabled'] and not staff['Ana Rocha']['ai_enabled'] and not staff['Rita Lopes']['ai_enabled'], 'AI is on for the owner and off for everyone else by default')
ok(call('GET', '/me', owner).json()['ai_enabled'] is True and call('GET', '/me', ther).json()['ai_enabled'] is False, '/me says whether this person has AI')
for who, tok in [('therapist without AI', ther), ('reception', recep)]:
    codes = [call('POST', f'/patients/{pid}/recordings', tok, START).status_code, call('GET', f'/patients/{pid}/recordings', tok).status_code,
             call('GET', '/ai/status', tok).status_code, call('POST', '/ai/rewrite-note', tok, {'patient_id': pid, 'text': 'x'}).status_code,
             call('POST', f'/patients/{pid}/letters/ai-draft', tok, {'lang': 'pt'}).status_code]
    ok(codes == [403] * 5, f'{who}: recording, recordings, AI status, note write-up and letter draft refused {codes}')
# 11 Nov 2026: reception can have the switch too, but for them it opens only the assistant's schedule jobs.
ok(call('PATCH', f'/staff/{rita}', owner, {'ai_enabled': True}).status_code == 200, 'reception can get the AI switch (assistant only)')
codes = [call('POST', f'/patients/{pid}/recordings', recep, START).status_code, call('GET', f'/patients/{pid}/recordings', recep).status_code,
         call('POST', '/ai/rewrite-note', recep, {'patient_id': pid, 'text': 'x'}).status_code, call('POST', f'/patients/{pid}/letters/ai-draft', recep, {'lang': 'pt'}).status_code]
ok(codes == [403] * 4, f'reception with AI on still can’t record, read recordings, write notes or letters {codes}')
ok(call('GET', '/ai/status', recep).status_code == 200, 'reception with AI on can see whether the AI PC is on')
ok(call('PATCH', f'/staff/{rita}', owner, {'ai_enabled': False}).status_code == 200, 'and the owner turns it off again')
ok(call('PATCH', f'/staff/{ana}', ther, {'ai_enabled': True}).status_code == 403, 'only the owner turns AI on')
ok(call('PATCH', f'/staff/{ana}', owner, {'ai_enabled': True}).status_code == 200, 'the owner turns AI on for a therapist')
ok(call('GET', '/me', ther).json()['ai_enabled'] is True and call('GET', '/ai/status', ther).status_code == 200, 'the therapist now has AI and can see whether it is on')
st = call('GET', '/ai/status', ther).json()
ok(st['available'] and st['speech']['available'] and st['speech']['url'] == 'http://127.0.0.1:11996', 'AI status includes the speech-to-text server')
for m, p in [('POST', '/ai/read-receipt'), ('GET', '/finance/reports'), ('GET', '/ai/settings')]:
    ok(call(m, p, ther, {'file_id': pid, 'message': 'x'}).status_code == 403, f'money and settings AI stay the owner’s: {p}')
if PSQL:
    sql(f"UPDATE staff SET ai_enabled = true WHERE id = '{rita}'")
    sql(f"UPDATE staff SET role = 'reception' WHERE id = '{rita}'")
    ok(sql(f"SELECT ai_enabled FROM staff WHERE id = '{rita}'") == 't', 'the database leaves the switch alone while the role stays')
    sql(f"UPDATE staff SET role = 'therapist' WHERE id = '{rita}'")
    ok(sql(f"SELECT ai_enabled FROM staff WHERE id = '{rita}'") == 'f', 'a change of role turns the switch off')
    sql(f"UPDATE staff SET role = 'reception', ai_enabled = false WHERE id = '{rita}'")
    sql(f"UPDATE staff SET ai_enabled = false WHERE role = 'owner'")
    ok(sql("SELECT bool_and(ai_enabled) FROM staff WHERE role = 'owner'") == 't', 'and on for the owner')

# ── recording in pieces ──
r = call('POST', f'/patients/{pid}/recordings', ther, START); rid = r.json()['id']
ok(r.status_code == 200 and os.path.isdir(folder(rid)), 'a recording starts and gets its own folder')
p0, p1, p2 = pcm(5), pcm(5, 330), pcm(4, 440)
r = call('PUT', f'/recordings/{rid}/chunks/1', ther, {'data': b64(p1)})
ok(r.status_code == 409 and r.json()['error'] == 'chunk_order' and r.json()['details']['expected'] == 0, 'pieces must come in order (says which is expected)')
ok(call('PUT', f'/recordings/{rid}/chunks/0', ther, {'data': b64(p0)}).json() == {'chunks': 1, 'seconds': 5}, 'first piece kept')
ok(call('PUT', f'/recordings/{rid}/chunks/0', ther, {'data': b64(p0)}).json() == {'chunks': 1, 'seconds': 5}, 'the same piece sent twice is kept once')
ok(call('PUT', f'/recordings/{rid}/chunks/1', owner, {'data': b64(p1)}).status_code == 404, 'only the person recording can add to it')
ok(call('PUT', f'/recordings/{rid}/chunks/1', recep, {'data': b64(p1)}).status_code == 403, 'reception can’t')
ok(call('PUT', f'/recordings/{rid}/chunks/1', ther, {'data': 'not base64!'}).status_code == 400, 'unreadable audio refused')
ok(call('PUT', f'/recordings/{rid}/chunks/1', ther, {'data': b64(b'\x01\x02\x03')}).status_code == 400, 'odd-length audio refused')
ok(call('PUT', f'/recordings/{rid}/chunks/1', ther, {'data': b64(p1)}).status_code == 200 and call('PUT', f'/recordings/{rid}/chunks/2', ther, {'data': b64(p2)}).json()['seconds'] == 14, 'more pieces; 14 s so far')
ok(call('GET', f'/recordings/{rid}', ther).status_code == 409 and call('GET', f'/recordings/{rid}/audio', ther).status_code == 409, 'no draft or audio while recording')

# ── waiting for the AI PC ──
state['stt'] = 'down'
r = call('POST', f'/recordings/{rid}/finish', ther)
ok(r.status_code == 200 and r.json() == {'status': 'queued', 'seconds': 14}, 'finish → queued')
wav = open(os.path.join(folder(rid), 'audio.wav'), 'rb').read()
ok(wav[:4] == b'RIFF' and struct.unpack('<I', wav[40:44])[0] == len(p0 + p1 + p2) and wav[44:] == p0 + p1 + p2, 'the pieces are joined into one 16 kHz WAV, in order')
ok(not [f for f in os.listdir(folder(rid)) if f.endswith('.pcm')], 'the pieces are removed once joined')
ok(call('POST', f'/recordings/{rid}/finish', ther).json()['status'] == 'queued', 'finishing twice is harmless')
ok(call('PUT', f'/recordings/{rid}/chunks/3', ther, {'data': b64(p2)}).status_code == 409, 'no more pieces after finishing')
time.sleep(3)
recs = call('GET', f'/patients/{pid}/recordings', ther).json()
ok(recs['recordings'][0]['status'] == 'queued' and not state['heard'] and recs['speech']['available'] is False, 'with the AI PC away it waits in the queue (and says so)')

# ── transcription and draft ──
state['stt'] = 'up'
rec = wait_status(ther, pid, rid, 'ready')
ok(rec and rec['status'] == 'ready' and rec['seconds'] == 14, 'transcribed and drafted once the AI PC is back')
h = state['heard'][-1]
ok(h['path'] == '/inference' and h['lang'] == 'pt' and h['fmt'] == 'verbose_json' and h['wav'] == wav and 'multipart/form-data' in h['ctype'], 'the whisper server got the whole WAV, in Portuguese')
rep_ask, fld_ask = state['asked'][-2], state['asked'][-1]
ok('quem disse o quê' in rep_ask['system'] and 'sem formato SOAP' in rep_ask['system'] and 'Exemplo inventado' in rep_ask['system'], 'the report prompt explains the unlabelled conversation and carries the plain-text rules')
ok(TRANSCRIPT_WORDS in rep_ask['prompt'] and '[BLANK_AUDIO]' not in rep_ask['prompt'], 'the model reads the transcript (without whisper’s silence marks)')
ok('Joana' not in json.dumps(state['asked']), 'the patient’s name is never sent to the AI')
ok(rep_ask['options']['num_ctx'] >= 8192 and sorted(fld_ask.get('format', {}).get('properties', {}).get('values', {}).get('required', [])) == ['function', 'pain', 'rotation', 'techniques'], 'the fields call asks for one value per template block')
rec_model = call('GET', '/ai/status', owner).json()['model']
r = call('GET', f'/recordings/{rid}', ther); d = r.json()
ok(r.status_code == 200 and d['report'] == 'O utente referiu menos dor no ombro esquerdo, agora 4/10, mas ainda dificuldade em elevar o braço.\n\nFez-se mobilização articular do ombro.', 'the report is plain paragraphs (bold and bullets removed)')
ok(d['values'] == {'pain': 4, 'rotation': 70, 'techniques': ['Mobilização articular']}, 'proposed values fit the blocks: rounded to the step, out-of-range and unknown options dropped')
ok(d['plan'].startswith('Na próxima sessão') and d['template'] == 'neck' and d['model'] == rec_model, 'a plan for next session, the template and the model')
ok(TRANSCRIPT_WORDS in d['transcript'] and d['transcript'].count('\n') == 2, 'the transcript, one line per segment')
ok(call('GET', f'/recordings/{rid}', ther2).status_code == 403, 'another therapist without AI can’t')
ok(call('PATCH', f'/staff/{staff["Bruno Dias"]["id"]}', owner, {'ai_enabled': True}).status_code == 200 and call('GET', f'/recordings/{rid}', ther2).status_code == 404
   and call('GET', f'/patients/{pid}/recordings', ther2).json()['recordings'] == [], 'another therapist with AI doesn’t see someone else’s recording')
ok(call('GET', f'/recordings/{rid}', owner).status_code == 200, 'the owner can')
a = call('GET', f'/recordings/{rid}/audio', ther)
ok(a.status_code == 200 and a.headers['Content-Type'] == 'audio/wav' and a.content == wav, 'the audio can be played back')
log = call('GET', f'/access-log?subject_type=patient&subject_id={pid}', owner).json()
acts = [x['action'] for x in log]
ok(all(x in acts for x in ['recording.started', 'recording.uploaded', 'recording.opened', 'recording.played']), 'started, uploaded, opened and played are in the patient’s access log')
ok(not any(TRANSCRIPT_WORDS.split()[1] in (x['detail'] or '') for x in log), 'the log never holds what was said')
if PSQL:
    dump = subprocess.run(['pg_dump', PSQL], capture_output=True, text=True).stdout
    ok(TRANSCRIPT_WORDS not in dump and 'progride-se o fortalecimento' not in dump, 'the database (and so its backups) holds neither the transcript nor the draft')

# ── the note made from it; signing deletes the recording ──
n = call('POST', f'/patients/{pid}/notes', ther, {'template': 'neck', 'blocks': BLOCKS, 'text': d['report'] + '\n\n' + d['plan'], 'ai_assisted': True, 'recording_id': rid}).json()['id']
ok(call('GET', f'/patients/{pid}/recordings', ther).json()['recordings'][0]['note_id'] == n, 'the note draft points at the recording')
other = call('POST', f'/patients/{pid}/recordings', ther, START).json()['id']
call('PATCH', f'/notes/{n}', ther, {'recording_id': other})
ok(call('GET', f'/patients/{pid}/recordings', ther).json()['recordings'][0]['note_id'] is None, 'only a ready recording can be linked')
call('DELETE', f'/recordings/{other}', ther)
r = call('POST', f'/notes/{n}/sign', ther)
ok(r.status_code == 200 and not os.path.exists(folder(rid)), 'signing the note deletes the recording’s folder (audio, transcript, draft)')
ok(call('GET', f'/patients/{pid}/recordings', ther).json()['recordings'] == [] and call('GET', f'/recordings/{rid}/audio', ther).status_code == 404, 'and it is gone from the app')
log = call('GET', f'/access-log?subject_type=patient&subject_id={pid}', owner).json()
ok(any(x['action'] == 'recording.deleted' and x['detail'].endswith('signed') for x in log), 'the deletion is logged, with why')
if PSQL: ok(sql(f"SELECT status || ' ' || deleted_why FROM recordings WHERE id = '{rid}'") == 'deleted signed', 'the row says deleted, signed')

# ── too short; failing AI; by hand; after 30 days ──
def record(tok=ther, secs=12, patient=None):
    r = call('POST', f'/patients/{patient or pid}/recordings', tok, START).json()['id']
    call('PUT', f'/recordings/{r}/chunks/0', tok, {'data': b64(pcm(secs))}); return r
short = record(secs=3)
ok(call('POST', f'/recordings/{short}/finish', ther).status_code == 400, 'a recording under 10 s isn’t sent')
ok(call('DELETE', f'/recordings/{short}', ther).status_code == 200 and not os.path.exists(folder(short)), 'deleted by hand: folder gone')
state['soap'] = True
bad = record(); call('POST', f'/recordings/{bad}/finish', ther)
rec = wait_status(ther, pid, bad, 'failed', 30)
ok(rec and rec['status'] == 'failed' and rec['error'] == 'ai_failed', 'a model that keeps writing SOAP: failed after three tries, never a structured draft')
ok(call('GET', f'/recordings/{bad}', ther).status_code == 409, 'no draft to read')
state['soap'] = False
ok(call('POST', f'/recordings/{bad}/retry', ther).status_code == 200 and wait_status(ther, pid, bad, 'ready'), 'Try Again puts it back in the queue and it finishes')
ok(call('POST', f'/recordings/{bad}/retry', ther).status_code == 409, 'only a failed one can be retried')
if PSQL:
    sql(f"UPDATE recordings SET delete_after = now() - interval '1 minute' WHERE id = '{bad}'")
    wait_status(ther, pid, bad, None, 10)
    ok(not os.path.exists(folder(bad)) and sql(f"SELECT deleted_why FROM recordings WHERE id = '{bad}'") == 'expired', 'after 30 days unsigned it deletes itself')

# ── with the patient, with the day ──
pid2 = call('POST', '/patients', recep, {'name': 'Rui Matos', 'therapist_id': ana}).json()['id']
r2 = record(patient=pid2); call('POST', f'/recordings/{r2}/finish', ther); wait_status(ther, pid2, r2, 'ready')
ok(call('POST', f'/patients/{pid2}/anonymise', owner, {'confirm_name': 'Rui Matos'}).status_code == 200 and not os.path.exists(folder(r2)), 'anonymising a patient deletes their recordings')
pid3 = call('POST', '/patients', recep, {'name': 'Sara Pinto', 'therapist_id': ana}).json()['id']
r3 = record(patient=pid3)
ok(call('POST', f'/patients/{pid3}/delete', owner, {'confirm_name': 'Sara Pinto'}).status_code == 200 and not os.path.exists(folder(r3)), 'deleting a patient deletes their recordings')
r4 = record()
day = date.today().isoformat()
ok(call('POST', f'/days/{day}/delete', owner, {'confirm': day}).status_code == 200 and not os.path.exists(folder(r4)), 'deleting a day deletes that day’s recordings')

# ── turning AI off again ──
ok(call('PATCH', f'/staff/{ana}', owner, {'ai_enabled': False}).status_code == 200 and call('POST', f'/patients/{pid}/recordings', ther, START).status_code == 403, 'AI off again: no more recording')
log = call('GET', '/access-log?actions=staff.', owner).json()
ok(sum(x['action'] in ('staff.ai_on', 'staff.ai_off') for x in log) == 5, 'switching AI on and off is logged')

# ── settings: where the speech-to-text runs ──
ok(call('PUT', '/ai/settings', owner, {'speech_url': 'api.openai.com'}).status_code == 400, 'an internet address is refused for speech-to-text')
r = call('PUT', '/ai/settings', owner, {'speech_url': '192.168.1.66'})
st = call('GET', '/ai/settings', owner).json()
ok(r.status_code == 200 and st['speech_url'] == 'http://192.168.1.66:8178' and st['speech_url_in_app'], 'a clinic address is saved, with the default port')
call('PUT', '/ai/settings', owner, {'url': '127.0.0.1:11995', 'model': 'gemma4:12b'})
call('PUT', '/ai/settings', owner, {'speech_url': '192.168.1.67'})
st = call('GET', '/ai/settings', owner).json()
ok(st['url'] == 'http://127.0.0.1:11995' and st['model'] == 'gemma4:12b' and st['speech_url'] == 'http://192.168.1.67:8178', 'changing only the speech address keeps the AI’s address and model')
ok(call('POST', '/ai/speech/test', owner, {'url': '127.0.0.1:11996'}).json()['available'] is True, 'Test finds the whisper server')
ok(call('POST', '/ai/speech/test', ther, {'url': '127.0.0.1:11996'}).status_code == 403, 'only the owner changes AI settings')
call('PUT', '/ai/settings', owner, {'speech_url': ''})
ok(call('GET', '/ai/settings', owner).json()['speech_url'] == 'http://127.0.0.1:11996', 'empty goes back to the server’s WHISPER_URL')

if LOG:
    text = open(LOG, encoding='utf-8', errors='replace').read()
    ok(TRANSCRIPT_WORDS not in text and 'Joana' not in text and 'progride-se' not in text, 'the server log holds no speech, draft or names')
print(f'\n{fails} failed' if fails else '\nall passed')
sys.exit(1 if fails else 0)
