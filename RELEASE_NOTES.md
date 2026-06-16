# Startup Manager 0.1.1

A compact interface focused on the startup list. Changes are verified against the initial local 0.1.0 build; neither version has been published to GitHub in this workspace.

## Improved

- Replace the sidebar, oversized branding and hero heading with a small navigation strip.
- Use 36-pixel single-line rows with aligned application, source, scope and state columns. The 13-entry example fits at 900 × 620; all 13 also fit with details open at 1120 × 760.
- Put Enable/Disable in every row and show command/location, copy and removal only for the selected entry.
- Replace letter placeholders with actual Windows executable/shortcut icons, loaded on a cached worker. Unavailable files use the standard application icon.
- Use neutral gray surfaces and restrained blue-gray focus/selection, including a matching application icon. Remove green interface accents.
- Keep ordinary refresh success messages out of the startup view; move location notices into a compact toolbar action.
- Virtualize long lists and avoid cloning every entry's task XML on pointer/scroll updates. A 500-entry regression lays out only visible rows.
- Preserve backups, recovery, task/service confirmations, settings and the shared updater.

## Verification and download

Eighteen automated tests pass, including real native icon extraction, compact geometry at both window sizes, row selection and inline action dispatch. The opt-in scheduled-task mutation test was not repeated for these UI-only changes. Real application PNGs, packaged icons and a bounded CPU/GDI idle/hidden/restore diagnostic accompany the build. See docs/VERIFICATION.md for measurements and remaining interaction/update limitations.

Use **StartupManager-v0.1.1.exe** for Windows 10/11 x64. Close a running older version before opening this build so the existing single-instance lock does not reactivate the old interface. Existing settings and recovery data are preserved. SHA256SUMS.txt describes the current executable; older local EXEs are not assets for this version.

---

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
