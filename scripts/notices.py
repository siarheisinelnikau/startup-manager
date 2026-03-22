"""Refresh dependency attribution using only the locked, local Cargo source cache."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
    "--filter-platform", "x86_64-pc-windows-msvc",
], cwd=ROOT, text=True, encoding="utf-8"))
active = {node["id"] for node in metadata["resolve"]["nodes"]}
parts = [
    "StartupManager third-party notices\n",
    "Generated from Cargo.lock for Windows x64. Includes build/test dependencies.\n"
    "SPDX expressions below describe each upstream package's licensing choices.\n"
    "Bundled font notices and upstream license/notice files follow their attribution.\n",
]
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["id"] not in active or package["name"] == "startup-manager":
        continue
    folder = Path(package["manifest_path"]).parent
    parts.append("\n" + "=" * 72 + "\n")
    parts.append(f'{package["name"]} {package["version"]}\n')
    parts.append(f'License: {package.get("license") or "See upstream license file"}\n')
    parts.append(f'Authors: {", ".join(package["authors"])}\n')
    parts.append(f'Repository: {package.get("repository") or package.get("homepage") or "Not supplied"}\n')
    paths = set()
    for path in folder.rglob("*"):
        if not path.is_file():
            continue
        name = path.name.lower()
        if name.startswith(("license", "copying", "notice")) or "license" in name or name in {"ofl.txt", "ufl.txt", "hack-regular.txt"}:
            if path.stat().st_size <= 512 * 1024:
                paths.add(path)
    # Shared path crates inherit their MIT license from the sibling workspace root.
    if package["name"] == "rust-auto-updator" and package.get("source") is None:
        shared_license = folder.parents[1] / "LICENSE"
        if shared_license.is_file():
            parts.append("\n--- workspace LICENSE ---\n")
            parts.append(shared_license.read_text(encoding="utf-8") + "\n")
    if package.get("license_file"):
        paths.add(folder / package["license_file"])
    for path in sorted(paths):
        parts.append(f'\n--- {path.relative_to(folder).as_posix()} ---\n')
        parts.append(path.read_text(encoding="utf-8", errors="replace") + "\n")
text = "\n".join(line.rstrip() for line in "".join(parts).splitlines()) + "\n"
(ROOT / "THIRD_PARTY_NOTICES.txt").write_text(text, encoding="utf-8", newline="\n")
print("Updated THIRD_PARTY_NOTICES.txt from the local dependency cache.")
