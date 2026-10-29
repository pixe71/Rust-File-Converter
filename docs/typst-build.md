# Static Typst build

The supplied Windows Typst executable is compiled from unmodified official
0.15.1 sources with static MSVC CRT. It replaces the official Windows binary
that imported VCRUNTIME140.dll. No VC++ DLL or runtime installer is supplied.

## Pinned inputs and build

| Input | Value |
|---|---|
| Tag | v0.15.1 |
| Commit | 9dfd3a08500b7896045f907433cf7b4b02434fad |
| Rust | 1.93.0 MSVC, LLVM 21.1.8 |
| Target | x86_64-pc-windows-msvc |
| RUSTFLAGS | -C target-feature=+crt-static |
| Features | Upstream defaults; self-update disabled |

The official Cargo.lock is used without dependency updates or source changes.
Source/archive/lock hashes and all notice inputs are pinned in
`packaging/typst-static.lock.json`. `licenses/Typst/BUILD.txt` records the
observed MSVC 14.44.35207 toolset, linker 14.44.35229.0 and SDK 10.0.26100.0,
build date, command, provenance and executable hash. It stays in place because
the build helper consumes it and its bytes are pinned for packaging.

Use Windows x64 with Rust 1.93.0 MSVC, rust-docs, MSVC C++ Build Tools and the
Windows SDK. Keep any GNU application build environment separate. Run:

```text
py -3 scripts/build_typst_windows.py
```

The helper verifies the official tag -> commit and archive, compares every
source file and Cargo.lock before and after building, then runs:

```text
RUSTFLAGS=-C target-feature=+crt-static
TYPST_VERSION=0.15.1
TYPST_COMMIT_SHA=9dfd3a08500b7896045f907433cf7b4b02434fad

cargo build --release --locked --target x86_64-pc-windows-msvc --package typst-cli --bin typst --jobs 2
```

Generated executable, build metadata and notices go into ignored
.cache/typst-static/. The optional --source-dir, --target-dir and --output
arguments support separate source/build/output directories.

Different SDK/linker versions may produce a different hash. Review and
explicitly repin a rebuilt candidate; packaging has no override.
Byte-identical builds across SDK versions are not promised.

## Import and runtime validation

Normal imports of the approved candidate:

```text
api-ms-win-core-synch-l1-2-0.dll
bcryptprimitives.dll
crypt32.dll
kernel32.dll
ntdll.dll
ole32.dll
secur32.dll
shell32.dll
ws2_32.dll
```

There are no delay imports. Both dumpbin /DEPENDENTS and
`scripts/windows_pe.py` were used. The parser rejects non-standard imports,
including VC++/MinGW/GCC runtime DLLs, before packaging executes the candidate.

`scripts/trace_windows_dlls.py` records actual Windows debug-event DLL loads.
--version and direct PDF compilation passed with empty PATH and no forbidden
runtime loaded. Windows components such as msvcp_win.dll and ucrtbase.dll are
distinct from VCRUNTIME140.dll/MSVCP140.dll. No system DLL is changed.

The retained build reports typst 0.15.1 (9dfd3a08), and direct Typst, Pandoc
TXT-to-PDF and DOCX-to-PDF tests passed with an explicit absolute engine path.

## Notices and assets

The build retains original LICENSE/NOTICE, locked dependency notices,
standard-library notices and small source inclusions. DEPENDENCIES.json indexes
335 normal/build packages. ASSETS-STATIC.json records all 34 unmodified raw
embedded font/ICC byte sequences in this executable.

The historical `licenses/Typst/ASSETS.json` describes the official binary;
it is not evidence for the new static executable. See [Third-party components](third-party.md)
for font exceptions, preferred source and other license details.

## CI artifact

`.github/workflows/typst-static.yml` is manual workflow_dispatch only.
It verifies sources, builds static CRT, checks normal/delay imports, traces
--version and PDF compilation, calculates SHA-256 and produces a review artifact.
It does not create a GitHub Release. The workflow was prepared but not run
during local 1.1.0 validation.
