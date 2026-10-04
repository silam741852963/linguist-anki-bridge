#!/usr/bin/env python3
"""Disposable Anki lab for the WP-15 release scenarios.

Run with the Python that ships Anki (on this machine `/usr/bin/python3.14`):

    disposable-anki-lab.py --dir EMPTY_DIRECTORY

It creates a new collection in EMPTY_DIRECTORY, never opens a user profile,
prints one JSON line `{"endpoint": "http://127.0.0.1:PORT", ...}` and serves
HTTP on loopback until stdin closes or `labTest.shutdown` is called.

Two surfaces share the one disposable collection:

- an AnkiConnect v6 read subset (`version`, `getActiveProfile`, `apiReflect`,
  `deckNamesAndIds`, `getDeckConfig`, `modelNamesAndIds`, `modelFieldNames`,
  `modelTemplates`, `modelStyling`, `findNotes`, `findCards`, `notesInfo`,
  `cardsInfo`, `retrieveMediaFile`) so the unchanged CLI can read and capture;
- `labTest.*` actions implementing the Rust `ApplyPort` boundary over the
  companion's real effect functions (`addons/linguist_bridge/effects.py`),
  plus setup helpers (decks, models, notes, study, edits, package export).

This is test infrastructure, not the native companion: there is no add-on
registration, main-thread critical section, persistent ledger, owner fencing
or crash recovery. Model manifest digests are registered by the harness: the
observed digest of a note type is the one the CLI read capture recorded, and
`create_note` compares the fixed managed manifest digest separately, because
no native manifest computation exists yet.
It does not advertise `labCapabilities`, so the CLI keeps collection writes
unavailable against it.
"""

import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import sys
import threading
import uuid

from anki.buildinfo import buildhash, version
from anki.collection import Collection

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "addons"))
from linguist_bridge import effects  # noqa: E402

PROFILE = "Disposable"
READ_ACTIONS = [
    "version", "getActiveProfile", "apiReflect", "deckNamesAndIds", "getDeckConfig",
    "modelNamesAndIds", "modelFieldNames", "modelTemplates", "modelStyling", "findNotes",
    "findCards", "notesInfo", "cardsInfo", "retrieveMediaFile",
]
VARIANTS = ["store_media", "create_note", "update_note", "restore_note",
            "delete_unstudied_created_note"]


