"""Pinned Windows tool preparation and portable ZIP packaging (Python 3.11+).

The hybrid ZIP includes verified static-CRT Typst and its original notices.
FFmpeg/FFprobe and Pandoc are provisioned after user consent.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tempfile
import tarfile
import tomllib
import urllib.parse
import urllib.request
import zipfile
from windows_pe import require_standard_imports

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "packaging/windows-tools.lock.json"
STATIC_LOCK = ROOT / "packaging/typst-static.lock.json"
STATIC_BUNDLE = ROOT / ".cache/typst-static"


def verify_static_typst(bundle=STATIC_BUNDLE):
    data = json.loads(STATIC_LOCK.read_text(encoding="utf-8"))
    executable = bundle / "typst.exe"
    if not executable.is_file() or executable.is_symlink() or sha256(executable) != data["binary_sha256"]:
        raise ValueError("Static Typst is missing or has an incorrect SHA-256; run build_typst_windows.py")
    mandatory = {"LICENSE", "NOTICE", "assets/LICENSE", "assets/NOTICE", "Cargo.lock", "DEPENDENCIES.json", "ASSETS-STATIC.json", "sources/NewComputerModern/NewCM10-Regular.sfd"}
    expected = data.get("notice_files", {})
    if not mandatory.issubset(expected):
        raise ValueError("Static Typst license/notice manifest is incomplete")
    actual = {path.relative_to(bundle / "notices").as_posix() for path in (bundle / "notices").rglob("*") if path.is_file()}
    if actual != set(expected):
        raise ValueError("Static Typst license/notice files are missing or unexpected")
    for relative, digest in expected.items():
        path = checked_path(bundle / "notices", relative)
        if path.is_symlink() or sha256(path) != digest:
            raise ValueError("Static Typst notice SHA-256 mismatch: " + relative)
    metadata = json.loads((bundle / "build-metadata.json").read_text(encoding="utf-8"))
    for key in ["version", "source_commit", "cargo_lock_sha256", "target", "rustflags", "binary_sha256"]:
        if metadata.get(key) != data[key]:
            raise ValueError("Static Typst build metadata mismatch: " + key)
    if metadata.get("source_modified") is not False:
        raise ValueError("Typst must use unmodified official sources")
    require_standard_imports(executable)  # Normal AND delay-load imports, before launch.
    version = execute(executable, "--version")
    if not re.match(r"typst " + re.escape(data["version"]) + r"\s", version) or data["source_commit"][:7] not in version:
        raise ValueError("Static Typst version/commit mismatch")
    return data


class HttpsRedirectHandler(urllib.request.HTTPRedirectHandler):
    max_redirections = 5
    max_repeats = 2

    def redirect_request(self, request, file, code, message, headers, newurl):
        if urllib.parse.urlparse(newurl).scheme.lower() != "https":
            raise ValueError("Download redirect must remain HTTPS")
        return super().redirect_request(request, file, code, message, headers, newurl)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checked_path(root, relative):
    name = PurePosixPath(relative)
    if "\\" in relative or ":" in relative or name.is_absolute() or ".." in name.parts:
        raise ValueError(f"Unsafe relative path: {relative}")
    result = root.joinpath(*name.parts).resolve()
    if not result.is_relative_to(root.resolve()) or result == root.resolve():
        raise ValueError(f"Path escapes destination: {relative}")
    return result


def read_source_member(archive, member):
    checked_path(ROOT, member)
    if zipfile.is_zipfile(archive):
        with zipfile.ZipFile(archive) as bundle:
            entry = bundle.getinfo(member)
            if entry.is_dir() or (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("Source notice must be a regular file")
            return bundle.read(entry)
    with tarfile.open(archive) as bundle:
        entry = bundle.getmember(member)
        if not entry.isfile():
            raise ValueError("Source notice must be a regular file")
        return bundle.extractfile(entry).read()




def verify_source_identity(artifact, archive):
    if artifact["scope"] == "locked-assets-crate":
        member = artifact["file"].removesuffix(".crate") + "/.cargo_vcs_info.json"
        vcs = json.loads(read_source_member(archive, member))
        if vcs.get("git", {}).get("sha1") != artifact["source_id"]:
            raise ValueError("Locked asset source commit mismatch")


def tool_manifest(data, tools):
    entries = []
    for tool in data["tools"]:
        for executable in tool["executables"]:
            if tool["name"] == "Typst":
                static = json.loads(STATIC_LOCK.read_text(encoding="utf-8"))
                entries.append({"name": "Typst", "version": static["version"], "file": executable,
                                "sha256": sha256(tools / executable), "source_id": static["source_commit"],
                                "distribution": "supplied MSVC static-CRT build", "source_archive": static["source_archive"],
                                "license": "Apache-2.0; original dependency and asset notices apply"})
                continue
            entries.append({
                "name": "FFprobe" if executable == "ffprobe.exe" else tool["name"],
                "version": tool["version"], "file": executable, "sha256": sha256(tools / executable),
                "binary_archive": {"file": tool["binary_archive"], "url": tool["url"],
                                   "sha256": tool["sha256"], "release": tool["release_url"],
                                   "published_at": tool["release_published_at"]},
                "license": tool["license"], "source_id": tool["source_id"], "source_url": tool["source_url"],
                "license_scope": "Main program; original component and asset notices also apply",
                "notice_sha256": tool["notice_sha256"], "source_notices": tool["source_notices"],
                "source_archives": tool["source_archives"], "correspondence_verified": tool["correspondence_verified"],
                "requires_corresponding_source": tool["requires_corresponding_source"],
                "redistribution_reviewed": tool["redistribution_reviewed"],
                "release_blockers": tool["release_blockers"],
            })
    return {"schema": 2, "platform": data["platform"], "tools": entries}


def load_lock(path=LOCK):
    data = json.loads(path.read_text(encoding="utf-8"))
    if data["schema"] != 1 or data["platform"] != "windows-x86_64":
        raise ValueError("Unsupported tool manifest")
    if len(data["tools"]) != 3 or {t["name"] for t in data["tools"]} != {"FFmpeg", "Pandoc", "Typst"}:
        raise ValueError("Manifest must include FFmpeg, Pandoc and Typst exactly once")
    for tool in data["tools"]:
        url = urllib.parse.urlparse(tool["url"])
        if url.scheme != "https" or "latest" in url.path.lower() or tool["version"] not in url.path:
            raise ValueError("Each archive needs a versioned HTTPS URL")
        if not re.fullmatch(r"[0-9a-f]{64}", tool["sha256"]):
            raise ValueError("Each archive needs a pinned SHA-256")
        expected = {"FFmpeg": ["ffmpeg.exe", "ffprobe.exe"], "Pandoc": ["pandoc.exe"], "Typst": ["typst.exe"]}
        if tool["executables"] != expected[tool["name"]]:
            raise ValueError("Unexpected executable list")
        required_notices = {"FFmpeg": ["LICENSE", "README.txt"], "Pandoc": ["COPYING.rtf", "COPYRIGHT.txt"],
                            "Typst": ["LICENSE", "NOTICE", "README.md"]}
        if tool["notices"] != required_notices[tool["name"]]:
            raise ValueError("Required original license/notice list changed")
        if set(tool["binary_sha256"]) != set(tool["executables"]) or set(tool["notice_sha256"]) != set(tool["notices"]):
            raise ValueError("Every executable and original notice must have a pinned hash")
        if not all(re.fullmatch(r"[0-9a-f]{64}", value) for value in
                   [*tool["binary_sha256"].values(), *tool["notice_sha256"].values()]):
            raise ValueError("Invalid executable/notice checksum")
        if not re.fullmatch(r"[0-9a-f]{40}", tool["source_id"]):
            raise ValueError("Each upstream source needs a full immutable commit")
        for artifact in tool["source_archives"]:
            if urllib.parse.urlparse(artifact["url"]).scheme != "https" or "latest" in artifact["url"].lower():
                raise ValueError("Source archives must use fixed HTTPS URLs")
            if artifact["source_id"] not in artifact["url"] and artifact["scope"] != "locked-assets-crate":
                raise ValueError("Source URL must identify its immutable commit")
            if not re.fullmatch(r"[0-9a-f]{64}", artifact["sha256"]):
                raise ValueError("Source archive checksum missing")
            if not re.fullmatch(r"[0-9a-f]{40}", artifact["source_id"]):
                raise ValueError("Source archive must identify a full immutable commit")
            checked_path(ROOT, artifact["file"])
        if len({artifact["id"] for artifact in tool["source_archives"]}) != len(tool["source_archives"]):
            raise ValueError("Duplicate source archive id")
        source_ids = {artifact["id"] for artifact in tool["source_archives"]}
        for notice in tool["source_notices"]:
            if notice["archive_id"] not in source_ids or not re.fullmatch(r"[0-9a-f]{64}", notice["sha256"]):
                raise ValueError("Source notice must identify a pinned archive and checksum")
            checked_path(ROOT, notice["member"])
            checked_path(ROOT, notice["path"])
        if tool["name"] == "Typst" and not {"assets/LICENSE", "assets/NOTICE"}.issubset(
            {notice["path"] for notice in tool["source_notices"]}
        ):
            raise ValueError("Typst's embedded asset notices are required")
        for relative in [tool["archive_root"], tool["binary_directory"] or ".", *tool["executables"], *tool["notices"]]:
            if relative != ".":
                checked_path(ROOT, relative)
    return data


def download(tool, cache):
    label = tool.get("name", tool.get("id", "source archive"))
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / (tool["sha256"] + ".zip")
    if not archive.exists():
        request = urllib.request.Request(tool["url"], headers={"User-Agent": "Rust-File-Converter-packaging"})
        with tempfile.NamedTemporaryFile(dir=cache, suffix=".download", delete=False) as stream:
            partial = Path(stream.name)
            try:
                opener = urllib.request.build_opener(HttpsRedirectHandler())
                with opener.open(request, timeout=60) as response:
                    if urllib.parse.urlparse(response.url).scheme != "https":
                        raise ValueError("Download redirected away from HTTPS")
                    shutil.copyfileobj(response, stream)
                stream.close()
                if sha256(partial) != tool["sha256"]:
                    raise ValueError(f"SHA-256 mismatch for {label}")
                partial.replace(archive)
            finally:
                stream.close()
                partial.unlink(missing_ok=True)
    if sha256(archive) != tool["sha256"]:
        raise ValueError(f"SHA-256 mismatch for cached {label}")
    return archive


def extract(archive, destination):
    with zipfile.ZipFile(archive) as bundle:
        seen = set()
        for entry in bundle.infolist():
            checked_path(destination, entry.filename.rstrip("/"))
            key = entry.filename.rstrip("/").casefold()
            if key in seen:
                raise ValueError("Duplicate/case-alias archive path")
            seen.add(key)
            for component in PurePosixPath(entry.filename).parts:
                if component.endswith((" ", ".")) or component.split(".")[0].upper() in {
                    "CON", "PRN", "AUX", "NUL", *[f"COM{i}" for i in range(1, 10)], *[f"LPT{i}" for i in range(1, 10)]
                }:
                    raise ValueError("Unsafe Windows archive path")
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("Archive symlinks are not accepted")
        bundle.extractall(destination)


def execute(program, *args):
    flags = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0
    result = subprocess.run([str(program), *args], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            timeout=30, creationflags=flags)
    output = result.stdout.decode("utf-8", errors="replace")
    if result.returncode:
        raise ValueError(f"{program.name} failed ({result.returncode}): {output}")
    return output


def verify_pe64(program):
    with program.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError(f"Not a PE executable: {program}")
        stream.seek(0x3c)
        offset = int.from_bytes(stream.read(4), "little")
        stream.seek(offset)
        if stream.read(6) != b"PE\0\0\x64\x86":
            raise ValueError(f"Not an x86_64 PE executable: {program}")


def verify_binaries(tools, data):
    versions = {}
    for tool in data["tools"]:
        if not tool.get("automatic_download", True):
            continue
        for executable in tool["executables"]:
            program = tools / executable
            if sha256(program) != tool["binary_sha256"][executable]:
                raise ValueError(f"Unexpected executable SHA-256: {executable}")
            verify_pe64(program)
            argument = "-version" if executable.startswith("ff") else "--version"
            output = execute(program, argument)
            prefix = executable.removesuffix(".exe") + (" version " if executable.startswith("ff") else " ") + tool["version"]
            if not output.splitlines() or not re.match(re.escape(prefix) + r"(?=[\s(-]|$)", output.splitlines()[0]):
                raise ValueError(f"Unexpected {executable} version")
            versions[executable] = output
    configuration = versions["ffmpeg.exe"]
    if any(flag not in configuration for flag in ["--enable-gpl", "--enable-version3", "--enable-static"]) or "--enable-nonfree" in configuration:
        raise ValueError("Expected the reviewed GPL FFmpeg variant without nonfree")
    encoders = execute(tools / "ffmpeg.exe", "-hide_banner", "-encoders")
    names = {line.split()[1] for line in encoders.splitlines()
             if len(line.split()) >= 2 and len(line.split()[0]) == 6 and line.split()[0][3] != "X"}
    required = {"libmp3lame", "aac", "pcm_s16le", "flac", "libvorbis", "libx264", "libx265"}
    if not required.issubset(names):
        raise ValueError(f"Required FFmpeg encoders missing: {required - names}")
    return versions


def prepare(data, cache, tools, licenses):
    with tempfile.TemporaryDirectory(prefix="rfc-tools-") as temporary:
        temporary = Path(temporary)
        staged_tools = temporary / "tools"
        staged_tools.mkdir()
        staged_licenses = temporary / "licenses"
        for tool in data["tools"]:
            if not tool.get("automatic_download", True):
                continue
            extracted = temporary / tool["name"]
            extracted.mkdir()
            extract(download(tool, cache), extracted)
            root = checked_path(extracted, tool["archive_root"])
            for executable in tool["executables"]:
                shutil.copy2(root / tool["binary_directory"] / executable, staged_tools / executable)
            notices = staged_licenses / tool["name"]
            notices.mkdir(parents=True)
            for notice in tool["notices"]:
                source = checked_path(root, notice)
                if sha256(source) != tool["notice_sha256"][notice]:
                    raise ValueError(f"Original notice checksum mismatch: {tool['name']}/{notice}")
                shutil.copy2(source, notices / Path(notice).name)
            for notice in tool["source_notices"]:
                artifact = next(a for a in tool["source_archives"] if a["id"] == notice["archive_id"])
                archive = download(artifact, cache)
                verify_source_identity(artifact, archive)
                content = read_source_member(archive, notice["member"])
                if hashlib.sha256(content).hexdigest() != notice["sha256"]:
                    raise ValueError("Source notice checksum mismatch")
                target = checked_path(notices, notice["path"])
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(content)
            (notices / "provenance.json").write_text(json.dumps(tool, indent=2), encoding="utf-8")
        versions = verify_binaries(staged_tools, data)
        for name, version in versions.items():
            group = "FFmpeg" if name.startswith("ff") else ("Pandoc" if name == "pandoc.exe" else "Typst")
            (staged_licenses / group / (name + "-version.txt")).write_text(version, encoding="utf-8")
        verify_static_typst()
        shutil.copy2(STATIC_BUNDLE / "typst.exe", staged_tools / "typst.exe")
        shutil.copytree(STATIC_BUNDLE / "notices", staged_licenses / "Typst", dirs_exist_ok=True)
        record = tool_manifest(data, staged_tools)
        tools.mkdir(parents=True, exist_ok=True)
        licenses.mkdir(parents=True, exist_ok=True)
        for path in staged_tools.iterdir():
            shutil.copy2(path, tools / path.name)
            if sha256(tools / path.name) != sha256(path):
                raise ValueError(f"Prepared executable checksum mismatch: {path.name}")
        shutil.copytree(staged_licenses, licenses, dirs_exist_ok=True)
        (tools / "tools-manifest.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    return record




def write_zip(root, destination):
    # Crates preserve upstream 1970 timestamps; ZIP's DOS date starts in 1980.
    # Clamp dates, never alter the original notice bytes.
    with zipfile.ZipFile(destination, "x", zipfile.ZIP_DEFLATED, strict_timestamps=False) as bundle:
        for path in sorted(root.rglob("*")):
            if path.is_file():
                bundle.write(path, path.relative_to(root.parent))


def publish_artifacts(pairs):
    """No replacement; rollback only the exact hard links created by this call."""
    published = []
    try:
        for staged, destination in pairs:
            os.link(staged, destination)
            published.append((staged, destination))
    except BaseException:
        for staged, destination in reversed(published):
            try:
                if os.path.samefile(staged, destination):
                    destination.unlink()
            except FileNotFoundError:
                pass
        raise




def package(app, output, typst_bundle=STATIC_BUNDLE):
    """Hybrid release: supplied verified Typst; no FFmpeg/Pandoc binaries."""
    if not app.is_file():
        raise ValueError(f"Build the release executable first: {app}")
    verify_pe64(app)
    static = verify_static_typst(typst_bundle)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    documents = ["README.md", "CHANGELOG.md", "LICENCE", "THIRD-PARTY-NOTICES.md", "icon.png", "screen.png"]
    if not (ROOT / "docs/README.md").is_file():
        raise ValueError("Missing documentation index: docs/README.md")
    documents.extend(path.relative_to(ROOT).as_posix()
                     for path in sorted((ROOT / "docs").rglob("*.md")))
    for document in documents:
        if not (ROOT / document).is_file() or not (ROOT / document).stat().st_size:
            raise ValueError(f"Missing package document: {document}")
    output.mkdir(parents=True, exist_ok=True)
    names = [f"Rust-File-Converter-v{version}-win64.zip", "SHA256SUMS.txt"]
    for name in names:
        if (output / name).exists() or (output / name).is_symlink():
            raise ValueError(f"Refusing to overwrite {output / name}")
    with tempfile.TemporaryDirectory(dir=output, prefix=".rust-file-converter-release-") as temporary:
        temporary = Path(temporary)
        folder = temporary / "Rust-File-Converter"
        folder.mkdir()
        shutil.copy2(app, folder / f"File_Converter_V{version}.exe")
        (folder / "tools").mkdir()
        shutil.copy2(typst_bundle / "typst.exe", folder / "tools/typst.exe")
        if sha256(folder / "tools/typst.exe") != static["binary_sha256"]:
            raise ValueError("Static Typst changed while packaging")
        for relative, digest in static["notice_files"].items():
            destination = checked_path(folder / "tools/typst", relative)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(checked_path(typst_bundle / "notices", relative), destination)
            if sha256(destination) != digest:
                raise ValueError("Static Typst notice changed while packaging")
        shutil.copy2(typst_bundle / "build-metadata.json", folder / "tools/typst/BUILD.json")
        for document in documents:
            destination = folder / document
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / document, destination)
        archive, sums = [temporary / name for name in names]
        write_zip(folder, archive)
        sums.write_text(f"{sha256(archive)}  {archive.name}\n", encoding="ascii")
        publish_artifacts([(temporary / name, output / name) for name in names])
    result = [{"file": name, "size": (output / name).stat().st_size, "sha256": sha256(output / name)} for name in names]
    print(json.dumps(result, indent=2))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "package"])
    parser.add_argument("--lock", type=Path, default=LOCK)
    parser.add_argument("--cache", type=Path, default=ROOT / ".cache/bundled-tools")
    parser.add_argument("--app", type=Path, default=ROOT / "target/release/file_converter.exe")
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--typst-bundle", type=Path, default=STATIC_BUNDLE)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("This script prepares Windows x86_64 executables only")
    data = load_lock(args.lock)
    if args.action == "prepare":
        prepare(data, args.cache, ROOT / "tools", ROOT / ".cache/tool-licenses")
        print("Local tools ready; this is not redistribution approval.")
    else:
        package(args.app, args.output, args.typst_bundle)


if __name__ == "__main__":
    main()
