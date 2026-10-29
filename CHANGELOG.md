# Changelog

## [1.1.0]

### Added
- Consent-driven automatic Windows x64 engine provisioning with background progress, cancellation and retry.
- Verified pinned downloads of FFmpeg/FFprobe 9.0.2 and Pandoc 3.13; manual installation is no longer required on Windows.
- Supplied Typst 0.15.1 compiled from the official locked sources with static MSVC CRT, original notices, dependency inventory and reproducible build instructions.
- Managed user storage with atomic activation, original vendor notices and a Repair engines action.
- Offline reuse after initial preparation, without network update checks.
- Centralized, launch-checked engine resolution: adjacent tools/, RFC_TOOLS_DIR, then PATH.
- Background engine diagnostics with selected paths and origins.
- Pinned Windows tool preparation, SHA-256 checks, vendor-license structure and a release compliance gate.
- A relocated portable integration test with empty PATH and Unicode/space paths.
- Local-file drag and drop using the same validation as native selection.
- Individual removal and deduplication of selected files, including file aliases.
- An explicit destination selector before conversion.
- Identified background jobs and per-file results with output paths and copyable errors.
- File-count batch progress and protection against overlapping jobs.
- Unit tests for output naming, staging, selection, jobs, document options and images.
- Separate opt-in FFmpeg/Pandoc integration tests and a three-platform CI matrix.

### Changed
- The official Windows ZIP includes the application, static-CRT Typst and its notices; the fully bundled FFmpeg/Pandoc offline/source-companion package is deferred.
- Supplied verified Typst takes priority over managed Typst; managed FFmpeg/Pandoc take priority over adjacent tools/, RFC_TOOLS_DIR and PATH.
- PDF output uses Pandoc's Typst writer and an absolute Typst executable path, replacing the TeX-engine requirement.
- Application version, window title and package metadata now use 1.1.0.
- Removed unused open/walkdir dependencies; enabled only the image formats offered by the UI.
- Applied targeted compatible dependency updates without migrating eframe/egui.
- External-process diagnostics are bounded to the last 64 KiB and shown in the UI.
- Window content can scroll and the window can resize to accommodate results.
- Documented engine requirements and remaining format limitations accurately.

### Fixed
- Replaced the official Typst executable's external VCRUNTIME140.dll requirement with a compiled and import-checked static-CRT build; provisioning no longer downloads Typst when supplied.
- Multi-dot filenames retain their complete stem.
- Existing outputs, homonyms and source/output conflicts receive numbered names.
- Returning to the menu no longer resets an active conversion.
- Stale job events cannot change a newer job's results.
- Failed conversions no longer disappear into console-only messages.
- PDF input is rejected; TXT output explicitly requests plain text.
- PDF output checks that the Typst engine can be launched independently of Pandoc.
- Large ICO inputs fit proportionally on a transparent 256 × 256 canvas.
- Invalid options and incomplete worker termination no longer produce artificial success.

### Security
- All engines share private staging and no-clobber finalization; failed staging is cleaned without deleting unrelated user files.
- Finalization rechecks collisions that appear during conversion.
- Normal window closure is deferred while a batch is active.
- External processes use argument arrays, closed standard input and bounded diagnostics; child processes are reaped on normal completion or worker unwinding.
