# Third-party components and licenses

Rust File Converter retains its [MIT LICENCE](../LICENCE). FFmpeg, Pandoc and
Typst are separate executable programs, not libraries linked into the main Rust
binary. Their own licenses, dependencies and embedded assets still apply.
This is technical provenance/notice documentation, not legal advice.

The [THIRD-PARTY-NOTICES](../THIRD-PARTY-NOTICES.md) index stays at the repository
root because the release packager includes it. It does not replace original
texts or certify redistribution clearance.

## Distribution scope and retained inputs

The hybrid ZIP supplies only static-CRT Typst, its original notices and small
source inclusions. FFmpeg/FFprobe and Pandoc archives are downloaded directly
from publishers after consent; they are not republished in the main ZIP.
This does not automatically remove legal obligations.

Original vendor texts remain byte-for-byte under
licenses/<vendor>/original/, with Git line-ending conversion disabled.
Separate Typst asset LICENSE/NOTICE remain under licenses/Typst/assets/.
The scripts' pinned JSON inputs, historical inventories and BUILD records
remain in packaging/ and licenses/. Human explanations have moved here.

Do not describe the developer tools/ folder as the official distribution.
The packager checks all supplied Typst notice hashes. Historical all-engine
bundled findings remain in the [source assessment](archive/bundled-compliance.md)
and [FFmpeg component inventory](archive/ffmpeg-components.md).

## FFmpeg / FFprobe

- Selected build: Gyan 9.0.2 essentials, Windows x64 static GPL build.
- Origin: https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2
- Build description: https://www.gyan.dev/ffmpeg/builds/
- Exact main source: https://github.com/FFmpeg/FFmpeg/tree/946fcce07b6dcd0331c8cc609192aeff5e1924f8
- Licensing guidance: https://ffmpeg.org/legal.html
- Pinned archive/binary hashes: packaging/windows-tools.lock.json.
- Retained originals: licenses/FFmpeg/original/LICENSE and README.txt.
- Configuration record: licenses/FFmpeg/BUILD-9.0.2.txt.

This build includes libx264/libx265 and is not LGPL-only. Preparation rejects
--enable-nonfree and checks MP3, AAC, PCM WAV, FLAC, Vorbis, H.264 and H.265
encoders. Developer preparation also records both executables' version outputs
under .cache/tool-licenses/FFmpeg/.

The archive gives 37 dependency version declarations, 42 enabled external
library entries and 13 hardware/API entries. The machine-readable inventory
remains licenses/FFmpeg/COMPONENTS.json. Unknown source/patch/build fields remain
unknown, not assertions of no modifications. Hardware declarations are not
automatically treated as statically linked implementations.

For any future direct redistribution, matching linked-library sources,
versions, patches, build instructions, original notices and recipient source
access still require review. The main FFmpeg snapshot alone is not the full
static-build source set, nor is an unverified third-party source offer enough.
Patent questions are separate from copyright licensing.

## Pandoc

- Selected binary: official Pandoc 3.13 Windows x86_64 ZIP.
- Origin: https://github.com/jgm/pandoc/releases/tag/3.13
- Source: https://github.com/jgm/pandoc/tree/30b135468435dc4fbccfc2694508fa8e3e9ddc33
- Copyright: https://github.com/jgm/pandoc/blob/3.13/COPYRIGHT
- Pinned hashes: packaging/windows-tools.lock.json.
- Retained originals: licenses/Pandoc/original/COPYING.rtf and COPYRIGHT.txt.

Pandoc is GPLv2-or-later with component/template exceptions in COPYRIGHT.
Preparation preserves both original files and records --version. The verified
main snapshot does not complete linked-dependency/source delivery review for
a future direct redistribution.

The official Windows build run/job 40389583308 / 121806764611 was identified,
but retrieved freeze output contained masked package/version values. That
unusable intermediate log was removed; no values were guessed. An unredacted
plan, source/notices and matching build inputs remain necessary for a future
fully bundled package.

## Supplied Typst

- Version: 0.15.1, built from unmodified official sources with static MSVC CRT.
- Source: https://github.com/typst/typst/tree/9dfd3a08500b7896045f907433cf7b4b02434fad
- Main license: Apache-2.0; component and asset terms also apply.
- Rebuild/provenance: [Static Typst build](typst-build.md),
  scripts/build_typst_windows.py, packaging/typst-static.lock.json and
  licenses/Typst/BUILD.txt.

The ZIP retains official LICENSE/NOTICE unchanged in tools/typst/.
Original dependency notices are indexed by DEPENDENCIES.json for 335 locked
normal/build packages. Compiler standard-library notices remain in
tools/typst/rust-runtime/. Exact versions/checksums appear in the official
Cargo.lock and generated inventory.

Original asset LICENSE/NOTICE are included in tools/typst/assets/. Their terms
include SIL OFL 1.1, GUST/LPPL, Bitstream/Arev, CC0, W3C/BSD and NewCM10-Regular
under GPLv3-or-later with Font/Distribution Exceptions. Do not relabel all
assets Apache-2.0 or omit those notices.

The new ASSETS-STATIC.json records all 34 unchanged embedded font/ICC byte
sequences. The repository's older licenses/Typst/ASSETS.json describes the
official binary, not the static candidate.

Preferred NewCM10-Regular SFD source, the original author's README/build
instructions and License.txt are included under
tools/typst/sources/NewComputerModern/. The official CTAN 8.1.1 OTF is
byte-identical to the locked Typst asset. No font was modified; its original
distribution exception for unmodified fonts is retained. Exact option-ext
0.2.0 (MPL-2.0) and glidesort 0.1.2 source crates also accompany the executable.

The earlier VCRUNTIME140.dll blocker applied to the official binary.
The supplied candidate's normal/delay imports and recorded dynamic traces
contain no forbidden VC++/MinGW runtime. This technical result does not
constitute a legal opinion or change the application's MIT license.
