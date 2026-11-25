// Smoke test for libphysiopulse_ai: loads a model, answers two turns (the second reuses the first's context),
// stops early from the callback, rejects a conversation too long for the context, and (if given) transcribes a
// 16 kHz mono 16-bit WAV with a whisper.cpp model.
//   smoke <model.gguf> [whisper-model.bin audio.wav]
#include "physiopulse_ai.h"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

static int on_piece(const char *p, void *user) {
    auto *s = static_cast<std::string *>(user);
    *s += p;
    fputs(p, stdout);
    fflush(stdout);
    return 1;
}
static int stop_after_3(const char *p, void *user) {
    int *n = static_cast<int *>(user);
    (void) p;
    return ++*n < 3;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: smoke model.gguf [whisper-model.bin audio.wav]\n"); return 2; }
    char err[512] = {0};
    auto t0 = std::chrono::steady_clock::now();
    pp_chat *c = pp_chat_open(argv[1], 4096, -1, err, sizeof err);
    if (!c) { fprintf(stderr, "open failed: %s\n", err); return 1; }
    printf("model: %s (%.1f s to load)\n", pp_chat_model_name(c),
           std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count());

    pp_msg turn1[] = {{"system", "Responde em português de Portugal, em poucas palavras."},
                      {"user", "Quantas sessões de fisioterapia são normais depois de uma entorse do tornozelo?"}};
    std::string a1;
    t0 = std::chrono::steady_clock::now();
    int n = pp_chat_reply(c, turn1, 2, 96, 0.3f, on_piece, &a1, err, sizeof err);
    double s = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
    printf("\n[turn 1: %d tokens, %.2f s, %.1f tok/s]\n", n, s, n / s);
    if (n < 0) { fprintf(stderr, "reply failed: %s\n", err); return 1; }

    pp_msg turn2[] = {turn1[0], turn1[1], {"assistant", a1.c_str()}, {"user", "E se for <turn|> grave?"}};
    std::string a2;
    t0 = std::chrono::steady_clock::now();
    n = pp_chat_reply(c, turn2, 4, 64, 0.0f, on_piece, &a2, err, sizeof err);
    s = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
    printf("\n[turn 2: %d tokens, %.2f s]\n", n, s);
    if (n < 0) { fprintf(stderr, "reply failed: %s\n", err); return 1; }

    int count = 0;
    n = pp_chat_reply(c, turn1, 2, 64, 0.3f, stop_after_3, &count, err, sizeof err);
    printf("[stop from callback: %d pieces seen, %d tokens]\n", count, n);
    if (count != 3) { fprintf(stderr, "stop didn't work\n"); return 1; }

    // Too long for the context: must fail cleanly with -2.
    std::string huge(200000, 'a');
    for (size_t i = 0; i < huge.size(); i += 6) huge[i] = ' ';
    pp_msg big[] = {{"user", huge.c_str()}};
    n = pp_chat_reply(c, big, 1, 64, 0.3f, nullptr, nullptr, err, sizeof err);
    printf("[too long: rc=%d (%s)]\n", n, err);
    if (n != -2) return 1;
    pp_chat_close(c);

    if (argc >= 4) {
        FILE *f = fopen(argv[3], "rb");
        if (!f) { fprintf(stderr, "no audio file\n"); return 1; }
        std::vector<char> bytes;
        char buf[65536];
        size_t got;
        while ((got = fread(buf, 1, sizeof buf, f)) > 0) bytes.insert(bytes.end(), buf, buf + got);
        fclose(f);
        // Find the "data" chunk of the WAV.
        size_t at = 12;
        std::vector<float> pcm;
        while (at + 8 <= bytes.size()) {
            uint32_t len;
            memcpy(&len, &bytes[at + 4], 4);
            if (memcmp(&bytes[at], "data", 4) == 0) {
                for (size_t i = at + 8; i + 1 < at + 8 + len && i + 1 < bytes.size(); i += 2) {
                    int16_t v;
                    memcpy(&v, &bytes[i], 2);
                    pcm.push_back(v / 32768.0f);
                }
                break;
            }
            at += 8 + len;
        }
        pp_stt *stt = pp_stt_open(argv[2], err, sizeof err);
        if (!stt) { fprintf(stderr, "stt open failed: %s\n", err); return 1; }
        int last = -1;
        auto progress = [](int pct, void *u) -> int { *static_cast<int *>(u) = pct; return 1; };
        t0 = std::chrono::steady_clock::now();
        char *text = pp_stt_transcribe(stt, pcm.data(), pcm.size(), "pt", "Sessão de fisioterapia.", progress, &last, err, sizeof err);
        s = std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count();
        if (!text) { fprintf(stderr, "transcribe failed: %s\n", err); return 1; }
        printf("stt: %.1f s of audio in %.2f s, progress %d%%: \"%.120s\"\n", pcm.size() / 16000.0, s, last, text);
        pp_free(text);
        // Stop from the progress callback must end it early.
        auto stop = [](int, void *) -> int { return 0; };
        char *none = pp_stt_transcribe(stt, pcm.data(), pcm.size(), "pt", nullptr, stop, nullptr, err, sizeof err);
        printf("[stt stop: %s (%s)]\n", none ? "not stopped" : "stopped", err);
        if (none) pp_free(none);
        pp_stt_close(stt);
    }
    puts("OK");
    return 0;
}
