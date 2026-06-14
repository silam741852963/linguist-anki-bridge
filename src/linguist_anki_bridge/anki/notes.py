from typing import List, Dict, Any
from linguist_anki_bridge.anki.client import invoke

def find_notes(query: str) -> List[int]:
    """Find notes by search query."""
    return invoke("findNotes", query=query)

def notes_info(notes: List[int]) -> List[Dict[str, Any]]:
    """Get information for multiple notes."""
    return invoke("notesInfo", notes=notes)

def retrieve_media_file(filename: str) -> str:
    """Retrieve media file by filename (returns base64 string)."""
    return invoke("retrieveMediaFile", filename=filename)

def update_note_fields(note_id: int, fields: Dict[str, str]) -> None:
    """Update specific fields of an existing note."""
    note_params = {
        "id": note_id,
        "fields": fields
    }
    invoke("updateNoteFields", note=note_params)

def add_note(deck_name: str, model_name: str, fields: Dict[str, str], tags: List[str] = None) -> int:
    """Add a new note to Anki."""
    note = {
        "deckName": deck_name,
        "modelName": model_name,
        "fields": fields,
        "tags": tags or []
    }
    return invoke("addNote", note=note)
