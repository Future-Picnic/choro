#!/usr/bin/env python3
"""Offline checks for the exact Expert packages shipped in Choro. No dependencies."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import re


def verify(root: Path) -> tuple[int, int]:
    catalog = json.loads((root / "catalog.json").read_text())
    assert catalog["version"] > 0
    skills = {s["id"]: s for s in catalog["skills"]}
    assert len(skills) == len(catalog["skills"]), "Duplicate skill IDs"
    assert {p.name for p in (root / "skills").iterdir()} == set(skills)
    for key, skill in skills.items():
        assert re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", key), key
        assert re.fullmatch(r"[a-f0-9]{40}", skill["upstream_revision"]), key
        assert f'/blob/{skill["upstream_revision"]}/' in skill["upstream"], key
        assert skill["edition"] in ("upstream", "adapted"), key
        assert skill["edition"] != "adapted" or skill["changes"].strip(), key
        package = root / "skills" / key
        paths = list(package.rglob("*"))
        assert not any(p.is_symlink() for p in paths), key
        files = {p.relative_to(package).as_posix(): p for p in paths if p.is_file()}
        assert set(files) == set(skill["files"]), f"Unrecorded or missing resources: {key}"
        assert {"SKILL.md", "LICENSE.txt", "NOTICE.md"} <= set(files), key
        assert len(files) <= 512 and sum(p.stat().st_size for p in files.values()) <= 4 * 1024 * 1024, key
        for name, path in files.items():
            assert all(p not in (".", "..") for p in PurePosixPath(name).parts), name
            data = path.read_bytes()
            assert len(data) <= 512_000 and b"\0" not in data, name
            data.decode("utf-8")
            assert hashlib.sha256(data).hexdigest() == skill["files"][name], f"Package changed without a manifest update: {key}/{name}"
        entry = files["SKILL.md"].read_text()
        assert re.match(r"\A---\n.*?\nname:|\A---\nname:", entry, re.S), key
        assert re.search(r"^description:\s*\S", entry, re.M), key
        if skill["edition"] == "upstream":
            assert skill["files"]["SKILL.md"] == skill["upstream_sha256"], f"An original edition was modified: {key}"
        for source in skill.get("additional_sources", []):
            assert skill["files"][source["path"]] == source["sha256"], key
    profiles = catalog["experts"]
    assert len({e["id"] for e in profiles}) == len(profiles)
    assert len({" ".join(e["name"].split()).lower() for e in profiles}) == len(profiles)
    used = set()
    for expert in profiles:
        assert 0 < len(expert["skills"]) <= 32 and len(set(expert["skills"])) == len(expert["skills"]), expert["name"]
        assert set(expert["skills"]) <= set(skills), expert["name"]
        used.update(expert["skills"])
        assert expert["provider"] in ("claude", "codex"), expert["name"]
        assert expert["instructions"].strip() and expert["expected_outcome"].strip(), expert["name"]
        assert sum((root / "skills" / s / "SKILL.md").stat().st_size for s in expert["skills"]) <= 128_000, expert["name"]
    assert used == set(skills), "Unassigned bundled skill"
    return len(profiles), len(skills)


if __name__ == "__main__":
    count, skills = verify(Path(__file__).resolve().parents[1] / "crates/ide-core/assets/experts")
    print(f"Expert catalog verified: {count} Experts, {skills} pinned skill packages.")
