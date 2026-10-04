//! Carry-over from the pre-rename install (docs/adr/0042).
//!
//! Until v1.1 the product shipped as *Lashon*. Its data lives in directories
//! and keychain entries named after the old identity: the per-identifier
//! data dirs (`dev.lashon.desktop`: settings, WebView storage, logs and the
//! multi-GB models), the per-user recipes dir (`lashon/recipes`), and the
//! keychain service `lashon`. Without a carry-over the renamed app would start
//! empty and download the models again.
//!
//! Each consumer adopts its own data where it resolves it: the Tauri shell
//! moves the identifier dirs before the builder runs, the recipes resolver
//! moves the recipes dir, and the keychain reads through to the old service.
//! [`adopt_dir`] is the shared, retry-safe move.
//!
//! Every consumer asks [`carry_over_enabled`] first (docs/adr/0046). On a
//! developer's machine the pre-rename data belongs to their real installed
//! app, so debug builds leave it alone unless the developer opts in.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

/// The bundle identifier the app used before the rename.
pub const LEGACY_IDENTIFIER: &str = "dev.lashon.desktop";

/// The app-named directory (under the per-user data dir) used before the
/// rename, e.g. `%LOCALAPPDATA%\lashon\recipes`.
pub const LEGACY_DIR_NAME: &str = "lashon";

/// The OS-keychain service name used before the rename.
pub const LEGACY_KEYCHAIN_SERVICE: &str = "lashon";

/// Set to `1` to let a debug build carry the pre-rename data over.
pub const ADOPT_LEGACY_ENV: &str = "OTTID_ADOPT_LEGACY";

/// Whether this process may touch the pre-rename data at all: move its
/// directories, adopt its keychain keys, or delete them (docs/adr/0046).
///
/// Always true in release builds, which is what users run. False in debug
/// builds (`npm run tauri dev`, `cargo run`, `cargo test`) unless
/// [`ADOPT_LEGACY_ENV`] is `1`. Read once per process, so every consumer
/// sees the same answer.
pub fn carry_over_enabled() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        carry_over_allowed(
            cfg!(debug_assertions),
            std::env::var_os(ADOPT_LEGACY_ENV).as_deref(),
        )
    })
}

/// The rule behind [`carry_over_enabled`], with the build kind and the
/// opt-in value passed in so it can be tested.
fn carry_over_allowed(debug_build: bool, opt_in: Option<&OsStr>) -> bool {
    !debug_build || opt_in == Some(OsStr::new("1"))
}

/// What [`adopt_dir`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Adopted {
    /// There was no old directory.
    Nothing,
    /// The old directory was renamed to the new path in one step.
    Renamed,
    /// Both existed: `moved` entries came across, `kept` stayed behind
    /// because the new directory already had an entry of that name.
    Merged { moved: usize, kept: usize },
}

/// Move the contents of `old` into `new`.
///
/// - When `new` doesn't exist, `old` is renamed to it. The two share a
///   parent, so the rename is instant even for gigabytes of models.
/// - When both exist (an earlier attempt was interrupted, or the new app
///   already ran), each top-level entry of `old` that `new` lacks is moved
///   across. An entry `new` already has wins, and the old copy stays.
/// - `old` is removed once it is empty.
///
/// Safe to call on every start: once `old` is gone it is a single `stat`.
/// On an error the remaining entries stay in `old`, and the next call
/// picks them up.
pub fn adopt_dir(old: &Path, new: &Path) -> io::Result<Adopted> {
    if !old.is_dir() {
        return Ok(Adopted::Nothing);
    }
    if !new.exists() {
        if let Some(parent) = new.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(old, new)?;
        return Ok(Adopted::Renamed);
    }
    let (mut moved, mut kept) = (0, 0);
    for entry in fs::read_dir(old)? {
        let entry = entry?;
        let target = new.join(entry.file_name());
        if target.symlink_metadata().is_ok() {
            kept += 1;
            continue;
        }
        fs::rename(entry.path(), &target)?;
        moved += 1;
    }
    if kept == 0 {
        remove_dir_if_empty(old);
    }
    Ok(Adopted::Merged { moved, kept })
}

