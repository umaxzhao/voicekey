#!/usr/bin/python3
"""VoiceKey IBus 引擎：按住右 Alt 录音，松开后调用 `voicekey --asr` 识别并上屏。

安装后需要在“设置 → 键盘 → 输入源”里添加 “VoiceKey 语音输入”，
再用 Super+Space 切到它。录音设备默认使用系统默认输入源。

可选配置 ~/.config/voicekey/ibus.json：
{
  "channel": "doubao",     // doubao/wetype/qwen/baidu/sogou/iflytek/chatterfly/wetypeoffline
  "device": "@DEFAULT_SOURCE@",  // parec 源名；arecord 时是 ALSA PCM 名
  "cmd": "/usr/bin/voicekey"     // voicekey 可执行文件路径
}
"""

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import wave

import gi

gi.require_version("IBus", "1.0")
from gi.repository import GLib, IBus  # noqa: E402

ENGINE_NAME = "voicekey"
COMPONENT_NAME = "org.freedesktop.IBus.VoiceKey"
RELEASE_MASK = getattr(IBus, "RELEASE_MASK", 1 << 30)
ALT_R = 0xFFEA
ISO_LEVEL3_SHIFT = 0xFE03
HOLD_KEYCODE = 108  # X11 / evdev 的右 Alt

SETTINGS_PATH = os.path.join(
    os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share")),
    "VoiceKey",
    "settings.json",
)
CONFIG_PATH = os.path.join(
    os.environ.get("XDG_CONFIG_HOME", os.path.expanduser("~/.config")),
    "voicekey",
    "ibus.json",
)

CHANNELS = {
    "doubao",
    "wetype",
    "qwen",
    "baidu",
    "sogou",
    "iflytek",
    "chatterfly",
    "wetypeoffline",
}


def load_config():
    cfg = {"channel": "doubao", "device": None, "cmd": "voicekey"}
    try:
        with open(SETTINGS_PATH, "r", encoding="utf-8") as f:
            settings = json.load(f)
        channel = settings.get("channel")
        if isinstance(channel, str) and channel in CHANNELS:
            cfg["channel"] = channel
    except (OSError, ValueError):
        pass
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as f:
            extra = json.load(f)
        if isinstance(extra, dict):
            cfg.update(extra)
    except (OSError, ValueError):
        pass

    if cfg.get("channel") not in CHANNELS:
        cfg["channel"] = "doubao"

    cmd = cfg.get("cmd") or "voicekey"
    if os.path.sep not in cmd:
        cmd = shutil.which(cmd) or "/usr/bin/voicekey"
    cfg["cmd"] = cmd
    return cfg


