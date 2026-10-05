# SPDX-License-Identifier: GPL-3.0-or-later
"""Exact local read-adapter source pins; no mutation compatibility is certified."""

from .registration import RegistrationError, register_read_actions


READ_COMPATIBILITY = {
    ("25.09.2", "3d813c83"): {
        "__init__.py": "629566e8eea59f3d67abf1b2339d5c0c621b2d894139e8335db030d022582873",
        "util.py": "059b4c6170bdb40286a040e4a7950bccd69259ad5c8dd16e2f0fa2587766b18d",
    },
}


def read_source_pins(anki_version, build_hash):
    if type(anki_version) is not str or type(build_hash) is not str:
        raise RegistrationError("BRIDGE_ANKI_BUILD_UNSUPPORTED")
    pins = READ_COMPATIBILITY.get((anki_version, build_hash))
    if pins is None:
        raise RegistrationError("BRIDGE_ANKI_BUILD_UNSUPPORTED")
    return pins.copy()


def register_supported_read_actions(module, anki_version, build_hash,
                                    manifest_supplier, status_supplier=None):
    """Pin both the Anki build and AnkiConnect bytes before additive registration."""
    return register_read_actions(module, read_source_pins(anki_version, build_hash),
                                 manifest_supplier, status_supplier)


# Builds whose AnkiConnect dispatcher, main-thread timer and collection
# executor semantics the write runtime relies on. The Rust client keeps its
# own pinned matrix and stays fail-closed for anything else.
WRITE_COMPATIBILITY = frozenset({("25.09.2", "3d813c83")})


def write_supported(anki_version, build_hash):
    return (anki_version, build_hash) in WRITE_COMPATIBILITY
