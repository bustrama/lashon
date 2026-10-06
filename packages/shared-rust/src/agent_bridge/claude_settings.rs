//! Adding Ottid's hook to Claude Code's user settings, and taking it out.
//!
//! **Which file.** Claude Code merges hooks from several settings files:
//! the user's `~/.claude/settings.json` (all projects), a project's
//! `.claude/settings.json` (committed) and `.claude/settings.local.json`
//! (that project only), and managed policy. Ottid is one per user, so it
//! writes the **user** file only, and never touches a project's files. The
//! user file moves with `CLAUDE_CONFIG_DIR` when that is set where Ottid
//! runs.
//!
//! **What changes.** One matcher group in `hooks.PermissionRequest`,
//! running `ottid-hook` in exec form (no shell, so a path with spaces needs
//! no quoting) for every tool. Any earlier Ottid handler (an old install
//! path) is replaced. Nothing else in the file changes: every other key and
//! value is carried through byte for byte, in its order.
//!
//! **Consent.** The Hub previews the change and applies it only when the
//! user confirms, and only if the file is still what was previewed. The
//! file is backed up next to itself first, and written in one rename.
//!
//! A file that isn't valid JSON, or whose `hooks` isn't shaped as Claude
//! Code documents it, is left alone with an error.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{HOOK_EVENT, HOOK_TIMEOUT_SECS};

/// What Claude Code shows while the hook waits.
pub const STATUS_MESSAGE: &str = "Waiting for your answer on Ottid's approval card";

/// The hook binary's name without its extension. A handler that runs it is
/// Ottid's, whatever folder it is in.
pub const HOOK_STEM: &str = "ottid-hook";

const UTF8_BOM: &str = "\u{FEFF}";

/// Claude Code's user settings file: `$CLAUDE_CONFIG_DIR/settings.json`, or
/// `~/.claude/settings.json`.
pub fn user_settings_path() -> Option<PathBuf> {
    settings_path_from(std::env::var_os("CLAUDE_CONFIG_DIR"), home_dir())
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    home.map(PathBuf::from)
}

fn settings_path_from(
    config_dir: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    match config_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("settings.json")),
        None => home.map(|home| home.join(".claude").join("settings.json")),
    }
}

/// The matcher group Ottid adds, in the order it is written.
#[derive(Serialize)]
struct Group<'a> {
    matcher: &'a str,
    hooks: [Handler<'a>; 1],
}

#[derive(Serialize)]
struct Handler<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    command: &'a str,
    args: [&'a str; 0],
    timeout: u64,
    #[serde(rename = "statusMessage")]
    status_message: &'a str,
}

fn group(hook_exe: &str) -> Group<'_> {
    Group {
        // Every tool: the card shows whatever Claude Code would ask about.
        matcher: "*",
        hooks: [Handler {
            kind: "command",
            command: hook_exe,
            // Exec form: Claude Code spawns the binary directly, with no shell.
            args: [],
            timeout: HOOK_TIMEOUT_SECS,
            status_message: STATUS_MESSAGE,
        }],
    }
}

/// The matcher group Ottid adds.
pub fn entry(hook_exe: &str) -> Value {
    serde_json::to_value(group(hook_exe)).unwrap_or(Value::Null)
}

/// Whether a hook handler runs `ottid-hook`.
pub(super) fn is_ours(handler: &Value) -> bool {
    if handler.get("type").and_then(Value::as_str) != Some("command") {
        return false;
    }
    let Some(command) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    let command = command.trim().trim_matches('"');
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let stem = match name.len().checked_sub(4) {
        Some(at) if name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(".exe") => {
            &name[..at]
        }
        _ => name,
    };
    stem.eq_ignore_ascii_case(HOOK_STEM)
}

/// Why the settings file is left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("the settings file isn't valid JSON")]
    NotJson,
    #[error("the settings file isn't a JSON object")]
    NotObject,
    #[error("`hooks` in the settings file isn't an object")]
    HooksNotObject,
    #[error("`hooks.PermissionRequest` in the settings file isn't a list")]
    EventNotList,
    #[error("a key Ottid would change appears twice in the settings file")]
    Duplicate,
}

impl SettingsError {
    pub fn code(self) -> &'static str {
        match self {
            SettingsError::NotJson => "not-json",
            SettingsError::NotObject => "not-object",
            SettingsError::HooksNotObject => "hooks-not-object",
            SettingsError::EventNotList => "event-not-list",
            SettingsError::Duplicate => "duplicate-key",
        }
    }
}

