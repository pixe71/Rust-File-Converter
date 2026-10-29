# Conversion engines

## Official Windows setup

The hybrid release supplies static-CRT Typst 0.15.1 beside the application.
Missing FFmpeg/FFprobe 9.0.2 and Pandoc 3.13 are prepared after explicit network
consent. Images remain available if preparation is cancelled. Files being
converted are never uploaded.

The GUI checks engines in the background. Conversion engines shows their
availability, origin and actual paths. Preparation displays progress,
cancellation and retry. Windows x64 is the only platform with automatic
downloads; Linux/macOS currently use manually available executables.

## Resolution

`src/tools.rs` resolves relative to std::env::current_exe(), never the working
directory. On Windows, an adjacent tools/typst.exe is checked first against the
approved static build hash.

| Engine | Resolution order |
|---|---|
| Typst | Supplied adjacent tools/typst.exe, managed installation, RFC_TOOLS_DIR, PATH |
| FFmpeg / FFprobe / Pandoc | Managed installation, adjacent tools/, RFC_TOOLS_DIR, PATH |

RFC_TOOLS_DIR must be an absolute directory. Empty/relative PATH entries are
ignored. Each selected executable must run its version command within five
seconds. An existing unusable higher-priority engine is reported rather than
silently skipped. A damaged supplied Typst requires restoring the complete ZIP;
repair does not download the old runtime-dependent official Typst binary.

Manual engines remain supported on other platforms, using filenames without
.exe. The supplied Typst hash restriction applies to adjacent Windows Typst;
developers can select another build via RFC_TOOLS_DIR when no adjacent file
takes priority.

## Managed storage and provisioning

`src/engines.rs` manages
%LOCALAPPDATA%/RustFileConverter/tools/<engine>/install-*/.
FFmpeg and Pandoc have separate generations containing executable(s) and
original vendor notices. active.json selects a validated generation.

Downloads use the exact versions, HTTPS URLs and SHA-256 values in
`packaging/windows-tools.lock.json`. That file retains the former official
Typst archive for provenance; automatic_download=false prevents its runtime
download. `packaging/typst-static.lock.json` independently pins supplied Typst.

The installer streams to private staging, verifies the archive hash before
extraction, rejects dangerous paths/aliases/links, checks executable/notice
hashes and x64 architecture, then runs version and capability checks. The active
marker is replaced atomically only after successful validation. Normal errors
remove only owned staging. Hard termination can leave inert staging that is
not considered installed. See [Packaging](packaging.md).

## Repair and offline use

Repair engines requests consent and prepares new validated FFmpeg/Pandoc
generations. A failed repair preserves the previous installation. Supplied
Typst stays unchanged. Older generations are retained: another process may
still have resolved or be using their paths. There is no cross-process lease
or automatic pruning; each full repair adds about 450 MB.

Once engines are available, startup and conversion make no update checks or
unnecessary network requests. Versions remain fixed for this application
release. A complete manually prepared tools/ directory can support initial
offline use.

## Source development

The repository tools/ directory is not automatically searched from the current
working directory. Copy tools/ beside the built executable or set RFC_TOOLS_DIR
to its absolute location.

To prepare the pinned developer tools, first follow [Static Typst build](typst-build.md),
then run from the repository root:

```text
py -3 scripts/package_windows.py prepare
```

The developer command downloads FFmpeg/Pandoc and copies verified static Typst
into ignored tools/. Notices/provenance go into .cache/tool-licenses/.
Python is a development dependency, not an end-user requirement. Development
preparation is not approval to redistribute the whole tools/ directory.

## Conversion behavior and limitations

### Images

Images use the native image crate. ICO output fits the source proportionally
inside a transparent 256 x 256 canvas. Large images shrink without stretching;
small images are not enlarged.

Conversions are static: animation, EXIF/ICC metadata and automatic EXIF
orientation are not preserved. Formats without alpha discard transparency.
WebP output is lossless. JPEG uses the crate's default quality; no quality
control is exposed.

### Audio and video

The pinned FFmpeg essentials build supplies the encoders used by the app.
Custom builds must provide compatible encoders. FFprobe is resolved and
diagnosed; no advanced inspection feature is added.

Audio bitrate applies to MP3 and AAC, not WAV/FLAC. OGG explicitly requires
non-experimental libvorbis with VBR quality 5. A missing encoder produces a
clear error; no experimental fallback is selected. A forced Vorbis bitrate can
fail for mono or low-rate inputs, so OGG does not use the bitrate selector.

Video offers libx264, libx265 and video stream copy. Copy applies only to video;
audio and stream selection follow FFmpeg defaults. Not every container/codec
combination is valid; failures retain FFmpeg diagnostics.

### Documents

Pandoc converts DOCX and UTF-8 TXT; PDF input and OCR are not supported.
TXT input is interpreted as Markdown. TXT output uses --to plain --wrap=none.

PDF uses Pandoc's Typst writer and an explicit absolute --pdf-engine path.
Pandoc alone does not enable PDF output: Typst must pass its launch check.
No MiKTeX/LaTeX installation is required. Missing/corrupt engines fail clearly
without publishing a partial final output. Pandoc may change layout or lose
document features.
