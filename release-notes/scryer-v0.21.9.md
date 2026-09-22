# Scryer 0.21.9 release notes

## Highlights

- **Apple Silicon macOS builds use ThinLTO.** This changes how the ARM64 executable is optimized to address repeated build timeouts with full link-time optimization. Application behavior and configuration are unchanged from 0.21.8. Intel macOS and all other platform build profiles are unchanged.

## Upgrading

No configuration changes or new database migrations are introduced in this patch.
