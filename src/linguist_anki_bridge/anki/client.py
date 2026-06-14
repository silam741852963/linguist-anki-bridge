import requests
from typing import Any, Dict, Optional

from linguist_anki_bridge.config import settings

class AnkiConnectError(Exception):
    pass

def invoke(action: str, **params) -> Any:
    """Invokes the AnkiConnect API locally."""
    request_data = {"action": action, "version": 6}
    if params:
        request_data["params"] = params

    try:
        response = requests.post(settings.anki_connect_url, json=request_data)
        response.raise_for_status()
    except requests.RequestException as e:
        raise AnkiConnectError(f"Failed to connect to AnkiConnect at {settings.anki_connect_url}. Is Anki running? Error: {e}")

    result = response.json()
    if len(result) != 2:
        raise AnkiConnectError(f"AnkiConnect response has an unexpected number of fields: {result}")
    
    if result.get("error") is not None:
        raise AnkiConnectError(f"AnkiConnect Error: {result['error']}")

    return result.get("result")
