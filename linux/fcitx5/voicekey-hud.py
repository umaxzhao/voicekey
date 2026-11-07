#!/usr/bin/python3
"""VoiceKey Fcitx5 录音/识别浮层：紧凑胶囊 + 横向渐变声波。

由 voicekey.so 启动，stdin 接收 listen / wait / hide。
"""

import array
import math
import os
import sys
import threading
import time

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GLib, Gtk

W, H = 196, 36
N = 28
state = "listen"
started = time.time()
win = None
label = None
text_label = None
stack = None
wave_box = None
bars = []
cur_w, cur_h = 196, 36
msg_token = 0
pcm_path = ""
pcm_pos = 0
level = 0.0
last_poll = 0.0

CYAN = (0x3E, 0xE7, 0xFF)
VIOLET = (0x7C, 0x6B, 0xFF)
PINK = (0xFF, 0x5F, 0xA2)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def bar_colors():
    colors = []
    for i in range(N):
        p = i / (N - 1) if N > 1 else 0.5
        colors.append(lerp(CYAN, VIOLET, p * 2) if p < 0.5 else lerp(VIOLET, PINK, (p - 0.5) * 2))
    return colors


BASE_CSS = """
window { background-color: transparent; }
#capsule {
  background-image: linear-gradient(135deg, rgba(26,28,45,0.97), rgba(36,38,62,0.94));
  border: 1px solid rgba(255,255,255,0.30);
  border-radius: 11px;
  box-shadow: 0 6px 18px rgba(0,0,0,0.45), inset 0 1px 0 rgba(255,255,255,0.12);
}
#caption {
  color: rgba(255,255,255,0.86);
  font-family: "Noto Sans CJK SC", "Ubuntu", sans-serif;
  font-size: 10px;
}
"""


def css_for_bars():
    lines = [BASE_CSS]
    for i, (r, g, b) in enumerate(bar_colors()):
        lines.append(
            f".bar{i} {{ background-color: rgb({r},{g},{b}); border-radius: 1px; }}"
        )
    return "\n".join(lines)


