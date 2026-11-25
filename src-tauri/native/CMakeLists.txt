# PhysioPulse on-device AI: one shared library (libphysiopulse_ai) with a small C interface
# (include/physiopulse_ai.h) for the Tauri app, with llama.cpp built into it statically. It ships in the app bundle
# (Contents/Frameworks).
#
# Built by src-tauri/build.rs through the `cmake` crate. Sources come from tools/local-ai/setup.sh:
#   vendor/llama.cpp     llama.cpp at a pinned tag (chat; its ggml is the one both use)
#   vendor/whisper.cpp   whisper.cpp at a pinned tag (speech to text; only its src/ is compiled, against llama.cpp's ggml)
cmake_minimum_required(VERSION 3.21)
project(physiopulse_ai C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)

set(PP_LLAMA_DIR   "${CMAKE_CURRENT_SOURCE_DIR}/vendor/llama.cpp"   CACHE PATH "llama.cpp source")
set(PP_WHISPER_DIR "${CMAKE_CURRENT_SOURCE_DIR}/vendor/whisper.cpp" CACHE PATH "whisper.cpp source")

foreach(dir "${PP_LLAMA_DIR}" "${PP_WHISPER_DIR}")
    if (NOT EXISTS "${dir}/CMakeLists.txt")
        message(FATAL_ERROR "${dir} not found. Run tools/local-ai/setup.sh first.")
    endif()
endforeach()

# llama.cpp: only the library and `common` (for the models' jinja chat templates). No tools, servers, network.
set(BUILD_SHARED_LIBS      OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_COMMON     ON  CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS      OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS      OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES   OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER     OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_APP        OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_MTMD       OFF CACHE BOOL "" FORCE)
set(LLAMA_OPENSSL          OFF CACHE BOOL "" FORCE)
if (APPLE)
    # Metal for the GPU, its shaders inside the library; a generic arm64 build so it runs on any M1–M4 Mac.
    set(GGML_METAL               ON  CACHE BOOL "" FORCE)
    set(GGML_METAL_EMBED_LIBRARY ON  CACHE BOOL "" FORCE)
    set(GGML_NATIVE              OFF CACHE BOOL "" FORCE)
endif()
add_subdirectory("${PP_LLAMA_DIR}" llama.cpp EXCLUDE_FROM_ALL)

# whisper.cpp's library, compiled here against llama.cpp's ggml (two copies of ggml can't live in one program).
add_library(pp_whisper STATIC "${PP_WHISPER_DIR}/src/whisper.cpp" "${PP_WHISPER_DIR}/src/aneforge/whisper-aneforge.cpp")
target_include_directories(pp_whisper PUBLIC "${PP_WHISPER_DIR}/include" PRIVATE "${PP_WHISPER_DIR}/src")
target_compile_definitions(pp_whisper PRIVATE WHISPER_VERSION="pinned")
target_link_libraries(pp_whisper PUBLIC ggml)

add_library(physiopulse_ai SHARED src/chat.cpp src/stt.cpp)
target_include_directories(physiopulse_ai PUBLIC include)
target_link_libraries(physiopulse_ai PRIVATE llama-common llama pp_whisper)

# Found through the app's rpath (Contents/Frameworks in the bundle).
if (APPLE)
    set_target_properties(physiopulse_ai PROPERTIES INSTALL_NAME_DIR "@rpath" BUILD_WITH_INSTALL_RPATH ON)
endif()

install(TARGETS physiopulse_ai LIBRARY DESTINATION lib RUNTIME DESTINATION bin ARCHIVE DESTINATION lib)
install(FILES include/physiopulse_ai.h DESTINATION include)
