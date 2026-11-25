# On-device AI (chat and dictation on the computer itself)

Owner, 25 Nov 2026: "make the app have its own .cpp for tts and chat. use gemma 4-4b or something better that runs well in this app and is light enough for a macbook air m2". Then: "dont need to read anything aloud. what about stt" → "yes. use the best model for stt, as it isnt time sensitive, but quality is". So: chat + speech-to-text, no read-aloud.

## What it is

The desktop app has its own C++ module, `src-tauri/native/`, built into the app as one library (`libphysiopulse_ai.dylib`, in `PhysioPulse.app/Contents/Frameworks`):

| Part | Engine | Model |
| --- | --- | --- |
| Chat | llama.cpp (tag `b12692`), Metal, the model's own jinja chat template (llama.cpp `common/chat`) | **Gemma 4 E4B**, 4-bit Q4_K_M (~5 GB). 8 GB Mac: Gemma 4 E2B. |
| Speech to text | whisper.cpp (tag `v1.9.6`), Metal, compiled against llama.cpp's ggml (one copy of ggml in the app) | **Whisper large-v3**, full precision (3.1 GB). 8 GB Mac: large-v3 q5_0 (1.1 GB). |

- C interface: `native/include/physiopulse_ai.h` (`pp_chat_*`, `pp_stt_*`).
- Rust: `src-tauri/src/local_ai.rs` — `local_ai_status`, `local_chat` (streams over a Tauri `Channel`), `local_chat_stop`, `local_transcribe` (raw body: float32 16 kHz mono; header `x-lang`; progress as the `local-stt-progress` event), `local_transcribe_stop`, `local_ai_unload`, `local_ai_open_folder`. Each model loads on first use and is let go after 10 idle minutes.
- UI (`src/components/LocalChat.tsx`, `src/lib/localAi.ts`): the Assistant panel has **Clínica | Neste Mac** (Neste PC on Windows) when the module is in the build, and a **Ditar** mic button beside the text field in both modes: press, speak, press again; the text is added to the field to check and send. The browser preview has stand-ins.
- **"Neste Mac" has no tools and sees no clinic data**: only what the person types or dictates. The clinic assistant (tools, agenda, files) stays on the AI PC through the server, unchanged.

## Speech-to-text: quality settings

Quality over speed (owner). Whisper **large-v3, not turbo**: turbo cuts the decoder from 32 to 4 layers for speed and is slightly less accurate (FLEURS average 7.75% vs 7.40% WER). whisper.cpp runs it with beam search 5 (best of 5), temperature fallback, the language fixed to pt (or en), and a starting prompt in **European Portuguese** with clinic words (`PROMPT_PT` in `local_ai.rs`) so spelling follows pt-PT, not pt-BR. Measured pt-PT figures don't exist in the sources found; judge on the clinic's own voices.

Considered and not chosen: Voxtral (no maintained C/C++ runtime), Granite Speech, Parakeet v3, Qwen3-ASR (no published Portuguese accuracy, or Portuguese not confirmed).

## Privacy (CLAUDE.md rule 4)

Nothing leaves the computer: no network calls, and no prompt, answer, audio or transcript logged or written to disk (llama.cpp's and whisper.cpp's logs are cut to errors). Audio is held in memory only until it is transcribed. Only `tools/local-ai/setup.sh` downloads anything (sources and model files).

## Why Gemma 4 E4B (checked 25 Nov 2026)

Apache 2.0 (April 2026), 4.5 B effective parameters, 128 K context, multilingual, in llama.cpp from release; ~5 GB at Q4_K_M, ~6.5 GB RAM with a 4 K context. Qwen 3.5 4B is about as fast and ties on the one shared benchmark; Gemma was kept because the clinic already uses Gemma 4 for letters and notes and its Portuguese is strong. Any GGUF in the models folder works (preferred names: `PREFERRED` in `local_ai.rs`).

Sources: huggingface.co/blog/gemma4, localmodel.run/model/gemma-4-e4b, benchlm.ai/compare/gemma-4-e4b-vs-qwen3-5-4b, llmcheck.net/blog/gemma-4-vs-qwen-35-apple-silicon, weesperneonflow.ai (Voxtral vs Whisper 2026), vexascribe.com/how-accurate-is-whisper, micdrop.dev/blog/top-open-source-speech-to-text-models.

## Set up and build (on the Mac)

```bash
brew install cmake                  # once
npm run local-ai:setup              # llama.cpp + whisper.cpp into src-tauri/native/vendor; Gemma 4 + Whisper large-v3 into ~/Library/Application Support/app.physiopulse/models (~8 GB)
npm run tauri dev                   # first build compiles llama.cpp and whisper.cpp (a few minutes)
npm run tauri:mac                   # .app/.dmg with libphysiopulse_ai.dylib in Contents/Frameworks
```

- `MODEL=e2b` / `STT=ggml-large-v3-q5_0.bin` override the choices. Without setup, macOS builds still work (warning, no on-device AI, no switch or mic in the panel). Windows: only with `--features local-ai`. `PHYSIOPULSE_NO_LOCAL_AI=1` skips it.
- llama.cpp and whisper.cpp share ggml: move their tags together (`setup.sh`).

## Tests

- `native/test/smoke.cpp`: chat (two turns reusing the context, stop, too-long → -2) and, given a whisper model and a 16 kHz WAV, transcription with progress and stop.
- `cargo test --features local-ai --lib local_ai`; with real files: `PP_TEST_MODEL=… PP_TEST_STT=… PP_TEST_WAV=… cargo test --features local-ai --lib local_ai -- --include-ignored`.
- 25 Nov 2026, Claude's Linux sandbox (no Mac; Hugging Face blocked there): module and app built with and without it, no warnings; smoke and Rust tests passed with stand-in models (a tiny random-weight model with Gemma 4's real tokenizer and template; a tiny random-weight Whisper converted with whisper.cpp's script, on whisper.cpp's jfk.wav); `tsc` clean; preview screenshots. **Not yet run: the real models on the MacBook Air** (speed, memory, Portuguese quality).

## Possible next steps

- Transcribe session recordings on the Mac with large-v3 too (today they go to whisper `large-v3-turbo` on the AI PC); or switch the AI PC to large-v3.
- Let the server use the Mac as its AI computer, so the clinic assistant itself could run on the Air.
