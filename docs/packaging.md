# Windows packaging

The 1.1.0 ZIP supplies the application and static-CRT Typst. FFmpeg/FFprobe and
Pandoc are provisioned after consent, not redistributed in this ZIP.

## Build and create the ZIP

First follow [Static Typst build](typst-build.md). From the repository root:

```text
cargo build --release --locked
py -3 scripts/package_windows.py package
py -3 scripts/test_package_windows.py
```

--app selects another release application, --typst-bundle another verified
Typst build output (default .cache/typst-static/), and --output the destination
(default dist/). Existing outputs are never overwritten. No command commits,
publishes a release or downloads engines during the package step.

The package script validates application PE x64 architecture and supplied
Typst's approved hash, version/commit, normal AND delay imports, metadata and
every license/notice hash. It fails closed if a required input is missing,
altered or unsuitable. There is no force bypass.

## Layout

```text
Rust-File-Converter/
  File_Converter_V1.1.0.exe
  README.md
  CHANGELOG.md
  LICENCE
  THIRD-PARTY-NOTICES.md
  icon.png
  screen.png
  docs/                    user README links remain usable after extraction
  tools/
    typst.exe
    typst/
      LICENSE, NOTICE, BUILD.txt, BUILD.json, Cargo.lock
      DEPENDENCIES.json, ASSETS-STATIC.json
      assets/              original asset notices
      dependencies/        original locked dependency notices
      rust-runtime/        standard-library copyright/licenses
      sources/             NewCM preferred source, option-ext, glidesort
```

No FFmpeg/FFprobe/Pandoc binaries, Cargo.toml, application source, tests or
scripts are copied. Documentation and images accompany the README.
SHA256SUMS.txt is written beside the ZIP with its actual SHA-256.
Archive timestamps before ZIP's 1980 epoch are clamped; original notice bytes
are unchanged. Byte-identical ZIP generation is not promised, but versions,
URLs, input hashes and layout are fixed.

## Download/extraction safeguards

The developer prepare command and runtime installer use the pinned archive
inputs. HTTPS redirects are bounded and cannot downgrade to HTTP. Downloads
must match SHA-256 before extraction or execution. Extraction rejects absolute
paths, traversal, Windows aliases/device names and unsafe links.

Runtime preparation extracts only named binaries/notices. File hashes, x64
architecture, versions and required FFmpeg encoders are checked before atomic
activation. Normal failures remove owned staging while preserving older
engines. More lifecycle details: [Conversion engines](engines.md).

## Notices and deferred distribution

Original licenses and the pinned data consumed by scripts stay under
licenses/ and packaging/. Human documentation is here in docs/.
The main project's MIT LICENCE is unchanged. The ZIP contains original Typst
notices and the small source inclusions described in [Third-party components](third-party.md).

The former all-engine offline ZIP and large source-companion archive are
deferred. FFmpeg/Pandoc corresponding-source questions remain in the
[historical assessment](archive/bundled-compliance.md); changing delivery
strategy does not declare them resolved or constitute legal advice.

See [Testing](testing.md) for ordinary checks and real isolated-package tests.
