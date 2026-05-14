//! Tests for `bin/flow-rs-darwin-arm64` — the committed prebuilt FLOW
//! binary that ships to marketplace end users.
//!
//! `/plugin install` copies the binary into the plugin cache with its
//! git mode bit preserved, so end users get a runnable binary before
//! any hook fires. These tests guard three properties of the
//! committed artifact: it exists, git tracks it as executable
//! (`100755`), and it is a macOS Apple Silicon Mach-O binary.
//!
//! These tests do NOT verify the committed binary's bytes match the
//! current `src/` generation — there is no mechanical source-vs-binary
//! drift gate. Coherence is maintained by the `/flow-release` Step 6
//! rebuild-and-stage discipline (`bin/setup --stage-binary`), which is
//! exercised end-to-end only when a maintainer runs `/flow-release`.

mod common;

use std::fs::File;
use std::io::Read;
use std::process::Command;

/// The committed binary must exist at bin/flow-rs-darwin-arm64.
#[test]
fn committed_binary_file_exists() {
    let path = common::bin_dir().join("flow-rs-darwin-arm64");
    assert!(
        path.exists(),
        "bin/flow-rs-darwin-arm64 must be committed — {} not found",
        path.display()
    );
}

/// git must track the committed binary with the executable mode
/// `100755` so `/plugin install` preserves the exec bit per-file.
#[test]
fn committed_binary_has_executable_git_mode() {
    let output = Command::new("git")
        .args(["ls-files", "--stage", "bin/flow-rs-darwin-arm64"])
        .current_dir(common::repo_root())
        .output()
        .expect("run git ls-files");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mode = stdout.split_whitespace().next().unwrap_or_else(|| {
        panic!(
            "git ls-files returned no entry for bin/flow-rs-darwin-arm64; stdout: {:?}",
            stdout
        )
    });
    assert_eq!(
        mode, "100755",
        "bin/flow-rs-darwin-arm64 must be tracked as executable (100755), got {}",
        mode
    );
}

/// The committed binary must be a macOS 64-bit Apple Silicon Mach-O:
/// magic `MH_MAGIC_64` (0xFEEDFACF, little-endian bytes CF FA ED FE)
/// at offset 0, and CPU type `CPU_TYPE_ARM64` (0x0100000C,
/// little-endian bytes 0C 00 00 01) at offset 4.
#[test]
fn committed_binary_is_macho_arm64() {
    let path = common::bin_dir().join("flow-rs-darwin-arm64");
    let mut file = File::open(&path).unwrap_or_else(|e| panic!("open {}: {}", path.display(), e));
    let mut header = [0u8; 8];
    file.read_exact(&mut header)
        .unwrap_or_else(|e| panic!("read 8-byte header from {}: {}", path.display(), e));
    assert_eq!(
        &header[0..4],
        &[0xCF, 0xFA, 0xED, 0xFE],
        "bin/flow-rs-darwin-arm64 must start with the Mach-O 64-bit magic"
    );
    assert_eq!(
        &header[4..8],
        &[0x0C, 0x00, 0x00, 0x01],
        "bin/flow-rs-darwin-arm64 must declare CPU type CPU_TYPE_ARM64"
    );
}
