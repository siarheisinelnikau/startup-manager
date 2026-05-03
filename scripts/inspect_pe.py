"""Read-only package inspection; never loads or executes the Windows binary."""
from pathlib import Path
import hashlib
import json
import struct
import sys

path = Path(sys.argv[1]).resolve()
image = path.read_bytes()
if image[:2] != b"MZ":
    raise SystemExit("Missing DOS header")
pe = struct.unpack_from("<I", image, 0x3C)[0]
if image[pe:pe + 4] != b"PE\0\0":
    raise SystemExit("Missing PE header")
machine, section_count = struct.unpack_from("<HH", image, pe + 4)
optional_size = struct.unpack_from("<H", image, pe + 20)[0]
optional = pe + 24
magic = struct.unpack_from("<H", image, optional)[0]
subsystem = struct.unpack_from("<H", image, optional + 68)[0]
if (machine, magic, subsystem) != (0x8664, 0x20B, 2):
    raise SystemExit("Expected a Windows x64 PE32+ GUI executable")
sections = []
for index in range(section_count):
    section = optional + optional_size + index * 40
    virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from("<IIII", image, section + 8)
    sections.append((virtual_address, max(virtual_size, raw_size), raw_pointer))


def offset(rva):
    for address, size, raw in sections:
        if address <= rva < address + size:
            return raw + rva - address
    raise ValueError(f"Unmapped RVA: {rva:x}")


imports_rva = struct.unpack_from("<I", image, optional + 112 + 8)[0]
imports = []
if imports_rva:
    descriptor = offset(imports_rva)
    while any(image[descriptor:descriptor + 20]):
        name_rva = struct.unpack_from("<I", image, descriptor + 12)[0]
        name_offset = offset(name_rva)
        end = image.index(b"\0", name_offset)
        imports.append(image[name_offset:end].decode("ascii"))
        descriptor += 20
if any(name.lower().startswith(("vcruntime", "msvcp")) for name in imports):
    raise SystemExit("Unexpected dynamic MSVC runtime dependency")
if any(name.lower().startswith(("opengl", "d3d", "dxgi", "vulkan")) for name in imports):
    raise SystemExit("Unexpected GPU graphics API dependency in the CPU/GDI build")
print(json.dumps({
    "file": path.name,
    "bytes": len(image),
    "sha256": hashlib.sha256(image).hexdigest(),
    "architecture": "x86_64",
    "subsystem": "Windows GUI",
    "imports": sorted(set(imports), key=str.lower),
}, indent=2))
