//! Path safety guard for the M8.2 `file_*` tool family.
//!
//! Every file tool runs its `path` argument through `resolve_safe_path`
//! BEFORE any I/O. The canonical path must live under one of:
//!
//! - the user's home directory (`$HOME`, `%USERPROFILE%`), or
//! - the OS temp directory (`$TMPDIR` on Unix, `%TEMP%` on Windows —
//!   `std::env::temp_dir()` already normalises this).
//!
//! Anything else (`C:\Windows\System32\…`, `/etc/passwd`, `/usr/bin/…`,
//! a removable drive on Windows) is refused with a clear error the LLM
//! can read back to the user. The confirmation modal is a *secondary*
//! gate — the path check is the first line of defence and is enforced
//! in code, never bypassable by the LLM (`docs/stories/m8-os-tools.md`
//! "Security invariants").
//!
//! The helper resolves the path *before* the file may exist (so
//! `file_write("~/Documents/new.txt")` works for a brand-new file): we
//! canonicalise the deepest existing ancestor and append the remaining
//! components literally. This avoids a TOCTOU-shaped surprise where a
//! symlink in the parent chain redirects an `~/scratch/x` write into
//! `/etc/x`.

use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, Result};

/// Resolve `requested` to its canonical form and ensure the result lives
/// under an allowed root. Returns the canonical `PathBuf` on success.
///
/// `~` and `~/...` are expanded against the user's home directory before
/// canonicalisation, since LLMs reliably emit shell-style paths and the
/// Win32 `start` shell does not expand them on the user's behalf.
pub fn resolve_safe_path(requested: &str) -> Result<PathBuf> {
    if requested.trim().is_empty() {
        return Err(anyhow!("path is empty"));
    }
    let expanded = expand_tilde(requested);
    let absolute = absolutise(&expanded)?;
    let canonical = canonicalise_with_fallback(&absolute)?;
    let roots = allowed_roots();
    if roots.iter().any(|r| canonical.starts_with(r)) {
        return Ok(canonical);
    }
    let roots_display = roots
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(anyhow!(
        "path outside the allowed roots — got `{}`, must live under one of: {}",
        canonical.display(),
        roots_display
    ))
}

/// Expand a leading `~` or `~/…` to the user's home directory. Anything
/// else is returned unchanged.
fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    } else if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    } else if let Some(rest) = input.strip_prefix("~\\") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(input)
}

/// Convert a relative path to an absolute one by joining onto the current
/// working directory. Pure path manipulation — does not touch the
/// filesystem.
fn absolutise(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(normalise(path));
    }
    let cwd = std::env::current_dir().map_err(|e| anyhow!("cannot read current directory: {e}"))?;
    Ok(normalise(&cwd.join(path)))
}

/// Resolve every existing prefix of `path` via `canonicalize`, then
/// append the not-yet-existing tail. Without this, `file_write` of a
/// brand-new file under an existing dir would fail the safety check
/// (since `canonicalize` errors on missing files).
fn canonicalise_with_fallback(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = std::fs::canonicalize(&ancestor) {
            let mut out = canon;
            for component in tail.iter().rev() {
                out.push(component);
            }
            return Ok(strip_unc_prefix(&out));
        }
        let Some(name) = ancestor.file_name().map(|n| n.to_os_string()) else {
            return Err(anyhow!(
                "cannot resolve any ancestor of `{}`",
                path.display()
            ));
        };
        tail.push(name);
        if !ancestor.pop() {
            return Err(anyhow!(
                "cannot resolve any ancestor of `{}`",
                path.display()
            ));
        }
    }
}

/// Strip the Windows `\\?\` UNC prefix that `canonicalize` adds. The
/// prefix is technically part of the canonical path but confuses
/// `starts_with` comparisons against an ad-hoc root like
/// `C:\Users\Alice`. Pure string manipulation; no-op on non-Windows.
#[cfg(target_os = "windows")]
fn strip_unc_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        // Skip "UNC\" if present — true network paths stay as-is.
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        return PathBuf::from(rest.to_string());
    }
    path.to_path_buf()
}

