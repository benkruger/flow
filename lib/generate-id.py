"""Generate an 8-character hex session ID.

Usage: bin/flow generate-id

Prints an 8-character lowercase hex string derived from a UUID4.
Used by skills that need a unique session identifier without
relying on system tools (uuidgen) or shell pipes.
"""

import uuid


def generate_id():
    """Return an 8-character lowercase hex string."""
    return uuid.uuid4().hex[:8]


def main():
    print(generate_id())


if __name__ == "__main__":
    main()
