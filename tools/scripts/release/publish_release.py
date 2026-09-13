"""Upload complete artifacts to a draft, then publish without replacing public assets."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

from tools.scripts.release.package_agentxctl import TARGETS, checksum


def gh(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(("gh", *args), check=False, capture_output=True, text=True, encoding="utf-8", timeout=600)
    if check and result.returncode:
        raise RuntimeError(result.stderr.strip() or "GitHub CLI request failed")
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--directory", type=Path, default=Path(".local/dist"))
    parser.add_argument("--notes", type=Path, required=True)
    args = parser.parse_args()
    version = args.tag.removeprefix("agentxctl-v")
    files = []
    for target, (_, standalone, extension) in TARGETS.items():
        for name in (standalone, f"agentxctl-{version}-{target}.{extension}"):
            artifact = args.directory / name
            digest = artifact.with_name(name + ".sha256")
            if digest.read_text(encoding="utf-8").strip() != f"{checksum(artifact)}  {name}":
                raise ValueError(f"artifact checksum mismatch: {name}")
            files.extend((str(artifact), str(digest)))
    receipt = args.directory / "release-images.json"
    images = json.loads(receipt.read_text(encoding="utf-8"))
    if images["version"] != version or len(images["images"]) != 11:
        raise ValueError("a complete image verification receipt is required")
    files.append(str(receipt))
    # A single tag-scoped workflow owns the draft. Never overwrite a public release.
    found = gh("release", "view", args.tag, "--json", "isDraft", check=False)
    if found.returncode == 0:
        if not json.loads(found.stdout)["isDraft"]:
            raise ValueError("release is already public; publish changes under a new version")
    else:
        if "release not found" not in found.stderr.lower():
            raise RuntimeError(found.stderr.strip())
        gh(
            "release",
            "create",
            args.tag,
            "--verify-tag",
            "--draft",
            "--title",
            args.tag,
            "--notes-file",
            str(args.notes),
            *(("--prerelease",) if "-" in version else ()),
        )
    gh("release", "upload", args.tag, *files, "--clobber")
    gh("release", "edit", args.tag, "--draft=false", "--notes-file", str(args.notes))
    print(gh("release", "view", args.tag, "--json", "url,assets,isDraft,isPrerelease").stdout)


if __name__ == "__main__":
    main()
