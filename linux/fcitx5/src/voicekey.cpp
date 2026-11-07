#include "voicekey.h"

#include <algorithm>
#include <chrono>
#include <cctype>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <regex>
#include <sstream>
#include <thread>
#include <vector>

#include <fcntl.h>
#include <signal.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

#include <fcitx/addonfactory.h>
#include <fcitx/addonmanager.h>
#include <fcitx/event.h>
#include <fcitx/inputpanel.h>
#include <fcitx/text.h>
#include <fcitx/userinterface.h>
#include <fcitx-utils/eventdispatcher.h>
#include <fcitx-utils/handlertable.h>
#include <fcitx-utils/keysym.h>
#include <fcitx-utils/log.h>

namespace voicekey {
namespace {

constexpr fcitx::KeySym kAltR = FcitxKey_Alt_R;
constexpr fcitx::KeySym kIsoLevel3 = FcitxKey_ISO_Level3_Shift;

bool validChannel(const std::string &channel) {
    static const char *channels[] = {
        "doubao", "wetype", "qwen", "baidu",
        "sogou",  "iflytek", "chatterfly", "wetypeoffline",
    };
    for (const char *candidate : channels) {
        if (channel == candidate) {
            return true;
        }
    }
    return false;
}

char lowerAscii(char c) {
    return static_cast<char>(
        std::tolower(static_cast<unsigned char>(c)));
}

int modifierFamily(fcitx::KeySym value) {
    switch (value) {
    case FcitxKey_Control_L:
    case FcitxKey_Control_R:
        return 1;
    case FcitxKey_Alt_L:
    case FcitxKey_Alt_R:
    case FcitxKey_ISO_Level3_Shift:
        return 2;
    case FcitxKey_Shift_L:
    case FcitxKey_Shift_R:
        return 3;
    case FcitxKey_Super_L:
    case FcitxKey_Super_R:
    case FcitxKey_Meta_L:
    case FcitxKey_Meta_R:
        return 4;
    default:
        return 0;
    }
}

} // namespace

VoiceKeyModule::VoiceKeyModule(fcitx::AddonManager *manager) {
    instance_ = manager ? manager->instance() : nullptr;
    if (!instance_) {
        return;
    }
    loadConfig();
    watcher_ = instance_->watchEvent(
        fcitx::EventType::InputContextKeyEvent,
        fcitx::EventWatcherPhase::PreInputMethod,
        [this](fcitx::Event &event) {
            handleKey(static_cast<fcitx::KeyEvent &>(event));
        });
    focusWatcher_ = instance_->watchEvent(
        fcitx::EventType::InputContextFocusOut,
        fcitx::EventWatcherPhase::PreInputMethod,
        [this](fcitx::Event &) {
            if (state_ == State::Recording) {
                cancel();
            }
        });
    FCITX_INFO() << "VoiceKey Fcitx5 module loaded";
}

VoiceKeyModule::~VoiceKeyModule() { cancel(); }

void VoiceKeyModule::handleKey(fcitx::KeyEvent &event) {
    if (!instance_ || event.isVirtual()) {
        return;
    }
    const auto sym = event.key().sym();
    if (sym == FcitxKey_Super_L || sym == FcitxKey_Super_R ||
        sym == FcitxKey_Meta_L || sym == FcitxKey_Meta_R) {
        FCITX_INFO() << "VoiceKey super event sym=" << sym
                     << " release=" << event.isRelease();
    }
    if (!isHoldKey(sym)) {
        return;
    }
    FCITX_INFO() << "VoiceKey hold key "
                 << (event.isRelease() ? "release" : "press")
                 << " sym=" << event.key().sym()
                 << " code=" << event.key().code();
    if (!event.isRelease()) {
        if (state_ == State::Idle) {
            startRecording(event.inputContext());
        }
        event.filterAndAccept();
        return;
    }
    if (state_ == State::Recording) {
        stopRecording();
    }
    event.filterAndAccept();
}

bool VoiceKeyModule::isHoldKey(fcitx::KeySym sym) const {
    if (sym == holdSym_) {
        return true;
    }
    // 同一个修饰键的左右两侧、以及 Super/Meta 的不同叫法都视为同一个键。
    // 这样用户按左 Ctrl 或右 Ctrl 都能触发，不依赖桌面报告哪一侧。
    const int wanted = modifierFamily(holdSym_);
    return wanted != 0 && wanted == modifierFamily(sym);
}

void VoiceKeyModule::loadHudCommand() {
    hudCommand_.clear();
    const char *home = std::getenv("HOME");
    const std::string local =
        std::string(home ? home : "") + "/.local/share/voicekey/voicekey-hud.py";
    const std::string system = "/usr/lib/VoiceKey/voicekey-hud.py";
    for (const auto &path : {system, local}) {
        if (!path.empty() && access(path.c_str(), F_OK) == 0) {
            hudCommand_ = "/usr/bin/python3 " + shellQuote(path);
            return;
        }
    }
}

void VoiceKeyModule::startHud(const std::string &pcm) {
    if (hudCommand_.empty()) {
        return;
    }
    if (!hud_) {
        const std::string command = "env GDK_BACKEND=x11 " + hudCommand_;
        hud_ = popen(command.c_str(), "w");
        if (!hud_) {
            return;
        }
    }
    // 把录音文件路径一起传给浮层，浮层实时读取 PCM 计算音量。
    fprintf(hud_, "listen %s\n", pcm.c_str());
    fflush(hud_);
}

void VoiceKeyModule::hudState(const char *state) {
    if (!hud_) {
        return;
    }
    fprintf(hud_, "%s\n", state);
    fflush(hud_);
    if (std::strcmp(state, "hide") == 0) {
        pclose(hud_);
        hud_ = nullptr;
    }
}

void VoiceKeyModule::hudMessage(const std::string &text) {
    if (!hud_) {
        return;
    }
    std::string line = text;
    for (char &c : line) {
        if (c == '\n' || c == '\r') {
            c = ' ';
        }
    }
    // 浮层只显示一行，过长的错误信息截断，避免撑破窗口。
    if (line.size() > 120) {
        std::size_t cut = 120;
        while (cut > 0 &&
               (static_cast<unsigned char>(line[cut]) & 0xC0) == 0x80) {
            --cut;
        }
        line.resize(cut);
        line += "…";
    }
    fprintf(hud_, "msg %s\n", line.c_str());
    fflush(hud_);
}

void VoiceKeyModule::scheduleHudHide(int ms) {
    const std::uint64_t token = session_;
    std::thread([this, token, ms]() {
        std::this_thread::sleep_for(std::chrono::milliseconds(ms));
        instance_->eventDispatcher().schedule([this, token]() {
            if (token == session_) {
                hudState("hide");
            }
        });
    }).detach();
}

void VoiceKeyModule::startRecording(fcitx::InputContext *ic) {
    if (!ic) {
        return;
    }
    loadConfig();

    char tmpl[] = "/tmp/voicekey-fcitx5-XXXXXX";
    const int fd = mkstemp(tmpl);
    if (fd < 0) {
        showError("无法创建临时录音文件");
        return;
    }
    close(fd);
    pcmPath_ = tmpl;

    if (!spawnRecorder(pcmPath_)) {
        unlink(pcmPath_.c_str());
        pcmPath_.clear();
        showError("无法启动录音程序（parec/arecord）");
        return;
    }

    ++session_;
    state_ = State::Recording;
    ic_ = ic->watch();
    FCITX_INFO() << "VoiceKey start recording, channel=" << channel_
                 << " command=" << command_;
    clearPreedit();
    startHud(pcmPath_);

    // 防止焦点切换后收不到按键松开事件，最长录音 2 分钟。
    const std::uint64_t token = session_;
    std::thread([this, token]() {
        std::this_thread::sleep_for(std::chrono::seconds(120));
        instance_->eventDispatcher().schedule([this, token]() {
            if (token == session_ && state_ == State::Recording) {
                cancel();
            }
        });
    }).detach();
}

bool VoiceKeyModule::spawnRecorder(const std::string &pcm) {
    const int fd = open(pcm.c_str(), O_WRONLY | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) {
        return false;
    }
    const pid_t pid = fork();
    if (pid < 0) {
        close(fd);
        return false;
    }
    if (pid == 0) {
        dup2(fd, STDOUT_FILENO);
        close(fd);
        const int nullfd = open("/dev/null", O_WRONLY);
        if (nullfd >= 0) {
            dup2(nullfd, STDERR_FILENO);
            close(nullfd);
        }
        if (device_.empty() || device_.front() == '@') {
            const std::string dev =
                device_.empty() ? "@DEFAULT_SOURCE@" : device_;
            execlp("parec", "parec", "--format=s16le", "--rate=16000",
                   "--channels=1", "--raw", "--latency-msec=80",
                   (std::string("--device=") + dev).c_str(),
                   static_cast<char *>(nullptr));
            execlp("arecord", "arecord", "-q", "-f", "S16_LE", "-r", "16000",
                   "-c", "1", "-t", "raw", static_cast<char *>(nullptr));
        } else {
            execlp("arecord", "arecord", "-q", "-D", device_.c_str(), "-f",
                   "S16_LE", "-r", "16000", "-c", "1", "-t", "raw",
                   static_cast<char *>(nullptr));
            execlp("parec", "parec", "--format=s16le", "--rate=16000",
                   "--channels=1", "--raw", "--latency-msec=80",
                   (std::string("--device=") + device_).c_str(),
                   static_cast<char *>(nullptr));
        }
        _exit(127);
    }
    pid_ = pid;
    return true;
}

void VoiceKeyModule::stopRecording() {
    const pid_t pid = pid_;
    pid_ = -1;
    const std::string pcm = pcmPath_;
    pcmPath_.clear();
    auto ref = ic_;
    const std::uint64_t token = session_;
    const int tail = tailMs_;

    state_ = State::Recognizing;
    FCITX_INFO() << "VoiceKey stop recording, tail=" << tail << "ms";
    clearPreedit();
    hudState("wait");

    std::thread([this, pid, pcm, ref, token, tail]() {
        if (pid > 0) {
            // 松开按键后继续录一小段尾音，避免最后一个字被截断。
            if (tail > 0) {
                std::this_thread::sleep_for(std::chrono::milliseconds(tail));
            }
            kill(pid, SIGINT);
            int status = 0;
            waitpid(pid, &status, 0);
        }

        std::string text;
        std::string error;
        struct stat st {};
        if (stat(pcm.c_str(), &st) != 0 || st.st_size < 1600) {
            error = "没有录到声音";
        } else {
            const std::string wav = pcm + ".wav";
            if (!writeWav(pcm, wav)) {
                error = "无法保存录音";
            } else if (!runAsr(wav, text, error)) {
                // runAsr already filled error
            }
            unlink(wav.c_str());
        }
        unlink(pcm.c_str());

        FCITX_INFO() << "VoiceKey asr result: " << (error.empty() ? text : error);
        instance_->eventDispatcher().schedule([this, ref, token, text, error]() {
            if (token != session_) {
                return;
            }
            state_ = State::Idle;
            auto *ic = ref.get();
            if (!ic) {
                hudState("hide");
                return;
            }
            if (!error.empty()) {
                // 失败/没识别到内容也放到浮层里，不再往输入框塞文字。
                clearPreeditOn(ref);
                hudMessage(error);
                scheduleHudHide(2400);
                return;
            }
            if (text.empty()) {
                clearPreeditOn(ref);
                hudMessage("没有识别到内容");
                scheduleHudHide(2200);
                return;
            }
            hudState("hide");
            clearPreeditOn(ref);
            ic->commitString(text);
        });
    }).detach();
}

void VoiceKeyModule::cancel() {
    ++session_;
    if (pid_ > 0) {
        kill(pid_, SIGINT);
        int status = 0;
        waitpid(pid_, &status, 0);
        pid_ = -1;
    }
    if (!pcmPath_.empty()) {
        unlink(pcmPath_.c_str());
        unlink((pcmPath_ + ".wav").c_str());
        pcmPath_.clear();
    }
    state_ = State::Idle;
    hudState("hide");
    clearPreedit();
}

void VoiceKeyModule::setPreedit(const std::string &text) {
    setPreeditOn(ic_, text);
}

void VoiceKeyModule::clearPreedit() { setPreeditOn(ic_, ""); }

void VoiceKeyModule::setPreeditOn(
    const fcitx::TrackableObjectReference<fcitx::InputContext> &ref,
    const std::string &text) {
    auto *ic = ref.get();
    if (!ic) {
        return;
    }
    fcitx::Text value;
    if (!text.empty()) {
        value.append(text);
    }
    ic->inputPanel().setClientPreedit(value);
    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}

void VoiceKeyModule::clearPreeditOn(
    const fcitx::TrackableObjectReference<fcitx::InputContext> &ref) {
    setPreeditOn(ref, "");
}

void VoiceKeyModule::showError(const std::string &message) {
    showErrorOn(ic_, message);
}

void VoiceKeyModule::showErrorOn(
    const fcitx::TrackableObjectReference<fcitx::InputContext> &ref,
    const std::string &message) {
    setPreeditOn(ref, message);
    const std::uint64_t token = session_;
    std::thread([this, ref, token]() {
        std::this_thread::sleep_for(std::chrono::seconds(3));
        instance_->eventDispatcher().schedule([this, ref, token]() {
            if (token != session_) {
                return;
            }
            clearPreeditOn(ref);
        });
    }).detach();
}

bool VoiceKeyModule::runAsr(const std::string &wav, std::string &text,
                            std::string &error) {
    const std::string command = shellQuote(command_) + " --asr " +
                                shellQuote(channel_) + " " + shellQuote(wav) +
                                " 2>&1";
    FILE *pipe = popen(command.c_str(), "r");
    if (!pipe) {
        error = "无法启动 voicekey";
        return false;
    }

    std::string output;
    char buffer[4096];
    while (fgets(buffer, sizeof(buffer), pipe)) {
        output += buffer;
    }
    const int status = pclose(pipe);

    std::string last;
    std::istringstream lines(output);
    std::string line;
    while (std::getline(lines, line)) {
        line = trim(line);
        if (!line.empty()) {
            last = line;
        }
    }

    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        error = last.empty() ? "识别进程失败" : last;
        return false;
    }
    if (last.empty()) {
        error = "没有识别到内容";
        return false;
    }
    text = last;
    return true;
}

