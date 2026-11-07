#!/bin/bash
# 构建 Fcitx5 VoiceKey 模块。
#
# 依赖：cmake、g++、libfcitx5core-dev、libfcitx5config-dev、libfcitx5utils-dev
# 如果开发包不在系统默认路径，可设置 FCITX5_DEV_PREFIX 指向提取出的前缀。
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
BUILD="$DIR/build"

CMAKE_ARGS=(-S "$DIR" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release)
if [ -n "${FCITX5_DEV_PREFIX:-}" ]; then
    CMAKE_ARGS+=(-DCMAKE_PREFIX_PATH="$FCITX5_DEV_PREFIX")
fi

cmake "${CMAKE_ARGS[@]}"
cmake --build "$BUILD" -j"$(nproc)"

OUT="$ROOT/app/src-tauri/resources/fcitx5"
mkdir -p "$OUT"
cp "$BUILD/voicekey.so" "$OUT/voicekey.so"
cp "$DIR/data/voicekey.conf" "$OUT/voicekey-addon.conf"
echo "已生成 $OUT/voicekey.so"
