#!/usr/bin/env python3
"""Collect legal texts from checksum-verified Cargo archives; never run a build.

Run after `cargo fetch --locked` to populate the cache. Upstream supplements in
licenses/third-party/upstream are reviewed inputs, with URLs/hashes in SOURCES.json.
This conservative lock inventory is not a bill of materials for a release binary.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile

LEGAL = re.compile(r"(?:^|[. _-])(licen[sc]es?|copying|copyrights?|notices?|unlicense|authors|contributors)([. _-]|$)", re.I)
ATTRIBUTION = re.compile(r"copyright|SPDX-License-Identifier|permission is hereby granted|redistribution and use|licensed under|licen[sc]e:", re.I)
TEXT_SUFFIXES = {".rs", ".c", ".h", ".cpp", ".cc", ".s", ".md", ".txt", ".py", ".sh", ".sublime-syntax", ".toml"}
PRIMARY_TEXT_EXCLUSIONS = {
    "cesu8@1.1.0": {
        "COPYRIGHT-RUST.txt": "References primary Rust MIT/Apache terms; full nested compiler-rt MIT text applies to a different component, not proof of the crate's primary license text."
    }
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def primary_terms_present(expression, text):
    """Conservative content check; attribution filenames alone prove nothing.

    This identifies a substantive text for a declared primary license, not legal
    clearance or satisfaction of every AND component's attribution obligations.
    """
    text = re.sub(r"\s+", " ", text.lower())
    checks = {
        "MIT": "permission is hereby granted" in text and "software is provided" in text,
        "Apache-2.0": "terms and conditions for use" in text and "grant of patent license" in text,
        "MPL-2.0": "mozilla public license" in text and "covered software" in text and "each contributor hereby grants" in text,
        "BSD": "redistribution and use" in text and "binary forms" in text and "disclaim" in text,
        "ISC": "permission to use, copy, modify" in text and "software is provided" in text,
        "Zlib": "origin of this software" in text and "altered source versions" in text,
        "BSL-1.0": "permission is hereby granted" in text and "machine-executable object code" in text,
        "Unicode-3.0": "unicode" in text and "permission is hereby granted" in text and 'provided "as is"' in text,
        "CDLA-Permissive-2.0": "community data license agreement" in text and "permissive" in text,
        "0BSD": "permission to use, copy, modify" in text and "software is provided" in text,
        "CC0-1.0": "creative commons" in text and "waiver" in text,
        "Unlicense": "free and unencumbered" in text and "public domain" in text,
    }
    return any(label in (expression or "") and present for label, present in checks.items())


def lock_packages(text):
    # Only Cargo's scalar package identity/checksum fields are needed, not TOML
    # dependency expressions. Cargo itself validates/resolves the whole lockfile.
    result = {}
    for block in text.split("[[package]]")[1:]:
        fields = dict(re.findall(r'^(name|version|source|checksum) = ("[^"\n]*")$', block, re.M))
        fields = {k: json.loads(v) for k, v in fields.items()}
        if fields.get("source", "").startswith("registry+"):
            result[(fields["name"], fields["version"])] = fields
    return result


def embedded_notice(name, data):
    """Retain leading source comment notices and README license sections."""
    if PurePosixPath(name).suffix.lower() not in TEXT_SUFFIXES:
        return ""
    text = data[:65536].decode("utf-8", errors="replace")
    if not ATTRIBUTION.search(text):
        return ""
    if PurePosixPath(name).suffix.lower() in {".md", ".txt"}:
        sections = re.findall(r"(?ims)^#{1,6} (?:licen[sc]e|copyright|legal|acknowledg)[^\n]*\n.*?(?=^#{1,6} |\Z)", text)
        return "\n".join(sections).strip()
    lines = text.splitlines()
    header = []
    in_block = False
    for line in lines[:250]:
        s = line.strip()
        if in_block:
            header.append(line)
            if "*/" in s:
                in_block = False
        elif s.startswith("/*"):
            header.append(line)
            in_block = "*/" not in s
        elif s.startswith(("//", "#!", "<!--")) or not s:
            header.append(line)
        elif s.startswith("# ") and PurePosixPath(name).suffix.lower() in {".py", ".sh", ".toml", ".sublime-syntax"}:
            header.append(line)
        elif s in {"%YAML 1.2", "---"}:
            header.append(line)
        else:
            break
    header = "\n".join(header).strip()
    return header if ATTRIBUTION.search(header) else ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if generated files differ")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    cargo = shutil.which("cargo") or ("/opt/homebrew/bin/cargo" if Path("/opt/homebrew/bin/cargo").exists() else "cargo")
    metadata = json.loads(subprocess.check_output([cargo, "metadata", "--offline", "--locked", "--format-version", "1"], cwd=root))
    lock_data = (root / "Cargo.lock").read_bytes()
    locked = lock_packages(lock_data.decode())
    upstream = root / "licenses/third-party/upstream"
    supplements = json.loads((upstream / "SOURCES.json").read_text())
    for item in supplements["sources"]:
        data = (upstream / item["file"]).read_bytes()
        if digest(data) != item["sha256"]:
            raise SystemExit("Supplement checksum mismatch: " + item["file"])
    outputs = {}
    packages = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if not package["source"]:
            continue
        key = (package["name"], package["version"])
        entry = locked[key]
        src = Path(package["manifest_path"]).parent
        archive = src.parents[2] / "cache" / src.parent.name / ("%s-%s.crate" % key)
        archive_data = archive.read_bytes()
        if digest(archive_data) != entry["checksum"]:
            raise SystemExit("Cargo.lock checksum mismatch: " + str(archive))
        destination = "licenses/third-party/crates/%s-%s/" % key
        files = []
        embedded = []
        primary_texts = []
        with tarfile.open(fileobj=io.BytesIO(archive_data), mode="r:gz") as tar:
            for member in tar:
                if not member.isfile():
                    continue
                path = PurePosixPath(member.name)
                relative = PurePosixPath(*path.parts[1:])
                if not relative.parts or ".." in relative.parts:
                    raise SystemExit("Unsafe archive path")
                name = str(relative)
                data = None
                legal_name = LEGAL.search(relative.name) and relative.suffix.lower() not in {".rs", ".c", ".h", ".cpp", ".cc", ".py", ".sh"}
                if legal_name or (package["license_file"] and name == package["license_file"]):
                    data = tar.extractfile(member).read()
                    output = destination + name
                    outputs[output] = data
                    files.append({"path": output, "sha256": digest(data), "origin": "crate"})
                    if name not in PRIMARY_TEXT_EXCLUSIONS.get("%s@%s" % key, {}):
                        primary_texts.append(data.decode("utf-8", errors="replace"))
                elif member.size <= 2000000 and relative.suffix.lower() in TEXT_SUFFIXES:
                    data = tar.extractfile(member).read()
                    notice = embedded_notice(name, data)
                    if notice:
                        embedded.append("===== %s =====\n%s\n" % (name, notice))
        if embedded:
            output = destination + "EMBEDDED_NOTICES.txt"
            data = ("Extracted attribution/license comments; original crate paths follow.\n\n" + "\n".join(embedded)).encode()
            outputs[output] = data
            files.append({"path": output, "sha256": digest(data), "origin": "embedded-comments"})
        matching = [x for x in supplements["sources"] if x.get("package") == "%s@%s" % key]
        for item in matching:
            files.append({"path": "licenses/third-party/upstream/" + item["file"], "sha256": item["sha256"], "origin": "upstream", "url": item["url"]})
            primary_texts.append((upstream / item["file"]).read_text(errors="replace"))
        has_primary_text = primary_terms_present(package["license"], "\n".join(primary_texts))
        if not has_primary_text:
            for item in supplements["sources"]:
                if item.get("license_template") and item["license_template"] in (package["license"] or ""):
                    files.append({"path": "licenses/third-party/upstream/" + item["file"], "sha256": item["sha256"], "origin": "canonical-template", "url": item["url"]})
        packages.append({"name": key[0], "version": key[1], "license_expression": package["license"], "repository": package["repository"], "registry_source": entry["source"], "crate_sha256": entry["checksum"], "source_url": "https://static.crates.io/crates/%s/%s-%s.crate" % (key[0], key[0], key[1]), "texts": files, "primary_original_license_text_found": has_primary_text, "primary_text_review_exclusions": PRIMARY_TEXT_EXCLUSIONS.get("%s@%s" % key, {}), "attribution_text_count": sum(f["origin"] != "canonical-template" for f in files)})
    if set(locked) != {(p["name"], p["version"]) for p in packages}:
        raise SystemExit("Metadata/lock registry package set differs; review before regenerating")
    gaps = ["%s@%s" % (p["name"], p["version"]) for p in packages if not p["primary_original_license_text_found"]]
    inventory = {"schema_version": 1, "scope": "Conservative complete Cargo.lock registry inventory; includes target-specific, build, dev and optional resolution. Not a release binary bill of materials.", "cargo_lock_sha256": digest(lock_data), "registry_package_count": len(packages), "legal_text_gaps": gaps, "limitations": supplements["limitations"], "supplemental_sources": supplements["sources"], "packages": packages}
    outputs["licenses/dependency-inventory.json"] = (json.dumps(inventory, indent=2, ensure_ascii=False) + "\n").encode()
    lines = ["# Third-party notices", "", "This is a conservative inventory of all **%d registry packages in Cargo.lock**. It includes target-specific, build, development and optional resolution; it is **not a bill of materials proving what a release binary contains**. Each crate archive was verified against its Cargo.lock SHA-256 before extracting the original legal files. The CA project license does not replace these terms.", "", "Regenerate offline with `python3 tools/generate_dependency_notices.py`; verify with `--check`. The generator runs Cargo metadata, never compilation or build scripts. Populate the cache first with `cargo fetch --locked` if needed. Upstream supplements are checked-in inputs with exact URLs and SHA-256 in [SOURCES.json](licenses/third-party/upstream/SOURCES.json).", "", "## Redistribution", "", "Ship this file, dependency-inventory.json and the referenced legal texts with binary distributions. Select permissible alternatives where a dependency offers OR terms; AND terms remain cumulative. Retain original copyright and attribution notices. Apache components require the license, applicable upstream NOTICE entries and change notices for modified upstream files. Native system libraries and actual linkage must be checked per release target.", "", "`nucleo 0.5.0` and `nucleo-matcher 0.3.1` are MPL-2.0. Recipients of binaries must be informed how to obtain their covered source, including modifications. Exact unmodified source links and Cargo.lock checksums appear below and in the inventory. These links describe the current upstream dependencies; any future modifications require corresponding source. Separate CA files retain their own license. The nucleo boxcar MIT notice is retained in its EMBEDDED_NOTICES.txt.", "", "## Embedded and native components", "", "- `ring`: retain Apache, ISC, BoringSSL and once_cell notices. `aws-lc-sys`: retain its complete compound-license attribution text.", "- `onig_sys`: the Rust wrapper metadata is MIT; bundled Oniguruma has separate BSD-style COPYING. `zstd-sys`: retain the native zstd BSD license as well as wrapper terms.", "- `libsqlite3-sys`: the enabled bundled SQLite is public domain; the wrapper is MIT. The inventory also conservatively retains SQLCipher's license from the archive, without claiming SQLCipher is linked.", "- `encoding_rs`: retain WHATWG BSD attribution in addition to the selected MIT/Apache option. ICU/Unicode data and webpki root certificates retain Unicode-3.0 and CDLA-Permissive-2.0 terms respectively.", "- `syntect`: CA loads its built-in syntax/theme packs. Supplemental texts retain the pinned upstream theme and syntax licenses; see the provenance limitation below.", "", "## Known limits", ""]
    lines[2] = lines[2] % len(packages)
    lines += ["- " + x for x in supplements["limitations"]]
    if gaps:
        lines += ["- No original substantive primary license text was located by the conservative content check for: " + ", ".join("`" + x + "`" for x in gaps) + ". Available original attribution is retained; canonical license templates supplement declared terms without inventing copyright holders. These templates do not resolve missing original notices or establish ownership."]
    lines += ["", "## MPL source availability", ""]
    for p in packages:
        if p["license_expression"] == "MPL-2.0":
            lines += ["- [%s %s exact source](%s), SHA-256 `%s`." % (p["name"], p["version"], p["source_url"], p["crate_sha256"])]
    lines += ["", "## Package inventory", "", "| Package | Declared license expression | Preserved legal texts |", "| --- | --- | --- |"]
    for p in packages:
        links = ", ".join("[%s](%s)" % (Path(f["path"]).name.replace("|", "\\|"), f["path"].replace(" ", "%20")) for f in p["texts"])
        lines.append("| `%s %s` | `%s` | %s |" % (p["name"], p["version"], p["license_expression"] or "UNDECLARED", links or "No text located; see known limits"))
    lines += ["", "## Supplemental texts", ""]
    for item in supplements["sources"]:
        if not item.get("package"):
            lines.append("- [%s](licenses/third-party/upstream/%s) — [exact upstream source](%s)." % (item["component"], item["file"], item["url"]))
    outputs["THIRD_PARTY_NOTICES.md"] = ("\n".join(lines) + "\n").encode()
    existing = set(str(p.relative_to(root)) for p in (root / "licenses/third-party/crates").rglob("*") if p.is_file())
    stale = existing - set(outputs)
    changed = [p for p, data in outputs.items() if not (root / p).exists() or (root / p).read_bytes() != data]
    if args.check:
        if changed or stale:
            raise SystemExit("Generated notice files differ: %d changed, %d stale" % (len(changed), len(stale)))
    else:
        for p in stale:
            (root / p).unlink()
        for p, data in outputs.items():
            target = root / p
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
    print("Verified %d locked registry archives; %d legal-text files; %d standalone-text gaps" % (len(packages), len(outputs) - 2, len(gaps)))


if __name__ == "__main__":
    main()
