# Architecture and output safety

The existing eframe/egui application remains in place. Version 1.1.0 separates
shared conversion logic into small modules, with no Tokio or parallel batch
scheduler.

## Responsibilities

| Repository path | Responsibility |
|---|---|
| `src/main.rs` | Application entry point, icon and window setup |
| `src/gui.rs` | Category selection, files, options, progress and results |
| `src/selection.rs` | Shared selection/drop checks and indexed deduplication |
| `src/job.rs` | Job identity, worker events and per-file results |
| `src/output.rs` | Output naming, reservations, private staging and publication |
| `src/converters/` | Native images and external audio/video/document commands |
| `src/tools.rs` | Executable resolution and bounded launch checks |
| `src/engines.rs` | Pinned Windows provisioning and managed-install integrity |

Engine lifecycle details are in [Conversion engines](engines.md).

## Selection and jobs

File dialogs and local-file drag and drop use the same selection logic.
Paths and file identities are indexed to avoid repeated pairwise filesystem
comparisons. Invalid menu drops do not determine the conversion category;
the first admissible category is selected and other categories are reported.

Adding a file checks its extension, existence, regular-file status and
readability. Content decoding happens in the conversion worker, not on the GUI
thread. Selection is not a guarantee that a file's contents are valid.

A batch runs sequentially on a background std::thread. Each job has a JobId;
events must match the active job and expected source. Only one batch can run.
Returning to the menu does not reset an active batch. Each result retains its
source, optional destination, status, short message and technical error details.

## Outputs and errors

All converters use one output planner. File stems retain multiple dots:
photo.v1.png becomes photo.v1.jpg. Existing names, reserved outputs and all batch
sources are protected; conflicts produce name (1).ext, name (2).ext, etc.

Conversions write inside an application-owned
.rust-file-converter-* staging directory in the destination. The engine keeps
a useful output extension. Only after success and basic output validation is
the final file published without replacing an existing name. Publication also
handles collisions appearing after initial planning.

Normal failure or unwinding removes only owned staging. Output checks include
regular-file/non-empty checks (empty plain text is permitted), image dimensions
and PDF/DOCX signatures. They do not fully validate every media stream or all
document semantics.

External tools receive argument arrays and closed stdin. Technical diagnostics
retain the last 64 KiB, separately from a short UI summary, and appear in a
bounded, copyable details area. Child processes are reaped after completion
or worker unwinding.

## Remaining limits

There is no conversion cancellation/timeout, persistent history or CLI.
Closing the window during a batch is deferred until completion. Forced
termination or a machine crash can leave recognizable staging directories;
there is no broad temporary-file deletion scan. Filesystem/hardware durability
failures cannot be made fully transactional by application-level publication.

See [Testing](testing.md) for coverage and [Conversion engines](engines.md)
for format-specific limitations.
