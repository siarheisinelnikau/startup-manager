"""Render embedded Win32 icon group 1 without launching the application.
Requires Windows and Pillow. This is a resource proof, not an OS tray screenshot.
"""
import argparse
import ctypes as c
import json
import struct
from pathlib import Path
from PIL import Image, ImageDraw


def verify(executable, output):
    kernel = c.WinDLL("kernel32", use_last_error=True)
    user = c.WinDLL("user32", use_last_error=True)
    gdi = c.WinDLL("gdi32", use_last_error=True)
    ptr = c.c_void_p
    kernel.LoadLibraryExW.argtypes = [c.c_wchar_p, ptr, c.c_uint]
    kernel.LoadLibraryExW.restype = ptr
    kernel.FindResourceW.argtypes = [ptr, ptr, ptr]
    kernel.FindResourceW.restype = ptr
    kernel.LoadResource.argtypes = [ptr, ptr]
    kernel.LoadResource.restype = ptr
    kernel.LockResource.argtypes = [ptr]
    kernel.LockResource.restype = ptr
    kernel.SizeofResource.argtypes = [ptr, ptr]
    kernel.SizeofResource.restype = c.c_uint
    kernel.FreeLibrary.argtypes = [ptr]
    user.LoadImageW.argtypes = [ptr, ptr, c.c_uint, c.c_int, c.c_int, c.c_uint]
    user.LoadImageW.restype = ptr
    user.DestroyIcon.argtypes = [ptr]
    user.DrawIconEx.argtypes = [ptr, c.c_int, c.c_int, ptr, c.c_int, c.c_int, c.c_uint, ptr, c.c_uint]
    user.DrawIconEx.restype = c.c_int
    gdi.CreateCompatibleDC.argtypes = [ptr]
    gdi.CreateCompatibleDC.restype = ptr
    gdi.CreateDIBSection.argtypes = [ptr, ptr, c.c_uint, c.POINTER(ptr), ptr, c.c_uint]
    gdi.CreateDIBSection.restype = ptr
    gdi.SelectObject.argtypes = [ptr, ptr]
    gdi.SelectObject.restype = ptr
    gdi.DeleteObject.argtypes = [ptr]
    gdi.DeleteDC.argtypes = [ptr]
    module = kernel.LoadLibraryExW(str(executable.resolve()), None, 0x22)
    if not module:
        raise c.WinError(c.get_last_error())
    try:
        resource = kernel.FindResourceW(module, ptr(1), ptr(14))
        if not resource:
            raise RuntimeError("Executable has no embedded icon group 1")
        data = c.string_at(kernel.LockResource(kernel.LoadResource(module, resource)), kernel.SizeofResource(module, resource))
        reserved, kind, count = struct.unpack_from("<HHH", data)
        assert reserved == 0 and kind == 1
        sizes = sorted({data[6 + i * 14] or 256 for i in range(count)})
        expected = [16, 20, 24, 32, 48, 64, 256]
        assert all(size in sizes for size in expected), sizes
        proof = Image.new("RGB", (840, 220), "#888888")
        labels = ImageDraw.Draw(proof)
        results = []
        for index, size in enumerate(expected):
            icon = user.LoadImageW(module, ptr(1), 1, size, size, 0)
            if not icon:
                raise RuntimeError(f"Windows cannot decode icon at {size}px")
            bounds = []
            try:
                for row, background in enumerate((245, 24)):
                    dc = gdi.CreateCompatibleDC(None)
                    info = struct.pack("<IiiHHIIiiII", 40, size, -size, 1, 32, 0, size * size * 4, 0, 0, 0, 0)
                    bits = ptr()
                    bitmap = gdi.CreateDIBSection(dc, c.create_string_buffer(info), 0, c.byref(bits), None, 0)
                    if not bitmap:
                        raise c.WinError(c.get_last_error())
                    previous = gdi.SelectObject(dc, bitmap)
                    try:
                        c.memmove(bits, bytes([background, background, background, 255]) * size * size, size * size * 4)
                        assert user.DrawIconEx(dc, 0, 0, icon, size, size, 0, None, 3)
                        image = Image.frombytes("RGB", (size, size), c.string_at(bits, size * size * 4), "raw", "BGRX")
                        changed = [(x, y) for y in range(size) for x in range(size) if image.getpixel((x, y)) != (background,) * 3]
                        assert changed, "Embedded icon rendered completely blank"
                        bbox = [min(x for x,y in changed), min(y for x,y in changed), max(x for x,y in changed)+1, max(y for x,y in changed)+1]
                        bounds.append(bbox)
                        assert all(image.getpixel(point) == (background,) * 3 for point in [(0,0),(size-1,0),(0,size-1),(size-1,size-1)]), "Icon corners lost their transparent padding"
                        preview = image.resize((64, 64), Image.Resampling.NEAREST)
                        proof.paste(preview, (index * 120 + 25, row * 100 + 22))
                    finally:
                        gdi.SelectObject(dc, previous)
                        gdi.DeleteObject(bitmap)
                        gdi.DeleteDC(dc)
            finally:
                user.DestroyIcon(icon)
            labels.text((index * 120 + 25, 202), f"{size}px", fill="white")
            results.append({"size":size, "light_dark_artwork_bounds":bounds})
        output.parent.mkdir(parents=True, exist_ok=True)
        proof.save(output)
        report = {"artifact":str(executable.resolve()), "proof":"Win32 LoadImageW + DrawIconEx, offscreen; application not launched", "icon_sizes":sizes, "results":results, "os_tray_interaction_verified":False}
        output.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report))
    finally:
        kernel.FreeLibrary(module)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    verify(args.executable, args.output)
