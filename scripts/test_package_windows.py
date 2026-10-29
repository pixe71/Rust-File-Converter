"""Offline structural tests for pinned preparation and release safeguards."""
import copy
import json
import os
import struct
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_windows as packaging
from windows_pe import require_standard_imports




def fake_pe(path, machine=b"\x64\x86"):
    data = bytearray(256)
    data[:2] = b"MZ"
    data[0x3c:0x40] = (128).to_bytes(4, "little")
    data[128:134] = b"PE\0\0" + machine
    path.write_bytes(data)


def full_pe(path, normal="kernel32.dll", delay=None):
    data = bytearray(2048)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<HH", data, 132, 0x8664, 0)
    struct.pack_into("<H", data, 148, 240)
    optional = 152
    struct.pack_into("<H", data, optional, 0x20b)
    struct.pack_into("<I", data, optional + 60, len(data))
    struct.pack_into("<I", data, optional + 108, 16)
    if normal:
        struct.pack_into("<II", data, optional + 120, 600, 40)
        struct.pack_into("<IIIII", data, 600, 0, 0, 0, 900, 0)
        data[900:900 + len(normal) + 1] = normal.encode() + b"\0"
    if delay:
        struct.pack_into("<II", data, optional + 112 + 13 * 8, 700, 64)
        struct.pack_into("<IIIIIIII", data, 700, 1, 1000, 0, 0, 0, 0, 0, 0)
        data[1000:1000 + len(delay) + 1] = delay.encode() + b"\0"
    path.write_bytes(data)


def static_fixture(root):
    bundle = root / "static"
    notices = bundle / "notices"
    notices.mkdir(parents=True)
    names = ["LICENSE", "NOTICE", "assets/LICENSE", "assets/NOTICE", "Cargo.lock",
             "DEPENDENCIES.json", "ASSETS-STATIC.json", "sources/NewComputerModern/NewCM10-Regular.sfd"]
    for name in names:
        path = notices / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"test fixture")
    full_pe(bundle / "typst.exe")
    data = {"version": "0.15.1", "source_commit": "9dfd3a08500b7896045f907433cf7b4b02434fad",
            "target": "x86_64-pc-windows-msvc", "rustflags": "-C target-feature=+crt-static",
            "cargo_lock_sha256": "a" * 64, "binary_sha256": packaging.sha256(bundle / "typst.exe"),
            "notice_files": {name: packaging.sha256(notices / name) for name in names}}
    (bundle / "build-metadata.json").write_text(json.dumps(dict(data, source_modified=False)))
    lock = root / "static-lock.json"
    lock.write_text(json.dumps(data))
    return bundle, lock, data


