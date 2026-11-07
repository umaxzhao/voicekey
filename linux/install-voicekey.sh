#!/usr/bin/env bash
# VoiceKey Linux 一键安装脚本（Ubuntu 26.04+ / x86_64）
#
#   ./install-voicekey.sh                 # 安装 deb、下载离线模型、配置 Fcitx5
#   ./install-voicekey.sh --hold Super_L  # 用左 Super 作为语音键（会处理 GNOME 概览冲突）
#   ./install-voicekey.sh --online        # Fcitx5 跟随图形界面的渠道，不固定离线
#   ./install-voicekey.sh --no-model      # 不下载离线模型（仍可用在线渠道）
#   ./install-voicekey.sh --autostart     # 登录时启动托盘应用
#   ./install-voicekey.sh --check         # 只检查环境，不安装
#
set -euo pipefail

HOLD="F9"
CHANNEL="wetypeoffline"
DOWNLOAD_MODEL=1
FIX_OVERLAY=1
AUTOSTART=0
CHECK_ONLY=0
FORCE_MODEL=0
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

usage() { sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --hold) HOLD="${2:-}"; shift 2 ;;
    --online) CHANNEL=""; shift ;;
    --no-model) DOWNLOAD_MODEL=0; shift ;;
    --force-model) FORCE_MODEL=1; DOWNLOAD_MODEL=1; shift ;;
    --no-overlay) FIX_OVERLAY=0; shift ;;
    --autostart) AUTOSTART=1; shift ;;
    --check) CHECK_ONLY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知参数：$1（用 --help 查看）" ;;
  esac
done

case "$HOLD" in
  Super_L|Super_R|Control_L|Control_R|Alt_L|Alt_R|Shift_L|Shift_R|F5|F6|F7|F8|F9|F10|F11|F12) ;;
  *) die "不支持的语音键：$HOLD" ;;
esac

say "检查环境"
[ "$(uname -s)" = "Linux" ] || die "只支持 Linux"
[ "$(uname -m)" = "x86_64" ] || die "只支持 x86_64（当前：$(uname -m)）"

GLIBC="$(ldd --version 2>/dev/null | head -1 | awk '{print $NF}')"
[ -n "$GLIBC" ] || die "无法检测 glibc 版本"
if [ "$(printf '%s\n%s\n' "2.43" "$GLIBC" | sort -V | head -1)" != "2.43" ]; then
  die "需要 glibc >= 2.43（Ubuntu 26.04+），当前是 $GLIBC。请改用源码构建。"
fi
say "系统：$(. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME") / glibc $GLIBC"

DEB="$(find "$DIR" -maxdepth 1 -name 'VoiceKey_*_amd64.deb' -print | sort | tail -1)"
if [ -z "$DEB" ]; then
  DEB="$(find "$DIR/../target/release/bundle/deb" -maxdepth 1 -name 'VoiceKey_*_amd64.deb' -print 2>/dev/null | sort | tail -1)"
fi
[ -n "$DEB" ] && [ -f "$DEB" ] || die "找不到 VoiceKey_*_amd64.deb（请把它和本脚本放在同一目录）"
DEB="$(readlink -f "$DEB")"
say "安装包：$(basename "$DEB")"

if [ "$CHECK_ONLY" = 1 ]; then
  say "环境检查通过"
  exit 0
fi

command -v sudo >/dev/null || die "需要 sudo"
say "安装 VoiceKey（会提示输入 sudo 密码）"
sudo apt install -y "$DEB"

[ -x /usr/bin/voicekey ] || die "安装后没有找到 /usr/bin/voicekey"
[ -x /usr/bin/voicekey-asr ] || die "安装后没有找到 /usr/bin/voicekey-asr"

