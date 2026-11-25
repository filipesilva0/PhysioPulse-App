#!/usr/bin/env bash
# PhysioPulse on-device AI: everything the app's C++ module (src-tauri/native) needs, on a Mac with Apple silicon.
#
#   tools/local-ai/setup.sh            # sources for the build + the models (picks the sizes by RAM)
#   MODEL=e4b tools/local-ai/setup.sh  # force Gemma 4 E4B (default on 16 GB); MODEL=e2b for the smaller one (8 GB)
#   SKIP_MODELS=1 tools/local-ai/setup.sh   # build sources only (e.g. on a build machine)
#
# What it puts where:
#   src-tauri/native/vendor/llama.cpp     llama.cpp at a pinned tag (chat; built into the app by src-tauri/build.rs)
#   src-tauri/native/vendor/whisper.cpp   whisper.cpp at a pinned tag (speech to text)
#   ~/Library/Application Support/app.physiopulse/models/
#       gemma-4-E4B-it-Q4_K_M.gguf (or E2B)   the chat model
#       ggml-large-v3.bin (or -q5_0 on 8 GB)  Whisper large-v3, the speech-to-text model
# Only these downloads touch the internet; the app itself never does for AI.
set -euo pipefail

LLAMA_TAG="b12692"
WHISPER_TAG="v1.9.6"   # compiled against llama.cpp's ggml: move both tags together
WHISPER_MODELS="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
NATIVE="$ROOT/src-tauri/native"
MODELS="${MODELS_DIR:-$HOME/Library/Application Support/app.physiopulse/models}"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null || { echo "Missing: $1. $2"; exit 1; }; }

[[ "$(uname -s)" == "Darwin" ]] || { echo "This script is for macOS (the on-device AI targets Apple silicon)."; exit 1; }
[[ "$(uname -m)" == "arm64" ]] || echo "Warning: not Apple silicon; the chat model will be slow without Metal."
need git  "Install the Xcode command line tools: xcode-select --install"
need curl ""
need cmake "Install it with: brew install cmake"

fetch_src() {  # name tag url
  local dir="$NATIVE/vendor/$1"
  if [[ -d "$dir/.git" ]]; then
    git -C "$dir" fetch --depth 1 origin tag "$2" -q
    git -C "$dir" checkout -q "$2"
  else
    mkdir -p "$NATIVE/vendor"
    git clone -q --depth 1 --branch "$2" "$3" "$dir"
  fi
}
say "1/4 llama.cpp $LLAMA_TAG"
fetch_src llama.cpp "$LLAMA_TAG" https://github.com/ggml-org/llama.cpp
say "2/4 whisper.cpp $WHISPER_TAG"
fetch_src whisper.cpp "$WHISPER_TAG" https://github.com/ggml-org/whisper.cpp
rm -rf "$NATIVE/vendor/sherpa-onnx"   # from the first version (read-aloud, removed 25 Nov 2026)

if [[ "${SKIP_MODELS:-}" == "1" ]]; then say "Done (models skipped)."; exit 0; fi
mkdir -p "$MODELS"

say "3/4 chat model"
ram_gb=$(( $(sysctl -n hw.memsize) / 1073741824 ))
choice="${MODEL:-$([[ $ram_gb -ge 16 ]] && echo e4b || echo e2b)}"
case "$choice" in
  e4b) size="E4B" ;;
  e2b) size="E2B" ;;
  *) echo "MODEL must be e4b or e2b"; exit 1 ;;
esac
target="$MODELS/gemma-4-$size-it-Q4_K_M.gguf"
echo "This Mac has ${ram_gb} GB: Gemma 4 $size, 4-bit (Q4_K_M)."
if [[ -s "$target" ]]; then
  echo "Already there: $(basename "$target")"
else
  url=""
  for repo in "ggml-org/gemma-4-$size-it-GGUF" "unsloth/gemma-4-$size-it-GGUF"; do
    # The repo's file list → the Q4_K_M file (names differ a little between publishers).
    file=$(curl -fsL "https://huggingface.co/api/models/$repo/tree/main" 2>/dev/null \
      | grep -o '"path":"[^"]*Q4_K_M[^"]*\.gguf"' | grep -v -i mmproj | head -1 | cut -d'"' -f4 || true)
    if [[ -n "$file" ]]; then url="https://huggingface.co/$repo/resolve/main/$file"; break; fi
  done
  [[ -n "$url" ]] || { echo "Couldn't find a Gemma 4 $size Q4_K_M GGUF on Hugging Face. Put any .gguf chat model in: $MODELS"; exit 1; }
  echo "From $url"
  curl -fL --progress-bar -C - -o "$target.part" "$url"
  mv "$target.part" "$target"
fi

say "4/4 speech-to-text model"
# Quality over speed (owner, 25 Nov 2026): Whisper large-v3, full precision (3.1 GB) on 16 GB; on 8 GB its 5-bit
# version (1.1 GB), still large-v3, so both fit next to the chat model.
stt="${STT:-$([[ $ram_gb -ge 16 ]] && echo ggml-large-v3.bin || echo ggml-large-v3-q5_0.bin)}"
if [[ -s "$MODELS/$stt" ]]; then
  echo "Already there: $stt"
else
  echo "Whisper large-v3 ($stt)"
  curl -fL --progress-bar -C - -o "$MODELS/$stt.part" "$WHISPER_MODELS/$stt"
  mv "$MODELS/$stt.part" "$MODELS/$stt"
fi
rm -rf "$MODELS/voices"   # from the first version (read-aloud, removed)

say "Done. Models in: $MODELS"
du -sh "$MODELS"/* 2>/dev/null || true
echo
echo "Next: npm run tauri dev   (the first build compiles llama.cpp: a few minutes)"
echo "      npm run tauri:mac   (the .app/.dmg with the AI libraries inside)"
