# <img src="icon.png" width="25px" alt="Rust File Converter icon"> Rust File Converter

A desktop file converter built with **Rust**, featuring a simple graphical interface for images, audio, video and documents.

![Rust](https://img.shields.io/badge/Made_with-Rust-orange?style=for-the-badge&logo=rust)
![GUI](https://img.shields.io/badge/GUI-egui-blue?style=for-the-badge)

## Screenshot

<img src="screen.png" width="300px" alt="Rust File Converter screenshot">

## Features

- Convert multiple files using drag and drop or file selection.
- Remove individual files and ignore duplicate selections.
- Choose the destination folder before converting.
- Keep original and existing files with automatic output renaming.
- Follow progress and review results and errors for each file.

### Images

Native conversion without external engines.

- **Inputs:** JPG/JPEG, PNG, BMP, GIF, WebP, ICO.
- **Outputs:** JPG, PNG, BMP, GIF, WebP, ICO.

### Audio

Audio conversion with bitrate selection where applicable.

- **Inputs:** MP3, WAV, OGG, FLAC, M4A, AAC.
- **Outputs:** MP3, WAV, OGG, FLAC, AAC.

### Video

Video conversion with codec selection.

- **Inputs:** MP4, MKV, AVI, MOV, WebM.
- **Outputs:** MP4, MKV, AVI, MOV.

### Documents

- **Inputs:** DOCX, UTF-8 TXT.
- **Outputs:** plain TXT, DOCX, PDF.

PDF input is not supported. TXT input is interpreted as Markdown.

## Official Windows release

The official Windows release is ready to use without manually installing FFmpeg, Pandoc or a PDF engine.

1. Download the Windows ZIP from [GitHub Releases](https://github.com/pixe71/Rust-File-Converter/releases).
2. Extract it.
3. Run `File_Converter_V1.1.0.exe`.
4. On first launch, allow Rust File Converter to prepare its conversion engines.

FFmpeg/FFprobe and Pandoc are downloaded and verified automatically when needed.
Typst is bundled with the application for PDF generation.

No administrator rights or PATH configuration are required.
After the initial setup, the conversion engines remain available offline.

## Build from source

With Rust and the platform's build tools installed:

```bash
git clone https://github.com/pixe71/Rust-File-Converter.git
cd Rust-File-Converter
cargo run --release --locked
```

To build without launching:

```bash
cargo build --release --locked
```

The Windows executable is `target/release/file_converter.exe`.
For development, engines can be found in PATH, supplied in `tools/` beside the
executable, or configured with `RFC_TOOLS_DIR`.

## Technologies Used

- **Language:** [Rust](https://www.rust-lang.org/)
- **Interface:** [eframe / egui](https://github.com/emilk/egui)
- **Images:** the Rust `image` crate
- **Audio / video:** FFmpeg
- **Documents / PDF:** Pandoc and Typst

## Technical documentation

For technical documentation, see [docs/](docs/).

## License

Rust File Converter is distributed under the [MIT License](LICENCE).
Conversion engines retain their own licenses; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