/// A change to the settings file, before it is made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// The whole file after the change.
    #[serde(skip)]
    pub text: String,
    /// The matcher group added, pretty-printed, or `None` when nothing is.
    pub added: Option<String>,
    /// Ottid handlers taken out: old install paths, or all of them.
    pub removed: usize,
    /// Whether the file changes at all.
    pub changes: bool,
}

/// What the settings file says about Ottid's hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    /// The commands of Ottid's handlers in `hooks.PermissionRequest`.
    pub commands: Vec<String>,
    /// `disableAllHooks` is on in this file: no hook runs, Ottid's neither.
    pub hooks_disabled: bool,
}

/// An object's members, in order, with their values as written.
struct Members(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Members {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Walk;
        impl<'de> Visitor<'de> for Walk {
            type Value = Members;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Members, A::Error> {
                let mut members = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Box<RawValue>>()? {
                    members.push((key, value));
                }
                Ok(Members(members))
            }
        }
        deserializer.deserialize_map(Walk)
    }
}

impl Members {
    fn parse(raw: &str, not_object: SettingsError) -> Result<Self, SettingsError> {
        if !raw.trim_start().starts_with('{') {
            return Err(not_object);
        }
        serde_json::from_str(raw).map_err(|_| not_object)
    }

    /// The one member called `key`.
    fn find(&self, key: &str) -> Result<Option<usize>, SettingsError> {
        let mut found = self.0.iter().enumerate().filter(|(_, (k, _))| k == key);
        let first = found.next().map(|(i, _)| i);
        if found.next().is_some() {
            return Err(SettingsError::Duplicate);
        }
        Ok(first)
    }
}

/// A value to write: carried through as it was, or new.
enum Out {
    Raw(Box<RawValue>),
    New(String),
}

impl Out {
    fn text(&self) -> &str {
        match self {
            Out::Raw(raw) => raw.get(),
            Out::New(text) => text,
        }
    }
}

/// Pretty-print a new value at `depth` (two spaces a level, as Claude Code
/// writes its settings).
fn pretty(value: &Value, depth: usize) -> String {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    let pad = "  ".repeat(depth);
    text.replace('\n', &format!("\n{pad}"))
}

fn write_object(members: &[(String, Out)], depth: usize) -> String {
    if members.is_empty() {
        return "{}".to_string();
    }
    let pad = "  ".repeat(depth + 1);
    let mut text = String::from("{\n");
    for (i, (key, value)) in members.iter().enumerate() {
        let key = serde_json::to_string(key).unwrap_or_default();
        let _ = write!(text, "{pad}{key}: {}", value.text());
        text.push_str(if i + 1 < members.len() { ",\n" } else { "\n" });
    }
    text.push_str(&"  ".repeat(depth));
    text.push('}');
    text
}

fn write_array(items: &[Out], depth: usize) -> String {
    if items.is_empty() {
        return "[]".to_string();
    }
    let pad = "  ".repeat(depth + 1);
    let mut text = String::from("[\n");
    for (i, item) in items.iter().enumerate() {
        let _ = write!(text, "{pad}{}", item.text());
        text.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
    }
    text.push_str(&"  ".repeat(depth));
    text.push(']');
    text
}

/// The file split into what Ottid may change and what it carries through.
struct Parsed {
    bom: bool,
    newline: bool,
    top: Members,
    /// Index of `hooks` in `top`, and its members.
    hooks: Option<(usize, Members)>,
    /// Index of `PermissionRequest` in the hooks, and its groups.
    groups: Option<(usize, Vec<Box<RawValue>>)>,
    hooks_disabled: bool,
}

fn parse(current: Option<&str>) -> Result<Parsed, SettingsError> {
    parse_event(current, HOOK_EVENT)
}
fn parse_event(current: Option<&str>, event: &str) -> Result<Parsed, SettingsError> {
    let original = current.unwrap_or("{}");
    let bom = original.starts_with(UTF8_BOM);
    let text = original.strip_prefix(UTF8_BOM).unwrap_or(original);
    if text.trim().is_empty() {
        return parse_event(Some("{}"), event);
    }
    serde_json::from_str::<&RawValue>(text).map_err(|_| SettingsError::NotJson)?;
    let top = Members::parse(text, SettingsError::NotObject)?;
    let hooks_disabled = match top.find("disableAllHooks") {
        Ok(Some(i)) => top.0[i].1.get().trim() == "true",
        _ => false,
    };
    let hooks = match top.find("hooks")? {
        Some(i) => Some((
            i,
            Members::parse(top.0[i].1.get(), SettingsError::HooksNotObject)?,
        )),
        None => None,
    };
    let groups = match &hooks {
        Some((_, members)) => match members.find(event)? {
            Some(j) => {
                let raw = members.0[j].1.get();
                if !raw.trim_start().starts_with('[') {
                    return Err(SettingsError::EventNotList);
                }
                let list: Vec<Box<RawValue>> =
                    serde_json::from_str(raw).map_err(|_| SettingsError::EventNotList)?;
                Some((j, list))
            }
            None => None,
        },
        None => None,
    };
    Ok(Parsed {
        bom,
        newline: current.is_none_or(|text| text.ends_with('\n')),
        top,
        hooks,
        groups,
        hooks_disabled,
    })
}