class Lab:
    def __init__(self, directory, endpoint_holder):
        self.path = str(Path(directory) / "collection.anki2")
        self.col = Collection(self.path)
        self.endpoint = endpoint_holder
        self.digests = {}
        self.managed = {}
        self.ledger = {}
        self.fence = 0
        self.owner = None
        self.lineage = str(uuid.uuid4())
        self.bridge = str(uuid.uuid4())
        self.session = str(uuid.uuid4())
        self.dispatched = []

    # ---------------------------------------------------------------- helpers
    def manifest_digest(self, model):
        """Observed digest, as the read capture records it (harness-registered)."""
        return self.digests.get(str(model["id"]), f"unregistered:{model['id']}")

    def managed_digest(self, model):
        """Fixed managed manifest digest that `create_note` bodies carry."""
        return self.managed.get(str(model["id"]), self.manifest_digest(model))

    def observe(self, note_id):
        try:
            return effects.observe_note(self.col, int(note_id), self.manifest_digest)
        except Exception:  # NotFoundError for a missing note
            return None

    def model_by_name(self, name):
        model = self.col.models.by_name(name)
        if model is None:
            raise LookupError("model not found")
        return model

    # ---------------------------------------------------------------- AnkiConnect reads
    def version(self, _):
        return 6

    def getActiveProfile(self, _):
        return PROFILE

    def apiReflect(self, params):
        return {"scopes": ["actions"], "actions": READ_ACTIONS}

    def deckNamesAndIds(self, _):
        return {deck.name: deck.id for deck in self.col.decks.all_names_and_ids()}

    def getDeckConfig(self, params):
        deck = self.col.decks.by_name(params["deck"])
        if deck is None:
            return False
        return {"dyn": bool(deck.get("dyn")), "id": deck["id"], "name": deck["name"]}

    def modelNamesAndIds(self, _):
        return {model.name: model.id for model in self.col.models.all_names_and_ids()}

    def modelFieldNames(self, params):
        return [field["name"] for field in self.model_by_name(params["modelName"])["flds"]]

    def modelTemplates(self, params):
        model = self.model_by_name(params["modelName"])
        return {t["name"]: {"Front": t["qfmt"], "Back": t["afmt"]} for t in model["tmpls"]}

    def modelStyling(self, params):
        return {"css": self.model_by_name(params["modelName"])["css"]}

    def findNotes(self, params):
        return list(self.col.find_notes(params["query"]))

    def findCards(self, params):
        return list(self.col.find_cards(params["query"]))

    def notesInfo(self, params):
        out = []
        for note_id in params["notes"]:
            try:
                note = self.col.get_note(int(note_id))
            except Exception:
                out.append({})
                continue
            model = self.col.models.get(note.mid)
            out.append({
                "noteId": note.id, "modelName": model["name"], "tags": list(note.tags),
                "fields": {f["name"]: {"value": note[f["name"]], "order": f["ord"]}
                           for f in model["flds"]},
                "cards": list(self.col.card_ids_of_note(note.id)), "mod": note.mod,
            })
        return out

    def cardsInfo(self, params):
        out = []
        for card_id in params["cards"]:
            card = self.col.get_card(int(card_id))
            deck = self.col.decks.get(card.did)
            out.append({
                "cardId": card.id, "note": card.nid, "deckName": deck["name"], "ord": card.ord,
                "type": card.type, "queue": card.queue, "due": card.due, "interval": card.ivl,
                "factor": card.factor, "reps": card.reps, "lapses": card.lapses,
                "left": card.left, "mod": card.mod,
            })
        return out

    def retrieveMediaFile(self, params):
        name = params["filename"]
        path = Path(self.col.media.dir()) / name
        if "/" in name or not path.is_file():
            return False
        return base64.b64encode(path.read_bytes()).decode()

    # ---------------------------------------------------------------- ApplyPort
    def binding(self):
        fingerprint = lambda text: hashlib.sha256(text.encode()).hexdigest()
        return {
            "endpoint": self.endpoint[0],
            "profile_fingerprint": fingerprint("profile:" + PROFILE),
            "path_fingerprint": fingerprint("path:" + self.path),
            "bridge_id": self.bridge, "lineage_id": self.lineage,
            "session_epoch": self.session,
            "capability_digest": "lab-jcs-v1:lab-native-capabilities-v1:"
            + fingerprint("disposable-lab:" + ",".join(VARIANTS)),
        }

    def lab_binding(self, _):
        return self.binding()

    def lab_variants(self, _):
        return VARIANTS

    def lab_begin(self, params):
        if params["binding"] != self.binding():
            raise RuntimeError("BRIDGE_BINDING_MISMATCH")
        self.fence += 1
        self.owner = {"token": str(uuid.uuid4()), "fence": self.fence}
        return self.owner

    def lab_end(self, params):
        if self.owner == params:
            self.owner = None
        return True

    def lab_note(self, params):
        return self.observe(params["note_id"])

    def lab_notesTagged(self, params):
        return [self.observe(n) for n in self.col.find_notes(f'"tag:{params["tag"]}"')]

    def lab_modelsNamed(self, params):
        out = []
        for entry in self.col.models.all_names_and_ids():
            if entry.name != params["name"]:
                continue
            model = self.col.models.get(entry.id)
            out.append({
                "id": model["id"], "name": model["name"],
                "fields": [f["name"] for f in model["flds"]],
                "templates": [{"name": t["name"], "ordinal": t["ord"], "front": t["qfmt"],
                               "back": t["afmt"]} for t in model["tmpls"]],
                "css": model["css"],
            })
        return out

    def lab_deck(self, params):
        deck = self.col.decks.by_name(params["name"])
        if deck is None:
            return None
        return {"id": deck["id"], "name": deck["name"], "filtered": bool(deck.get("dyn"))}

    def lab_media(self, params):
        digest = effects.media_sha256(self.col, params["filename"])
        if digest is None:
            return None
        size = (Path(self.col.media.dir()) / params["filename"]).stat().st_size
        return {"filename": params["filename"], "sha256": digest, "size_bytes": size}

    def lab_mediaBytes(self, params):
        data = self.retrieveMediaFile({"filename": params["filename"]})
        return data or None

    def lab_mutate(self, params):
        request = params["request"]
        operation = request["operation_id"]
        if operation in self.ledger:
            return self.ledger[operation]
        if request["binding"] != self.binding() or request["owner"] != self.owner:
            status = {"state": "failed_before_write", "reason": "owner_or_binding_mismatch"}
            self.ledger[operation] = status
            return status
        effect = request["effect"]
        variant = effect["variant"]
        self.dispatched.append(variant)
        try:
            if variant == "store_media":
                data = base64.b64decode(params["media_base64"])
                effects.store_media(self.col, effect["filename"], data, effect["sha256"])
            elif variant == "create_note":
                body = effect["envelope"]["body"]
                effects.create_note(self.col, body, self.managed_digest)
            elif variant == "update_note":
                effects.update_note(self.col, effect, self.manifest_digest)
            elif variant == "restore_note":
                effects.restore_note(self.col, effect, self.manifest_digest)
            elif variant == "delete_unstudied_created_note":
                effects.delete_unstudied_created_note(self.col, effect, self.manifest_digest)
            else:
                raise effects.EffectError("BRIDGE_VARIANT_UNSUPPORTED")
            status = {"state": "verified"}
        except effects.EffectError as error:
            status = {"state": "failed_before_write", "reason": str(error)}
        self.ledger[operation] = status
        return status

    def lab_status(self, params):
        return self.ledger.get(params["operation_id"], {"state": "absent"})

    # ---------------------------------------------------------------- setup helpers
    def lab_registerDigest(self, params):
        table = self.managed if params.get("kind") == "managed" else self.digests
        table[str(params["model_id"])] = params["digest"]
        return True

    def lab_createDeck(self, params):
        return self.col.decks.id(params["name"])

    def lab_installModel(self, params):
        manifest = params["manifest"]
        if self.col.models.by_name(manifest["name"]) is not None:
            raise RuntimeError("model exists")
        model = self.col.models.new(manifest["name"])
        model["css"] = manifest["css"]
        for name in manifest["fields"]:
            self.col.models.add_field(model, self.col.models.new_field(name))
        for template in sorted(manifest["templates"], key=lambda t: t["ordinal"]):
            item = self.col.models.new_template(template["name"])
            item["qfmt"] = template["front"]
            item["afmt"] = template["back"]
            self.col.models.add_template(model, item)
        self.col.models.add_dict(model)
        return self.col.models.by_name(manifest["name"])["id"]

    def lab_modelId(self, params):
        return self.model_by_name(params["name"])["id"]

    def lab_addNote(self, params):
        note = self.col.new_note(self.model_by_name(params["model"]))
        for name, value in params["fields"].items():
            note[name] = value
        note.tags = params.get("tags", [])
        self.col.add_note(note, self.col.decks.id(params["deck"]))
        return note.id

    def lab_writeMedia(self, params):
        return self.col.media.write_data(params["filename"], base64.b64decode(params["data"]))

    def lab_study(self, params):
        reviewed = []
        for card_id in self.col.card_ids_of_note(int(params["note_id"])):
            card = self.col.get_card(card_id)
            if params.get("ordinal") is not None and card.ord != params["ordinal"]:
                continue
            card.start_timer()
            self.col.sched.answerCard(card, params.get("ease", 3))
            reviewed.append(card_id)
        return reviewed

    def lab_setConfig(self, params):
        """Collection config, e.g. `fsrs` to enable FSRS memory states."""
        self.col.set_config(params["key"], params["value"])
        return True

    def lab_editField(self, params):
        note = self.col.get_note(int(params["note_id"]))
        note[params["field"]] = params["value"]
        self.col.update_note(note)
        return True

    def lab_scope(self, params):
        """Checkpoint scope manifest of the given notes (all when empty)."""
        notes = [int(n) for n in params.get("note_ids") or self.col.find_notes("")]
        cards, models = [], set()
        for note_id in sorted(notes):
            note = self.col.get_note(note_id)
            models.add(note.mid)
            for card_id in sorted(self.col.card_ids_of_note(note_id)):
                card = self.col.get_card(card_id)
                reviews = self.col.db.scalar("select count() from revlog where cid = ?", card_id)
                cards.append({"card_id": card_id, "note_id": note_id, "reps": card.reps,
                              "review_count": reviews})
        media = []
        directory = Path(self.col.media.dir())
        for path in sorted(directory.iterdir()):
            if path.is_file():
                media.append({"name": path.name, "sha1": hashlib.sha1(path.read_bytes()).hexdigest()})
        return {"schema_version": 1,
                "requirement": {"scheduling": True, "media": True, "schema": True},
                "note_ids": sorted(notes), "cards": cards, "model_ids": sorted(models),
                "media": media}

    def lab_exportPackage(self, params):
        # Collection-package export closes the collection; reopen it after.
        try:
            self.col.export_collection_package(params["path"], include_media=True, legacy=False)
        finally:
            try:
                self.col.close()
            except Exception:
                pass
            self.col = Collection(self.path)
        data = Path(params["path"]).read_bytes()
        return {"size_bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

    def lab_reviewLog(self, params):
        rows = self.col.db.all("select id, cid, ease, ivl, type from revlog where cid = ? order by id",
                               int(params["card_id"]))
        return rows

    def lab_dispatched(self, _):
        return self.dispatched

    def dispatch(self, action, params):
        if action.startswith("labTest."):
            handler = getattr(self, "lab_" + action.split(".", 1)[1], None)
        elif action in READ_ACTIONS:
            handler = getattr(self, action)
        else:
            handler = None
        if handler is None:
            raise RuntimeError(f"unsupported action {action}")
        return handler(params or {})


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--dir", required=True, type=Path)
    args = parser.parse_args()
    args.dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    if any(args.dir.iterdir()):
        raise SystemExit("LAB_DIRECTORY_NOT_EMPTY")
    endpoint = [""]
    lab = Lab(args.dir, endpoint)
    lock = threading.Lock()
    stop = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            request = json.loads(self.rfile.read(length))
            action = request.get("action", "")
            try:
                with lock:
                    if action == "labTest.shutdown":
                        stop.set()
                        result, error = True, None
                    else:
                        result, error = lab.dispatch(action, request.get("params")), None
            except Exception as failure:  # reported as an AnkiConnect error envelope
                result, error = None, f"{type(failure).__name__}: {failure}"
            body = json.dumps({"result": result, "error": error}, ensure_ascii=False).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)

    server = HTTPServer(("127.0.0.1", 0), Handler)
    endpoint[0] = f"http://127.0.0.1:{server.server_address[1]}"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(json.dumps({"endpoint": endpoint[0], "anki_version": version, "anki_build": buildhash,
                      "collection": lab.path}), flush=True)

    def watch_stdin():
        sys.stdin.read()
        stop.set()

    threading.Thread(target=watch_stdin, daemon=True).start()
    stop.wait()
    server.shutdown()
    with lock:
        lab.col.close()


if __name__ == "__main__":
    main()
