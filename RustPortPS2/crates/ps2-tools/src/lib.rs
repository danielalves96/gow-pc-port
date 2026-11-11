//! The installer's helper tools, in one binary (`ps2-tools`) so the installers need no Python.
//!
//! - [`elfcheck`]: `verify-elf`, identity and bounds of the local `SCUS_973.99`.
//! - [`iso`]: `iso-extract`, the ISO9660 extractor that also records the disc layout.
//! - [`vu1`]: `vu1-manifest` and `vu1-programs`, the VU1 micro-memories built from the ELF.
//! - [`iop_gen`]: `iop-gen`, the IRX modules compiled ahead of time to C++.
//! - [`iop_manifest`]: `iop-manifest`, the IOP module dump rebuilt from the disc's IRX files (no game session).
//! - [`vu1n_gen`]: `vu1n-gen`, the native VU1 block generator.
//! - [`memcard`]: `mc-format` and `mc-check`, the 8 MB memory card image.
//! - [`xdelta`]: `xdelta-apply`, applies an xdelta3 (VCDIFF) translation patch to the disc.
//! - [`chd`]: `chd-extract`, converts a CHD disc image (MAME's format, made by `chdman`) to an ISO.
//! - [`hash`]: the FNV, BLAKE2b and SHA-1 fingerprints those use.

pub mod chd;
pub mod elfcheck;
mod flac;
pub mod hash;
pub mod iop_gen;
pub mod iop_manifest;
pub mod iso;
pub mod memcard;
pub mod vu1;
pub mod vu1n_gen;
pub mod xdelta;

/// Writes `text` to `path` only when the file does not already hold it, and says whether it wrote.
///
/// The generators run on every installation, and the generated sources sit in a tree that CMake globs: rewriting an identical
/// file still moves its timestamp, so the next build recompiles it for nothing. The old content is compared with `\r\n` read as
/// `\n`, so a copy that was checked out with Windows line endings counts as unchanged too. A file that cannot be read as UTF-8
/// is replaced.
pub fn write_if_changed(path: &std::path::Path, text: &str) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|old| old.replace("\r\n", "\n") == text) {
        return Ok(false);
    }
    std::fs::write(path, text)?;
    Ok(true)
}
