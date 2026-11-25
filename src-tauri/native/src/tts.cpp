// On-device speech with sherpa-onnx (a Piper/VITS voice, e.g. pt_PT "tugão", read by ONNX Runtime on the CPU).
// Built only when sherpa-onnx is found (PP_HAVE_TTS); otherwise every call says speech isn't in this build.
//
// Pinned to sherpa-onnx 1.12.x: 2.0 dropped the built-in espeak-ng phonemizer that Piper voices need.

#include "physiopulse_ai.h"

#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <string>

namespace {
void set_err(char *err, size_t len, const char *fmt, ...) {
    if (!err || len == 0) return;
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(err, len, fmt, ap);
    va_end(ap);
}
} // namespace

#ifdef PP_HAVE_TTS

#include "sherpa-onnx/c-api/c-api.h"

namespace fs = std::filesystem;

struct pp_tts {
    const SherpaOnnxOfflineTts *tts = nullptr;
};

extern "C" int pp_tts_available(void) { return 1; }

extern "C" pp_tts *pp_tts_open(const char *voice_dir, int threads, char *err, size_t err_len) {
    if (!voice_dir) { set_err(err, err_len, "no voice folder"); return nullptr; }
    std::error_code ec;
    const fs::path dir(voice_dir);
    std::string model;
    for (const auto &e : fs::directory_iterator(dir, ec)) {
        if (e.path().extension() == ".onnx") { model = e.path().string(); break; }
    }
    const std::string tokens = (dir / "tokens.txt").string();
    const std::string data = (dir / "espeak-ng-data").string();
    if (model.empty() || !fs::exists(tokens, ec) || !fs::is_directory(data, ec)) {
        set_err(err, err_len, "the voice folder needs one .onnx, tokens.txt and espeak-ng-data/");
        return nullptr;
    }

    SherpaOnnxOfflineTtsConfig cfg;
    memset(&cfg, 0, sizeof cfg);
    cfg.model.vits.model = model.c_str();
    cfg.model.vits.tokens = tokens.c_str();
    cfg.model.vits.data_dir = data.c_str();
    cfg.model.vits.lexicon = "";
    cfg.model.vits.noise_scale = 0.667f;
    cfg.model.vits.noise_scale_w = 0.8f;
    cfg.model.vits.length_scale = 1.0f;
    cfg.model.num_threads = threads > 0 ? threads : 2;
    cfg.model.debug = 0;
    cfg.model.provider = "cpu";
    cfg.max_num_sentences = 1;
    cfg.rule_fsts = "";
    cfg.rule_fars = "";

    const SherpaOnnxOfflineTts *t = SherpaOnnxCreateOfflineTts(&cfg);
    if (!t) { set_err(err, err_len, "could not load the voice at %s", voice_dir); return nullptr; }
    auto *h = new pp_tts;
    h->tts = t;
    return h;
}

extern "C" int pp_tts_speak(pp_tts *h, const char *text, int speaker, float speed,
                            float **samples, int *n_samples, int *sample_rate, char *err, size_t err_len) {
    if (!h || !text || !samples || !n_samples || !sample_rate) { set_err(err, err_len, "bad arguments"); return -1; }
    *samples = nullptr;
    *n_samples = 0;
    const SherpaOnnxGeneratedAudio *a = SherpaOnnxOfflineTtsGenerate(h->tts, text, speaker, speed > 0 ? speed : 1.0f);
    if (!a || a->n <= 0) {
        if (a) SherpaOnnxDestroyOfflineTtsGeneratedAudio(a);
        set_err(err, err_len, "the voice produced no audio");
        return -1;
    }
    float *out = static_cast<float *>(malloc(sizeof(float) * (size_t) a->n));
    if (!out) { SherpaOnnxDestroyOfflineTtsGeneratedAudio(a); set_err(err, err_len, "out of memory"); return -1; }
    memcpy(out, a->samples, sizeof(float) * (size_t) a->n);
    *samples = out;
    *n_samples = a->n;
    *sample_rate = a->sample_rate;
    SherpaOnnxDestroyOfflineTtsGeneratedAudio(a);
    return 0;
}

extern "C" void pp_tts_free(float *samples) { free(samples); }

extern "C" void pp_tts_close(pp_tts *h) {
    if (!h) return;
    if (h->tts) SherpaOnnxDestroyOfflineTts(h->tts);
    delete h;
}

#else  // no sherpa-onnx in this build

struct pp_tts {};
extern "C" int pp_tts_available(void) { return 0; }
extern "C" pp_tts *pp_tts_open(const char *, int, char *err, size_t err_len) {
    set_err(err, err_len, "speech isn't in this build (sherpa-onnx wasn't found)");
    return nullptr;
}
extern "C" int pp_tts_speak(pp_tts *, const char *, int, float, float **, int *, int *, char *err, size_t err_len) {
    set_err(err, err_len, "speech isn't in this build");
    return -1;
}
extern "C" void pp_tts_free(float *samples) { free(samples); }
extern "C" void pp_tts_close(pp_tts *) {}

#endif