/// What the file says about Ottid's hook.
pub fn find(current: Option<&str>) -> Result<Found, SettingsError> {
    find_with(current, is_ours)
}

pub(super) fn find_with(
    current: Option<&str>,
    owns: fn(&Value) -> bool,
) -> Result<Found, SettingsError> {
    let parsed = parse(current)?;
    let mut commands = Vec::new();
    for group in parsed.groups.iter().flat_map(|(_, groups)| groups) {
        let Ok(group) = serde_json::from_str::<Value>(group.get()) else {
            continue;
        };
        for handler in group
            .get("hooks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if owns(handler) {
                if let Some(command) = handler.get("command").and_then(Value::as_str) {
                    commands.push(command.to_string());
                }
            }
        }
    }
    Ok(Found {
        commands,
        hooks_disabled: parsed.hooks_disabled,
    })
}

/// The groups with Ottid's handlers taken out, and how many were.
fn without_ours(groups: Vec<Box<RawValue>>, owns: fn(&Value) -> bool) -> (Vec<Out>, usize) {
    let mut kept = Vec::new();
    let mut removed = 0;
    for raw in groups {
        let parsed = serde_json::from_str::<Value>(raw.get()).ok();
        let ours = parsed
            .as_ref()
            .and_then(|group| group.get("hooks"))
            .and_then(Value::as_array)
            .map(|handlers| handlers.iter().filter(|h| owns(h)).count())
            .unwrap_or(0);
        if ours == 0 {
            kept.push(Out::Raw(raw));
            continue;
        }
        removed += ours;
        let Some(mut group) = parsed else { continue };
        if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            handlers.retain(|h| !owns(h));
            if handlers.is_empty() {
                continue;
            }
        }
        // A group that still runs the user's own handlers stays.
        kept.push(Out::New(pretty(&group, 3)));
    }
    (kept, removed)
}

fn assemble(parsed: Parsed, groups: Vec<Out>, event: &str) -> String {
    let Parsed {
        bom,
        newline,
        top,
        hooks,
        groups: old_groups,
        ..
    } = parsed;
    let event_index = old_groups.map(|(j, _)| j);

    let (hooks_index, hooks_members) = match hooks {
        Some((i, members)) => (Some(i), members.0),
        None => (None, Vec::new()),
    };
    let mut hook_members: Vec<(String, Out)> = hooks_members
        .into_iter()
        .map(|(key, value)| (key, Out::Raw(value)))
        .collect();
    let list = (!groups.is_empty()).then(|| Out::New(write_array(&groups, 2)));
    match (event_index, list) {
        (Some(j), Some(list)) => hook_members[j].1 = list,
        (Some(j), None) => {
            hook_members.remove(j);
        }
        (None, Some(list)) => hook_members.push((event.to_string(), list)),
        (None, None) => {}
    }

    let mut top_members: Vec<(String, Out)> = top
        .0
        .into_iter()
        .map(|(key, value)| (key, Out::Raw(value)))
        .collect();
    let hooks_out = (!hook_members.is_empty()).then(|| Out::New(write_object(&hook_members, 1)));
    match (hooks_index, hooks_out) {
        (Some(i), Some(out)) => top_members[i].1 = out,
        (Some(i), None) => {
            top_members.remove(i);
        }
        (None, Some(out)) => top_members.push(("hooks".to_string(), out)),
        (None, None) => {}
    }

    let mut text = String::new();
    if bom {
        text.push_str(UTF8_BOM);
    }
    text.push_str(&write_object(&top_members, 0));
    if newline {
        text.push('\n');
    }
    text
}

/// Add Ottid's hook for `hook_exe`, replacing any earlier one.
pub fn plan_install(current: Option<&str>, hook_exe: &str) -> Result<Plan, SettingsError> {
    let text = serde_json::to_string_pretty(&group(hook_exe)).unwrap_or_default();
    plan_install_with(current, entry(hook_exe), &text, is_ours)
}

