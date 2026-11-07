# VoiceKey on Ubuntu / Linux

这是 VoiceKey 的 Linux（Ubuntu）移植版。桌面端原本只支持 macOS 和 Windows，本移植补齐了
Linux 平台层：全局快捷键、模拟按键、剪贴板粘贴、托盘、麦克风采集和 deb 打包。

## 支持范围

- 预编译 deb：Ubuntu 26.04+（glibc 2.43+），x86_64
- 源码构建：Ubuntu 24.04+（需要 libwebkit2gtk-4.1）
- **Fcitx5 全局语音模块**：保留现有输入法，按住快捷键即可说话上屏
- 独立托盘模式：**X11 完整可用**；Wayland 下默认走 XWayland/XTest，对 XWayland 应用有效
- 原生 Wayland 应用可试实验路径：`VOICEKEY_WAYLAND_YDOTOOL=1 voicekey`
  （需要 `wl-clipboard` + `ydotool`，通过 uinput 注入；可能与 Toshy/XWayKeyz 等键盘重映射工具冲突）
- 全局按键监听走 evdev，X11 与 Wayland 都可以；没有 `input` 权限时回退 XRecord（仅 X11）
- Wayland 下不支持逐字上屏，会在说话结束后一次性粘贴

## 一键安装（推荐）

