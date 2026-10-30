#ifndef IPP_CEF_ENGINE_H
#define IPP_CEF_ENGINE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// Callbacks run synchronously on the Host thread. Surface and string pointers
// are borrowed only for the callback. No callback may reenter the engine.
typedef void (*ipp_cef_paint)(void *, uint64_t, void *, uint32_t, uint32_t);
typedef void (*ipp_cef_event)(void *, uint64_t, uint32_t, const char *);
uint32_t ipp_cef_abi(void);
const char *ipp_cef_initialize(const char *runtime, const char *bundle, const char *profile,
                             ipp_cef_paint paint, ipp_cef_event event, void *context);
void ipp_cef_pump(void);
int ipp_cef_create(uint64_t id, const char *url, uint32_t width, uint32_t height);
void ipp_cef_begin_frame(uint64_t id);
void ipp_cef_close(uint64_t id);
void ipp_cef_pointer(uint64_t id, uint32_t kind, uint32_t button, double x, double y, double dx, double dy);
void ipp_cef_key(uint64_t id, uint32_t key, uint32_t modifiers);
void ipp_cef_text(uint64_t id, const char *text);
void ipp_cef_focus(uint64_t id, int focused);
int ipp_cef_shutdown(void);
#ifdef __cplusplus
}
#endif
#endif
