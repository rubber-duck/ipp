#include "include/cef_app.h"
#include "include/wrapper/cef_library_loader.h"
#import <AppKit/AppKit.h>
#include <cstdlib>
#include <string>

int main(int argc, char **argv) {
    NSString *frameworks = [[[NSBundle mainBundle] bundlePath] stringByDeletingLastPathComponent];
    NSString *path = [frameworks stringByAppendingPathComponent:@"Chromium Embedded Framework.framework/Chromium Embedded Framework"];
    const std::string library = [path fileSystemRepresentation];
    if (!cef_load_library(library.c_str())) return 1;
    return CefExecuteProcess(CefMainArgs(argc, argv), nullptr, nullptr);
}