#[cfg(not(target_os = "windows"))]
fn strip_unc_prefix(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Collapse `.` and `..` components without touching the filesystem.
/// `canonicalize` does this for us when the file exists; this is the
/// fallback path for non-existent leaves.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The list of canonical roots that file_* tools may touch.
pub fn allowed_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_dir() {
        if let Ok(canon) = std::fs::canonicalize(&home) {
            roots.push(strip_unc_prefix(&canon));
        } else {
            roots.push(home);
        }
    }
    let tmp = std::env::temp_dir();
    if let Ok(canon) = std::fs::canonicalize(&tmp) {
        roots.push(strip_unc_prefix(&canon));
    } else {
        roots.push(tmp);
    }
    roots
}

/// The user's home directory. `dirs`-free: we read `$HOME` on Unix and
/// `%USERPROFILE%` on Windows (falling back to `%HOMEDRIVE%%HOMEPATH%`).
pub fn home_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("HOME") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    if let Ok(v) = std::env::var("USERPROFILE") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    let drive = std::env::var("HOMEDRIVE").ok();
    let path = std::env::var("HOMEPATH").ok();
    if let (Some(d), Some(p)) = (drive, path) {
        if !d.is_empty() && !p.is_empty() {
            return Some(PathBuf::from(format!("{d}{p}")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_errs() {
        assert!(resolve_safe_path("").is_err());
        assert!(resolve_safe_path("   ").is_err());
    }

    #[test]
    fn home_root_accepted() {
        let Some(home) = home_dir() else { return };
        let target = home.join("lashon-test-file-does-not-exist");
        let result = resolve_safe_path(target.to_str().unwrap()).unwrap();
        assert!(
            allowed_roots().iter().any(|r| result.starts_with(r)),
            "{} not under any allowed root",
            result.display()
        );
    }

    #[test]
    fn tmp_root_accepted() {
        let tmp = std::env::temp_dir();
        let target = tmp.join("lashon-test-tmp-file");
        let result = resolve_safe_path(target.to_str().unwrap()).unwrap();
        assert!(allowed_roots().iter().any(|r| result.starts_with(r)));
    }

    #[test]
    fn tilde_expands_to_home() {
        if home_dir().is_none() {
            return;
        }
        // Use a leaf the test process won't conflict with even if real.
        let result = resolve_safe_path("~/lashon-test-tilde-leaf").unwrap();
        let home = home_dir().unwrap();
        let home_canon = std::fs::canonicalize(&home)
            .map(|p| strip_unc_prefix(&p))
            .unwrap_or(home);
        assert!(
            result.starts_with(&home_canon),
            "{} not under {}",
            result.display(),
            home_canon.display()
        );
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn system_root_refused() {
        let err = resolve_safe_path(r"C:\Windows\System32\drivers\etc\hosts")
            .err()
            .expect("system path must be refused");
        assert!(err.to_string().contains("outside the allowed roots"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn etc_passwd_refused() {
        let err = resolve_safe_path("/etc/passwd")
            .err()
            .expect("/etc/passwd must be refused");
        assert!(err.to_string().contains("outside the allowed roots"));
    }

    #[test]
    fn parent_traversal_does_not_escape_root() {
        // A `~/scratch/../../../etc/passwd`-style attempt must canonicalise
        // to outside the home root, and be refused.
        let Some(home) = home_dir() else { return };
        let evil = format!("{}/../../../../etc/passwd", home.display());
        let result = resolve_safe_path(&evil);
        // On all platforms the canonical form ends up either under a
        // disallowed root (Linux: `/etc/passwd`; Windows: `C:\etc\passwd`)
        // or under home (if the parent chain doesn't escape). Either way,
        // the function must NOT return an unsafe path silently — assert
        // that if it returned Ok, the path is still under an allowed
        // root.
        if let Ok(path) = result {
            assert!(
                allowed_roots().iter().any(|r| path.starts_with(r)),
                "parent traversal escaped: {}",
                path.display()
            );
        }
    }
}