从 [Releases](https://github.com/umaxzhao/voicekey/releases) 下载 `voicekey-linux-1.9.0-amd64.tar.gz`，解压后运行：

```bash
tar -xzf voicekey-linux-1.9.0-amd64.tar.gz
cd voicekey-linux-1.9.0-amd64
./install-voicekey.sh
```

脚本会完成：安装 deb、下载微信离线模型、写入 Fcitx5 配置、重启 Fcitx5 并检查模块是否加载。

常用选项：

```bash
./install-voicekey.sh --hold Super_L   # 用左 Super 说话（会自动处理 GNOME 概览冲突）
./install-voicekey.sh --online         # Fcitx5 跟随图形界面的渠道
./install-voicekey.sh --no-model       # 不下载离线模型
./install-voicekey.sh --check          # 只检查环境
```

要求：Ubuntu 26.04+、x86_64、Fcitx5。

## 安装依赖

```bash
sudo apt update
sudo apt install build-essential cmake pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libasound2-dev xdotool
```

还需要：

- Rust stable（<https://rustup.rs>）
- Node.js 22+

## 构建

```bash
./build-linux.sh
```

或手动：

```bash
cd app
npm ci
npm run build
cd ..
cargo build --release -p voicekey
```

产物：`target/release/voicekey`。

## 运行

```bash
./target/release/voicekey
```

首次启动会打开引导页。默认快捷键与 Windows 版一致：

| 设置项 | 默认值 |
| --- | --- |
| 长按说话 | 右 Alt |
| 点按说话 | 右 Ctrl |
| 静音自动结束 | 1.5 秒 |
| 边说边上屏 | 开 |

## Fcitx5 全局语音模块（推荐）

deb 会把 `voicekey.so` 安装到 Fcitx5 的 addon 目录，并注册为 **Module**（不是新的输入法源）。
它在你现有的拼音/五笔等输入法里都能用，不需要切换输入法，也不会替换 Fcitx5。

```bash
sudo apt install ./target/release/bundle/deb/VoiceKey_*.deb
fcitx5 -r
```

然后在任意输入框里**按住右 Alt** 说话，松开后由 `voicekey --asr` 识别并直接提交。
默认长按右 Alt；如果右 Alt 是 AltGr 且你需要用它输入特殊字符，可以改成别的键：

```ini
# ~/.config/voicekey/fcitx5.conf
[VoiceKey]
Channel=doubao
Device=@DEFAULT_SOURCE@
Command=/usr/bin/voicekey
HoldKey=Control_R
```

`HoldKey` 支持 `Alt_R`、`ISO_Level3_Shift`、`Control_R`、`F9` 等 Fcitx keysym 名称。
`Device` 为空或 `@...` 时用 `parec` 录系统默认源；写 ALSA PCM 名（如 `default`、`hw:1,0`）
时用 `arecord`。

源码构建 Fcitx5 模块需要：

```bash
sudo apt install libfcitx5core-dev libfcitx5config-dev libfcitx5utils-dev
./linux/fcitx5/build.sh
```

模块加载后可在 `fcitx5 -r` 日志中看到 `Loaded addon voicekey`。

## 作为 IBus 输入法使用（可选）

除了独立的托盘应用，还可以把 VoiceKey 注册成 IBus 输入法：切到「VoiceKey 语音输入」后，
按住右 Alt 说话，松开后由输入法直接 `commit_text` 给当前应用，不需要剪贴板/XTest，
所以原生 Wayland 应用也能用。

deb 安装会自动注册组件（`/usr/share/ibus/component/voicekey.xml`）。源码安装执行：

```bash
./linux/ibus/install.sh
ibus restart
```

然后到「设置 → 键盘 → 输入源」添加 **VoiceKey 语音输入**，用 Super+Space 切换过去。
识别渠道读取应用设置里的当前渠道，也可以写 `~/.config/voicekey/ibus.json` 覆盖：

```json
{
  "channel": "doubao",
  "device": "@DEFAULT_SOURCE@",
  "cmd": "/usr/bin/voicekey"
}
```

> 使用 IBus 模式时右 Alt 会被输入法占用；切回原来的输入法即可正常使用。
> 需要 `python3-gi`、`gir1.2-ibus-1.0`，以及 `parec`（pulseaudio-utils）或
> `arecord`（alsa-utils）之一。
>
> **如果当前桌面使用 Fcitx5（Ubuntu 上很常见），IBus 组件不会被加载。**
> 这时用独立托盘模式：想要对原生 Wayland 应用稳定上屏，建议登录时选择
> “Ubuntu on Xorg”；否则可试 `VOICEKEY_WAYLAND_YDOTOOL=1 voicekey`。
> 也可以用 `fcitx5-remote -n` 确认当前输入法框架。

无界面识别也可以直接调用：

```bash
voicekey --asr doubao test.wav   # WAV 需 16kHz 单声道
```

## 按键权限（重要）

Linux 下要“吞掉”快捷键（例如自定义组合键里的普通键），需要读取 `/dev/input/event*`
并写入 `/dev/uinput`：

```bash
sudo usermod -aG input "$USER"
```

注销并重新登录后生效。没有该权限时代码会自动回退到 XRecord 监听：快捷键仍然可用，
但组合键里的普通键可能同时传给前台应用。

如果 `/dev/uinput` 仍不可写（部分发行版默认不是 `root:input 0660`），加一条 udev 规则：

```bash
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/99-uinput.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

## 打包 deb

```bash
cd app
npx tauri build --bundles deb
```

产物在 `target/release/bundle/deb/VoiceKey_<版本>_amd64.deb`，安装：

```bash
sudo apt install ./target/release/bundle/deb/VoiceKey_*.deb
```

## 与 macOS / Windows 版的差异

- 候选框没有插入符位置（Linux 取焦点插入符需要 AT-SPI 等额外协议），会显示在当前屏幕
  底部居中；单渠道声波浮层不受影响。
- 录音时自动压低系统音量（ducking）暂未实现，设置项会安全地不起作用。
- 托盘、开机启动、单实例、应用内更新（Tauri 插件）沿用跨平台实现；Linux 更新渠道需要
  自行提供 `latest.json` 与 Linux 安装包。
- 文字上屏优先调用 `xdotool`；没有安装时退化为剪贴板 + Ctrl+V（`xdotool` 还用于把焦点
  还给原来的窗口，建议安装）。
- evdev grab 模式下运行中热插拔键盘，新键盘可能暂时没有反应；重启 VoiceKey 即可恢复。
  也可以用 `VOICEKEY_LINUX_NO_GRAB=1 voicekey` 启动，改用 XRecord（不吞键，但热插拔安全）。