/// Remove `dir` if it exists and is empty. Anything else (missing, not
/// empty, in use) is left alone: this is tidying, never a failure.
pub fn remove_dir_if_empty(dir: &Path) {
    let _ = fs::remove_dir(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn release_builds_always_carry_over() {
        assert!(carry_over_allowed(false, None));
        assert!(carry_over_allowed(false, Some(OsStr::new("0"))));
    }

    #[test]
    fn debug_builds_carry_over_only_when_opted_in() {
        assert!(!carry_over_allowed(true, None));
        assert!(carry_over_allowed(true, Some(OsStr::new("1"))));
    }

    #[test]
    fn debug_opt_in_accepts_only_1() {
        // Anything else leaves the real data alone: a guard on someone's
        // installed app should fail closed.
        for value in ["", "0", "true", "yes", " 1", "11"] {
            assert!(
                !carry_over_allowed(true, Some(OsStr::new(value))),
                "{value:?} must not opt in"
            );
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_builds_leave_legacy_data_alone() {
        // `cargo test` is a debug build: a test that reaches a consumer
        // must not move the developer's real data.
        if std::env::var_os(ADOPT_LEGACY_ENV).is_none() {
            assert!(!carry_over_enabled());
        }
    }

    #[test]
    fn missing_old_dir_is_nothing() {
        let root = tempfile::tempdir().unwrap();
        let out = adopt_dir(&root.path().join("old"), &root.path().join("new")).unwrap();
        assert_eq!(out, Adopted::Nothing);
        assert!(!root.path().join("new").exists());
    }

    #[test]
    fn old_dir_is_renamed_when_new_is_absent() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = (
            root.path().join("dev.lashon.desktop"),
            root.path().join("app.ottid.desktop"),
        );
        write(&old.join("settings.json"), "{\"ui.language\":\"he\"}");
        write(&old.join("models/stt/model.bin"), "weights");

        assert_eq!(adopt_dir(&old, &new).unwrap(), Adopted::Renamed);
        assert!(!old.exists());
        assert_eq!(
            fs::read_to_string(new.join("settings.json")).unwrap(),
            "{\"ui.language\":\"he\"}"
        );
        assert_eq!(
            fs::read_to_string(new.join("models/stt/model.bin")).unwrap(),
            "weights"
        );
    }

    #[test]
    fn rename_creates_the_new_parent() {
        // The recipes dir moves from `lashon/recipes` to `ottid/recipes`,
        // and `ottid/` may not exist yet.
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("lashon/recipes");
        let new = root.path().join("ottid/recipes");
        write(&old.join("my_recipe/recipe.yaml"), "id: my-recipe");

        assert_eq!(adopt_dir(&old, &new).unwrap(), Adopted::Renamed);
        assert!(new.join("my_recipe/recipe.yaml").is_file());
    }

    #[test]
    fn merge_moves_missing_entries_and_keeps_existing_ones() {
        // The new app already ran once (it created its own WebView storage),
        // or an earlier move was interrupted: entries the new dir lacks come
        // across, entries it has win.
        let root = tempfile::tempdir().unwrap();
        let (old, new) = (root.path().join("old"), root.path().join("new"));
        write(&old.join("models/stt/model.bin"), "weights");
        write(&old.join("EBWebView/state"), "old");
        write(&new.join("EBWebView/state"), "new");

        let out = adopt_dir(&old, &new).unwrap();
        assert_eq!(out, Adopted::Merged { moved: 1, kept: 1 });
        assert_eq!(
            fs::read_to_string(new.join("models/stt/model.bin")).unwrap(),
            "weights"
        );
        assert_eq!(
            fs::read_to_string(new.join("EBWebView/state")).unwrap(),
            "new"
        );
        // The losing copy stays put rather than being deleted.
        assert_eq!(
            fs::read_to_string(old.join("EBWebView/state")).unwrap(),
            "old"
        );
    }

    #[test]
    fn merge_removes_the_emptied_old_dir() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = (root.path().join("old"), root.path().join("new"));
        write(&old.join("logs/ottid.log"), "line");
        fs::create_dir_all(new.join("EBWebView")).unwrap();

        assert_eq!(
            adopt_dir(&old, &new).unwrap(),
            Adopted::Merged { moved: 1, kept: 0 }
        );
        assert!(!old.exists());
        assert!(new.join("logs/ottid.log").is_file());
    }

    #[test]
    fn second_call_is_a_no_op() {
        let root = tempfile::tempdir().unwrap();
        let (old, new) = (root.path().join("old"), root.path().join("new"));
        write(&old.join("settings.json"), "{}");
        adopt_dir(&old, &new).unwrap();
        assert_eq!(adopt_dir(&old, &new).unwrap(), Adopted::Nothing);
    }

    #[test]
    fn remove_dir_if_empty_leaves_non_empty_dirs() {
        let root = tempfile::tempdir().unwrap();
        let full = root.path().join("full");
        write(&full.join("keep.txt"), "x");
        remove_dir_if_empty(&full);
        assert!(full.join("keep.txt").is_file());

        let empty = root.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        remove_dir_if_empty(&empty);
        assert!(!empty.exists());

        // Missing is fine too.
        remove_dir_if_empty(&root.path().join("missing"));
    }
}