class VoiceKeyEngine(IBus.Engine):
    def __init__(self, bus, object_path):
        super().__init__(bus, object_path)
        self._rec = None
        self._out = None
        self._pcm = None
        self._tmp = None
        self._down = False
        self._busy = False
        self._token = 0
        self._error_token = 0
        self._cfg = load_config()

    # ---- 按键 ----

    def do_process_key_event(self, keyval, keycode, state):
        is_hold = keyval in (ALT_R, ISO_LEVEL3_SHIFT) or keycode == HOLD_KEYCODE
        if not is_hold:
            return False
        released = bool(state & RELEASE_MASK)
        if not released:
            if not self._down:
                self._down = True
                self._start()
            return True
        if self._down:
            self._down = False
            self._stop()
        return True

    def do_disable(self):
        self._down = False
        self._stop(commit=False)
        return False

    def do_focus_out(self):
        # 录音中切走焦点就结束本次录音，避免麦克风一直占用；识别结果也作废。
        self._down = False
        self._stop(commit=False)
        return False

    # ---- 录音 ----

    def _record_cmd(self):
        device = self._cfg.get("device")
        if shutil.which("parec"):
            cmd = [
                "parec",
                "--format=s16le",
                "--rate=16000",
                "--channels=1",
                "--raw",
                "--device=" + (device or "@DEFAULT_SOURCE@"),
            ]
            return cmd
        cmd = ["arecord", "-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "raw", "-"]
        if device:
            cmd[1:1] = ["-D", device]
        return cmd

    def _start(self):
        if self._busy or self._rec is not None:
            return
        self._token += 1
        self._cfg = load_config()
        fd, path = tempfile.mkstemp(prefix="voicekey-", suffix=".pcm")
        os.close(fd)
        self._pcm = path
        self._tmp = None
        try:
            self._out = open(path, "wb")
            self._rec = subprocess.Popen(
                self._record_cmd(),
                stdout=self._out,
                stderr=subprocess.DEVNULL,
            )
        except OSError as e:
            self._close_rec()
            self._cleanup()
            self._show_error(f"无法录音：{e}")
            return
        self._show_preedit("🎤 录音中…")

    def _stop(self, commit=True):
        rec = self._rec
        if rec is None:
            if not commit:
                self._token += 1
                self._busy = False
                self.hide_preedit_text()
            return
        self._token += 1
        token = self._token
        self._rec = None
        try:
            rec.send_signal(signal.SIGINT)
            rec.wait(timeout=2)
        except subprocess.TimeoutExpired:
            rec.kill()
            try:
                rec.wait(timeout=1)
            except subprocess.TimeoutExpired:
                pass
        except OSError:
            pass
        self._close_rec()

        pcm = self._pcm
        if not pcm or not os.path.exists(pcm) or os.path.getsize(pcm) < 1600:
            self._cleanup()
            self._show_error("没有录到声音")
            return
        if not commit:
            self._cleanup()
            self.hide_preedit_text()
            return
        if self._busy:
            self._cleanup()
            return

        try:
            wav = self._wrap_wav(pcm)
        except OSError as e:
            self._cleanup()
            self._show_error(f"保存录音失败：{e}")
            return
        self._pcm = None
        self._tmp = wav
        self._busy = True
        self._show_preedit("识别中…")
        threading.Thread(
            target=self._recognize, args=(wav, token), daemon=True
        ).start()

    def _wrap_wav(self, pcm):
        """把录音器的裸 PCM 包成标准 WAV，避免流式 WAV 头在 SIGINT 后不完整。"""
        fd, wav = tempfile.mkstemp(prefix="voicekey-", suffix=".wav")
        os.close(fd)
        try:
            with open(pcm, "rb") as src, wave.open(wav, "wb") as dst:
                dst.setnchannels(1)
                dst.setsampwidth(2)
                dst.setframerate(16000)
                while True:
                    chunk = src.read(65536)
                    if not chunk:
                        break
                    if len(chunk) % 2:
                        chunk = chunk[:-1]
                    dst.writeframes(chunk)
        except OSError:
            try:
                os.unlink(wav)
            except OSError:
                pass
            raise
        try:
            os.unlink(pcm)
        except OSError:
            pass
        return wav

    def _close_rec(self):
        if self._out is not None:
            try:
                self._out.close()
            except OSError:
                pass
            self._out = None

    def _cleanup(self):
        self._close_rec()
        for path in (self._pcm, self._tmp):
            if path:
                try:
                    os.unlink(path)
                except OSError:
                    pass
        self._pcm = None
        self._tmp = None

    # ---- 识别 ----

    def _recognize(self, path, token):
        cfg = self._cfg
        cmd = [cfg["cmd"], "--asr", cfg["channel"], path]
        text = ""
        error = None
        try:
            out = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=90,
                check=False,
            )
            text = (out.stdout or "").strip()
            if out.returncode != 0 or not text:
                error = (out.stderr or "").strip() or "识别失败"
        except subprocess.TimeoutExpired:
            error = "识别超时"
        except OSError as e:
            error = f"无法启动 voicekey：{e}"
        finally:
            try:
                os.unlink(path)
            except OSError:
                pass

        def done():
            if token != self._token:
                return False
            self._busy = False
            self._tmp = None
            if error:
                self._show_error(error)
            else:
                self.hide_preedit_text()
                self.commit_text(IBus.Text.new_from_string(text))
            return False

        GLib.idle_add(done)

    # ---- 提示 ----

    def _show_preedit(self, text):
        self.update_preedit_text(IBus.Text.new_from_string(text), 0, True)

    def _show_error(self, message):
        self._error_token += 1
        token = self._error_token
        # 只显示一行，避免把后端的长错误全塞进输入框。
        line = message.splitlines()[0][:120] if message else "识别失败"
        self._show_preedit("VoiceKey: " + line)

        def hide():
            if self._error_token == token:
                self.hide_preedit_text()
            return False

        GLib.timeout_add(3000, hide)


def main():
    IBus.init()
    bus = IBus.Bus()
    if not bus.is_connected():
        print("VoiceKey IBus: 无法连接 IBus 总线", file=sys.stderr)
        return 1
    bus.request_name(COMPONENT_NAME, 0)
    factory = IBus.Factory.new(bus.get_connection())
    factory.add_engine(ENGINE_NAME, VoiceKeyEngine)
    IBus.main()
    return 0


if __name__ == "__main__":
    sys.exit(main())