class PackagingTests(unittest.TestCase):
    def test_zip_preserves_original_notice_bytes_with_pre_1980_timestamp(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "package"
            root.mkdir()
            notice = root / "LICENSE"
            notice.write_bytes(b"unaltered upstream text\r\n")
            os.utime(notice, (0, 0))
            archive = Path(directory) / "package.zip"
            packaging.write_zip(root, archive)
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(bundle.read("package/LICENSE"), notice.read_bytes())
                self.assertEqual(bundle.getinfo("package/LICENSE").date_time[0], 1980)

    def test_download_redirects_are_bounded_and_never_downgrade_https(self):
        handler = packaging.HttpsRedirectHandler()
        request = packaging.urllib.request.Request("https://example.invalid/archive.zip")
        self.assertEqual(handler.max_redirections, 5)
        with self.assertRaisesRegex(ValueError, "remain HTTPS"):
            handler.redirect_request(request, None, 302, "Found", {}, "http://example.invalid/archive.zip")
        redirected = handler.redirect_request(request, None, 302, "Found", {}, "https://example.invalid/pinned.zip")
        self.assertEqual(redirected.full_url, "https://example.invalid/pinned.zip")

    def test_hybrid_package_includes_only_verified_typst_and_never_downloads(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\nversion="1.1.0"\n', encoding="utf-8")
            documents = ["README.md", "LICENCE", "CHANGELOG.md", "THIRD-PARTY-NOTICES.md", "icon.png", "screen.png", "docs/README.md", "docs/archive/review.md"]
            for filename in documents:
                (root / filename).parent.mkdir(parents=True, exist_ok=True)
                (root / filename).write_text("test only", encoding="utf-8")
            (root / "docs/ignore.exe").write_bytes(b"must not be packaged")
            app = root / "app.exe"
            fake_pe(app)
            static, lock, data = static_fixture(root)
            with (patch.object(packaging, "ROOT", root),
                  patch.object(packaging, "STATIC_LOCK", lock),
                  patch.object(packaging, "execute", return_value="typst 0.15.1 (9dfd3a08)\n"),
                  patch.object(packaging, "prepare", side_effect=AssertionError("No third-party preparation")),
                  patch.object(packaging, "download", side_effect=AssertionError("No downloads")),
                  patch("builtins.print")):
                packaging.package(app, root / "dist", static)
                archive = root / "dist/Rust-File-Converter-v1.1.0-win64.zip"
                with zipfile.ZipFile(archive) as bundle:
                    files = documents + ["File_Converter_V1.1.0.exe", "tools/typst.exe", "tools/typst/BUILD.json"]
                    files += ["tools/typst/" + name for name in data["notice_files"]]
                    self.assertEqual(set(bundle.namelist()), {"Rust-File-Converter/" + name for name in files})
                    for name in documents:
                        self.assertEqual(bundle.read("Rust-File-Converter/" + name), (root / name).read_bytes())
                self.assertEqual(len(list((root / "dist").glob("*.zip"))), 1)
                self.assertIn(packaging.sha256(archive), (root / "dist/SHA256SUMS.txt").read_text())
                with self.assertRaisesRegex(ValueError, "overwrite"):
                    packaging.package(app, root / "dist", static)

    def test_package_requires_documentation_index(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\nversion="1.1.0"\n')
            app = root / "app.exe"
            fake_pe(app)
            static, lock, _ = static_fixture(root)
            with (patch.object(packaging, "ROOT", root),
                  patch.object(packaging, "STATIC_LOCK", lock),
                  patch.object(packaging, "execute", return_value="typst 0.15.1 (9dfd3a08)\n")):
                with self.assertRaisesRegex(ValueError, "Missing documentation index"):
                    packaging.package(app, root / "dist", static)
            self.assertFalse((root / "dist").exists())

    def test_static_typst_rejects_missing_binary_or_wrong_hash_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle, lock, _ = static_fixture(Path(directory))
            with patch.object(packaging, "STATIC_LOCK", lock), patch.object(packaging, "execute", side_effect=AssertionError("No launch")):
                (bundle / "typst.exe").write_bytes(b"changed")
                with self.assertRaisesRegex(ValueError, "SHA-256"):
                    packaging.verify_static_typst(bundle)
                (bundle / "typst.exe").unlink()
                with self.assertRaisesRegex(ValueError, "missing"):
                    packaging.verify_static_typst(bundle)

    def test_static_typst_rejects_missing_original_license_and_notice(self):
        for name in ["LICENSE", "NOTICE"]:
            with tempfile.TemporaryDirectory() as directory:
                bundle, lock, _ = static_fixture(Path(directory))
                (bundle / "notices" / name).unlink()
                with patch.object(packaging, "STATIC_LOCK", lock), patch.object(packaging, "execute", side_effect=AssertionError("No launch")):
                    with self.assertRaisesRegex(ValueError, "notice files"):
                        packaging.verify_static_typst(bundle)

    def test_static_typst_rejects_wrong_version(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle, lock, _ = static_fixture(Path(directory))
            with patch.object(packaging, "STATIC_LOCK", lock), patch.object(packaging, "execute", return_value="typst 0.15.2 (9dfd3a08)"):
                with self.assertRaisesRegex(ValueError, "version/commit"):
                    packaging.verify_static_typst(bundle)

    def test_pe_rejects_normal_and_delay_external_runtimes(self):
        forbidden = ["VCRUNTIME140.dll", "VCRUNTIME140_1.dll", "MSVCP140.dll", "libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll", "msys-2.0.dll"]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.exe"
            full_pe(path)
            self.assertEqual(require_standard_imports(path)["delay_imports"], [])
            for name in forbidden:
                for delay in [False, True]:
                    full_pe(path, normal="kernel32.dll" if delay else name, delay=name if delay else None)
                    with self.subTest(name=name, delay=delay), self.assertRaisesRegex(ValueError, "Non-standard"):
                        require_standard_imports(path)

    def test_packaging_rejects_forbidden_imports_before_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle, lock, data = static_fixture(Path(directory))
            full_pe(bundle / "typst.exe", delay="VCRUNTIME140.dll")
            data["binary_sha256"] = packaging.sha256(bundle / "typst.exe")
            lock.write_text(json.dumps(data))
            (bundle / "build-metadata.json").write_text(json.dumps(dict(data, source_modified=False)))
            with patch.object(packaging, "STATIC_LOCK", lock), patch.object(packaging, "execute", side_effect=AssertionError("No launch")):
                with self.assertRaisesRegex(ValueError, "Non-standard"):
                    packaging.verify_static_typst(bundle)



    def test_asset_crate_source_identity_must_match_the_lock(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "asset.zip"
            artifact = {"scope": "locked-assets-crate", "file": "assets-1.0.crate", "source_id": "a" * 40}
            with zipfile.ZipFile(archive, "w") as bundle:
                bundle.writestr("assets-1.0/.cargo_vcs_info.json", json.dumps({"git": {"sha1": "b" * 40}}))
            with self.assertRaisesRegex(ValueError, "source commit mismatch"):
                packaging.verify_source_identity(artifact, archive)

    def test_required_license_and_asset_notice_cannot_be_omitted(self):
        data = packaging.load_lock()
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "lock.json"
            invalid = copy.deepcopy(data)
            invalid["tools"][0]["notices"] = []
            invalid["tools"][0]["notice_sha256"] = {}
            lock.write_text(json.dumps(invalid), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "Required original"):
                packaging.load_lock(lock)
            invalid = copy.deepcopy(data)
            invalid["tools"][2]["source_notices"] = []
            lock.write_text(json.dumps(invalid), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "embedded asset notices"):
                packaging.load_lock(lock)

    def test_missing_or_changed_original_notice_fails_before_execution(self):
        tool = copy.deepcopy(packaging.load_lock()["tools"][0])
        data = {"tools": [tool]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for missing in [True, False]:
                archive = root / "input.zip"
                with zipfile.ZipFile(archive, "w") as bundle:
                    for name in tool["executables"]:
                        bundle.writestr(tool["archive_root"] + "/bin/" + name, b"fake executable")
                    if not missing:
                        bundle.writestr(tool["archive_root"] + "/LICENSE", b"modified original")
                with patch.object(packaging, "download", return_value=archive), patch.object(
                    packaging, "verify_binaries", side_effect=AssertionError("Must not launch any binary")
                ):
                    with self.assertRaises((FileNotFoundError, ValueError)):
                        packaging.prepare(data, root, root / "tools", root / "licenses")


    def test_wrong_binary_hash_is_rejected_before_launch(self):
        data = packaging.load_lock()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "ffmpeg.exe").write_bytes(b"not the pinned executable")
            with patch.object(packaging, "execute", side_effect=AssertionError("Must not execute")):
                with self.assertRaisesRegex(ValueError, "Unexpected executable SHA-256"):
                    packaging.verify_binaries(root, data)

    def test_wrong_architecture_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "x86.exe"
            fake_pe(path, b"\x4c\x01")
            with self.assertRaisesRegex(ValueError, "x86_64"):
                packaging.verify_pe64(path)

    def test_version_prefix_must_match_exactly(self):
        data = copy.deepcopy(packaging.load_lock())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_pe(root / "ffmpeg.exe")
            data["tools"][0]["binary_sha256"]["ffmpeg.exe"] = packaging.sha256(root / "ffmpeg.exe")
            with patch.object(packaging, "execute", return_value="ffmpeg version 9.0.20\n"):
                with self.assertRaisesRegex(ValueError, "Unexpected ffmpeg.exe version"):
                    packaging.verify_binaries(root, data)

    def test_manifest_identifies_each_executable_and_original_archive(self):
        data = packaging.load_lock()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for tool in data["tools"]:
                for name in tool["executables"]:
                    (root / name).write_bytes(name.encode())
            manifest = packaging.tool_manifest(data, root)
            self.assertEqual(len(manifest["tools"]), 4)
            for entry in manifest["tools"]:
                self.assertEqual(entry["sha256"], packaging.sha256(root / entry["file"]))
                self.assertEqual(len(entry["source_id"]), 40)
                if entry["name"] == "Typst":
                    self.assertIn("sha256", entry["source_archive"])
                    self.assertIn("static-CRT", entry["distribution"])
                else:
                    self.assertIn("sha256", entry["binary_archive"])
                    self.assertFalse(entry["correspondence_verified"])

    def test_case_aliases_and_windows_devices_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["SAFE.TXT", "NUL.txt", "folder/trailing."]:
                archive = root / "archive.zip"
                with zipfile.ZipFile(archive, "w") as bundle:
                    bundle.writestr("safe.txt", b"a")
                    bundle.writestr(name, b"b")
                with self.assertRaises(ValueError):
                    packaging.extract(archive, root / "output")

    def test_multi_artifact_failure_rolls_back_only_owned_links(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, second = root / "staged-1", root / "staged-2"
            first.write_bytes(b"first")
            second.write_bytes(b"second")
            out1, out2 = root / "user.zip", root / "sources.zip"
            out2.write_bytes(b"existing user file")
            with self.assertRaises(FileExistsError):
                packaging.publish_artifacts([(first, out1), (second, out2)])
            self.assertFalse(out1.exists())
            self.assertEqual(out2.read_bytes(), b"existing user file")

    def test_lock_is_versioned_and_pinned(self):
        data = packaging.load_lock()
        self.assertEqual(len(data["tools"]), 3)
        self.assertEqual(sum(len(t["executables"]) for t in data["tools"]), 4)

    def test_latest_url_and_missing_checksum_are_rejected(self):
        data = packaging.load_lock()
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "lock.json"
            for field, value in [("url", "https://example.org/latest.exe"), ("sha256", "")]:
                invalid = copy.deepcopy(data)
                invalid["tools"][0][field] = value
                lock.write_text(json.dumps(invalid), encoding="utf-8")
                with self.assertRaises(ValueError):
                    packaging.load_lock(lock)

    def test_cached_checksum_mismatch_fails_without_download_or_extraction(self):
        tool = packaging.load_lock()["tools"][0]
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            (cache / (tool["sha256"] + ".zip")).write_bytes(b"corrupt")
            with patch("urllib.request.urlopen", side_effect=AssertionError("No download expected")):
                with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                    packaging.download(tool, cache)

    def test_archive_traversal_is_rejected_before_any_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            for member in ["../escape", "/absolute", "C:/escape", r"..\escape"]:
                archive = directory / "bad.zip"
                with zipfile.ZipFile(archive, "w") as bundle:
                    bundle.writestr("safe.txt", "safe")
                    bundle.writestr(member, "bad")
                destination = directory / "output"
                with self.assertRaises(ValueError):
                    packaging.extract(archive, destination)
                self.assertFalse(destination.exists())

    def test_archive_symlinks_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "link.zip"
            with zipfile.ZipFile(archive, "w") as bundle:
                info = zipfile.ZipInfo("link")
                info.external_attr = 0o120777 << 16
                bundle.writestr(info, "../escape")
            with self.assertRaisesRegex(ValueError, "symlink"):
                packaging.extract(archive, Path(directory) / "output")





if __name__ == "__main__":
    unittest.main()