pub(super) fn plan_install_with(
    current: Option<&str>,
    wanted: Value,
    entry_text: &str,
    owns: fn(&Value) -> bool,
) -> Result<Plan, SettingsError> {
    plan_install_event(current, wanted, entry_text, owns, HOOK_EVENT)
}
pub(super) fn plan_install_event(
    current: Option<&str>,
    wanted: Value,
    entry_text: &str,
    owns: fn(&Value) -> bool,
    event: &str,
) -> Result<Plan, SettingsError> {
    let parsed = parse_event(current, event)?;
    let groups: Vec<Box<RawValue>> = parsed
        .groups
        .as_ref()
        .map(|(_, g)| g.clone())
        .unwrap_or_default();

    // Already there, exactly, and nothing else of Ottid's: no change.
    let ours: Vec<Value> = groups
        .iter()
        .filter_map(|raw| serde_json::from_str::<Value>(raw.get()).ok())
        .filter(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|handlers| handlers.iter().any(owns))
        })
        .collect();
    if ours.len() == 1 && ours[0] == wanted {
        return Ok(Plan {
            text: current.unwrap_or_default().to_string(),
            added: None,
            removed: 0,
            changes: false,
        });
    }

    let (mut kept, removed) = without_ours(groups, owns);
    kept.push(Out::New(entry_text.replace('\n', "\n      ")));
    let text = assemble(parsed, kept, event);
    Ok(Plan {
        changes: current != Some(text.as_str()),
        text,
        added: Some(entry_text.to_string()),
        removed,
    })
}

/// Take every Ottid handler out.
pub fn plan_uninstall(current: Option<&str>) -> Result<Plan, SettingsError> {
    plan_uninstall_with(current, is_ours)
}

pub(super) fn plan_uninstall_with(
    current: Option<&str>,
    owns: fn(&Value) -> bool,
) -> Result<Plan, SettingsError> {
    plan_uninstall_event(current, owns, HOOK_EVENT)
}
pub(super) fn plan_uninstall_event(
    current: Option<&str>,
    owns: fn(&Value) -> bool,
    event: &str,
) -> Result<Plan, SettingsError> {
    let Some(text) = current else {
        return Ok(Plan {
            text: String::new(),
            added: None,
            removed: 0,
            changes: false,
        });
    };
    let parsed = parse_event(Some(text), event)?;
    let groups: Vec<Box<RawValue>> = parsed
        .groups
        .as_ref()
        .map(|(_, g)| g.clone())
        .unwrap_or_default();
    let (kept, removed) = without_ours(groups, owns);
    if removed == 0 {
        return Ok(Plan {
            text: text.to_string(),
            added: None,
            removed: 0,
            changes: false,
        });
    }
    let out = assemble(parsed, kept, event);
    Ok(Plan {
        changes: out != text,
        text: out,
        added: None,
        removed,
    })
}

// ---- The file ----

/// The file's state when it was previewed: a hash of its bytes, or
/// `absent`. A change is applied only to the file it was previewed on.
pub fn fingerprint(current: Option<&[u8]>) -> String {
    match current {
        None => "absent".to_string(),
        Some(bytes) => {
            let digest = Sha256::digest(bytes);
            digest
                .iter()
                .fold(String::with_capacity(64), |mut text, byte| {
                    let _ = write!(text, "{byte:02x}");
                    text
                })
        }
    }
}

/// Why a change wasn't made.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{0}")]
    Settings(#[from] SettingsError),
    #[error("the settings file isn't UTF-8")]
    NotText,
    #[error("the settings file changed since the preview")]
    Changed,
    #[error("{0}")]
    Io(#[from] io::Error),
}

impl ApplyError {
    pub fn code(&self) -> &'static str {
        match self {
            ApplyError::Settings(err) => err.code(),
            ApplyError::NotText => "not-text",
            ApplyError::Changed => "changed",
            ApplyError::Io(_) => "io",
        }
    }
}

/// The file as it is now: its text, or `None` when there is none.
pub fn read(path: &Path) -> Result<Option<String>, ApplyError> {
    refuse_symlink(path)?;
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| ApplyError::NotText),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ApplyError::Io(err)),
    }
}

fn refuse_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(io::Error::other("symlinked settings are not supported"))
        }
        Ok(_) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn create_settings_file(path: &Path, original: &Path) -> io::Result<fs::File> {
    #[cfg(not(windows))]
    let file = {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)?
    };
    #[cfg(windows)]
    let file = super::endpoint::create_user_only(path)?;
    let metadata = match fs::metadata(original) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    if let Some(meta) = metadata {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            // Preserve stricter owner permissions, never expose settings to groups/others.
            file.set_permissions(fs::Permissions::from_mode(meta.mode() & 0o600))?;
        }
        #[cfg(not(unix))]
        file.set_permissions(meta.permissions())?;
        #[cfg(windows)]
        copy_windows_dacl(original, &file)?;
    }
    Ok(file)
}

