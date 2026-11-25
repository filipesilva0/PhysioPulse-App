// On-device chat with llama.cpp (Metal on Apple silicon).
//
// One model and one context per handle. Each reply formats the whole conversation with the model's own jinja chat
// template (llama.cpp `common/chat`), keeps the part of the context that is the same as last time (the earlier
// turns), and only processes what is new, so a follow-up question starts answering at once.
//
// Never logs prompts or answers: llama.cpp's log is reduced to errors (CLAUDE.md rule 4).

#include "physiopulse_ai.h"

#include "chat.h"   // llama.cpp common/chat.h
#include "llama.h"

// llama_decode is marked deprecated in favour of llama_process + llama_batch_ext, which is still changing; keep the
// stable call until that settles.
#if defined(__clang__) || defined(__GNUC__)
#pragma GCC diagnostic ignored "-Wdeprecated-declarations"
#endif

#include <algorithm>
#include <cstdarg>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>
#include <vector>

namespace {

void set_err(char *err, size_t len, const char *fmt, ...) {
    if (!err || len == 0) return;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(err, len, fmt, ap);
    va_end(ap);
}

// Errors only, to stderr, and never the text being processed (llama.cpp's errors don't include it).
void quiet_log(ggml_log_level level, const char *text, void *) {
    if (level == GGML_LOG_LEVEL_ERROR) fputs(text, stderr);
}

std::once_flag g_backend_once;

void backend_init() {
    std::call_once(g_backend_once, [] {
        llama_log_set(quiet_log, nullptr);
        llama_backend_init();
    });
}

// How many bytes at the end of `s` are the start of a UTF-8 character that isn't complete yet.
size_t incomplete_utf8_tail(const std::string &s) {
    const size_t n = s.size();
    for (size_t back = 1; back <= 4 && back <= n; ++back) {
        const unsigned char c = (unsigned char) s[n - back];
        if ((c & 0xC0) == 0x80) continue;          // continuation byte: keep looking for the lead byte
        size_t need = 1;
        if ((c & 0xE0) == 0xC0) need = 2;
        else if ((c & 0xF0) == 0xE0) need = 3;
        else if ((c & 0xF8) == 0xF0) need = 4;
        return need > back ? back : 0;
    }
    return 0;
}

} // namespace

struct pp_chat {
    llama_model *model = nullptr;
    llama_context *ctx = nullptr;
    const llama_vocab *vocab = nullptr;
    common_chat_templates_ptr tmpls;
    std::vector<llama_token> in_ctx;  // the tokens the context holds now, in order
    std::string name;
    int n_ctx = 0;
};

extern "C" pp_chat *pp_chat_open(const char *model_path, int n_ctx, int n_gpu_layers, char *err, size_t err_len) {
    if (!model_path || !*model_path) { set_err(err, err_len, "no model path"); return nullptr; }
    backend_init();

    llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = n_gpu_layers < 0 ? 999 : n_gpu_layers;
    llama_model *model = llama_model_load_from_file(model_path, mp);
    if (!model) { set_err(err, err_len, "could not load the model at %s", model_path); return nullptr; }

    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = n_ctx > 0 ? (uint32_t) n_ctx : 8192;
    cp.n_batch = 512;
    cp.n_ubatch = 512;
    cp.no_perf = true;
    llama_context *ctx = llama_init_from_model(model, cp);
    if (!ctx) { llama_model_free(model); set_err(err, err_len, "could not create the context (not enough memory?)"); return nullptr; }

    auto *c = new pp_chat;
    c->model = model;
    c->ctx = ctx;
    c->vocab = llama_model_get_vocab(model);
    c->n_ctx = (int) llama_n_ctx(ctx);
    try {
        c->tmpls = common_chat_templates_init(model, "");
    } catch (const std::exception &e) {
        set_err(err, err_len, "the model's chat template can't be read: %s", e.what());
        pp_chat_close(c);
        return nullptr;
    }
    char buf[256] = {0};
    if (llama_model_meta_val_str(model, "general.name", buf, sizeof buf) > 0) c->name = buf;
    return c;
}

extern "C" const char *pp_chat_model_name(const pp_chat *chat) { return chat ? chat->name.c_str() : ""; }

extern "C" void pp_chat_close(pp_chat *chat) {
    if (!chat) return;
    chat->tmpls.reset();
    if (chat->ctx) llama_free(chat->ctx);
    if (chat->model) llama_model_free(chat->model);
    delete chat;
}

