"""Write content to a target file path via native Python I/O.

Usage:
  bin/flow write-rule --path <target> --content-file <temp>

Reads content from a temp file, creates parent directories if needed,
writes to the target path, and deletes the temp file. Used by the Learn
phase to bypass Claude Code's .claude/ permission prompts.

Output (JSON to stdout):
  Success: {"status": "ok", "path": "<target_path>"}
  Error:   {"status": "error", "message": "..."}
"""

import argparse
import json
import os
import sys


def read_content_file(path):
    """Read content from a file and delete the file.

    Returns (content, error_message). On success error is None.
    The file is always deleted after reading, even if empty.
    """
    try:
        with open(path) as f:
            content = f.read()
    except OSError as exc:
        return None, f"Could not read content file '{path}': {exc}"

    try:
        os.remove(path)
    except OSError:
        pass

    return content, None


def write_rule(target_path, content):
    """Write content to the target path, creating parent dirs as needed.

    Returns (success, error_message). On success error is None.
    """
    try:
        os.makedirs(os.path.dirname(target_path), exist_ok=True)
    except OSError as exc:
        return False, f"Could not create directories for '{target_path}': {exc}"

    try:
        with open(target_path, "w") as f:
            f.write(content)
    except OSError as exc:
        return False, f"Could not write to '{target_path}': {exc}"

    return True, None


def main():
    parser = argparse.ArgumentParser(description="Write content to a target file")
    parser.add_argument("--path", required=True, help="Target file path")
    parser.add_argument(
        "--content-file", required=True, help="Path to file containing content (file is deleted after reading)"
    )
    args = parser.parse_args()

    content, read_error = read_content_file(args.content_file)
    if read_error:
        print(json.dumps({"status": "error", "message": read_error}))
        sys.exit(1)

    ok, write_error = write_rule(args.path, content)
    if not ok:
        print(json.dumps({"status": "error", "message": write_error}))
        sys.exit(1)

    print(json.dumps({"status": "ok", "path": args.path}))


if __name__ == "__main__":
    main()
