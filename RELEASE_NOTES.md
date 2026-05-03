# Startup Manager 0.1.0

First portable Windows x64 release of Startup Manager, licensed under MIT.

## Added

- Review user/all-user registry startup entries, Startup folders, sign-in/boot scheduled tasks and automatic/disabled services.
- Search names, commands, locations and scope; filter by enabled state and inspect the original startup command before making a change.
- Disable, enable and remove supported startup entries, with flushed local backups and a recovery history. Restoration refuses changed state, corrupt files and conflicting destinations.
- Add an executable to current-user startup using the Windows file picker, and export a local startup report.
- Detect conventional Windows startup-approval markers, preserving reversible management of entries disabled by Windows.
- Integrate rust-auto-updator for background startup checks, a persistent opt-out, manual checks, progress and user-initiated installation.
- CPU-rendered desktop interface with GDI presentation, retained bitmaps and an event loop that sleeps when no update is needed.

## Download and scope

Use **StartupManager-v0.1.0.exe** on Windows 10/11 x64. No installer or Rust runtime is required. **SHA256SUMS.txt** verifies the exact executable. Settings and recovery backups remain under `%LOCALAPPDATA%\StartupManager`.

Administrator rights are required for some protected/all-user changes. Running applications and services are not stopped. Services can be disabled and restored but cannot be deleted. Password-backed scheduled tasks can be disabled but are not removed. Unsupported startup mechanisms, reparse points and oversized startup files are left unchanged.

Local verification covers 13 automated regressions, an explicit native scheduled-task round trip, real read-only inventory, real production screenshots, packaged icon loading and a finite optimized native rendering diagnostic. Real service-mode mutation, interactive keyboard/file-picker traversal and online update installation are not certified. The first repository/release must be published before automatic release discovery can succeed.
