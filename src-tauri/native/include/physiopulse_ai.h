/* PhysioPulse on-device AI: chat (llama.cpp) and speech-to-text (whisper.cpp), as a small C interface.
 *
 * Everything runs on this computer. Nothing is sent anywhere, and neither prompts nor answers are logged
 * (CLAUDE.md rule 4): llama.cpp's own log is kept to errors only.
 *
 * Errors are written to `err` (a buffer of `err_len` bytes, may be NULL) in English, for the developer; the UI
 * shows its own bilingual text. Every function is safe to call from any thread, but one handle must not be used
 * by two threads at once (the Rust side holds each handle behind a mutex). */
#ifndef PHYSIOPULSE_AI_H
#define PHYSIOPULSE_AI_H

#include <stddef.h>

#if defined(_WIN32)
#  define PP_API __declspec(dllexport)
#else
#  define PP_API __attribute__((visibility("default")))
#endif

#ifdef __cplusplus
extern "C" {
#endif

/* ───────── chat ───────── */

typedef struct pp_chat pp_chat;

typedef struct {
    const char *role;    /* "system", "user" or "assistant" */
    const char *content; /* UTF-8 */
} pp_msg;

/* Called with each new piece of the answer (whole UTF-8 characters only). Return 0 to stop early. */
typedef int (*pp_piece_cb)(const char *piece, void *user);

/* Loads a GGUF model. n_ctx: context size in tokens (0 = 8192). n_gpu_layers: -1 = all on the GPU (Metal). */
PP_API pp_chat *pp_chat_open(const char *model_path, int n_ctx, int n_gpu_layers, char *err, size_t err_len);

/* Answers the conversation `msgs[0..n)` (the last one is the user's). Formats it with the model's own chat
 * template (jinja), reuses what is already in the context from the previous turn, and streams the answer to `cb`.
 * Returns the number of tokens generated (>= 0), or:
 *   -1 failed (see err), -2 the conversation doesn't fit in the context. */
PP_API int pp_chat_reply(pp_chat *chat, const pp_msg *msgs, size_t n, int max_tokens, float temperature,
                  pp_piece_cb cb, void *user, char *err, size_t err_len);

/* The model's name from its metadata ("" if none). Valid while the handle is open. */
PP_API const char *pp_chat_model_name(const pp_chat *chat);

PP_API void pp_chat_close(pp_chat *chat);

/* ───────── speech to text ───────── */

typedef struct pp_stt pp_stt;

/* Called with progress 0–100 while transcribing. Return 0 to stop. */
typedef int (*pp_progress_cb)(int percent, void *user);

/* Loads a whisper.cpp model (ggml-large-v3.bin or a quantised one). Uses the GPU (Metal) when there is one. */
PP_API pp_stt *pp_stt_open(const char *model_path, char *err, size_t err_len);

/* Transcribes mono float32 audio at 16 kHz. language: "pt", "en"… prompt: optional words that set the spelling and
 * vocabulary (European Portuguese, clinic terms). Returns the text (UTF-8, free with pp_free) or NULL (see err). */
PP_API char *pp_stt_transcribe(pp_stt *stt, const float *pcm, size_t n_samples, const char *language, const char *prompt,
                               pp_progress_cb progress, void *user, char *err, size_t err_len);

PP_API void pp_stt_close(pp_stt *stt);

PP_API void pp_free(void *p);

#ifdef __cplusplus
}
#endif

#endif
