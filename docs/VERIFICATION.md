# Verification — 2026-11-01

Artifact: `StartupManager-v0.1.0.exe`, optimized Windows x64 GUI build, 4,391,936 bytes.

SHA-256: `a69418466316d84152b440fead66e7834702a300172e79a378e858e7ba22393a`.

## Automated and native checks

- `cargo fmt --all --check` and strict all-target Clippy passed with the locked offline dependency graph on Rust 1.96.1.
- Thirteen automated regressions passed. They cover byte-preserving file/REG_EXPAND_SZ recovery, destination conflicts, stale commands, backup corruption, source-boundary checks, native Windows approval-marker round trips, search, UTC dates and hidden/idle repaint scheduling.
- The explicit scheduled-task integration test passed separately: it created one uniquely named task with **no triggers**, enabled it, undid that change, deleted/restored it, rejected stale XML and confirmed final deletion of the fixture. No task action was executed.
- Native registry tests used uniquely named keys below `HKCU\Software\StartupManagerTests`. Existing startup values were not changed.
- Real read-only inventory outside the sandbox found 13 registry entries, 50 sign-in/boot tasks and 97 automatic/disabled services. One unsupported Startup-folder item was left unchanged with a warning. Ordinary sandbox reads correctly surfaced denied locations instead of reporting them as empty.
- PE inspection verified x86_64, Windows GUI subsystem and no imported dynamic MSVC runtime or GPU graphics API. This import check supplements the renderer source/runtime evidence below.
- Windows loaded and drew the final EXE's icon resource at 16, 20, 24, 32, 48, 64 and 256 pixels on light/dark backgrounds. No application entrypoint was executed for the resource proof.

## Renderer and event-loop gate

The production path is **egui -> egui_software_backend CPU rasterization -> retained pixel buffer -> softbuffer's Win32 GDI DIB/BitBlt backend**. Pinned backend source and the application event loop were inspected. The host uses `ControlFlow::Wait` or a finite repaint deadline, coalesces requests, compares scenes before rasterization, and skips hidden/minimized layout, raster and presentation. Background work uses waiting channels and finite worker operations.

The finite native diagnostic used the packaged EXE, PID 2756, isolated six-entry fixtures, no network/OS mutations, one-second process CPU samples, native hide/restore and a 45-second external deadline. The process exited normally. Navigation to Settings used the real application page state and renderer, without injected OS input.

| State | Quiet sample | UI/layout passes | CPU rasters | GDI presents | Maximum CPU, one core | Maximum machine-normalized CPU |
| --- | --- | --- | --- | --- | --- | --- |
| Startup list, settled | 5 seconds | 0 | 0 | 0 | 1.560% | 0.0780% |
| Settings after navigation, settled | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |
| Hidden window | 5 seconds | 0 | 0 | 0 | 1.559% | 0.0780% |
| Restored window, settled | 5 seconds | 0 | 0 | 0 | 0.000% | 0.0000% |

The complete session produced 5 UI passes, 2 rasterizations and 3 presentations, including startup, navigation and restore. Restore reused the retained bitmap. Maximum measured draw callback duration was 16.047 ms. The diagnostic sampler/report writer contributes to process CPU; zero-valued samples are timer-quantized, not a claim of zero CPU cycles.

These checks establish a sleeping static CPU/GDI interface and stopped hidden rendering in the measured fixture. GPU utilization, Windows compositor latency, subjective input latency, large real-world inventories and interactive minimize/keyboard/file-picker behavior were not measured. No full-workload performance claim is made.

## Actual visual review

Eight current PNGs were produced by the executable's bounded render-save-exit mode and inspected: main list, Settings, empty state, error state, History, 125% main view, 900 x 620 compact main view and compact error state. Every capture process exited. The captures use the same UI components/fonts and CPU rasterizer as production, with deterministic example data and disabled mutations/network.

Visual review caught and fixed an overflowing toolbar and an error message clipped below the window. Errors now precede the list, full commands remain available through tooltips/copy, and Settings can scroll at smaller heights. Captures are client-area evidence; they do not prove OS title-bar/taskbar icon appearance, window activation, real keyboard traversal or file-picker interaction. Window small/big icon assignments were reviewed in source, while packaged resources were verified separately.

## Remaining boundaries

- No real service startup mode was changed, no existing scheduled task was modified, and no installed application update was performed.
- Startup Manager's first GitHub repository/release is not yet published. Its online update flow cannot discover a first release until that exists. Shared updater controller tests cover opt-out/duplicate suppression; the library's 19 tests and native replacement/rollback smoke fixture passed separately.
- Publication through Windows Computer Use stopped because the helper could not confidently determine Chrome's current URL. No further browser input was sent. The GitHub plugin exposes no repository/release creation or asset-upload operation. No remote source or release was modified.
- The canonical shared updater repository/tag must be published before consumer CI can provision the sibling dependency.
