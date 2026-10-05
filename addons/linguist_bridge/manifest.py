# SPDX-License-Identifier: GPL-3.0-or-later
"""One canonical model manifest projection (RI-05).

The projection is `{name, fields, templates: [{name, ordinal, front, back}],
css}` with fields in Anki field order and templates sorted by ordinal. Its
digest is the lowercase SHA-256 of the RFC 8785 bytes. The Rust read capture,
apply orchestration and this companion compute the same digest, so one value
serves source capture, `source.model_manifest`, create-note checks and the
managed-model install. Managed manifest versions are not part of the
projection: a collection model has no version.
"""
import hashlib
import json


def jcs(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode("utf-8")


def projection(name, fields, templates, css):
    ordered = sorted(templates, key=lambda template: template["ordinal"])
    return {
        "name": name,
        "fields": list(fields),
        "templates": [{"name": t["name"], "ordinal": t["ordinal"],
                       "front": t["front"], "back": t["back"]} for t in ordered],
        "css": css,
    }


def projection_of_model(model):
    """Projection of an Anki notetype dictionary."""
    fields = [field["name"] for field in sorted(model["flds"], key=lambda f: f["ord"])]
    templates = [{"name": t["name"], "ordinal": t["ord"], "front": t["qfmt"],
                  "back": t["afmt"]} for t in model["tmpls"]]
    return projection(model["name"], fields, templates, model["css"])


def digest(value):
    return hashlib.sha256(jcs(value)).hexdigest()


def model_digest(model):
    return digest(projection_of_model(model))


def managed_digest(manifest):
    """Digest of a managed manifest body (`name, version, fields, templates, css`)."""
    return digest(projection(manifest["name"], manifest["fields"], manifest["templates"],
                             manifest["css"]))