std::string VoiceKeyModule::shellQuote(const std::string &value) {
    std::string out = "'";
    for (const char c : value) {
        if (c == '\'') {
            out += "'\\''";
        } else {
            out += c;
        }
    }
    out += "'";
    return out;
}

bool VoiceKeyModule::writeWav(const std::string &pcm,
                              const std::string &wav) {
    std::ifstream in(pcm, std::ios::binary | std::ios::ate);
    if (!in) {
        return false;
    }
    const auto total = static_cast<std::uint32_t>(in.tellg());
    if (total < 2) {
        return false;
    }
    // 末尾补一段数字静音，帮助离线模型确认一句话已经结束，避免最后一个字被截断。
    constexpr std::uint32_t kTailSilenceMs = 700;
    const std::uint32_t pcmSize = total & ~1U;
    const std::uint32_t silenceBytes = 16000u * 2u * kTailSilenceMs / 1000u;
    const std::uint32_t dataSize = pcmSize + silenceBytes;
    in.seekg(0, std::ios::beg);

    std::ofstream out(wav, std::ios::binary | std::ios::trunc);
    if (!out) {
        return false;
    }
    auto u16 = [&out](std::uint16_t value) {
        out.put(static_cast<char>(value & 0xff));
        out.put(static_cast<char>((value >> 8) & 0xff));
    };
    auto u32 = [&out](std::uint32_t value) {
        for (int i = 0; i < 4; ++i) {
            out.put(static_cast<char>((value >> (8 * i)) & 0xff));
        }
    };

    out.write("RIFF", 4);
    u32(36 + dataSize);
    out.write("WAVE", 4);
    out.write("fmt ", 4);
    u32(16);
    u16(1);      // PCM
    u16(1);      // mono
    u32(16000);  // sample rate
    u32(32000);  // byte rate
    u16(2);      // block align
    u16(16);     // bits per sample
    out.write("data", 4);
    u32(dataSize);

    std::vector<char> buffer(65536);
    std::uint32_t left = pcmSize;
    while (left > 0) {
        const auto want = std::min<std::uint32_t>(
            left, static_cast<std::uint32_t>(buffer.size()));
        in.read(buffer.data(), want);
        const auto got = static_cast<std::uint32_t>(in.gcount());
        if (got == 0) {
            break;
        }
        out.write(buffer.data(), got);
        left -= got;
    }

    std::vector<char> zeros(65536, 0);
    std::uint32_t leftSilence = silenceBytes;
    while (leftSilence > 0) {
        const auto want = std::min<std::uint32_t>(
            leftSilence, static_cast<std::uint32_t>(zeros.size()));
        out.write(zeros.data(), want);
        leftSilence -= want;
    }
    return out.good();
}