MODEL_DIR="$HOME/.local/share/VoiceKey/wtoffline"
MODEL_READY=0
[ -f "$MODEL_DIR/dict.decoder.utf8.txt" ] && ls "$MODEL_DIR"/*.xnet >/dev/null 2>&1 && MODEL_READY=1

install_model() (
  set -e
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  URL="https://download.z.weixin.qq.com/publish_words/v1/ime_asr_decoder_model/20260710220526250_ime_asr_decoder_model.apk"
  curl -L --fail --retry 3 --progress-bar -o "$TMP/model.apk" "$URL"
  echo "28386481eb94ff73b28ee3949c0cf690  $TMP/model.apk" | md5sum -c -
  python3 - "$TMP/model.apk" "$TMP/wtoffline" <<'PY'
import io, pathlib, sys, zipfile

apk, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(apk) as outer:
    inner = outer.read("ime_asr_decoder_model.zip")
with zipfile.ZipFile(io.BytesIO(inner)) as z:
    for info in z.infolist():
        name = pathlib.PurePosixPath(info.filename).name
        if name.endswith(".xnet") or name == "dict.decoder.utf8.txt":
            (out / name).write_bytes(z.read(info))
if not (out / "dict.decoder.utf8.txt").is_file() or not list(out.glob("*.xnet")):
    sys.exit("模型解包不完整")
PY
  mkdir -p "$HOME/.local/share/VoiceKey"
  rm -rf "$MODEL_DIR"
  mv "$TMP/wtoffline" "$MODEL_DIR"
)

if [ "$MODEL_READY" = 1 ] && [ "$FORCE_MODEL" = 0 ]; then
  say "离线模型已存在，跳过下载"
elif [ "$DOWNLOAD_MODEL" = 1 ]; then
  if ! command -v curl >/dev/null; then
    warn "未安装 curl，跳过离线模型；在线渠道仍可使用"
    CHANNEL=""
  else
    say "下载微信离线模型（约 100MB）"
    if install_model; then
      MODEL_READY=1
      say "离线模型已就绪"
    else
      warn "离线模型下载/解包失败，改用在线渠道；可稍后在 VoiceKey 设置里重试"
      CHANNEL=""
    fi
  fi
else
  warn "未下载离线模型；Fcitx5 会使用在线渠道，或稍后在 VoiceKey 设置里下载"
  CHANNEL=""
fi

say "写入 Fcitx5 配置（语音键：$HOLD）"
mkdir -p "$HOME/.config/voicekey"
CONF="$HOME/.config/voicekey/fcitx5.conf"
python3 - "$CONF" "$HOLD" "$CHANNEL" <<'PY'
import pathlib, sys

path = pathlib.Path(sys.argv[1])
hold, channel = sys.argv[2], sys.argv[3]
lines = path.read_text().splitlines() if path.exists() else []
out, keys, section = [], set(), False

for raw in lines:
    line = raw.strip()
    if line.startswith("[") and line.endswith("]"):
        if line == "[VoiceKey]":
            section = True
        out.append(raw)
        continue
    if "=" in line and not line.startswith(("#", ";")):
        key = line.split("=", 1)[0].strip().lower()
        if key in {"holdkey", "tailms", "command", "channel"}:
            keys.add(key)
            if key == "holdkey":
                out.append(f"HoldKey={hold}")
            elif key == "tailms":
                out.append("TailMs=600")
            elif key == "command":
                out.append("Command=/usr/bin/voicekey-asr")
            elif key == "channel" and channel:
                out.append(f"Channel={channel}")
            continue
    out.append(raw)

if not section:
    out.insert(0, "[VoiceKey]")
if "holdkey" not in keys:
    out.append(f"HoldKey={hold}")
if "tailms" not in keys:
    out.append("TailMs=600")
if "command" not in keys:
    out.append("Command=/usr/bin/voicekey-asr")
if channel and "channel" not in keys:
    out.append(f"Channel={channel}")

path.parent.mkdir(parents=True, exist_ok=True)
path.write_text("\n".join(out).rstrip() + "\n")
PY

if [ "$FIX_OVERLAY" = 1 ] && [ "$HOLD" = "Super_L" ] && command -v gsettings >/dev/null 2>&1; then
  CUR="$(gsettings get org.gnome.mutter overlay-key 2>/dev/null || true)"
  if [ "$CUR" = "'Super'" ] || [ "$CUR" = "'Super_L'" ]; then
    gsettings set org.gnome.mutter overlay-key 'Super_R' 2>/dev/null \
      && say "GNOME 活动概览已改到右 Super"
  fi
fi

if [ "$AUTOSTART" = 1 ]; then
  say "创建开机自启项"
  mkdir -p "$HOME/.config/autostart"
  cat > "$HOME/.config/autostart/VoiceKey.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=VoiceKey
Exec=env VOICEKEY_LINUX_NO_GRAB=1 /usr/bin/voicekey
X-GNOME-Autostart-enabled=true
Terminal=false
EOF
fi

say "重启 Fcitx5"
if command -v systemctl >/dev/null 2>&1 && systemctl --user is-active --quiet fcitx5 2>/dev/null; then
  systemctl --user restart fcitx5
elif command -v fcitx5 >/dev/null 2>&1; then
  fcitx5 -r >/dev/null 2>&1 || true
else
  warn "没有找到 Fcitx5；Fcitx5 全局语音不可用，但独立应用仍可使用"
fi

sleep 2
say "确认 Fcitx5 模块"
if journalctl --user -u fcitx5 -b --no-pager 2>/dev/null | grep -q "Loaded addon voicekey"; then
  say "VoiceKey 模块已加载"
else
  warn "暂未看到模块加载日志；如不生效，执行 fcitx5 -r 后再看："
  warn "  journalctl --user -u fcitx5 -b --no-pager | grep -i voicekey | tail"
fi

cat <<EOF

安装完成。

语音键：$HOLD
识别渠道：${CHANNEL:-跟随 VoiceKey 图形界面}

使用：
  1. 在任意输入框里按住 $HOLD 说话
  2. 松开后识别并上屏
  3. 修改语音键/尾音：打开 VoiceKey → Fcitx5 页

提示：
  - 如果 $HOLD 和你常用软件冲突，可在 VoiceKey 的 Fcitx5 页改键
  - 在线渠道在「识别」页切换；离线渠道不上传音频
  - 卸载：sudo apt remove voice-key
EOF
