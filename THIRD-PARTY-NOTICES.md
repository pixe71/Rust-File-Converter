# Third-party conversion engines — Rust File Converter 1.1.0

Rust File Converter's own code retains its existing MIT license in LICENCE.
The programs below are separate executables with their own licenses and
attributions. This index does not replace any original license or NOTICE and
does not certify redistribution clearance.

## Distribution mode in 1.1.0

The official application ZIP supplies only a self-built static-CRT Typst engine
with original notices and small source inclusions. After explicit consent,
FFmpeg/FFprobe and Pandoc are downloaded from the publishers below with pinned
SHA-256 checks. Their archives are not republished. This does not automatically
remove legal obligations or close the historical fully bundled source audit.

Windows storage: `%LOCALAPPDATA%/RustFileConverter/tools/<engine>/install-*/`.
Each active generation contains the executable(s) and the original notices
under `licenses/`. `active.json` selects the validated generation. The application
shows the actual executable paths in **Conversion engines**. A manually supplied
adjacent `tools/`, RFC_TOOLS_DIR or PATH is still supported.

The repository preserves original notices byte-for-byte under
`licenses/<vendor>/original/`, plus Typst asset notices under
`licenses/Typst/assets/`. The pinned archive/executable hashes, source identities
and outstanding audit findings remain in `packaging/windows-tools.lock.json`.
These notices supplement, never replace, the publishers' original texts.

## FFmpeg / FFprobe

- Version: 9.0.2 essentials, Windows x64 static build, both executables from the same ZIP.
- Binary provider: Gyan; https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2
- Binary archive: ffmpeg-9.0.2-essentials_build.zip
- Archive SHA-256: 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
- Main license: GNU GPL version 3 or later, as reported by this build.
- Original archive notices retained in each installed FFmpeg generation: licenses/LICENSE and licenses/README.txt
- Exact main source: https://github.com/FFmpeg/FFmpeg/tree/946fcce07b6dcd0331c8cc609192aeff5e1924f8
- Dependency versions and outstanding correspondence checks: licenses/FFmpeg/COMPONENTS.json in the repository.
- Source obligations also concern the linked components and distributor patches/build instructions; a main FFmpeg checkout alone is not a complete static-build source bundle.

## Pandoc

- Version: 3.13; official Windows x86_64 ZIP.
- Origin: https://github.com/jgm/pandoc/releases/tag/3.13
- Binary archive: pandoc-3.13-windows-x86_64.zip
- Archive SHA-256: 23e1f2e978a3df80db8bf65c9c59534f41a23140480cfb4f783a882ab7dc6a4e
- Main license: GNU GPL version 2 or later, with the exceptions and component terms in COPYRIGHT.
- Original archive notices retained in each installed Pandoc generation: licenses/COPYING.rtf and licenses/COPYRIGHT.txt
- Exact main source: https://github.com/jgm/pandoc/tree/30b135468435dc4fbccfc2694508fa8e3e9ddc33
- The exact Windows dependency plan, linked dependency sources and applicable notices must accompany the corresponding-source review; a newly resolved build plan is not a substitute.

## Typst

- Version: 0.15.1; Windows x86_64 MSVC static-CRT build from unmodified official sources.
- Origin: https://github.com/typst/typst/releases/tag/v0.15.1
- Supplied executable SHA-256: c5c90354ff5ac8f213be9ffe95ef534f97e8c34796c48f8f3e6ffebb9698878f
- Build record: tools/typst/BUILD.txt and BUILD.json in the ZIP; rebuild script and pinned inputs: scripts/build_typst_windows.py and packaging/typst-static.lock.json.
- Main license: Apache-2.0. Component and embedded asset licenses also apply.
- Original official LICENSE and NOTICE retained unchanged in tools/typst/.
- Original embedded asset notices retained in tools/typst/assets/.
- Original dependency notices: tools/typst/dependencies/, indexed by DEPENDENCIES.json (335 locked normal/build packages); standard-library notices in rust-runtime/.
- Exact main source: https://github.com/typst/typst/tree/9dfd3a08500b7896045f907433cf7b4b02434fad
- Locked asset crate: typst-assets 0.15.1; source commit ab9eed7b046c6a29f6cdb8566f4b44fb46a2f57f.
- Asset terms include SIL OFL 1.1, GUST/LPPL, Bitstream/Arev, CC0, W3C/BSD terms and NewCM10-Regular under GPLv3-or-later with Font/Distribution Exceptions. Preserve the complete asset NOTICE, not just the program's Apache license.
- ASSETS-STATIC.json records all 34 unmodified embedded font/ICC byte sequences. No font was altered.
- Preferred NewCM10-Regular source is included in tools/typst/sources/NewComputerModern/NewCM10-Regular.sfd, with the author's original README/build instructions and License.txt. The official CTAN 8.1.1 OTF is byte-identical to the locked Typst asset. Its original distribution exception for unmodified fonts is retained.
- Exact source crates for option-ext 0.2.0 (MPL-2.0) and glidesort 0.1.2 are included in tools/typst/sources/. Other locked versions/source checksums and notices are indexed in DEPENDENCIES.json and Cargo.lock.
- These provenance and notice records do not constitute legal advice or change Rust File Converter's MIT license.

## Fully bundled mode deferred

No large FFmpeg/Pandoc source companion is created because their binaries are
not included. Typst's small source inclusions and original notices travel with
its supplied executable. Historical FFmpeg/Pandoc source questions in
[the historical source assessment](docs/archive/bundled-compliance.md) remain relevant to a future fully offline package.
