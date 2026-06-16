# Verification — Startup Manager 0.1.1

Date: 2026-11-01. Rust 1.96.1, Windows x64, optimized portable GUI executable.

Artifact: `StartupManager-v0.1.1.exe`, 4,440,576 bytes.

SHA-256: `e392e8f36dcc50dfac46e7601ef130b08e72d8c9020f3bfcd7521617272120f5`.

## Interface acceptance

The previous interface reserved a sidebar, large branding/heading, a refresh message, tall two-line rows and 205 logical pixels below the list even without a selection. The user's 13-entry inventory consequently showed only about three rows.

The new interface uses a compact top navigation strip and 36-pixel single-line rows. The table receives the remaining height. Details consume 96 pixels only when selected; each row has a direct Enable/Disable button. Successful refreshes create no permanent message. Location notices open from a compact toolbar button. Interface accents and the application icon use neutral gray/blue-gray instead of green.

Measured from the actual production UI layout in the PNG sidecars:

| View | Logical dimensions | Fully visible rows | Available list height | Actual file icons |
| --- | --- | --- | --- | --- |
| Startup, no selection | 1120 × 760 | 13 / 13 | 632 px | 13 |
| Startup, no selection | 900 × 620 | 13 / 13 | 492 px | 13 |
| Startup at 125% | 900 × 620 | 13 / 13 | 492 logical px | 13 |
| Selected item with details | 1120 × 760 | 13 / 13 | 536 px | 13 |
| Large inventory | 1120 × 760 | 17 full rows + clipped boundary row / 500 | 632 px | 13 cached native resources reused across 500 examples |

These are isolated example registrations of installed Windows utilities, with real icons extracted from those executables. They are not a claim that those utilities are registered for startup on the user's computer. The captures do not change Windows configuration or send update requests.

Names begin beside their icons, at x=54 logical pixels, instead of being centered in the wide name column. Row selection and inline action dispatch are exercised by synthetic input sent to the actual egui widgets. Large lists use visible-row virtualization and clone an entry snapshot only for a selected detail/action.

## Icon pipeline

The normal application sends icon requests after inventory changes to a separate waiting worker. The worker resolves quoted commands/environment variables and reads EXE or shortcut icons through Windows Shell/resource APIs. Pixels and textures are cached by file identity/metadata. Painting performs no filesystem or icon-extraction call. Direct UNC and mapped remote-drive targets are skipped. Missing/unreadable targets use a native generic application icon, with a neutral glyph as a last fallback. These operations never run the startup command.

Windows GDI draws native HICON resources to a DIB; black/white passes reconstruct transparency. A direct resource-extraction fallback handles shell-association lookup failures. Native tests verify executable lookup, environment/quote handling, visible pixels and alpha.

The final application's own embedded icon was loaded and drawn at 16, 20, 24, 32, 48, 64 and 256 px on light/dark backgrounds. Source bindings still assign both small/window and big/taskbar icons. OS Explorer caching, actual taskbar appearance and native keyboard traversal were not inspected through computer use.

## Automated checks

- Formatting and strict all-target Clippy passed.
- Eighteen automated regressions passed; the one opt-in scheduled-task mutation test remains skipped in the normal suite. Four UI regressions were rerun after the final stable row-action ID adjustment.
- Tests cover 13-row capacity at both sizes, capacity with selected details, left alignment, 500-row virtualization, actual row clicks/inline dispatch, native icons, and the existing recovery/scheduling regressions.
- PE inspection verified x86_64, GUI subsystem, and no imported dynamic MSVC runtime or GPU graphics API. Native icon loading and source/backend inspection supplement the import evidence.
- Eleven current application-generated PNGs cover populated, selected, compact/125%, 500-entry, empty/error, History and Settings states. Every bounded capture exited normally. README uses current real screenshots.

## Renderer/load gate

Source and pinned dependencies retain **egui -> CPU software rasterizer -> retained bitmap -> softbuffer Win32 GDI DIB/BitBlt**. No polling render loop or continuous animation was added. Workers wait for jobs; icon-result wakes are bounded. Hidden windows preserve dirty state without constructing/rasterizing/presenting the window.

Packaged EXE diagnostic PID 10880; 13-entry fixture with native icons, no network or user-data changes, one-second process CPU samples, programmatic Settings navigation, native hide/restore, and a 45-second external deadline. The process exited normally.

| State | Quiet sample | UI/layout | Window rasters | Presents | Maximum one-core CPU | Maximum machine CPU |
| --- | --- | --- | --- | --- | --- | --- |
| Startup list | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Settings after navigation | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Hidden | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Restored Settings | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |

The complete session produced 5 UI passes, 2 rasterizations and 3 presentations including initialization, navigation and restore. Restore reused the retained bitmap. Maximum callback draw duration was 20.574 ms. Zero CPU samples are timer-quantized and do not mean literally zero cycles. GPU utilization and compositor latency were not sampled.

## Scope and remaining checks

Normal asynchronous icon arrival, perceived native input latency, actual OS keyboard/file-picker traversal and 500-entry native interaction timing are not certified by a static PNG or the 13-entry native diagnostic. Automated input tests exercise real UI components without injecting OS input.

No existing startup entry, task or service was changed for this UI revision. Initial 0.1.0 validation separately covered an isolated native scheduled-task round trip and real read-only inventory; backend mutation code is unchanged. Real service-mode mutation and installed-app update installation remain unverified. The shared updater remains integrated with its existing preference and shutdown contract.

No GitHub repository/release was created or modified in this UI revision. Publication prerequisites from the original preparation remain outstanding. Sources, assets and license notices are local.
