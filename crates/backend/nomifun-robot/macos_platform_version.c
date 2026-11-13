// Clang lowers `@available` to `__isPlatformVersionAtLeast`. This is the
// macOS 10.15+ path from compiler-rt's os_version_check.c: pack the version
// and ask libSystem. Arm64 Macs start at 11, so the symbol is always present
// and this file does not need the older plist fallback.
#include <stdbool.h>
#include <stdint.h>

typedef struct {
    uint32_t platform;
    uint32_t version;
} dyld_build_version_t;

extern bool _availability_version_check(uint32_t count,
                                        dyld_build_version_t versions[]);

int32_t __isPlatformVersionAtLeast(uint32_t platform, uint32_t major,
                                   uint32_t minor, uint32_t subminor) {
    dyld_build_version_t version = {
        platform,
        ((major & 0xffffu) << 16) | ((minor & 0xffu) << 8) | (subminor & 0xffu),
    };
    return _availability_version_check(1, &version) ? 1 : 0;
}