def place():
    display = Gdk.Display.get_default()
    monitor = display.get_primary_monitor() if display else None
    if monitor is None and display:
        monitor = display.get_monitor(0)
    if monitor:
        geo = monitor.get_geometry()
        win.move(geo.x + (geo.width - cur_w) // 2, geo.y + geo.height - cur_h - 72)


def read_level():
    """读取录音文件新增的 PCM，返回 0..1 的当前音量。"""
    global pcm_pos, level
    if not pcm_path:
        return 0.0
    try:
        with open(pcm_path, "rb") as f:
            f.seek(0, os.SEEK_END)
            size = f.tell()
            if size <= pcm_pos:
                return level * 0.8
            # 积压太多时只取最后约 200ms，避免波形滞后
            if size - pcm_pos > 9600:
                pcm_pos = size - 9600
            f.seek(pcm_pos)
            data = f.read(min(6400, size - pcm_pos))
            pcm_pos += len(data)
    except OSError:
        return level * 0.8

    usable = len(data) - (len(data) % 2)
    if usable < 2:
        return level * 0.8
    samples = array.array("h")
    samples.frombytes(data[:usable])
    if sys.byteorder != "little":
        samples.byteswap()
    n = len(samples)
    rms = math.sqrt(sum(v * v for v in samples) / n) / 32768.0
    # 环境底噪不驱动波形
    if rms < 0.008:
        return 0.0
    # 对数映射，让正常说话也能有明显的起伏
    return min(1.0, max(0.0, (math.log10(rms / 0.008) + 1.2) / 1.6))


def text_width(text):
    units = sum(11 if ord(ch) > 0x2E7F else 7 for ch in text)
    return max(78, min(340, units + 34))


def show_text(text):
    global cur_w, cur_h
    # 双保险：即使 Stack 切换失败，识别中也不出现声波。
    if wave_box is not None:
        wave_box.hide()
    text_label.set_text(text)
    stack.set_visible_child_name("text")
    cur_w, cur_h = text_width(text), H
    win.resize(cur_w, cur_h)
    place()
    win.show_all()


def hide_msg(token):
    if token == msg_token and state == "msg":
        win.hide()
    return False


def tick():
    global level, last_poll
    if win is None or not win.get_visible():
        return True
    now = time.time()
    if state == "listen" and now - last_poll >= 0.05:
        target = read_level()
        last_poll = now
        if target > level:
            level = level * 0.35 + target * 0.65
        else:
            level = level * 0.82 + target * 0.18
    elif state == "wait":
        level *= 0.86

    t = now - started
    for i, bar in enumerate(bars):
        p = i / (N - 1) if N > 1 else 0.5
        env = math.sin(math.pi * p) ** 0.7
        if level < 0.02:
            # 没有声音时保持静止的细线
            bh = 2.0
        else:
            vary = 0.55 + 0.45 * math.sin(t * 6.0 + i * 0.45)
            bh = max(2.0, (0.18 + 0.9 * level) * env * vary * (H - 14))
        bar.set_size_request(2, int(bh))
    return True


def apply_cmd(line):
    global state, started, pcm_path, pcm_pos, level, last_poll
    global msg_token, cur_w, cur_h
    parts = line.split(" ", 1)
    cmd = parts[0]
    if cmd == "listen":
        msg_token += 1
        pcm_path = parts[1].strip() if len(parts) > 1 else ""
        pcm_pos = 0
        level = 0.0
        last_poll = 0.0
        started = time.time()
        state = "listen"
        label.set_text("录音中")
        if wave_box is not None:
            wave_box.show()
        stack.set_visible_child_name("wave")
        cur_w, cur_h = W, H
        win.resize(cur_w, cur_h)
        place()
        win.show_all()
    elif cmd == "wait":
        state = "wait"
        show_text("识别中")
    elif cmd == "msg":
        msg_token += 1
        state = "msg"
        show_text(parts[1].strip() if len(parts) > 1 else "没有识别到内容")
        GLib.timeout_add(2600, hide_msg, msg_token)
    elif cmd == "hide":
        msg_token += 1
        state = "hide"
        win.hide()
    return False


def reader():
    try:
        for line in sys.stdin:
            GLib.idle_add(apply_cmd, line.strip())
    finally:
        GLib.idle_add(Gtk.main_quit)


def build():
    global win, label, text_label, stack, wave_box
    win = Gtk.Window(type=Gtk.WindowType.POPUP)
    win.set_decorated(False)
    win.set_skip_taskbar_hint(True)
    win.set_skip_pager_hint(True)
    win.set_keep_above(True)
    win.set_accept_focus(False)
    win.set_focus_on_map(False)
    win.set_default_size(W, H)
    win.set_app_paintable(True)
    screen = win.get_screen()
    visual = screen.get_rgba_visual() if screen else None
    if visual:
        win.set_visual(visual)

    capsule = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
    capsule.set_name("capsule")
    capsule.set_margin_top(2)
    capsule.set_margin_bottom(2)
    capsule.set_margin_start(2)
    capsule.set_margin_end(2)
    capsule.set_valign(Gtk.Align.FILL)

    stack = Gtk.Stack()
    stack.set_transition_type(Gtk.StackTransitionType.NONE)

    row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
    row.set_margin_start(20)
    row.set_margin_end(14)

    label = Gtk.Label(label="录音中")
    label.set_name("caption")
    label.set_valign(Gtk.Align.CENTER)
    label.set_size_request(30, -1)
    label.set_margin_end(4)
    row.pack_start(label, False, False, 0)

    wave = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=2)
    wave.set_valign(Gtk.Align.CENTER)
    wave.set_halign(Gtk.Align.CENTER)
    wave.set_hexpand(True)
    # 固定波形区域高度，避免最高的一根变化时整体上下抖动
    wave.set_size_request(-1, H - 14)
    for i in range(N):
        bar = Gtk.Box()
        bar.get_style_context().add_class(f"bar{i}")
        bar.set_size_request(2, 4)
        bar.set_valign(Gtk.Align.CENTER)
        bar.set_halign(Gtk.Align.CENTER)
        wave.pack_start(bar, False, False, 0)
        bars.append(bar)
    row.pack_start(wave, True, True, 0)
    wave_box = wave
    stack.add_named(row, "wave")

    text_label = Gtk.Label(label="识别中")
    text_label.set_name("caption")
    text_label.set_valign(Gtk.Align.CENTER)
    text_label.set_halign(Gtk.Align.CENTER)
    text_label.set_justify(Gtk.Justification.CENTER)
    text_label.set_margin_start(14)
    text_label.set_margin_end(14)
    stack.add_named(text_label, "text")

    capsule.pack_start(stack, True, True, 0)
    win.add(capsule)

    provider = Gtk.CssProvider()
    provider.load_from_data(css_for_bars().encode("utf-8"))
    Gtk.StyleContext.add_provider_for_screen(
        screen, provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    )

    win.connect("destroy", Gtk.main_quit)


def main():
    build()
    place()
    win.show_all()
    threading.Thread(target=reader, daemon=True).start()
    GLib.timeout_add(16, tick)
    Gtk.main()


if __name__ == "__main__":
    main()
