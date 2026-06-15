import urllib.request
import json
import logging
from urllib.error import URLError

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
            
        use_timeout = custom_timeout if custom_timeout else self.timeout
            
        try:
            req = urllib.request.Request(
                self.url,
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"}
            )
            with urllib.request.urlopen(req, timeout=use_timeout) as res:
                response = json.loads(res.read().decode("utf-8"))
                if response.get("error"):
                    raise Exception(response["error"])
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

    def get_model_fields(self, model_name: str) -> list:
        return self._request("modelFieldNames", modelName=model_name)

    def find_notes(self, query: str) -> list:
        return self._request("findNotes", query=query)

    def get_notes_info(self, note_ids: list) -> list:
        if not note_ids:
            return []
        return self._request("notesInfo", notes=note_ids)

    def update_note_fields(self, note_id: int, fields: dict):
        note = {
            "id": note_id,
            "fields": fields
        }
        return self._request("updateNoteFields", note=note)

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

    def store_media_file(self, filename: str, data_base64: str) -> str:
        # Stores base64 content into Anki media folder
        return self._request("storeMediaFile", filename=filename, data=data_base64)

    def export_package(self, deck_name: str, path: str) -> bool:
        # Exports deck to .apkg path
        return self._request("exportPackage", custom_timeout=300.0, deck=deck_name, path=path, includeSched=False)