void VoiceKeyModule::loadConfig() {
    channel_ = "doubao";
    device_.clear();
    if (access("/usr/bin/voicekey-asr", X_OK) == 0) {
        command_ = "/usr/bin/voicekey-asr";
    } else if (access("/usr/bin/voicekey", X_OK) == 0) {
        command_ = "/usr/bin/voicekey";
    } else {
        command_ = "voicekey-asr";
    }
    holdSym_ = kAltR;
    tailMs_ = 600;
    loadHudCommand();

    const std::string settings = readFile(dataHome() + "/VoiceKey/settings.json");
    if (!settings.empty()) {
        std::smatch match;
        const std::regex re("\"channel\"\\s*:\\s*\"([A-Za-z0-9_-]+)\"");
        if (std::regex_search(settings, match, re) &&
            validChannel(match[1].str())) {
            channel_ = match[1].str();
        }
    }

    std::ifstream in(configHome() + "/voicekey/fcitx5.conf");
    std::string line;
    while (std::getline(in, line)) {
        line = trim(line);
        if (line.empty() || line.front() == '#' || line.front() == ';' ||
            line.front() == '[') {
            continue;
        }
        const auto eq = line.find('=');
        if (eq == std::string::npos) {
            continue;
        }
        std::string key = trim(line.substr(0, eq));
        const std::string value = trim(line.substr(eq + 1));
        std::transform(key.begin(), key.end(), key.begin(), lowerAscii);
        if (key == "channel") {
            if (validChannel(value)) {
                channel_ = value;
            }
        } else if (key == "device") {
            device_ = value;
        } else if (key == "command") {
            if (!value.empty()) {
                command_ = value;
            }
        } else if (key == "holdkey") {
            const auto sym = fcitx::Key::keySymFromString(value);
            if (sym != FcitxKey_None) {
                holdSym_ = sym;
            }
        } else if (key == "tailms") {
            try {
                const int tail = std::stoi(value);
                if (tail >= 0 && tail <= 2000) {
                    tailMs_ = tail;
                }
            } catch (...) {
                // 忽略非法值，保留默认尾音
            }
        } else if (key == "hudcommand") {
            if (!value.empty()) {
                hudCommand_ = value;
            }
        }
    }
}

