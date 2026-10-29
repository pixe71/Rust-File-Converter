"""Read normal and delay-load PE imports without loading or executing the file."""
import json
from pathlib import Path
import struct
import sys

WINDOWS_DLLS = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "combase.dll",
    "crypt32.dll", "dbghelp.dll", "dnsapi.dll", "dwmapi.dll", "gdi32.dll",
    "imm32.dll", "iphlpapi.dll", "kernel32.dll", "msvcrt.dll", "ncrypt.dll",
    "normaliz.dll", "ntdll.dll", "ole32.dll", "oleaut32.dll", "opengl32.dll",
    "powrprof.dll", "profapi.dll", "psapi.dll", "rpcrt4.dll", "secur32.dll",
    "shell32.dll", "shlwapi.dll", "user32.dll", "userenv.dll", "uxtheme.dll",
    "version.dll", "winhttp.dll", "wininet.dll", "winmm.dll", "wintrust.dll",
    "ws2_32.dll", "uiautomationcore.dll",
}


def inspect_imports(path):
    data = Path(path).read_bytes()

    def unpack(format, offset):
        if offset < 0 or offset + struct.calcsize(format) > len(data):
            raise ValueError("Truncated PE structure")
        return struct.unpack_from(format, data, offset)

    if data[:2] != b"MZ":
        raise ValueError("Not a PE executable")
    pe, = unpack("<I", 60)
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE signature")
    machine, count = unpack("<HH", pe + 4)
    optional_size, = unpack("<H", pe + 20)
    optional = pe + 24
    magic, = unpack("<H", optional)
    if machine != 0x8664 or magic != 0x20b or optional_size < 240 or count > 128:
        raise ValueError("Expected a Windows x86_64 PE32+ executable")
    headers_size, = unpack("<I", optional + 60)
    image_base, = unpack("<Q", optional + 24)
    directory_count, = unpack("<I", optional + 108)
    if directory_count < 14:
        raise ValueError("PE import directories are missing")
    sections = []
    for index in range(count):
        virtual_size, address, raw_size, raw = unpack("<IIII", optional + optional_size + index * 40 + 8)
        sections.append((address, max(virtual_size, raw_size), raw))

    def offset(rva):
        for address, size, raw in sections:
            if address <= rva < address + size:
                result = raw + rva - address
                if result >= len(data):
                    raise ValueError("PE RVA exceeds file")
                return result
        if 0 <= rva < min(headers_size, len(data)):
            return rva
        raise ValueError("Unmapped PE RVA")

    def string(rva):
        start = offset(rva)
        end = data.find(b"\0", start, min(start + 260, len(data)))
        if end < 0:
            raise ValueError("Invalid PE import name")
        name = data[start:end].decode("ascii")
        if not name or any(character in name for character in "\\/:"):
            raise ValueError("Unsafe PE import name")
        return name

    def imports(index, descriptor_size):
        rva, size = unpack("<II", optional + 112 + index * 8)
        if not rva and not size:
            return []
        start = offset(rva)
        result = []
        for position in range(4096):
            entry = start + position * descriptor_size
            values = unpack("<" + "I" * (descriptor_size // 4), entry)
            if not any(values):
                return result
            if index == 1:
                name = values[3]
            else:
                name = values[1] if values[0] & 1 else values[1] - image_base
            result.append(string(name))
        raise ValueError("Unterminated PE import table")

    return {"architecture": "x86_64", "imports": sorted(set(imports(1, 20)), key=str.lower),
            "delay_imports": sorted(set(imports(13, 32)), key=str.lower)}


def require_standard_imports(path):
    result = inspect_imports(path)
    unexpected = [name for name in result["imports"] + result["delay_imports"]
                  if name.lower() not in WINDOWS_DLLS and not name.lower().startswith(
                      ("api-ms-win-core-", "api-ms-win-crt-", "ext-ms-win-"))]
    if unexpected:
        raise ValueError("Non-standard Windows runtime/import: " + ", ".join(unexpected))
    return result


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: py -3 scripts/windows_pe.py executable.exe")
    print(json.dumps(require_standard_imports(Path(sys.argv[1])), indent=2))
