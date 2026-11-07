#!/bin/bash
# 把 VoiceKey 注册为 IBus 输入法（源码安装用；deb 安装已自动完成这一步）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SRC="$ROOT/app/src-tauri/resources/ibus"
BIN="${VOICEKEY_BIN:-$(command -v voicekey || true)}"
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    BIN="$ROOT/target/release/voicekey"
fi
if [ ! -x "$BIN" ]; then
    echo "找不到 voicekey 可执行文件；请先运行 ./build-linux.sh，或设置 VOICEKEY_BIN。" >&2
    exit 1
fi
if ! command -v ibus >/dev/null; then
    echo "未找到 ibus，请先安装：sudo apt install ibus" >&2
    exit 1
fi
if ! python3 -c 'import gi; gi.require_version("IBus", "1.0")' >/dev/null 2>&1; then
    echo "缺少 python3-gi / gir1.2-ibus-1.0，请先安装：sudo apt install python3-gi gir1.2-ibus-1.0" >&2
    exit 1
fi
if ! command -v parec >/dev/null && ! command -v arecord >/dev/null; then
    echo "需要 parec（pulseaudio-utils）或 arecord（alsa-utils）来录音。" >&2
    exit 1
fi

sudo install -d /usr/lib/VoiceKey /usr/share/ibus/component
sudo install -m 755 "$SRC/voicekey_ibus.py" /usr/lib/VoiceKey/voicekey_ibus.py
sudo install -m 644 "$SRC/voicekey.xml" /usr/share/ibus/component/voicekey.xml

# 让 IBus 引擎默认调用本项目的二进制（deb 安装不需要这一步）。
mkdir -p "$HOME/.config/voicekey"
if [ ! -f "$HOME/.config/voicekey/ibus.json" ]; then
    printf '{\n  "cmd": "%s"\n}\n' "$BIN" > "$HOME/.config/voicekey/ibus.json"
fi

echo "已安装 VoiceKey IBus 组件。"
echo "1) 运行：ibus restart（或注销重新登录）"
echo "2) 打开 设置 → 键盘 → 输入源，添加“VoiceKey 语音输入”"
echo "3) 切换到该输入法后，按住右 Alt 说话，松开上屏"