std::string VoiceKeyModule::dataHome() {
    if (const char *value = std::getenv("XDG_DATA_HOME");
        value && *value) {
        return value;
    }
    if (const char *home = std::getenv("HOME"); home && *home) {
        return std::string(home) + "/.local/share";
    }
    return {};
}

std::string VoiceKeyModule::configHome() {
    if (const char *value = std::getenv("XDG_CONFIG_HOME");
        value && *value) {
        return value;
    }
    if (const char *home = std::getenv("HOME"); home && *home) {
        return std::string(home) + "/.config";
    }
    return {};
}

std::string VoiceKeyModule::readFile(const std::string &path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        return {};
    }
    std::ostringstream out;
    out << in.rdbuf();
    return out.str();
}

std::string VoiceKeyModule::trim(std::string value) {
    auto notSpace = [](unsigned char c) { return !std::isspace(c); };
    value.erase(value.begin(),
                std::find_if(value.begin(), value.end(), notSpace));
    value.erase(
        std::find_if(value.rbegin(), value.rend(), notSpace).base(),
        value.end());
    return value;
}

} // namespace voicekey

namespace {
class VoiceKeyFactory : public fcitx::AddonFactory {
public:
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new voicekey::VoiceKeyModule(manager);
    }
};
} // namespace

FCITX_ADDON_FACTORY(VoiceKeyFactory);
