# Verification — Startup Manager 0.1.2

2026-11-01. Optimized Windows x64 GUI build, Rust 1.96.1.

Artifact: `StartupManager-v0.1.2.exe`, 4,442,112 bytes.

SHA-256: `dad5e48cdb745a9dca0d41c441852dbefad4696bbc9ab65d3936a845018571a9`.

## Current UI changes

Enabled/disabled row text is replaced with a checked-circle / minus-circle. Distinct shapes, state tooltips and accessible row labels retain the meaning without depending on color. The state column is narrower.

Row Enable/Disable buttons have no background or border at rest. Their 70 × 28 logical-pixel hit area and stable per-entry IDs are retained. Hover adds a subtle underline; keyboard focus adds an outline. Accessible action names include the application name. No new animation, timer, network request or startup mutation was added.

All four actual UI component regressions pass: thirteen-entry capacity at 1120 × 760 and 900 × 620, capacity with details open at the larger size, name alignment, 500-row virtualization, row selection and inline action dispatch. Formatting and strict all-target Clippy pass. Backend code is unchanged; the 18-test full suite and separate native task round trip are prior baseline evidence, not newly repeated mutation tests for this styling change.

Eleven refreshed application-generated PNGs were produced by the packaged EXE and the changed main/selected/compact-125% views were inspected. Thirteen example rows remain fully visible, including at 900 × 620. The examples use real icons from installed Windows tools; they are not actual startup registrations. Capture mode performs no startup mutation or online update check, and every capture process exited.

PE inspection verified x64, GUI subsystem and no imported dynamic MSVC runtime or GPU graphics API. The final executable's icon resources loaded/drew at 16, 20, 24, 32, 48, 64 and 256 pixels. These checks do not establish OS icon-cache appearance or native keyboard/file-dialog traversal.

## Rendering/load gate

The inspected production path remains **egui -> CPU software rasterizer -> retained bitmap -> Win32 GDI DIB/BitBlt**. The event loop waits for input/data changes or a finite deadline; hidden/minimized windows skip layout/raster/presentation. Flat controls and vector status icons introduce no repaint loop.

Final packaged EXE diagnostic PID 16180; 13-entry isolated fixture, native icons, one-second process CPU samples, real page navigation/hide/restore, no network/user-data changes and a 45-second external deadline. The process exited normally.

| State | Quiet sample | UI passes | Window rasters | Presents | Max one-core CPU | Max machine CPU |
| --- | --- | --- | --- | --- | --- | --- |
| Startup list | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Settings after navigation | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Hidden window | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Restored Settings | 5 seconds | 0 | 0 | 0 | 1.561% | 0.0781% |

The complete session included 5 UI passes, 2 rasterizations and 3 presentations for initialization, navigation and restore. Restore reused the retained bitmap. Maximum draw callback: 9.797 ms. Zero CPU samples are timer-quantized, not literally zero cycles. GPU utilization, compositor latency and perceived native input latency were not measured.

## Boundaries

Synthetic UI tests exercise the real egui controls without injecting OS input. Real service-mode changes, installed-app update installation and manual OS keyboard/file-picker traversal remain outside this verification. No existing startup entry, task or service was changed for this UI revision.

Settings, backups, recovery, native file icons, 36-pixel rows, optional detail panel, virtualization and the shared updater retain their existing behavior. GitHub publication prerequisites remain outstanding; this is a local packaged build.
