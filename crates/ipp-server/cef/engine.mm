// Optional native engine distribution. IPP's Rust Host owns Worlds, clocks,
// source identities and renderer reservations; this shim owns only CEF objects.
#include "engine.h"
#include "include/cef_app.h"
#include "include/cef_client.h"
#include "include/cef_render_handler.h"
#include "include/cef_request_handler.h"
#include "include/cef_application_mac.h"
#include "include/wrapper/cef_library_loader.h"
#include <map>
#include <string>
#include <vector>

@interface IppCefApplication : NSApplication <CefAppProtocol> {
    BOOL sending;
}
@end
@implementation IppCefApplication
- (BOOL)isHandlingSendEvent { return sending; }
- (void)setHandlingSendEvent:(BOOL)value { sending = value; }
- (void)sendEvent:(NSEvent *)event {
    BOOL previous = sending;
    sending = YES;
    @try { [super sendEvent:event]; } @finally { sending = previous; }
}
@end

namespace {
ipp_cef_paint paint_callback;
ipp_cef_event event_callback;
void *callback_context;
bool initialized = false;
bool ready = false;

void report(uint64_t id, uint32_t kind, const std::string &message) {
    if (event_callback) event_callback(callback_context, id, kind, message.c_str());
}

class Page;
std::map<uint64_t, CefRefPtr<Page>> pages;

class Page : public CefClient, public CefRenderHandler, public CefLifeSpanHandler,
             public CefLoadHandler, public CefRequestHandler {
public:
    Page(uint64_t id, std::string url, uint32_t width, uint32_t height)
        : id(id), url(std::move(url)), width(width), height(height) {}

    CefRefPtr<CefRenderHandler> GetRenderHandler() override { return this; }
    CefRefPtr<CefLifeSpanHandler> GetLifeSpanHandler() override { return this; }
    CefRefPtr<CefLoadHandler> GetLoadHandler() override { return this; }
    CefRefPtr<CefRequestHandler> GetRequestHandler() override { return this; }
    void GetViewRect(CefRefPtr<CefBrowser>, CefRect &rect) override {
        rect = CefRect(0, 0, width, height);
    }
    bool GetScreenInfo(CefRefPtr<CefBrowser>, CefScreenInfo &screen) override {
        screen.device_scale_factor = 1;
        screen.rect = screen.available_rect = CefRect(0, 0, width, height);
        return true;
    }
    void OnAfterCreated(CefRefPtr<CefBrowser> created) override { browser = created; }
    void OnBeforeClose(CefRefPtr<CefBrowser>) override {
        browser = nullptr;
        report(id, 4, "closed");
        pages.erase(id);
    }
    void OnPaint(CefRefPtr<CefBrowser>, PaintElementType, const RectList &, const void *, int, int) override {
        report(id, 2, "CEF accelerated rendering unavailable: CPU OnPaint is not supported");
    }
    void OnAcceleratedPaint(CefRefPtr<CefBrowser>, PaintElementType kind, const RectList &,
                            const CefAcceleratedPaintInfo &info) override {
        if (kind != PET_VIEW || closing) return;
        if (info.format != CEF_COLOR_TYPE_BGRA_8888 || info.extra.visible_rect.x || info.extra.visible_rect.y ||
            info.extra.visible_rect.width != info.extra.coded_size.width || info.extra.visible_rect.height != info.extra.coded_size.height) {
            report(id, 2, "CEF delivered an unsupported cropped or non-BGRA frame");
            return;
        }
        paint_callback(callback_context, id, info.shared_texture_io_surface,
                       info.extra.coded_size.width, info.extra.coded_size.height);
    }
    void OnLoadEnd(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, int) override {
        if (frame->IsMain()) report(id, 1, frame->GetURL());
    }
    void OnLoadError(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, ErrorCode code,
                     const CefString &text, const CefString &) override {
        if (frame->IsMain() && code != ERR_ABORTED) report(id, 2, text);
    }
    bool OnBeforeBrowse(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, CefRefPtr<CefRequest> request,
                        bool, bool) override {
        if (!frame->IsMain()) return false;
        const std::string next = request->GetURL();
        if (!navigated && next == url) { navigated = true; return false; }
        // Navigation is client-owned; a replacement source gets a new browser
        // incarnation so late same-size frames cannot acquire new authority.
        report(id, 3, next);
        return true;
    }
    const uint64_t id;
    const std::string url;
    const int width, height;
    CefRefPtr<CefBrowser> browser;
    bool closing = false;
private:
    bool navigated = false;
    IMPLEMENT_REFCOUNTING(Page);
};

class EngineApp : public CefApp, public CefBrowserProcessHandler {
public:
    CefRefPtr<CefBrowserProcessHandler> GetBrowserProcessHandler() override { return this; }
    void OnContextInitialized() override { ready = true; }
    void OnBeforeCommandLineProcessing(const CefString &, CefRefPtr<CefCommandLine> line) override {
        line->AppendSwitchWithValue("use-angle", "metal");
        line->AppendSwitch("enable-gpu");
        line->AppendSwitchWithValue("password-store", "basic");
        line->AppendSwitch("no-proxy-server");
    }
private:
    IMPLEMENT_REFCOUNTING(EngineApp);
};

CefRefPtr<CefBrowserHost> host(uint64_t id) {
    auto found = pages.find(id);
    if (found == pages.end() || !found->second->browser || found->second->closing) return nullptr;
    return found->second->browser->GetHost();
}

bool mac_key(uint32_t key, CefKeyEvent &event) {
    // CEF synthesizes an NSEvent on macOS: an empty character pair means a
    // modifier transition, and windows_key_code alone cannot identify a key.
    // Logical commands use the corresponding ANSI hardware key; committed
    // Unicode text is delivered independently by ipp_cef_text.
    int native;
    char16_t character;
    switch (key) {
        case 8: native = 51; character = NSDeleteCharacter; break;
        case 9: native = 48; character = NSTabCharacter; break;
        case 13: native = 36; character = NSCarriageReturnCharacter; break;
        case 27: native = 53; character = 27; break;
        case 32: native = 49; character = ' '; break;
        case 33: native = 116; character = NSPageUpFunctionKey; break;
        case 34: native = 121; character = NSPageDownFunctionKey; break;
        case 35: native = 119; character = NSEndFunctionKey; break;
        case 36: native = 115; character = NSHomeFunctionKey; break;
        case 37: native = 123; character = NSLeftArrowFunctionKey; break;
        case 38: native = 126; character = NSUpArrowFunctionKey; break;
        case 39: native = 124; character = NSRightArrowFunctionKey; break;
        case 40: native = 125; character = NSDownArrowFunctionKey; break;
        case 46: native = 117; character = NSDeleteFunctionKey; break;
        default: {
            constexpr int letters[] = {0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46,
                                       45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6};
            constexpr int digits[] = {29, 18, 19, 20, 21, 23, 22, 26, 28, 25};
            if (key >= 'A' && key <= 'Z') {
                native = letters[key - 'A'];
                character = key + (event.modifiers & EVENTFLAG_SHIFT_DOWN ? 0 : 'a' - 'A');
            } else if (key >= '0' && key <= '9') {
                native = digits[key - '0'];
                character = key;
            } else return false;
        }
    }
    event.native_key_code = native;
    event.unmodified_character = character;
    event.character = character;
    if ((event.modifiers & EVENTFLAG_CONTROL_DOWN) && key >= 'A' && key <= 'Z')
        event.character = key - 'A' + 1;
    return true;
}
}

