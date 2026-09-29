# SPDX-License-Identifier: GPL-3.0-or-later
"""Packaged as __init__.py in the explicit .ankiaddon artifact."""
from .startup import install_anki_hooks

try:
    install_anki_hooks()
except Exception:
    # Unsupported builds must not prevent Anki itself from starting.
    STARTUP_ERROR = "BRIDGE_STARTUP_UNAVAILABLE"
