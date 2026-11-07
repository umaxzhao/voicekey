#!/bin/bash
# 构建 Ubuntu / Linux 版 VoiceKey
#
# 依赖（Ubuntu 22.04+）：
#   sudo apt install build-essential cmake pkg-config \
#     libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
#     librsvg2-dev libasound2-dev xdotool
#
# 需要 Rust stable 与 Node 22+。
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null; then
    echo "未找到 cargo，请先安装 Rust：https://rustup.rs" >&2
    exit 1
fi

# 有些机器只装了运行时库，开发用的 .pc 在 Homebrew/Linuxbrew 里。
if [ -d "$HOME/.linuxbrew/opt" ]; then
    BREW_PC=""
    for d in "$HOME"/.linuxbrew/opt/*/lib/pkgconfig; do
        [ -d "$d" ] && BREW_PC="${BREW_PC}${d}:"
    done
    export PKG_CONFIG_PATH="/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig:${BREW_PC}${PKG_CONFIG_PATH:+$PKG_CONFIG_PATH:}$HOME/.linuxbrew/lib/pkgconfig"
fi

missing=()
for pc in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 alsa ayatana-appindicator3-0.1; do
    pkg-config --exists "$pc" || missing+=("$pc")
done
if [ ${#missing[@]} -ne 0 ]; then
    echo "缺少开发库：${missing[*]}" >&2
    echo "Ubuntu/Debian 安装：" >&2
    echo "  sudo apt install build-essential cmake pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev xdotool" >&2
    exit 1
fi

if ! command -v xdotool >/dev/null; then
    echo "提示：未安装 xdotool，文字上屏会退化为剪贴板 + Ctrl+V，且候选框无法把焦点还给原窗口。" >&2
    echo "     建议：sudo apt install xdotool" >&2
fi

if [ -n "${FCITX5_DEV_PREFIX:-}" ] || pkg-config --exists Fcitx5Core 2>/dev/null; then
    ./linux/fcitx5/build.sh
else
    echo "提示：未找到 Fcitx5 开发包，跳过 Fcitx5 模块（deb 中不会包含 Fcitx5 集成）。" >&2
    echo "      需要时安装：sudo apt install libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev" >&2
fi

cd app
if [ ! -d node_modules ]; then
    npm ci
fi
npm run build
cd ..

# 必须启用 custom-protocol，否则 Tauri 会把 release 二进制当成 dev 模式并去连 localhost:1420。
cargo build --release -p voicekey --features tauri/custom-protocol
cargo build --release -p voicekey --bin voicekey-asr
mkdir -p app/src-tauri/resources
cp target/release/voicekey-asr app/src-tauri/resources/voicekey-asr

echo
echo "已生成：target/release/voicekey"
echo "已生成：target/release/voicekey-asr（Fcitx5 使用的轻量识别入口）"
echo "运行：  ./target/release/voicekey"
echo "打包：  cd app && npx tauri build --bundles deb"
echo "IBus：  ./linux/ibus/install.sh（注册为输入法，Wayland 推荐）"