uint32_t ipp_cef_abi(void) { return 1; }

const char *ipp_cef_initialize(const char *runtime, const char *bundle, const char *profile,
                             ipp_cef_paint paint, ipp_cef_event event, void *context) {
    @autoreleasepool {
        if (initialized || ![NSThread isMainThread]) return "CEF requires one initialization on the main thread";
        const std::string framework = std::string(runtime) + "/Chromium Embedded Framework.framework";
        if (!cef_load_library((framework + "/Chromium Embedded Framework").c_str())) return "Could not load the pinned CEF framework";
        if (!NSApp) [IppCefApplication sharedApplication];
        if (![NSApp respondsToSelector:@selector(isHandlingSendEvent)] || ![NSApp respondsToSelector:@selector(setHandlingSendEvent:)])
            return "Host NSApplication must implement CefAppProtocol";
        paint_callback = paint;
        event_callback = event;
        callback_context = context;
        CefSettings settings;
        settings.no_sandbox = true;
        settings.windowless_rendering_enabled = true;
        settings.external_message_pump = true;
        settings.log_severity = LOGSEVERITY_WARNING;
        CefString(&settings.framework_dir_path) = framework;
        CefString(&settings.main_bundle_path) = bundle;
        CefString(&settings.browser_subprocess_path) = std::string(bundle) + "/Contents/Frameworks/IppCEF Helper.app/Contents/MacOS/IppCEF Helper";
        CefString(&settings.root_cache_path) = profile;
        CefString(&settings.log_file) = std::string(profile) + "/cef.log";
        char name[] = "ipp-gles-host";
        char *argv[] = {name, nullptr};
        CefMainArgs args(1, argv);
        initialized = CefInitialize(args, settings, new EngineApp, nullptr);
        if (!initialized) return "CEF initialization failed";
        return nullptr;
    }
}

