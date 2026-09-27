# SPDX-License-Identifier: GPL-3.0-or-later
"""Read-only lab-native-v1 declarations. No Anki/Qt imports or persistence."""
from uuid import UUID

PROTOCOL = "lab-native-v1"
COMPANION_VERSION = "0.1.0"


def _label(value, limit):
    if (type(value) is not str or not value.strip() or len(value.encode("utf-8")) > limit
            or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in value)):
        raise ValueError("NATIVE_MANIFEST_LABEL_INVALID")
    return value


def _digest(value):
    if (type(value) is not str or len(value) != 64
            or any(char not in "0123456789abcdef" for char in value)):
        raise ValueError("NATIVE_MANIFEST_DIGEST_INVALID")
    return value


def _uuid(value):
    if type(value) is not str:
        raise ValueError("NATIVE_MANIFEST_ID_INVALID")
    try:
        parsed = UUID(value)
    except (ValueError, AttributeError):
        raise ValueError("NATIVE_MANIFEST_ID_INVALID") from None
    if not parsed.int or str(parsed) != value:
        raise ValueError("NATIVE_MANIFEST_ID_INVALID")
    return value


def build_capabilities(*, bridge_id, anki_version, anki_connect_source_digest,
                       api_key_configured):
    """Use a supplied durable installation ID; never allocate identity during a read.

    This builder declares only the capability action. The caller must establish
    pinned registration before serving it. Session/native variants stay absent.
    """
    if type(api_key_configured) is not bool:
        raise ValueError("NATIVE_MANIFEST_AUTH_INVALID")
    return {
        "protocol": PROTOCOL,
        "companion_version": COMPANION_VERSION,
        "bridge_id": _uuid(bridge_id),
        "integration": {
            "anki_version": _label(anki_version, 128),
            "anki_connect_source_digest": _digest(anki_connect_source_digest),
        },
        "collection_session": None,
        "actions": ["labCapabilities"],
        "mutation_variants": [],
        "api_key_configured": api_key_configured,
    }
