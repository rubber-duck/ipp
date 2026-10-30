// AppKit owns window/input lifetime; ANGLE borrows the CAMetalLayer. Calls are
// confined to the Host's main thread. This shim contains no IPP runtime state.
#import <AppKit/AppKit.h>
#import <QuartzCore/CAMetalLayer.h>

#include <stdint.h>
#include <string.h>

typedef struct {
    uint32_t kind, code, modifiers, length;
    double x, y, dx, dy;
    uint8_t text[128];
} IppWindowEvent;

_Static_assert(sizeof(IppWindowEvent) == 176, "Rust window event ABI");

// Equivalent to CEF's CrAppControlProtocol, without a CEF build dependency.
@interface IppApplication : NSApplication {
    BOOL sending;
}
@end
@implementation IppApplication
- (BOOL)isHandlingSendEvent { return sending; }
- (void)setHandlingSendEvent:(BOOL)value { sending = value; }
- (void)sendEvent:(NSEvent *)event {
    BOOL previous = sending;
    sending = YES;
    @try { [super sendEvent:event]; } @finally { sending = previous; }
}
@end

@interface IppView : NSView <NSWindowDelegate> {
    IppWindowEvent events[256];
    NSUInteger count;
    NSSize lastSize;
    CGFloat lastScale;
}
- (void)enqueue:(IppWindowEvent)event;
- (void)metrics;
- (uint32_t)drain:(IppWindowEvent *)output capacity:(uint32_t)capacity;
@end

@implementation IppView
- (BOOL)acceptsFirstResponder { return YES; }
- (BOOL)acceptsFirstMouse:(NSEvent *)event { return YES; }
- (BOOL)isFlipped { return YES; }
- (CALayer *)makeBackingLayer { return [CAMetalLayer layer]; }

- (void)enqueue:(IppWindowEvent)event {
    if (count && event.kind == 1 && events[count - 1].kind == 1) {
        events[count - 1] = event;
        return;
    }
    if (count == 256) {
        // Lost transitions must release capture/focus before accepting more input.
        count = 0;
        events[count++] = (IppWindowEvent){.kind = 7};
        lastSize = NSZeroSize;
    }
    events[count++] = event;
}

- (void)metrics {
    CGFloat scale = self.window.backingScaleFactor;
    NSSize size = self.bounds.size;
    if (NSEqualSizes(size, lastSize) && scale == lastScale) return;
    lastSize = size;
    lastScale = scale;
    self.layer.contentsScale = scale;
    ((CAMetalLayer *)self.layer).drawableSize = NSMakeSize(round(size.width * scale), round(size.height * scale));
    [self enqueue:(IppWindowEvent){.kind = 8, .x = round(size.width * scale), .y = round(size.height * scale), .dx = scale}];
}

- (uint32_t)drain:(IppWindowEvent *)output capacity:(uint32_t)capacity {
    uint32_t length = (uint32_t)MIN(count, capacity);
    memcpy(output, events, length * sizeof(*output));
    count -= length;
    memmove(events, events + length, count * sizeof(*events));
    return length;
}

- (void)pointer:(NSEvent *)event kind:(uint32_t)kind {
    NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
    [self enqueue:(IppWindowEvent){.kind = kind, .code = (uint32_t)event.buttonNumber,
        .modifiers = (event.modifierFlags & NSEventModifierFlagShift) != 0,
        .x = point.x / self.bounds.size.width, .y = point.y / self.bounds.size.height,
        .dx = kind == 4 ? -event.scrollingDeltaX : 0, .dy = kind == 4 ? -event.scrollingDeltaY : 0}];
}

- (void)mouseMoved:(NSEvent *)event { [self pointer:event kind:1]; }
- (void)mouseDragged:(NSEvent *)event { [self pointer:event kind:1]; }
- (void)rightMouseDragged:(NSEvent *)event { [self pointer:event kind:1]; }
- (void)otherMouseDragged:(NSEvent *)event { [self pointer:event kind:1]; }
- (void)mouseDown:(NSEvent *)event { [self.window makeFirstResponder:self]; [self pointer:event kind:2]; }
- (void)rightMouseDown:(NSEvent *)event { [self pointer:event kind:2]; }
- (void)otherMouseDown:(NSEvent *)event { [self pointer:event kind:2]; }
- (void)mouseUp:(NSEvent *)event { [self pointer:event kind:3]; }
- (void)rightMouseUp:(NSEvent *)event { [self pointer:event kind:3]; }
- (void)otherMouseUp:(NSEvent *)event { [self pointer:event kind:3]; }
- (void)scrollWheel:(NSEvent *)event { [self pointer:event kind:4]; }

