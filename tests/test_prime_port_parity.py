"""Rust prime-check hash verification tests.

After the prime-setup.py → prime_setup.rs port (PR #894), the Python
source no longer exists. These tests verify that:

1. compute_setup_hash reads src/prime_setup.rs (not lib/prime-setup.py)
2. Config hash round-trip works (Rust writes, Rust reads)
3. The deleted Python files do not return (tombstones)
"""

import hashlib
import json
import subprocess

import pytest
from conftest import REPO_ROOT

BIN_FLOW = REPO_ROOT / "bin" / "flow"
RUST_SRC = REPO_ROOT / "src" / "prime_setup.rs"


def _compute_setup_hash_from_rust_source():
    """Compute setup hash the same way Rust does — SHA-256 of src/prime_setup.rs."""
    content = RUST_SRC.read_bytes()
    return hashlib.sha256(content).hexdigest()[:12]


# --- Setup hash targets Rust source ---


def test_setup_hash_reads_rust_source(tmp_path):
    """compute_setup_hash must hash src/prime_setup.rs bytes.

    We verify by computing the hash locally from the Rust source and
    checking that prime-setup stores the same hash in .flow.json.
    """
    expected_hash = _compute_setup_hash_from_rust_source()

    # Create a minimal git repo so prime-setup succeeds
    subprocess.run(["git", "init"], cwd=tmp_path, capture_output=True)
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=tmp_path,
        capture_output=True,
    )

    result = subprocess.run(
        [str(BIN_FLOW), "prime-setup", str(tmp_path), "--framework", "rails"],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, f"prime-setup failed: {result.stderr}"

    flow_json = tmp_path / ".flow.json"
    data = json.loads(flow_json.read_text())
    assert data["setup_hash"] == expected_hash, (
        f"setup_hash mismatch: .flow.json has {data['setup_hash']}, expected {expected_hash} from src/prime_setup.rs"
    )


# --- Config hash round-trip ---


@pytest.mark.parametrize("framework", ["rails", "python", "ios", "go", "rust"])
def test_rust_config_hash_round_trip(framework, tmp_path):
    """prime-check accepts hashes written by prime-setup for all frameworks.

    Writes .flow.json via prime-setup with a real framework, then changes
    the version so prime-check must auto-upgrade. If config_hash and
    setup_hash match, auto-upgrade succeeds.
    """
    # Create a minimal git repo
    subprocess.run(["git", "init"], cwd=tmp_path, capture_output=True)
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init"],
        cwd=tmp_path,
        capture_output=True,
    )

    # Run prime-setup to get real hashes
    result = subprocess.run(
        [str(BIN_FLOW), "prime-setup", str(tmp_path), "--framework", framework],
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, f"prime-setup failed for {framework}: {result.stderr}"

    flow_json = tmp_path / ".flow.json"
    data = json.loads(flow_json.read_text())

    # Downgrade the version so prime-check will try auto-upgrade
    data["flow_version"] = "0.0.1"
    flow_json.write_text(json.dumps(data))

    # Run prime-check — should auto-upgrade
    result = subprocess.run(
        [str(BIN_FLOW), "prime-check"],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert result.returncode == 0, f"prime-check failed: {result.stderr}"
    last_line = result.stdout.strip().splitlines()[-1]
    check_data = json.loads(last_line)
    assert check_data["status"] == "ok", f"prime-check rejected hashes for {framework}: {check_data}"
    assert check_data.get("auto_upgraded") is True, (
        f"prime-check did not auto-upgrade {framework} — hashes diverge. Response: {check_data}"
    )


# --- Tombstones: deleted Python files must not return ---


def test_no_python_prime_setup():
    """Tombstone: lib/prime-setup.py removed in PR #894. Must not return."""
    assert not (REPO_ROOT / "lib" / "prime-setup.py").exists(), (
        "lib/prime-setup.py was deleted in PR #894 (ported to src/prime_setup.rs). It must not be re-introduced."
    )


def test_no_python_test_prime_setup():
    """Tombstone: tests/test_prime_setup.py removed in PR #894. Must not return."""
    assert not (REPO_ROOT / "tests" / "test_prime_setup.py").exists(), (
        "tests/test_prime_setup.py was deleted in PR #894 (ported to tests/prime_setup.rs). "
        "It must not be re-introduced."
    )
