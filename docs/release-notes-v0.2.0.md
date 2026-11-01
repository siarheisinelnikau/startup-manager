# Startup Manager 0.2.0

Interface localization and a light appearance, with existing settings and recovery data preserved. Changes are compared with the local 0.1.2 source baseline.

## Added

- Choose English, Russian, French, German, Spanish or Italian in Settings. Navigation, startup lists, filters, recovery history, confirmations, the executable picker and update controls follow the saved language immediately.
- Switch between Light and Dark in the same Appearance section. Both palettes cover lists, details, dialogs, controls and status messages.
- Use translated source and scope names when searching startup entries.

## Improved

- Save language/theme automatically while preserving existing startup-update preferences and backup data. Unknown future preference values fall back safely.
- Give translated row actions and toolbar controls enough space without losing compact list capacity or virtualization.
- Add isolated capture language/theme arguments for verifying the actual application without touching normal preferences.

## Verification and download

Use **StartupManager-v0.2.0.exe** for Windows 10/11 x64. Settings and recovery backups remain in `%LOCALAPPDATA%\StartupManager`. Detailed OS errors and downloaded release notes retain their source language. Native file dialog system buttons follow Windows’ language and appearance.

Verification records are in `docs/VERIFICATION-v0.2.0.md`. Automated checks cover translation parameters, preference migration, all six languages in both themes, compact list capacity and the existing recovery/render scheduling regressions. Online update installation, real service changes and manual native keyboard/file-dialog interaction remain outside this verification.
