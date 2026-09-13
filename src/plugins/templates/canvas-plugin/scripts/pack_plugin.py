from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

root = Path(__file__).resolve().parents[1]
output = root / "dist" / "acme-json-mapper.agentx-plugin"
output.parent.mkdir(parents=True, exist_ok=True)
entries = {
    "manifest.json": root / "manifest.json",
    "nodes/json_mapper.json": root / "nodes" / "json_mapper.json",
    "runtime/entry.js": root / "dist" / "runtime" / "entry.js",
    "ui/entry.js": root / "dist" / "ui" / "entry.js",
    "ui/styles.css": root / "src" / "ui" / "styles.css",
    "ui/logo.svg": root / "src" / "ui" / "logo.svg",
    "docs/README.md": root / "README.md",
    "docs/AGENTS.md": root / "AGENTS.md",
}
missing = [str(path) for path in entries.values() if not path.is_file()]
if missing:
    raise SystemExit("Build output is missing: " + ", ".join(missing))
with ZipFile(output, "w", ZIP_DEFLATED) as archive:
    for name, path in sorted(entries.items()):
        info = ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
        info.compress_type = ZIP_DEFLATED
        info.external_attr = 0o100644 << 16
        archive.writestr(info, path.read_bytes())
print(output)
