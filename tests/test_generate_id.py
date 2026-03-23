"""Tests for lib/generate-id.py — session ID generation."""

import re
import subprocess
import sys

from conftest import BIN_DIR, LIB_DIR

sys.path.insert(0, str(LIB_DIR))

from importlib import import_module

generate_id_mod = import_module("generate-id")


def test_generate_id_returns_8_chars():
    result = generate_id_mod.generate_id()
    assert len(result) == 8


def test_generate_id_is_hex():
    result = generate_id_mod.generate_id()
    assert re.fullmatch(r"[0-9a-f]{8}", result), f"Not valid hex: {result}"


def test_generate_id_unique():
    a = generate_id_mod.generate_id()
    b = generate_id_mod.generate_id()
    assert a != b


def test_main_prints_id(capsys):
    generate_id_mod.main()
    captured = capsys.readouterr()
    output = captured.out.strip()
    assert len(output) == 8
    assert re.fullmatch(r"[0-9a-f]{8}", output)


def test_cli_integration():
    result = subprocess.run(
        [sys.executable, str(LIB_DIR / "generate-id.py")],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    output = result.stdout.strip()
    assert len(output) == 8
    assert re.fullmatch(r"[0-9a-f]{8}", output)
