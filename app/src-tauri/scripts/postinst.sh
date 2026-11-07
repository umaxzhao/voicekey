#!/bin/sh
# VoiceKey deb postinst：确保 IBus 组件可执行。
set -e

if [ -f /usr/bin/voicekey-asr ]; then
    chmod 755 /usr/bin/voicekey-asr 2>/dev/null || true
fi
if [ -f /usr/lib/x86_64-linux-gnu/fcitx5/voicekey.so ]; then
    chmod 755 /usr/lib/x86_64-linux-gnu/fcitx5/voicekey.so 2>/dev/null || true
fi
if [ -f /usr/lib/VoiceKey/voicekey-hud.py ]; then
    chmod 755 /usr/lib/VoiceKey/voicekey-hud.py 2>/dev/null || true
fi
if [ -f /usr/lib/VoiceKey/voicekey_ibus.py ]; then
    chmod 755 /usr/lib/VoiceKey/voicekey_ibus.py
fi
if [ -f /usr/share/ibus/component/voicekey.xml ]; then
    chmod 644 /usr/share/ibus/component/voicekey.xml
fi

if [ -f /usr/share/fcitx5/addon/voicekey.conf ]; then
    cat <<'EOF'
VoiceKey: Fcitx5 模块已安装。
运行 `fcitx5 -r` 或注销重新登录后，在任意输入框中按住右 Alt 说话。
EOF
fi

if [ -f /usr/share/ibus/component/voicekey.xml ]; then
    cat <<'EOF'
VoiceKey: IBus 组件已安装。
如果使用 IBus，请在“设置 → 键盘 → 输入源”中添加“VoiceKey 语音输入”，
然后运行 `ibus restart` 或注销重新登录。
EOF
fi

exit 0