#[cfg(windows)]
fn copy_windows_dacl(original: &Path, target: &fs::File) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{
        GetNamedSecurityInfoW, SetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    };
    let source = wide(original);
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: paths are NUL-terminated; the DACL remains in the live descriptor until copied.
    unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(source.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut descriptor,
        )
        .ok()
        .map_err(io::Error::other)?;
        let result = SetSecurityInfo(
            HANDLE(target.as_raw_handle()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl),
            None,
        )
        .ok()
        .map_err(io::Error::other);
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
    }
}

/// A change that was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Applied {
    /// The copy of the file from before, if there was a file.
    pub backup: Option<PathBuf>,
    pub changed: bool,
}

/// Plan against the file at `path` and make the change, if the file is
/// still the one with `expected` fingerprint.
pub fn apply(
    path: &Path,
    expected: &str,
    plan: impl FnOnce(Option<&str>) -> Result<Plan, SettingsError>,
) -> Result<Applied, ApplyError> {
    let current = read(path)?;
    if fingerprint(current.as_deref().map(str::as_bytes)) != expected {
        return Err(ApplyError::Changed);
    }
    let plan = plan(current.as_deref())?;
    if !plan.changes {
        return Ok(Applied {
            backup: None,
            changed: false,
        });
    }
    let backup = match current {
        Some(_) => Some(back_up(path)?),
        None => None,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let staging = path.with_extension(format!("json.ottid-{}.tmp", std::process::id()));
    let mut file = create_settings_file(&staging, path)?;
    let written =
        io::Write::write_all(&mut file, plan.text.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    let replace = written.and_then(|()| {
        refuse_symlink(path)?;
        fs::rename(&staging, path)
    });
    if let Err(err) = replace {
        let _ = fs::remove_file(&staging);
        return Err(ApplyError::Io(err));
    }
    Ok(Applied {
        backup,
        changed: true,
    })
}

/// Copy the file to `settings.json.ottid-backup-<seconds>`, next to it,
/// never over an earlier backup.
fn back_up(path: &Path) -> io::Result<PathBuf> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".to_string());
    for n in 0..100 {
        let suffix = if n == 0 {
            String::new()
        } else {
            format!("-{n}")
        };
        let backup = path.with_file_name(format!("{name}.ottid-backup-{seconds}{suffix}"));
        match create_settings_file(&backup, path) {
            Ok(mut file) => {
                let bytes = fs::read(path)?;
                io::Write::write_all(&mut file, &bytes)?;
                file.sync_all()?;
                return Ok(backup);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::other("no free backup name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXE: &str = r"C:\Program Files\Ottid\binaries\ottid-hook\ottid-hook.exe";

    /// A user settings file as Claude Code writes it (two-space JSON), with
    /// keys Ottid must not touch, Hebrew, and hooks of the user's own.
    const USER: &str = r#"{
  "model": "opus",
  "permissions": {
    "allow": [
      "Bash(npm test)",
      "Read(~/מסמכים/**)"
    ],
    "defaultMode": "default"
  },
  "env": {
    "GREETING": "שלום עולם"
  },
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.claude/hooks/log.sh"
          }
        ]
      }
    ],
    "PermissionRequest": [
      {
        "matcher": "ExitPlanMode",
        "hooks": [
          {
            "type": "command",
            "command": "notify-send plan"
          }
        ]
      }
    ]
  },
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline.sh"
  }
}
"#;

    fn groups(text: &str) -> Vec<Value> {
        let value: Value = serde_json::from_str(text.trim_start_matches(UTF8_BOM)).unwrap();
        value["hooks"]["PermissionRequest"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn the_entry_runs_the_hook_in_exec_form_for_every_tool() {
        let entry = entry(EXE);
        assert_eq!(entry["matcher"], "*");
        let handler = &entry["hooks"][0];
        assert_eq!(handler["type"], "command");
        // Exec form: no shell, so the space in "Program Files" needs no quoting.
        assert_eq!(handler["command"], EXE);
        assert_eq!(handler["args"], json!([]));
        assert_eq!(handler["statusMessage"], STATUS_MESSAGE);
        // Longer than ottid-hook's own wait, so it gives up first.
        let timeout = handler["timeout"].as_u64().unwrap();
        assert!(timeout > super::super::CLIENT_WAIT.as_secs());
        // No token, ever.
        assert!(!entry.to_string().to_lowercase().contains("token"));
    }

    #[test]
    fn a_missing_file_gets_just_the_hook() {
        let plan = plan_install(None, EXE).unwrap();
        assert!(plan.changes);
        assert_eq!(plan.removed, 0);
        let value: Value = serde_json::from_str(&plan.text).unwrap();
        assert_eq!(
            value,
            json!({ "hooks": { "PermissionRequest": [entry(EXE)] } })
        );
        assert!(plan.text.ends_with("}\n"));
        let added = plan.added.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&added).unwrap(), entry(EXE));
        // Written in the order a reader expects: what it matches, then what runs.
        assert!(added.find("\"matcher\"").unwrap() < added.find("\"hooks\"").unwrap());
        assert!(added.find("\"type\"").unwrap() < added.find("\"command\"").unwrap());
    }

    #[test]
    fn install_appends_and_carries_everything_else_through_unchanged() {
        let plan = plan_install(Some(USER), EXE).unwrap();
        assert!(plan.changes);
        // The user's group stays first and whole; Ottid's is appended.
        let after = groups(&plan.text);
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], groups(USER)[0]);
        assert_eq!(after[1], entry(EXE));

        // Every other key, byte for byte and in order.
        for unchanged in [
            "  \"model\": \"opus\",\n",
            "  \"permissions\": {\n    \"allow\": [\n      \"Bash(npm test)\",\n      \"Read(~/מסמכים/**)\"\n    ],\n    \"defaultMode\": \"default\"\n  },\n",
            "  \"env\": {\n    \"GREETING\": \"שלום עולם\"\n  },\n",
            "    \"PreToolUse\": [\n      {\n        \"matcher\": \"Bash\",",
            "  \"statusLine\": {\n    \"type\": \"command\",\n    \"command\": \"bash ~/.claude/statusline.sh\"\n  }\n}\n",
        ] {
            assert!(plan.text.contains(unchanged), "lost: {unchanged}\n---\n{}", plan.text);
        }
        let keys: Vec<String> = serde_json::from_str::<serde_json::Map<String, Value>>(&plan.text)
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys.len(), 5);
        let before: Value = serde_json::from_str(USER).unwrap();
        let after: Value = serde_json::from_str(&plan.text).unwrap();
        for key in ["model", "permissions", "env", "statusLine"] {
            assert_eq!(before[key], after[key], "{key}");
        }
        assert_eq!(before["hooks"]["PreToolUse"], after["hooks"]["PreToolUse"]);
    }

    #[test]
    fn install_then_uninstall_gives_the_file_back_byte_for_byte() {
        let installed = plan_install(Some(USER), EXE).unwrap().text;
        let removed = plan_uninstall(Some(&installed)).unwrap();
        assert!(removed.changes);
        assert_eq!(removed.removed, 1);
        assert_eq!(removed.text, USER);

        // A file with no hooks at all loses the keys Ottid added.
        let plain = "{\n  \"model\": \"sonnet\"\n}\n";
        let installed = plan_install(Some(plain), EXE).unwrap().text;
        assert_eq!(plan_uninstall(Some(&installed)).unwrap().text, plain);
    }

    #[test]
    fn installing_again_changes_nothing() {
        let installed = plan_install(Some(USER), EXE).unwrap().text;
        let again = plan_install(Some(&installed), EXE).unwrap();
        assert!(!again.changes);
        assert_eq!(again.text, installed);
        assert_eq!(again.added, None);
    }

    #[test]
    fn an_old_install_path_is_replaced() {
        let old = plan_install(Some(USER), r"D:\Old\ottid-hook.exe")
            .unwrap()
            .text;
        let plan = plan_install(Some(&old), EXE).unwrap();
        assert!(plan.changes);
        assert_eq!(plan.removed, 1);
        let after = groups(&plan.text);
        assert_eq!(after.len(), 2);
        assert_eq!(after[1], entry(EXE));
        assert_eq!(
            find(Some(&plan.text)).unwrap().commands,
            vec![EXE.to_string()]
        );
    }

    #[test]
    fn uninstall_keeps_the_users_handlers_in_a_shared_group() {
        let shared = r#"{
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": "C:\\Ottid\\ottid-hook.exe", "args": [] },
          { "type": "command", "command": "my-logger" }
        ]
      }
    ]
  }
}
"#;
        let plan = plan_uninstall(Some(shared)).unwrap();
        assert_eq!(plan.removed, 1);
        let after = groups(&plan.text);
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0]["hooks"],
            json!([{ "type": "command", "command": "my-logger" }])
        );
    }

    #[test]
    fn uninstall_with_nothing_of_ottids_changes_nothing() {
        let plan = plan_uninstall(Some(USER)).unwrap();
        assert!(!plan.changes);
        assert_eq!(plan.text, USER);
        assert!(!plan_uninstall(None).unwrap().changes);
    }

    #[test]
    fn find_reports_ottids_handlers_and_disabled_hooks() {
        assert_eq!(find(Some(USER)).unwrap().commands, Vec::<String>::new());
        assert!(!find(Some(USER)).unwrap().hooks_disabled);
        let installed = plan_install(Some(USER), EXE).unwrap().text;
        assert_eq!(
            find(Some(&installed)).unwrap().commands,
            vec![EXE.to_string()]
        );
        let disabled = "{ \"disableAllHooks\": true }";
        assert!(find(Some(disabled)).unwrap().hooks_disabled);
        assert_eq!(find(None).unwrap().commands, Vec::<String>::new());
    }

    #[test]
    fn only_the_hook_binary_counts_as_ottids() {
        for ours in [
            EXE,
            "/opt/ottid/ottid-hook",
            "\"C:\\Program Files\\Ottid\\OTTID-HOOK.EXE\"",
            "ottid-hook",
        ] {
            assert!(
                is_ours(&json!({ "type": "command", "command": ours })),
                "{ours}"
            );
        }
        for not_ours in [
            "ottid-hooks",
            "my-ottid-hook-wrapper.sh",
            "C:\\ottid-hook\\other.exe",
            "ottid-hook.exe.bak",
        ] {
            assert!(
                !is_ours(&json!({ "type": "command", "command": not_ours })),
                "{not_ours}"
            );
        }
        assert!(!is_ours(&json!({ "type": "http", "command": EXE })));
    }

    #[test]
    fn a_file_ottid_cant_read_safely_is_refused() {
        assert_eq!(
            plan_install(Some("{ \"a\": 1, // comment\n }"), EXE),
            Err(SettingsError::NotJson)
        );
        assert_eq!(
            plan_install(Some("{ \"a\": "), EXE),
            Err(SettingsError::NotJson)
        );
        assert_eq!(
            plan_install(Some("[1, 2]"), EXE),
            Err(SettingsError::NotObject)
        );
        assert_eq!(
            plan_install(Some("{ \"hooks\": [] }"), EXE),
            Err(SettingsError::HooksNotObject)
        );
        assert_eq!(
            plan_install(Some("{ \"hooks\": { \"PermissionRequest\": {} } }"), EXE),
            Err(SettingsError::EventNotList)
        );
        assert_eq!(
            plan_install(Some("{ \"hooks\": {}, \"hooks\": {} }"), EXE),
            Err(SettingsError::Duplicate)
        );
        assert_eq!(
            plan_uninstall(Some("not json")),
            Err(SettingsError::NotJson)
        );
    }

    #[test]
    fn a_minified_or_empty_file_still_works() {
        let minified = r#"{"model":"opus","hooks":{"Stop":[]}}"#;
        let plan = plan_install(Some(minified), EXE).unwrap();
        let value: Value = serde_json::from_str(&plan.text).unwrap();
        assert_eq!(value["model"], "opus");
        assert_eq!(value["hooks"]["Stop"], json!([]));
        assert_eq!(value["hooks"]["PermissionRequest"][0], entry(EXE));
        // No trailing newline before, none after.
        assert!(!plan.text.ends_with('\n'));

        let empty = plan_install(Some(""), EXE).unwrap();
        assert_eq!(groups(&empty.text), vec![entry(EXE)]);
    }

    #[test]
    fn a_byte_order_mark_is_kept() {
        let with_bom = format!("{UTF8_BOM}{{\n  \"model\": \"opus\"\n}}\n");
        let plan = plan_install(Some(&with_bom), EXE).unwrap();
        assert!(plan.text.starts_with(UTF8_BOM));
        assert_eq!(groups(&plan.text), vec![entry(EXE)]);
        assert_eq!(plan_uninstall(Some(&plan.text)).unwrap().text, with_bom);
    }

    #[test]
    fn the_settings_file_follows_claude_config_dir() {
        let home = PathBuf::from("home");
        assert_eq!(
            settings_path_from(None, Some(home.clone())),
            Some(home.join(".claude").join("settings.json"))
        );
        assert_eq!(
            settings_path_from(Some("cfg".into()), Some(home.clone())),
            Some(PathBuf::from("cfg").join("settings.json"))
        );
        assert_eq!(
            settings_path_from(Some("".into()), Some(home.clone())),
            Some(home.join(".claude").join("settings.json"))
        );
        assert_eq!(settings_path_from(None, None), None);
    }

    #[test]
    fn apply_backs_up_writes_and_refuses_a_file_changed_since_the_preview() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, USER).unwrap();
        let seen = fingerprint(Some(USER.as_bytes()));

        // Changed under the preview: nothing is written.
        fs::write(&path, "{}").unwrap();
        let refused = apply(&path, &seen, |current| plan_install(current, EXE));
        assert!(matches!(refused, Err(ApplyError::Changed)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");

        fs::write(&path, USER).unwrap();
        let applied = apply(&path, &seen, |current| plan_install(current, EXE)).unwrap();
        assert!(applied.changed);
        let backup = applied.backup.expect("a backup of the old file");
        assert_eq!(fs::read_to_string(&backup).unwrap(), USER);
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings.json.ottid-backup-"));
        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(
            find(Some(&written)).unwrap().commands,
            vec![EXE.to_string()]
        );
        assert!(!dir.path().join("settings.json.ottid-tmp").exists());

        // Uninstall from what is there now; a second backup doesn't
        // overwrite the first.
        let now = fingerprint(Some(written.as_bytes()));
        let undone = apply(&path, &now, plan_uninstall).unwrap();
        assert_ne!(undone.backup.as_ref(), Some(&backup));
        assert_eq!(fs::read_to_string(&path).unwrap(), USER);
    }

    #[test]
    fn apply_creates_a_missing_file_without_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".claude").join("settings.json");
        let applied = apply(&path, "absent", |current| plan_install(current, EXE)).unwrap();
        assert!(applied.changed);
        assert_eq!(applied.backup, None);
        assert_eq!(
            groups(&fs::read_to_string(&path).unwrap()),
            vec![entry(EXE)]
        );
    }

    #[test]
    fn an_existing_staging_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let staging = path.with_extension(format!("json.ottid-{}.tmp", std::process::id()));
        fs::write(&staging, "leave me alone").unwrap();
        assert!(apply(&path, "absent", |current| plan_install(current, EXE)).is_err());
        assert_eq!(fs::read_to_string(staging).unwrap(), "leave me alone");
        assert!(!path.exists());
    }

    #[cfg(windows)]
    #[test]
    fn replacement_and_backup_preserve_the_windows_dacl() {
        use std::os::windows::io::AsRawHandle;
        use windows::core::PWSTR;
        use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
            SE_FILE_OBJECT,
        };
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
        fn dacl(path: &Path) -> String {
            let file = fs::File::open(path).unwrap();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            let mut text = PWSTR::null();
            // SAFETY: live handle, valid out-parameters, both allocations freed below.
            unsafe {
                GetSecurityInfo(
                    HANDLE(file.as_raw_handle()),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    None,
                    None,
                    Some(&mut descriptor),
                )
                .ok()
                .unwrap();
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    None,
                )
                .unwrap();
                // SetSecurityInfo marks inheritance as processed (AI), even
                // for a protected DACL with the same explicit entries.
                let result = text.to_string().unwrap().replacen("D:PAI", "D:P", 1);
                let _ = LocalFree(Some(HLOCAL(text.0.cast())));
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
                result
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut file = super::super::endpoint::create_user_only(&path).unwrap();
        io::Write::write_all(&mut file, USER.as_bytes()).unwrap();
        drop(file);
        let original = dacl(&path);
        let applied = apply(&path, &fingerprint(Some(USER.as_bytes())), |current| {
            plan_install(current, EXE)
        })
        .unwrap();
        assert_eq!(dacl(&path), original);
        assert_eq!(dacl(&applied.backup.unwrap()), original);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_settings_including_dangling_links_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.json");
        let path = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(read(&path).is_err());
        assert!(apply(&path, "absent", |current| plan_install(current, EXE)).is_err());
        fs::write(&target, USER).unwrap();
        assert!(
            apply(&path, &fingerprint(Some(USER.as_bytes())), |current| {
                plan_install(current, EXE)
            })
            .is_err()
        );
        assert_eq!(fs::read_to_string(target).unwrap(), USER);
    }

    #[cfg(unix)]
    #[test]
    fn replacement_and_backup_keep_settings_private() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, USER).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let applied = apply(&path, &fingerprint(Some(USER.as_bytes())), |current| {
            plan_install(current, EXE)
        })
        .unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(applied.backup.unwrap()).unwrap().mode() & 0o777,
            0o600
        );
    }
}
