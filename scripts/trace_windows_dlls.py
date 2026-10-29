"""Trace DLL load events of our own child process; never alter Windows DLLs."""
import ctypes as c
from ctypes import wintypes as w
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def trace(command, environment=None, cwd=None, timeout=60):
    if os.name != "nt" or c.sizeof(c.c_void_p) != 8:
        raise ValueError("DLL tracing needs 64-bit Windows Python")
    class ExceptionRecord(c.Structure):
        _fields_ = [("code", w.DWORD), ("flags", w.DWORD), ("record", c.c_void_p),
                    ("address", c.c_void_p), ("count", w.DWORD), ("information", c.c_size_t * 15)]
    class ExceptionInfo(c.Structure):
        _fields_ = [("record", ExceptionRecord), ("first", w.DWORD)]
    class CreateInfo(c.Structure):
        _fields_ = [("file", w.HANDLE), ("process", w.HANDLE), ("thread", w.HANDLE),
                    ("base", c.c_void_p), ("offset", w.DWORD), ("size", w.DWORD),
                    ("tls", c.c_void_p), ("start", c.c_void_p), ("name", c.c_void_p),
                    ("unicode", w.WORD)]
    class LoadInfo(c.Structure):
        _fields_ = [("file", w.HANDLE), ("base", c.c_void_p), ("offset", w.DWORD),
                    ("size", w.DWORD), ("name", c.c_void_p), ("unicode", w.WORD)]
    class EventData(c.Union):
        _fields_ = [("exception", ExceptionInfo), ("create", CreateInfo), ("load", LoadInfo),
                    ("exit_code", w.DWORD), ("padding", c.c_byte * 160)]
    class Event(c.Structure):
        _fields_ = [("code", w.DWORD), ("pid", w.DWORD), ("tid", w.DWORD), ("data", EventData)]
    assert c.sizeof(Event) == 176
    kernel = c.WinDLL("kernel32", use_last_error=True)
    kernel.WaitForDebugEvent.argtypes = [c.POINTER(Event), w.DWORD]
    kernel.WaitForDebugEvent.restype = w.BOOL
    kernel.ContinueDebugEvent.argtypes = [w.DWORD, w.DWORD, w.DWORD]
    kernel.ContinueDebugEvent.restype = w.BOOL
    kernel.GetFinalPathNameByHandleW.argtypes = [w.HANDLE, w.LPWSTR, w.DWORD, w.DWORD]
    kernel.GetFinalPathNameByHandleW.restype = w.DWORD
    kernel.K32GetMappedFileNameW.argtypes = [w.HANDLE, c.c_void_p, w.LPWSTR, w.DWORD]
    kernel.K32GetMappedFileNameW.restype = w.DWORD
    kernel.CloseHandle.argtypes = [w.HANDLE]
    kernel.CloseHandle.restype = w.BOOL
    modules = set()
    with tempfile.TemporaryFile() as log:
        process = subprocess.Popen([str(argument) for argument in command], env=environment, cwd=cwd,
                                   stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                                   creationflags=0x2 | subprocess.CREATE_NO_WINDOW)
        deadline = time.monotonic() + timeout
        try:
            while True:
                event = Event()
                if not kernel.WaitForDebugEvent(c.byref(event), 250):
                    error = c.get_last_error()
                    if error != 121:
                        raise c.WinError(error)
                    if time.monotonic() > deadline:
                        raise TimeoutError("Debugged child timed out")
                    continue
                status = 0x10002  # DBG_CONTINUE
                if event.code == 6:
                    info = event.data.load
                    buffer = c.create_unicode_buffer(32768)
                    count = kernel.GetFinalPathNameByHandleW(info.file, buffer, len(buffer), 0) if info.file else 0
                    if not count:
                        count = kernel.K32GetMappedFileNameW(int(process._handle), info.base, buffer, len(buffer))
                    if not count:
                        raise RuntimeError("A loaded DLL could not be identified")
                    modules.add(buffer.value)
                    if info.file:
                        kernel.CloseHandle(info.file)
                elif event.code == 3 and event.data.create.file:
                    kernel.CloseHandle(event.data.create.file)
                elif event.code == 1 and event.data.exception.record.code not in (0x80000003, 0x80000004):
                    status = 0x80010001  # DBG_EXCEPTION_NOT_HANDLED
                exited = event.code == 5
                exit_code = event.data.exit_code if exited else None
                if not kernel.ContinueDebugEvent(event.pid, event.tid, status):
                    raise c.WinError(c.get_last_error())
                if exited:
                    process.wait(timeout=5)
                    break
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=5)
        log.seek(0)
        output = log.read().decode("utf-8", errors="replace")
    # msvcp_win.dll is a Windows OS component, not the redistributable MSVCP140.dll.
    forbidden = [name for name in modules if any(part in Path(name).name.lower()
                 for part in ["vcruntime", "msvcp140", "msvcp120", "msvcp110", "libgcc", "libstdc++", "libwinpthread", "msys-"])]
    result = {"command": [str(value) for value in command], "exit_code": exit_code,
              "loaded_modules": sorted(modules), "forbidden_modules": forbidden, "output": output}
    if exit_code or forbidden:
        raise ValueError(json.dumps(result))
    return result


if __name__ == "__main__":
    environment = dict(os.environ, PATH="")
    environment.pop("RFC_TOOLS_DIR", None)
    print(json.dumps(trace(sys.argv[1:], environment=environment), indent=2))
