# SPDX-License-Identifier: GPL-3.0-or-later
"""Read-only lab-native-v1 declarations. No Anki/Qt imports or persistence."""
from uuid import UUID

PROTOCOL = "lab-native-v1"
COMPANION_VERSION = "0.3.0"


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


def validated_session(value):
    if value is None:
        return None
    if type(value) is not dict or set(value) != {
            "lineage_id", "session_epoch", "profile_fingerprint", "path_fingerprint"}:
        raise ValueError("NATIVE_MANIFEST_SESSION_INVALID")
    try:
        return {
            "lineage_id": _uuid(value["lineage_id"]),
            "session_epoch": _uuid(value["session_epoch"]),
            "profile_fingerprint": _digest(value["profile_fingerprint"]),
            "path_fingerprint": _digest(value["path_fingerprint"]),
        }
    except ValueError:
        raise ValueError("NATIVE_MANIFEST_SESSION_INVALID") from None


ACTIONS = ("labCapabilities", "labBegin", "labInspect", "labMutate", "labOperationStatus",
           "labRebind", "labEnd")
VARIANTS = ("install_model", "export_checkpoint", "store_media", "create_note",
            "update_note", "restore_note", "delete_unstudied_created_note")


def build_capabilities(*, bridge_id, anki_version, anki_connect_source_digest,
                       api_key_configured, operation_status_available=False,
                       collection_session=None, actions=None, mutation_variants=()):
    """Use a supplied durable installation ID; never allocate identity during a read.

    The caller must establish pinned registration before serving either read
    action. An observed session is a declaration, never a write certification.
    """
    if type(api_key_configured) is not bool or type(operation_status_available) is not bool:
        raise ValueError("NATIVE_MANIFEST_AUTH_INVALID")
    return {
        "protocol": PROTOCOL,
        "companion_version": COMPANION_VERSION,
        "bridge_id": _uuid(bridge_id),
        "integration": {
            "anki_version": _label(anki_version, 128),
            "anki_connect_source_digest": _digest(anki_connect_source_digest),
        },
        "collection_session": validated_session(collection_session),
        "actions": (list(actions) if actions is not None else
                    ["labCapabilities"] + (["labOperationStatus"] if operation_status_available else [])),
        "mutation_variants": list(mutation_variants),
        "api_key_configured": api_key_configured,
    }
