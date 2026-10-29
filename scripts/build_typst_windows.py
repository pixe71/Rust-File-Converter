"""Rebuild unmodified official Typst with static MSVC CRT; never publish anything."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tomllib
import urllib.request
import zipfile

import package_windows as packaging
from windows_pe import require_standard_imports

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "packaging/typst-static.lock.json"


def run(arguments, **options):
    result = subprocess.run([str(value) for value in arguments], check=True, capture_output=True,
                            encoding="utf-8", errors="replace", **options)
    return result.stdout


def pinned_download(item, cache):
    cache.mkdir(parents=True, exist_ok=True)
    return packaging.download(item, cache)


def verify_source(source, archive, data):
    prefix = "typst-" + data["source_commit"] + "/"
    with zipfile.ZipFile(archive) as bundle:
        expected = set()
        for entry in bundle.infolist():
            if entry.is_dir():
                continue
            if not entry.filename.startswith(prefix):
                raise ValueError("Unexpected source archive root")
            relative = entry.filename[len(prefix):]
            path = packaging.checked_path(source, relative)
            if path.read_bytes() != bundle.read(entry):
                raise ValueError("Typst source modified: " + relative)
            expected.add(relative)
        actual = {path.relative_to(source).as_posix() for path in source.rglob("*") if path.is_file()}
        if actual != expected:
            raise ValueError("Unexpected files in the source snapshot")
    if packaging.sha256(source / "Cargo.lock") != data["cargo_lock_sha256"]:
        raise ValueError("Official Cargo.lock mismatch")


def copy_original(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return packaging.sha256(destination)


def dependency_notices(source, output, cache, environment, data):
    text = run(["cargo", "tree", "--locked", "--target", data["target"], "--package", "typst-cli",
                "--edges", "normal,build", "--prefix", "none", "--format", "{p}|{l}"], cwd=source, env=environment)
    packages = {}
    for line in text.splitlines():
        match = re.match(r"^(\S+) v([^\s|]+).*?\|(.*?)(?: \(\*\))?$", line)
        if match:
            packages[(match[1], match[2])] = match[3]
    locked = {(item["name"], item["version"]): item for item in tomllib.loads(
        (source / "Cargo.lock").read_text(encoding="utf-8"))["package"]}
    cargo = Path(environment.get("CARGO_HOME", Path.home() / ".cargo"))
    records = []
    for (name, version), license in sorted(packages.items()):
        item = locked[(name, version)]
        if "source" not in item:
            directory = source / "crates" / name
            files = [source / "LICENSE", source / "NOTICE"]
        else:
            matches = list((cargo / "registry/src").glob("*/" + name + "-" + version))
            if len(matches) != 1:
                raise ValueError("Cannot identify registry source: " + name + " " + version)
            directory = matches[0]
            files = [path for path in directory.rglob("*") if path.is_file() and re.match(
                r"^(licen[cs]e|copying|copyright|notice|authors)([._ -].*)?$", path.name, re.I)]
            manifest = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))["package"]
            if manifest.get("license-file"):
                files.append(packaging.checked_path(directory, manifest["license-file"]))
            for relative in data.get("inline_license_files", {}).get(name, []):
                files.append(packaging.checked_path(directory, relative))
            for supplement in data.get("license_supplements", {}).get(name, []):
                vcs = json.loads((directory / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
                if vcs["git"]["sha1"] != supplement["source_commit"]:
                    raise ValueError("Supplemental notice is not from this exact package commit")
                target = output / "dependencies" / (name + "-" + version) / supplement["file"]
                copy_original(pinned_download(supplement, cache), target)
        if not files and not data.get("license_supplements", {}).get(name):
            raise ValueError("No original license text for " + name + " " + version)
        notices = []
        for original in sorted(set(files)):
            relative = original.relative_to(directory) if original.is_relative_to(directory) else Path(original.name)
            filename = hashlib.sha256(str(relative).encode()).hexdigest()[:10] + "-" + original.name
            target = Path("dependencies") / (name + "-" + version) / filename
            digest = copy_original(original, output / target)
            notices.append({"file": target.as_posix(), "original_path": relative.as_posix(), "sha256": digest})
        for supplement in data.get("license_supplements", {}).get(name, []):
            notices.append(dict(supplement, file=(Path("dependencies") / (name + "-" + version) / supplement["file"]).as_posix()))
        record = {"name": name, "version": version, "declared_license": license,
                  "source": item.get("source", data["source_archive"]["url"]),
                  "source_checksum": item.get("checksum"), "notices": notices}
        if "MPL-2.0" in license or name in data.get("source_inclusions", []):
            archives = list((cargo / "registry/cache").glob("*/" + name + "-" + version + ".crate"))
            if len(archives) != 1 or packaging.sha256(archives[0]) != item["checksum"]:
                raise ValueError("Exact dependency source archive is missing")
            target = Path("sources") / archives[0].name
            copy_original(archives[0], output / target)
            record["included_source"] = target.as_posix()
        records.append(record)
    (output / "DEPENDENCIES.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
    return len(records)


def prepare_notices(source, output, cache, environment, data):
    copy_original(ROOT / "licenses/Typst/BUILD.txt", output / "BUILD.txt")
    for name in ["LICENSE", "NOTICE"]:
        copy_original(source / name, output / name)
    for name in ["LICENSE", "NOTICE"]:
        copy_original(ROOT / "licenses/Typst/assets" / name, output / "assets" / name)
    copy_original(source / "Cargo.lock", output / "Cargo.lock")
    font = data["font_source"]
    archive = pinned_download(font, cache)
    with tarfile.open(archive) as bundle:
        member = bundle.getmember(font["member"])
        if not member.isfile():
            raise ValueError("Font source must be a regular file")
        content = bundle.extractfile(member).read()
    if hashlib.sha256(content).hexdigest() != font["member_sha256"]:
        raise ValueError("Preferred font source mismatch")
    folder = output / "sources/NewComputerModern"
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "NewCM10-Regular.sfd").write_bytes(content)
    for key, filename in [("readme", "README"), ("license", "License.txt")]:
        copy_original(pinned_download(font[key], cache), folder / filename)
    return dependency_notices(source, output, cache, environment, data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=ROOT / ".cache/typst-build/downloads")
    parser.add_argument("--source-dir", type=Path)
    parser.add_argument("--target-dir", type=Path, default=ROOT / ".cache/typst-build/target")
    parser.add_argument("--output", type=Path, default=ROOT / ".cache/typst-static")
    parser.add_argument("--jobs", type=int, default=2)
    args = parser.parse_args()
    data = json.loads(LOCK.read_text(encoding="utf-8"))
    if os.name != "nt":
        parser.error("This build requires Windows x64 and MSVC C++ Build Tools/SDK")
    # A GET only; never execute upstream workflows or make a Git commit/tag.
    request = urllib.request.Request("https://api.github.com/repos/typst/typst/git/ref/tags/" + data["source_tag"],
                                     headers={"User-Agent": "RustFileConverter-Typst-build"})
    with urllib.request.urlopen(request, timeout=30) as response:
        reference = json.load(response)
    if reference["object"]["type"] != "commit" or reference["object"]["sha"] != data["source_commit"]:
        raise ValueError("Official Typst tag does not match the pinned commit")
    archive = pinned_download(data["source_archive"], args.cache)
    if args.source_dir:
        source = args.source_dir.resolve()
    else:
        extracted = args.cache.parent / ("source-" + data["source_commit"])
        source = extracted / ("typst-" + data["source_commit"])
        if not extracted.exists():
            packaging.extract(archive, extracted)
    verify_source(source, archive, data)
    version = run(["rustc", "--version", "--verbose"])
    if not version.startswith("rustc " + data["rust_version"] + " ") or "host: x86_64-pc-windows-msvc" not in version:
        raise ValueError("Use the documented Rust MSVC toolchain " + data["rust_version"])
    environment = dict(os.environ, RUSTFLAGS=data["rustflags"], TYPST_VERSION=data["version"],
                       TYPST_COMMIT_SHA=data["source_commit"], CARGO_TARGET_DIR=str(args.target_dir.resolve()))
    environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
    command = ["cargo", "build", "--release", "--locked", "--target", data["target"],
               "--package", "typst-cli", "--bin", "typst", "--jobs", str(args.jobs)]
    subprocess.run(command, cwd=source, env=environment, check=True)
    verify_source(source, archive, data)
    executable = args.target_dir.resolve() / data["target"] / "release/typst.exe"
    imports = require_standard_imports(executable)
    actual_version = run([executable, "--version"], env=dict(environment, PATH=""))
    if not actual_version.startswith("typst " + data["version"] + " (") or data["source_commit"][:7] not in actual_version:
        raise ValueError("Compiled Typst version/commit mismatch")
    args.output.mkdir(parents=True, exist_ok=True)
    copy_original(executable, args.output / "typst.exe")
    count = prepare_notices(source, args.output / "notices", args.cache, environment, data)
    metadata = {"version": data["version"], "source_commit": data["source_commit"],
                "source_archive": data["source_archive"], "cargo_lock_sha256": data["cargo_lock_sha256"],
                "target": data["target"], "rustc": version, "rustflags": data["rustflags"],
                "command": command, "source_modified": False, "features": "upstream defaults; self-update disabled",
                "build_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                "binary_sha256": packaging.sha256(args.output / "typst.exe"), "imports": imports,
                "dependency_notice_packages": count, "font_source": data["font_source"]}
    rust_docs = Path(run(["rustc", "--print", "sysroot"]).strip()) / "share/doc/rust"
    if not (rust_docs / "COPYRIGHT-library.html").is_file():
        raise ValueError("Install the rust-docs component for the pinned build toolchain to retain standard-library notices")
    copy_original(rust_docs / "COPYRIGHT-library.html", args.output / "notices/rust-runtime/COPYRIGHT-library.html")
    for path in sorted((rust_docs / "licenses").glob("*.txt")):
        copy_original(path, args.output / "notices/rust-runtime/licenses" / path.name)
    asset_roots = list((Path(environment.get("CARGO_HOME", Path.home() / ".cargo")) / "registry/src").glob("*/typst-assets-0.15.1"))
    if len(asset_roots) != 1:
        raise ValueError("Cannot identify the locked Typst asset sources")
    executable_bytes = executable.read_bytes()
    assets = []
    for path in sorted((asset_roots[0] / "files").rglob("*")):
        if path.suffix.lower() in (".otf", ".ttf", ".icc"):
            content = path.read_bytes()
            if content not in executable_bytes:
                raise ValueError("An embedded asset was changed or is missing")
            assets.append({"file": path.relative_to(asset_roots[0]).as_posix(), "sha256": hashlib.sha256(content).hexdigest(), "unmodified_bytes_present": True})
    (args.output / "notices/ASSETS-STATIC.json").write_text(json.dumps(assets, indent=2), encoding="utf-8")
    (args.output / "build-metadata.json").write_text(json.dumps(metadata, indent=2), encoding="utf-8")
    print(json.dumps(metadata, indent=2))


if __name__ == "__main__":
    main()
