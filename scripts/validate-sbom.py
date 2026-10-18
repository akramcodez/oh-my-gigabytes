#!/usr/bin/env python3
"""Validate a CycloneDX 1.5 document against the official JSON schema (release plan R16).

usage: validate-sbom.py SBOM.json SCHEMA_DIR
SCHEMA_DIR holds bom-1.5.schema.json, spdx.schema.json and jsf-0.82.schema.json from the
CycloneDX specification repository at a pinned release.
"""
import json
import sys
from pathlib import Path

from jsonschema import Draft7Validator
from referencing import Registry, Resource
from referencing.jsonschema import DRAFT7

SCHEMAS = ("bom-1.5.schema.json", "spdx.schema.json", "jsf-0.82.schema.json")


def validate(bom: Path, folder: Path) -> list[str]:
    documents = {name: json.loads((folder / name).read_text()) for name in SCHEMAS}
    registry = Registry().with_resources(
        (doc["$id"], Resource.from_contents(doc, default_specification=DRAFT7))
        for doc in documents.values()
    )
    validator = Draft7Validator(documents["bom-1.5.schema.json"], registry=registry)
    found = validator.iter_errors(json.loads(bom.read_text()))
    return [f"{'/'.join(map(str, e.absolute_path)) or '(root)'}: {e.message}" for e in found]


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    errors = validate(Path(argv[1]), Path(argv[2]))
    for line in errors[:20]:
        print(f"invalid: {line}", file=sys.stderr)
    if errors:
        print(f"{len(errors)} schema violation(s)", file=sys.stderr)
        return 1
    print("CycloneDX 1.5 document is valid")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
