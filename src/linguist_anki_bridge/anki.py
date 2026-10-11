import urllib.request
import json
import logging
import html
import re
from urllib.error import URLError


def _anki_search_quote(value: str) -> str:
    """Quote user text for Anki's search grammar."""
    return str(value).replace("\\", "\\\\").replace('"', '\\"')


def _plain_field_value(value: str) -> str:
    text = re.sub(r"<[^>]+>", "", str(value or ""))
    return " ".join(html.unescape(text).split())

class AnkiConnectClient:
    def __init__(self, url="http://localhost:8765", timeout=3.0):
        self.url = url
        self.timeout = timeout

    def _request(self, action, custom_timeout=None, **params):
        payload = {
            "action": action,
            "version": 6,
        }
        if params:
            payload["params"] = params
            
        use_timeout = custom_timeout if custom_timeout is not None else self.timeout
            
        try:
            req = urllib.request.Request(
                self.url,
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"}
            )
            with urllib.request.urlopen(req, timeout=use_timeout) as res:
                response = json.loads(res.read().decode("utf-8"))
                if not isinstance(response, dict) or "error" not in response or "result" not in response:
                    raise RuntimeError("Invalid AnkiConnect response envelope")
                if response.get("error"):
                    raise RuntimeError(response["error"])
                return response.get("result")
        except URLError as e:
            logging.error(f"AnkiConnect connection error: {e}")
            raise ConnectionError(f"Could not connect to Anki at {self.url}. Make sure Anki is running with AnkiConnect.")
        except Exception as e:
            logging.error(f"AnkiConnect error: {e}")
            raise

    def is_online(self) -> bool:
        try:
            # Quick lightweight check
            self._request("version")
            return True
        except Exception:
            return False

    def get_decks(self) -> list:
        return self._request("deckNames")

    def get_models(self) -> list:
        return self._request("modelNames")

    def get_tags(self) -> list:
        return self._request("getTags")

    def get_model_fields(self, model_name: str) -> list:
        return self._request("modelFieldNames", modelName=model_name)

    def get_model_templates(self, model_name: str) -> dict:
        return self._request("modelTemplates", modelName=model_name)

    def get_model_styling(self, model_name: str) -> dict:
        return self._request("modelStyling", modelName=model_name)

    def supports_action(self, action: str) -> bool:
        """Return whether this AnkiConnect installation exposes an action."""
        try:
            reflected = self._request(
                "apiReflect", scopes=["actions"], actions=[str(action)],
            )
        except Exception:
            return False
        return isinstance(reflected, dict) and action in (reflected.get("actions") or [])

    def create_model(self, model_name: str, fields: list, css: str, templates: dict):
        card_templates = [
            {"Name": name, "Front": value["Front"], "Back": value["Back"]}
            for name, value in templates.items()
        ]
        return self._request(
            "createModel",
            modelName=model_name,
            inOrderFields=fields,
            css=css,
            isCloze=False,
            cardTemplates=card_templates,
        )

    def update_model_templates(self, model_name: str, templates: dict):
        return self._request(
            "updateModelTemplates",
            model={"name": model_name, "templates": templates},
        )

    def update_model_styling(self, model_name: str, css: str):
        return self._request(
            "updateModelStyling",
            model={"name": model_name, "css": css},
        )

    def add_model_template(self, model_name: str, name: str, template: dict):
        return self._request(
            "modelTemplateAdd",
            modelName=model_name,
            template={"Name": name, "Front": template["Front"], "Back": template["Back"]},
        )

    def rename_model_template(self, model_name: str, old_name: str, new_name: str):
        return self._request(
            "modelTemplateRename",
            modelName=model_name,
            oldTemplateName=old_name,
            newTemplateName=new_name,
        )

    def reposition_model_template(self, model_name: str, name: str, index: int):
        return self._request(
            "modelTemplateReposition", modelName=model_name, templateName=name, index=index,
        )

    def install_model(self, model_name: str, fields: list, css: str, templates: dict) -> str:
        """Create a managed note type, or refresh it only when its schema is unchanged."""
        if model_name not in self.get_models():
            self.create_model(model_name, fields, css, templates)
            return "created"

        existing_fields = self.get_model_fields(model_name)
        if existing_fields != fields:
            raise ValueError(
                f"Refusing to overwrite note type '{model_name}': expected fields "
                f"{fields}, found {existing_fields}"
            )
        existing_templates = self.get_model_templates(model_name)
        existing_names = list(existing_templates)
        desired_names = list(templates)

        # Upgrade the original managed one-card model without deleting its
        # ordinal-zero cards. Renaming retains their identity and scheduling.
        if existing_names == ["Japanese Recognition"] and desired_names:
            self.rename_model_template(model_name, existing_names[0], desired_names[0])
            existing_names[0] = desired_names[0]

        unexpected = [name for name in existing_names if name not in desired_names]
        if unexpected:
            raise ValueError(
                f"Refusing to remove unexpected templates from '{model_name}': {unexpected}. "
                "Removing a template can delete scheduled cards."
            )

        for name in desired_names:
            if name not in existing_names:
                self.add_model_template(model_name, name, templates[name])
                existing_names.append(name)

        for index, name in enumerate(desired_names):
            if existing_names.index(name) != index:
                self.reposition_model_template(model_name, name, index)
                existing_names.remove(name)
                existing_names.insert(index, name)

        self.update_model_templates(model_name, templates)
        self.update_model_styling(model_name, css)
        return "updated"

    def find_notes(self, query: str) -> list:
        return self._request("findNotes", query=query)

    def get_notes_info(self, note_ids: list) -> list:
        if not note_ids:
            return []
        return self._request("notesInfo", notes=note_ids)

    def find_exact_expression(self, deck_name: str, expression: str, field_names=()) -> list:
        """Return notes whose field text exactly equals ``expression``.

        Anki performs the indexed candidate search; the final comparison is
        local so HTML wrappers and unrelated substring matches cannot turn an
        injection into a duplicate card.
        """
        wanted = _plain_field_value(expression)
        if not wanted:
            return []
        query = (
            f'deck:"{_anki_search_quote(deck_name)}" '
            f'"{_anki_search_quote(wanted)}"'
        )
        notes = self.get_notes_info(self.find_notes(query))
        preferred = tuple(dict.fromkeys(str(name) for name in field_names if name))
        matches = []
        for note in notes:
            fields = note.get("fields", {})
            semantic_names = [name for name in preferred if name in fields]
            semantic_names.extend(
                name for name in fields
                if name not in semantic_names
                and any(token in name.lower() for token in ("expression", "word", "front", "vocab"))
            )
            ordered_names = tuple(semantic_names or fields.keys())
            if any(
                _plain_field_value(fields.get(name, {}).get("value", "")) == wanted
                for name in ordered_names
            ):
                matches.append(note)
        return matches

    def search_notes_in_deck(self, deck_name: str, text: str) -> list:
        """Search every note in a deck, independent of the modernization queue."""
        wanted = _plain_field_value(text)
        if not deck_name or not wanted:
            return []
        query = (
            f'deck:"{_anki_search_quote(deck_name)}" '
            f'"{_anki_search_quote(wanted)}"'
        )
        return self.get_notes_info(self.find_notes(query))

    def update_note_fields(self, note_id: int, fields: dict):
        note = {
            "id": note_id,
            "fields": fields
        }
        return self._request("updateNoteFields", note=note)

    def update_note_model(
        self, note_id: int, model_name: str, fields: dict, tags=None,
    ):
        """Migrate an existing note to another model without recreating it."""
        note = {
            "id": int(note_id),
            "modelName": str(model_name),
            "fields": fields,
            "tags": list(tags or []),
        }
        return self._request("updateNoteModel", note=note)

    def add_note(self, deck_name: str, model_name: str, fields: dict, tags=None) -> int:
        note = {
            "deckName": deck_name,
            "modelName": model_name,
            "fields": fields,
            "options": {
                "allowDuplicate": False,
                "duplicateScope": "deck"
            }
        }
        if tags:
            note["tags"] = tags
        return self._request("addNote", note=note)

    def retrieve_media_file(self, filename: str) -> str:
        # Returns base64 encoded data
        return self._request("retrieveMediaFile", filename=filename)

    def retrieve_media_files(self, filenames) -> dict[str, str | None]:
        """Retrieve several media files in one local AnkiConnect round trip."""
        ordered = list(dict.fromkeys(str(name) for name in filenames if name))
        if not ordered:
            return {}
        actions = [
            {"action": "retrieveMediaFile", "version": 6, "params": {"filename": name}}
            for name in ordered
        ]
        try:
            responses = self._request("multi", actions=actions)
            if not isinstance(responses, list) or len(responses) != len(ordered):
                raise RuntimeError("Invalid AnkiConnect multi response")
            result: dict[str, str | None] = {}
            for name, response in zip(ordered, responses):
                if isinstance(response, dict):
                    value = response.get("result") if not response.get("error") else None
                else:
                    value = response
                result[name] = value if isinstance(value, str) and value else None
            return result
        except Exception:
            # Older AnkiConnect releases may not expose ``multi``.
            result = {}
            for name in ordered:
                try:
                    value = self.retrieve_media_file(name)
                    result[name] = value if isinstance(value, str) and value else None
                except Exception:
                    # Omit unknown values: callers must not mistake a transport
                    # failure for proof that a file did not exist.
                    continue
            return result

    def store_media_file(self, filename: str, data_base64: str) -> str:
        # Stores base64 content into Anki media folder
        return self._request("storeMediaFile", filename=filename, data=data_base64)

    def delete_media_file(self, filename: str):
        return self._request("deleteMediaFile", filename=filename)

    def export_package(self, deck_name: str, path: str) -> bool:
        # Exports deck to .apkg path
        return self._request("exportPackage", custom_timeout=300.0, deck=deck_name, path=path, includeSched=False)

    def delete_notes(self, note_ids: list):
        return self._request("deleteNotes", notes=note_ids)