- (void)keyDown:(NSEvent *)event {
    uint32_t code = 0;
    BOOL shift = (event.modifierFlags & NSEventModifierFlagShift) != 0;
    BOOL command = (event.modifierFlags & NSEventModifierFlagCommand) != 0;
    switch (event.keyCode) {
        case 48: code = shift ? 2 : 1; break;
        case 36: case 76: code = 3; break;
        case 53: code = 5; break;
        case 123: code = 6; break;
        case 124: code = 7; break;
        case 126: code = 8; break;
        case 125: code = 9; break;
        case 115: code = 10; break;
        case 119: code = 11; break;
        case 109: code = 13; break;
        case 51: code = 14; break;
        case 117: code = 15; break;
        case 0: if (command) code = 16; break;
    }
    if (code) {
        [self enqueue:(IppWindowEvent){.kind = 5, .code = code, .modifiers = shift}];
    } else if (!command && !(event.modifierFlags & NSEventModifierFlagControl)) {
        // Committed keyboard text; IME marked-text composition is a future adapter.
        NSData *text = [event.characters dataUsingEncoding:NSUTF8StringEncoding];
        if (text.length && text.length <= sizeof(((IppWindowEvent *)0)->text)) {
            IppWindowEvent input = {.kind = 6, .length = (uint32_t)text.length};
            memcpy(input.text, text.bytes, text.length);
            [self enqueue:input];
        }
    }
}

- (void)windowDidResignKey:(NSNotification *)note { [self enqueue:(IppWindowEvent){.kind = 7}]; }
- (BOOL)windowShouldClose:(NSWindow *)sender { [self enqueue:(IppWindowEvent){.kind = 9}]; return NO; }
@end

void *ipp_window_open(void) {
    if (![NSThread isMainThread]) return NULL;
    @autoreleasepool {
        [IppApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        [NSApp finishLaunching];
        NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 960, 640)
            styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable
            backing:NSBackingStoreBuffered defer:NO];
        window.title = @"IPP — Native Demo";
        window.releasedWhenClosed = NO;
        window.contentMinSize = NSMakeSize(400, 300);
        IppView *view = [[IppView alloc] initWithFrame:NSMakeRect(0, 0, 960, 640)];
        view.wantsLayer = YES;
        window.contentView = view;
        window.delegate = view;
        window.acceptsMouseMovedEvents = YES;
        [window center];
        [window makeKeyAndOrderFront:nil];
        [window makeFirstResponder:view];
        [NSApp activateIgnoringOtherApps:YES];
        [view metrics];
        return (__bridge_retained void *)window;
    }
}

void *ipp_window_layer(void *handle) {
    NSWindow *window = (__bridge NSWindow *)handle;
    return (__bridge void *)window.contentView.layer;
}

uint32_t ipp_window_pump(void *handle, IppWindowEvent *output, uint32_t capacity) {
    @autoreleasepool {
        // Bound each pump so sustained input cannot starve the Host clock.
        for (NSUInteger i = 0; i < 256; ++i) {
            NSEvent *event = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate distantPast]
                inMode:NSDefaultRunLoopMode dequeue:YES];
            if (!event) break;
            [NSApp sendEvent:event];
        }
        [NSApp updateWindows];
        IppView *view = (IppView *)((__bridge NSWindow *)handle).contentView;
        [view metrics];
        return [view drain:output capacity:capacity];
    }
}

void ipp_window_resize(void *handle, uint32_t width, uint32_t height) {
    NSWindow *window = (__bridge NSWindow *)handle;
    // ANGLE calculates drawable extent from bounds * contentsScale, rather
    // than preserving a caller-assigned drawableSize on its own.
    window.contentView.layer.contentsScale = (CGFloat)width / window.contentView.bounds.size.width;
    ((CAMetalLayer *)window.contentView.layer).drawableSize = NSMakeSize(width, height);
}

void ipp_window_close(void *handle) {
    @autoreleasepool {
        NSWindow *window = (__bridge_transfer NSWindow *)handle;
        window.delegate = nil;
        [window close];
    }
}

#ifdef IPP_INSTRUMENTATION
// Tests post real AppKit events without Accessibility permission. Coordinates
// are normalized in the content view; resize uses logical points.
void ipp_window_test(void *handle, uint32_t kind, double x, double y) {
    @autoreleasepool {
        NSWindow *window = (__bridge NSWindow *)handle;
        if (kind == 8) { [window setContentSize:NSMakeSize(x, y)]; return; }
        if (kind == 9) { [window performClose:nil]; return; }
        NSEventType type = kind == 2 ? NSEventTypeLeftMouseDown : kind == 3 ? NSEventTypeLeftMouseUp : NSEventTypeMouseMoved;
        NSPoint point = [window.contentView convertPoint:NSMakePoint(x * window.contentView.bounds.size.width, y * window.contentView.bounds.size.height) toView:nil];
        NSEvent *event = [NSEvent mouseEventWithType:type location:point modifierFlags:0 timestamp:NSProcessInfo.processInfo.systemUptime
            windowNumber:window.windowNumber context:nil eventNumber:0 clickCount:1 pressure:1];
        [NSApp postEvent:event atStart:NO];
    }
}
#endif
