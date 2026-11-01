# Verification — Startup Manager 0.2.0

2026-11-01. Windows x64 optimized GUI build, with six interface languages and saved Light/Dark appearance preferences.

## Automated checks

- `cargo fmt --all --check`.
- `cargo clippy --all-targets --locked --offline -- -D warnings`.
- `cargo test --locked --offline --all-targets`: 23 passed, zero failed, one existing explicit scheduled-task mutation test ignored.
- `cargo build --release --locked --offline`.

The new catalog regression checks all 100 messages for nonempty translations and matching named parameters in Russian, French, German, Spanish and Italian. English is the full source/fallback language. Parameter substitution preserves user values literally, including braces and non-English paths. A cached error/export regression switches Russian → French → English and verifies that prefixes change while literal details stay intact. A source audit found explicit message lookups and no missing catalog entries; remaining catalog messages are selected dynamically for sources, operations and static status messages.

Preference migration tests preserve an existing disabled startup-update preference, add English/Dark defaults, accept unknown future language/theme values safely, and round-trip every language/theme combination. Real egui components lay out Startup, Settings, History, Error and Selected states for all six languages in both themes at 900 × 620; the startup list retains all 13 fixture rows. Existing compact-capacity, selection/action-dispatch, virtualization, backup integrity and redraw scheduling tests also pass. These component tests inject no OS input and perform no real startup changes.

## Rendering source review

The production path remains **egui → egui_software_backend CPU rasterization → retained bitmap → softbuffer Win32 GDI**. No graphics API renderer was added. The inspected event loop uses Wait/finite WaitUntil, skips hidden/minimized layout and drawing, retains the last bitmap for OS exposure, and rasterizes only changed scenes. Language/theme changes request a finite repaint only when the selected value changes. The selected theme also uses egui-winit’s native SetTheme command for the application window frame. Settings are saved only on a preference change; fonts are loaded once at construction rather than on theme changes.

The bounded offscreen capture mode uses the same UI and CPU rasterizer as production, selects an isolated fixture before loading normal settings or network workers, waits for a stable scene, and exits within 12 frames/five seconds. Capture arguments now select language/theme without saving preferences. Screenshot results and current runtime measurements are recorded below after checking the final packaged executable.

## Scope

Application names, paths, commands, downloaded release notes and detailed operating-system diagnostics retain their original content. Native dialog system controls follow the Windows language/theme. This change does not certify real service mutation, installed-app online updates, native keyboard/file-picker traversal, OS icon-cache behavior, perceived input latency or GPU/compositor latency. Historical version-specific evidence in `docs/VERIFICATION.md` remains labeled 0.1.2.