void ipp_cef_pump(void) {
    @autoreleasepool { if (initialized) CefDoMessageLoopWork(); }
}

int ipp_cef_create(uint64_t id, const char *url, uint32_t width, uint32_t height) {
    if (!ready || !id || pages.count(id) || pages.size() >= 32 || !width || !height || width > 4096 || height > 4096) return 0;
    CefRefPtr<Page> page = new Page(id, url, width, height);
    pages.emplace(id, page);
    CefWindowInfo window;
    window.SetAsWindowless(0);
    window.shared_texture_enabled = true;
    window.external_begin_frame_enabled = true;
    CefBrowserSettings settings;
    settings.windowless_frame_rate = 30;
    settings.background_color = 0;
    CefRefPtr<CefBrowser> browser = CefBrowserHost::CreateBrowserSync(window, page, url, settings, nullptr, nullptr);
    if (!browser) { pages.erase(id); return 0; }
    return 1;
}

void ipp_cef_begin_frame(uint64_t id) { if (auto browser = host(id)) browser->SendExternalBeginFrame(); }

void ipp_cef_close(uint64_t id) {
    auto found = pages.find(id);
    if (found != pages.end() && found->second->browser && !found->second->closing) {
        found->second->closing = true;
        found->second->browser->GetHost()->CloseBrowser(true);
    }
}

void ipp_cef_pointer(uint64_t id, uint32_t kind, uint32_t button, double x, double y, double dx, double dy) {
    auto browser = host(id);
    if (!browser) return;
    CefMouseEvent event;
    event.x = x;
    event.y = y;
    const CefBrowserHost::MouseButtonType type = button == 1 ? MBT_RIGHT : button == 2 ? MBT_MIDDLE : MBT_LEFT;
    if (kind == 2 || kind == 3) {
        browser->SetFocus(true);
        browser->SendMouseClickEvent(event, type, kind == 3, 1);
    } else if (kind == 4) browser->SendMouseWheelEvent(event, -dx, -dy);
    else browser->SendMouseMoveEvent(event, kind == 7);
}

void ipp_cef_key(uint64_t id, uint32_t key, uint32_t modifiers) {
    if (auto browser = host(id)) {
        CefKeyEvent event;
        event.type = KEYEVENT_RAWKEYDOWN;
        event.windows_key_code = key;
        event.modifiers = modifiers;
        if (!mac_key(key, event)) return;
        browser->SendKeyEvent(event);
        event.type = KEYEVENT_KEYUP;
        browser->SendKeyEvent(event);
    }
}

void ipp_cef_text(uint64_t id, const char *text) {
    if (auto browser = host(id)) {
        const std::u16string characters = CefString(text).ToString16();
        for (char16_t character : characters) {
            CefKeyEvent event;
            event.type = KEYEVENT_CHAR;
            event.character = character;
            event.unmodified_character = character;
            browser->SendKeyEvent(event);
        }
    }
}

void ipp_cef_focus(uint64_t id, int focused) { if (auto browser = host(id)) browser->SetFocus(focused != 0); }

int ipp_cef_shutdown(void) {
    if (!initialized) return 1;
    std::vector<uint64_t> ids;
    for (const auto &[id, _] : pages) ids.push_back(id);
    for (uint64_t id : ids) ipp_cef_close(id);
    // The Host pumps until every OnBeforeClose has relinquished the callbacks.
    if (!pages.empty()) return 0;
    CefShutdown();
    initialized = false;
    paint_callback = nullptr;
    event_callback = nullptr;
    callback_context = nullptr;
    return 1;
}
