from __future__ import annotations


class CliUsageError(ValueError):
    """User-correctable CLI input error.

    Raise this for invalid local inputs, unsafe paths, unsupported user
    options, or malformed user-supplied files. Let plain ValueError continue to
    represent programmer mistakes or unexpected internal state.
    """