namespace {

// Tokens for `text`. parse_special: the template's own markers (<|turn>…) become control tokens; what a person
// typed never does (so typing "<turn|>" can't end a turn early).
bool tokenize(const llama_vocab *vocab, const std::string &text, bool add_special, bool parse_special, std::vector<llama_token> &out) {
    if (text.empty()) return true;
    int n = -llama_tokenize(vocab, text.data(), (int32_t) text.size(), nullptr, 0, add_special, parse_special);
    if (n <= 0) return n == 0;
    const size_t at = out.size();
    out.resize(at + n);
    int got = llama_tokenize(vocab, text.data(), (int32_t) text.size(), out.data() + at, n, add_special, parse_special);
    if (got < 0) return false;
    out.resize(at + got);
    return true;
}

std::string piece(const llama_vocab *vocab, llama_token t) {
    char buf[256];
    int n = llama_token_to_piece(vocab, t, buf, sizeof buf, 0, /*special*/ false);
    if (n < 0) {
        std::string big(-n, '\0');
        n = llama_token_to_piece(vocab, t, big.data(), (int32_t) big.size(), 0, false);
        return n > 0 ? big.substr(0, n) : std::string();
    }
    return std::string(buf, n);
}

} // namespace

extern "C" int pp_chat_reply(pp_chat *c, const pp_msg *msgs, size_t n, int max_tokens, float temperature,
                             pp_piece_cb cb, void *user, char *err, size_t err_len) {
    if (!c || !msgs || n == 0) { set_err(err, err_len, "nothing to answer"); return -1; }
    if (max_tokens <= 0) max_tokens = 1024;

    // 1. The prompt, in the model's own format, without its "thinking" part (Gemma 4: enable_thinking = false).
    common_chat_templates_inputs in;
    for (size_t i = 0; i < n; ++i) {
        common_chat_msg m;
        m.role = msgs[i].role ? msgs[i].role : "user";
        m.content = msgs[i].content ? msgs[i].content : "";
        in.messages.push_back(std::move(m));
    }
    in.add_generation_prompt = true;
    in.use_jinja = true;
    in.enable_thinking = false;
    common_chat_params params;
    try {
        params = common_chat_templates_apply(c->tmpls.get(), in);
    } catch (const std::exception &e) {
        set_err(err, err_len, "chat template failed: %s", e.what());
        return -1;
    }

    std::vector<llama_token> prompt;
    const std::string full = params.prompt.str();
    const char *bos_text = nullptr;
    const llama_token bos = llama_vocab_bos(c->vocab);
    std::string bos_piece;
    if (bos != LLAMA_TOKEN_NULL) {
        char b[64];
        int k = llama_token_to_piece(c->vocab, bos, b, sizeof b, 0, true);
        if (k > 0) { bos_piece.assign(b, k); bos_text = bos_piece.c_str(); }
    }
    // Add a BOS only if the vocabulary wants one and the template didn't write it already.
    const bool add_bos = llama_vocab_get_add_bos(c->vocab) && !(bos_text && full.rfind(bos_text, 0) == 0);
    bool first = true;
    for (const auto &part : params.prompt.parts) {
        if (!tokenize(c->vocab, part.text, first && add_bos, /*parse_special*/ !part.is_input, prompt)) {
            set_err(err, err_len, "could not tokenize the conversation");
            return -1;
        }
        first = false;
    }
    if ((int) prompt.size() + max_tokens > c->n_ctx) {
        set_err(err, err_len, "the conversation (%d tokens) doesn't fit in the context (%d)", (int) prompt.size(), c->n_ctx);
        return -2;
    }

    // 2. Keep what the context already holds from last time; drop the rest.
    llama_memory_t mem = llama_get_memory(c->ctx);
    size_t keep = 0;
    while (keep < c->in_ctx.size() && keep < prompt.size() && c->in_ctx[keep] == prompt[keep]) ++keep;
    if (keep == prompt.size() && keep > 0) --keep;  // always process at least one token, to get logits
    if (keep < c->in_ctx.size()) {
        if (!llama_memory_seq_rm(mem, 0, (llama_pos) keep, -1)) {
            // Some caches (sliding-window layers) can't drop a tail: start the context again.
            llama_memory_clear(mem, true);
            keep = 0;
        }
    }
    c->in_ctx.resize(keep);

    // 3. Process the new part of the prompt, in batches.
    const int n_batch = (int) llama_n_batch(c->ctx);
    for (size_t i = keep; i < prompt.size(); i += n_batch) {
        const int len = (int) std::min<size_t>(n_batch, prompt.size() - i);
        llama_batch b = llama_batch_get_one(prompt.data() + i, len);
        if (llama_decode(c->ctx, b) != 0) {
            llama_memory_clear(mem, true);
            c->in_ctx.clear();
            set_err(err, err_len, "the model couldn't process the conversation");
            return -1;
        }
        c->in_ctx.insert(c->in_ctx.end(), prompt.begin() + i, prompt.begin() + i + len);
    }

    // 4. Generate. Low temperature: a clinic's admin questions want facts, not flourish.
    llama_sampler *smpl = llama_sampler_chain_init(llama_sampler_chain_default_params());
    if (temperature <= 0.0f) {
        llama_sampler_chain_add(smpl, llama_sampler_init_greedy());
    } else {
        llama_sampler_chain_add(smpl, llama_sampler_init_top_k(64));
        llama_sampler_chain_add(smpl, llama_sampler_init_top_p(0.95f, 1));
        llama_sampler_chain_add(smpl, llama_sampler_init_min_p(0.05f, 1));
        llama_sampler_chain_add(smpl, llama_sampler_init_temp(temperature));
        llama_sampler_chain_add(smpl, llama_sampler_init_dist(LLAMA_DEFAULT_SEED));
    }

    std::string pending;  // text not yet sent: an incomplete UTF-8 character, or what might be a stop string
    std::string said;     // everything generated, to spot stop strings
    size_t longest_stop = 0;
    for (const auto &s : params.additional_stops) longest_stop = std::max(longest_stop, s.size());

    int generated = 0;
    bool stopped = false;
    int rc = 0;
    while (generated < max_tokens && !stopped) {
        llama_token t = llama_sampler_sample(smpl, c->ctx, -1);
        if (llama_vocab_is_eog(c->vocab, t)) break;
        if ((int) c->in_ctx.size() + 1 >= c->n_ctx) break;  // context full: end the answer here

        // Markers such as Gemma 4's <|channel>/<channel|> are "user-defined" tokens, so they would print: skip them.
        const std::string p0 = piece(c->vocab, t);
        const bool marker = (llama_vocab_get_attr(c->vocab, t) & (LLAMA_TOKEN_ATTR_CONTROL | LLAMA_TOKEN_ATTR_USER_DEFINED))
                            && p0.size() > 2 && p0.front() == '<' && p0.back() == '>';
        const std::string p = marker ? std::string() : p0;
        said += p;
        pending += p;
        ++generated;

        // A stop string (some templates have them on top of the end-of-turn token): cut it off and finish.
        for (const auto &s : params.additional_stops) {
            const size_t at = said.find(s, said.size() >= p.size() + s.size() ? said.size() - p.size() - s.size() : 0);
            if (at != std::string::npos) {
                const size_t cut = said.size() - at;  // bytes of `said` from the stop on
                pending.resize(pending.size() >= cut ? pending.size() - cut : 0);
                stopped = true;
                break;
            }
        }

        // Send what is complete: whole characters, minus the bit that could still become a stop string.
        size_t hold = incomplete_utf8_tail(pending);
        if (!stopped && longest_stop > 1) hold = std::max(hold, std::min(pending.size(), longest_stop - 1));
        if (stopped) hold = incomplete_utf8_tail(pending);
        if (pending.size() > hold) {
            std::string out = pending.substr(0, pending.size() - hold);
            pending.erase(0, pending.size() - hold);
            if (cb && !cb(out.c_str(), user)) { stopped = true; }
        }
        if (stopped) break;

        llama_batch b = llama_batch_get_one(&t, 1);
        if (llama_decode(c->ctx, b) != 0) { rc = -1; set_err(err, err_len, "generation failed"); break; }
        c->in_ctx.push_back(t);
    }
    if (rc == 0 && !pending.empty() && cb) {
        pending.resize(pending.size() - incomplete_utf8_tail(pending));
        if (!pending.empty()) cb(pending.c_str(), user);
    }
    llama_sampler_free(smpl);
    return rc < 0 ? rc : generated;
}
