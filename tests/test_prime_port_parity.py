"""Python↔Rust parity tests for the prime-check port (#786).

Two layers of defense against constant drift and hash divergence between
lib/prime-setup.py (Python) and src/prime_check.rs (Rust):

1. Static constant parity — parse the three const arrays out of the Rust
   source and assert each entry matches the Python source entry-by-entry.
   Fails fast with a detailed diff if any entry drifts.

2. End-to-end hash round-trip — for each framework, compute the config
   hash and setup hash in Python, write a .flow.json containing those
   hashes with an old flow_version, invoke `bin/flow prime-check`, and
   assert it accepts them (auto_upgraded=true). Proves Rust produces
   byte-identical hash output to Python's `json.dumps(sort_keys=True)`.
"""

import json
import re
import subprocess

import pytest
from conftest import REPO_ROOT, import_lib

RUST_SRC = REPO_ROOT / "src" / "prime_check.rs"
BIN_FLOW = REPO_ROOT / "bin" / "flow"


def _load_prime_setup():
    """Load prime-setup module for constant + hash access."""
    return import_lib("prime-setup.py")


def _extract_rust_const(name: str) -> list[str]:
    """Extract entries from a `const NAME: &[&str] = &[ ... ];` block.

    Returns the list of string literals inside the array. Assumes
    well-formed Rust syntax — entries are comma-separated "..." strings
    optionally followed by trailing commas and whitespace.
    """
    content = RUST_SRC.read_text()
    pattern = rf"const {re.escape(name)}:\s*&\[&str\]\s*=\s*&\[(.*?)\];"
    match = re.search(pattern, content, re.DOTALL)
    if not match:
        raise AssertionError(f"Could not find `const {name}: &[&str] = &[...];` in {RUST_SRC}")
    body = match.group(1)
    string_literal = re.compile(r'"((?:[^"\\]|\\.)*)"')
    return [m.group(1) for m in string_literal.finditer(body)]


# --- Static constant parity ---


def test_universal_allow_parity():
    """Rust UNIVERSAL_ALLOW must equal Python UNIVERSAL_ALLOW entry-by-entry."""
    prime_setup = _load_prime_setup()
    rust = _extract_rust_const("UNIVERSAL_ALLOW")
    python = prime_setup.UNIVERSAL_ALLOW
    assert rust == python, (
        "UNIVERSAL_ALLOW drift between Python and Rust:\n"
        f"  Only in Python: {sorted(set(python) - set(rust))}\n"
        f"  Only in Rust:   {sorted(set(rust) - set(python))}\n"
    )


def test_flow_deny_parity():
    """Rust FLOW_DENY must equal Python FLOW_DENY entry-by-entry."""
    prime_setup = _load_prime_setup()
    rust = _extract_rust_const("FLOW_DENY")
    python = prime_setup.FLOW_DENY
    assert rust == python, (
        "FLOW_DENY drift between Python and Rust:\n"
        f"  Only in Python: {sorted(set(python) - set(rust))}\n"
        f"  Only in Rust:   {sorted(set(rust) - set(python))}\n"
    )


def test_exclude_entries_parity():
    """Rust EXCLUDE_ENTRIES must equal Python EXCLUDE_ENTRIES entry-by-entry."""
    prime_setup = _load_prime_setup()
    rust = _extract_rust_const("EXCLUDE_ENTRIES")
    python = prime_setup.EXCLUDE_ENTRIES
    assert rust == python, (
        "EXCLUDE_ENTRIES drift between Python and Rust:\n"
        f"  Only in Python: {sorted(set(python) - set(rust))}\n"
        f"  Only in Rust:   {sorted(set(rust) - set(python))}\n"
    )


# --- End-to-end hash round-trip ---


@pytest.mark.parametrize("framework", ["rails", "python", "ios", "go", "rust"])
def test_rust_accepts_python_computed_hashes(framework, tmp_path):
    """Rust prime-check must auto-upgrade .flow.json built with Python hashes.

    If Python and Rust hashes diverge for any framework, this test fails
    because Rust returns a version mismatch instead of auto_upgraded=true.
    """
    prime_setup = _load_prime_setup()
    config_hash = prime_setup.compute_config_hash(framework)
    setup_hash = prime_setup.compute_setup_hash()

    flow_json = tmp_path / ".flow.json"
    flow_json.write_text(
        json.dumps(
            {
                "flow_version": "0.0.1",
                "framework": framework,
                "config_hash": config_hash,
                "setup_hash": setup_hash,
            }
        )
    )

    result = subprocess.run(
        [str(BIN_FLOW), "prime-check"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, f"bin/flow prime-check failed: {result.stderr}"
    # bin/flow may emit build output before the JSON line; parse the last line.
    last_line = result.stdout.strip().splitlines()[-1]
    data = json.loads(last_line)
    assert data["status"] == "ok", f"Rust rejected Python hashes for {framework}: {data}"
    assert data.get("auto_upgraded") is True, (
        f"Rust did not auto-upgrade {framework} with Python-computed hashes — "
        f"this means Python and Rust hashes diverge. Response: {data}"
    )
    assert data["framework"] == framework


def test_setup_hash_reads_prime_setup_py_bytes(tmp_path):
    """compute_setup_hash must hash the current lib/prime-setup.py bytes.

    Guards against the Rust port accidentally hashing a different file
    (e.g., resolving the wrong path under plugin_root).
    """
    prime_setup = _load_prime_setup()
    python_hash = prime_setup.compute_setup_hash()

    # Build a .flow.json that Rust should accept by auto-upgrading.
    flow_json = tmp_path / ".flow.json"
    flow_json.write_text(
        json.dumps(
            {
                "flow_version": "0.0.1",
                "framework": "rails",
                "config_hash": prime_setup.compute_config_hash("rails"),
                "setup_hash": python_hash,
            }
        )
    )

    result = subprocess.run(
        [str(BIN_FLOW), "prime-check"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    data = json.loads(result.stdout.strip().splitlines()[-1])
    assert data.get("auto_upgraded") is True, (
        "Rust setup_hash does not match Python — verify "
        "compute_setup_hash reads lib/prime-setup.py bytes via plugin_root"
    )
