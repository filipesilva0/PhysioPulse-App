// On-device speech-to-text with whisper.cpp (Metal on Apple silicon), built against llama.cpp's ggml so both share
// one copy. Tuned for quality, not speed (owner, 25 Nov 2026): Whisper large-v3, beam search 5, temperature
// fallback, a European-Portuguese prompt so spelling follows pt-PT. Never logs audio or text.

#include "physiopulse_ai.h"
#include "whisper.h"

#include <algorithm>
#include <atomic>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>

namespace {
void set_err(char *err, size_t len, const char *fmt, ...) {
    if (!err || len == 0) return;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(err, len, fmt, ap);
    va_end(ap);
}
void quiet_log(ggml_log_level level, const char *text, void *) {
    if (level == GGML_LOG_LEVEL_ERROR) fputs(text, stderr);
}
struct Run {
    pp_progress_cb cb;
    void *user;
    std::atomic<bool> stop{false};
};
} // namespace

struct pp_stt {
    whisper_context *ctx = nullptr;
};

extern "C" pp_stt *pp_stt_open(const char *model_path, char *err, size_t err_len) {
    if (!model_path || !*model_path) { set_err(err, err_len, "no model path"); return nullptr; }
    whisper_log_set(quiet_log, nullptr);
    whisper_context_params cp = whisper_context_default_params();
    cp.use_gpu = true;
    cp.flash_attn = true;
    whisper_context *ctx = whisper_init_from_file_with_params(model_path, cp);
    if (!ctx) { set_err(err, err_len, "could not load the speech model at %s", model_path); return nullptr; }
    auto *s = new pp_stt;
    s->ctx = ctx;
    return s;
}

extern "C" char *pp_stt_transcribe(pp_stt *s, const float *pcm, size_t n, const char *language, const char *prompt,
                                   pp_progress_cb progress, void *user, char *err, size_t err_len) {
    if (!s || !pcm || n == 0) { set_err(err, err_len, "no audio"); return nullptr; }
    if (n < 16000 / 4) { set_err(err, err_len, "too short"); return nullptr; }

    whisper_full_params p = whisper_full_default_params(WHISPER_SAMPLING_BEAM_SEARCH);
    p.beam_search.beam_size = 5;
    p.greedy.best_of = 5;
    p.n_threads = (int) std::min(8u, std::max(1u, std::thread::hardware_concurrency()));
    p.language = (language && *language) ? language : "pt";
    p.detect_language = false;
    p.translate = false;
    p.no_timestamps = true;
    p.print_special = p.print_progress = p.print_realtime = p.print_timestamps = false;
    p.suppress_blank = true;
    p.suppress_nst = true;
    if (prompt && *prompt) p.initial_prompt = prompt;

    Run run{progress, user};
    p.progress_callback = [](whisper_context *, whisper_state *, int pct, void *u) {
        auto *r = static_cast<Run *>(u);
        if (r->cb && !r->cb(pct, r->user)) r->stop = true;
    };
    p.progress_callback_user_data = &run;
    p.abort_callback = [](void *u) { return static_cast<Run *>(u)->stop.load(); };
    p.abort_callback_user_data = &run;

    if (whisper_full(s->ctx, p, pcm, (int) n) != 0) {
        set_err(err, err_len, run.stop ? "stopped" : "transcription failed");
        return nullptr;
    }
    std::string text;
    const int segs = whisper_full_n_segments(s->ctx);
    for (int i = 0; i < segs; ++i) {
        const char *t = whisper_full_get_segment_text(s->ctx, i);
        if (t) text += t;
    }
    // Trim the leading space whisper puts before each segment.
    const size_t a = text.find_first_not_of(" \t\n");
    text = a == std::string::npos ? std::string() : text.substr(a);
    char *out = static_cast<char *>(malloc(text.size() + 1));
    if (!out) { set_err(err, err_len, "out of memory"); return nullptr; }
    memcpy(out, text.c_str(), text.size() + 1);
    return out;
}

extern "C" void pp_free(void *p) { free(p); }

extern "C" void pp_stt_close(pp_stt *s) {
    if (!s) return;
    if (s->ctx) whisper_free(s->ctx);
    delete s;
}
