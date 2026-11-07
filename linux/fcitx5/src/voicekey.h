#pragma once

#include <cstdint>
#include <cstdio>
#include <memory>
#include <string>

#include <fcitx/addoninstance.h>
#include <fcitx/inputcontext.h>
#include <fcitx/instance.h>
#include <fcitx-utils/key.h>
#include <fcitx-utils/trackableobject.h>

namespace fcitx {
class AddonManager;
class KeyEvent;
} // namespace fcitx

namespace voicekey {

class VoiceKeyModule : public fcitx::AddonInstance {
public:
    explicit VoiceKeyModule(fcitx::AddonManager *manager);
    ~VoiceKeyModule() override;

    VoiceKeyModule(const VoiceKeyModule &) = delete;
    VoiceKeyModule &operator=(const VoiceKeyModule &) = delete;

private:
    enum class State {
        Idle,
        Recording,
        Recognizing,
    };

    void handleKey(fcitx::KeyEvent &event);
    void startRecording(fcitx::InputContext *ic);
    void stopRecording();
    void cancel();

    bool isHoldKey(fcitx::KeySym sym) const;
    void loadConfig();

    void setPreedit(const std::string &text);
    void clearPreedit();
    void showError(const std::string &message);
    void setPreeditOn(const fcitx::TrackableObjectReference<fcitx::InputContext> &ref,
                      const std::string &text);
    void clearPreeditOn(const fcitx::TrackableObjectReference<fcitx::InputContext> &ref);
    void showErrorOn(const fcitx::TrackableObjectReference<fcitx::InputContext> &ref,
                     const std::string &message);

    static std::string dataHome();
    static std::string configHome();
    static std::string readFile(const std::string &path);
    static std::string trim(std::string value);
    static std::string shellQuote(const std::string &value);
    static bool writeWav(const std::string &pcm, const std::string &wav);

    bool runAsr(const std::string &wav, std::string &text, std::string &error);
    bool spawnRecorder(const std::string &pcm);

    void startHud(const std::string &pcm);
    void hudState(const char *state);
    void hudMessage(const std::string &text);
    void scheduleHudHide(int ms);
    void loadHudCommand();

    fcitx::Instance *instance_ = nullptr;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>> watcher_;
    std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>> focusWatcher_;
    fcitx::TrackableObjectReference<fcitx::InputContext> ic_;

    State state_ = State::Idle;
    pid_t pid_ = -1;
    std::string pcmPath_;
    std::uint64_t session_ = 0;
    FILE *hud_ = nullptr;
    std::string hudCommand_;

    std::string channel_ = "doubao";
    std::string device_;
    std::string command_ = "voicekey";
    fcitx::KeySym holdSym_ = FcitxKey_Alt_R;
    int tailMs_ = 600;
};

} // namespace voicekey
