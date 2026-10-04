// The Windows named-pipe listener is derived from Coucou
// (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis Raillé, by way of
// the hardened fork https://github.com/YojoTan/coucou: the explicit DACL
// that admits only the user, `first_pipe_instance` and
// `reject_remote_clients`, and a fresh instance per accepted connection
// (windows/src-tauri/src/pipe.rs). See THIRD-PARTY-NOTICES.

//! Where the bridge lives, and who can reach it.
//!
//! - **The bridge file** (`bridge.json`) tells `ottid-hook` the endpoint and
//!   the token. Only the user can read it: on Windows it is created with a
//!   protected DACL that has one entry, for the user's SID; on Unix it is
//!   0600 in a 0700 directory the user owns. It is written to a temporary
//!   name and renamed, so a reader never sees half of it, and deleted when
//!   Ottid stops.
//! - **The endpoint.** On Windows, a named pipe with a random name, the same
//!   user-only DACL, and remote clients refused; the first instance is
//!   created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so Ottid never serves on a
//!   pipe somebody else opened first. On Unix, a 0600 socket in the 0700
//!   directory.
//! - **The client** connects only to an endpoint of that form: a local pipe
//!   with Ottid's prefix, or the socket next to the bridge file.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::auth::Token;
use super::VERSION;

/// The bridge file's name, in [`default_dir`].
pub const BRIDGE_FILE: &str = "bridge.json";

/// The bridge file is a few hundred bytes; a bigger one isn't Ottid's.
const MAX_BRIDGE_FILE: u64 = 4096;

/// Tauri's identifier, the per-user directory every Ottid data dir is named
/// after.
const IDENTIFIER: &str = "app.ottid.desktop";

/// Where Ottid keeps the bridge file (and, on Unix, the socket).
///
/// - Windows: `%LOCALAPPDATA%\app.ottid.desktop\agent-bridge`
/// - macOS: `~/Library/Application Support/app.ottid.desktop/agent-bridge`
/// - Linux: `$XDG_RUNTIME_DIR/app.ottid.desktop/agent-bridge`, else under
///   `$XDG_DATA_HOME` (or `~/.local/share`)
pub fn default_dir() -> Option<PathBuf> {
    base_dir().map(|base| base.join(IDENTIFIER).join("agent-bridge"))
}

#[cfg(windows)]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn base_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

/// What the bridge file holds.
#[derive(Debug)]
pub struct BridgeFile {
    pub endpoint: String,
    pub token: Token,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    v: u32,
    endpoint: String,
    token: String,
}

/// Why the bridge file can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeFileError {
    /// Not there: Ottid isn't running, or the bridge isn't on.
    Missing,
    /// There, but not a bridge file this client can use.
    Invalid,
}

impl BridgeFile {
    pub fn read(path: &Path) -> Result<Self, BridgeFileError> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Err(BridgeFileError::Missing)
            }
            Err(_) => return Err(BridgeFileError::Invalid),
        };
        let mut text = Vec::new();
        file.take(MAX_BRIDGE_FILE + 1)
            .read_to_end(&mut text)
            .map_err(|_| BridgeFileError::Invalid)?;
        if text.len() as u64 > MAX_BRIDGE_FILE {
            return Err(BridgeFileError::Invalid);
        }
        let stored: Stored = serde_json::from_slice(&text).map_err(|_| BridgeFileError::Invalid)?;
        if stored.v != VERSION {
            return Err(BridgeFileError::Invalid);
        }
        let token = Token::from_hex(&stored.token).ok_or(BridgeFileError::Invalid)?;
        Ok(Self {
            endpoint: stored.endpoint,
            token,
        })
    }
}

/// Write the bridge file in `dir`, readable by this user only. Returns its
/// path.
pub fn write_bridge_file(dir: &Path, endpoint: &str, token: &Token) -> io::Result<PathBuf> {
    let text = serde_json::to_vec(&Stored {
        v: VERSION,
        endpoint: endpoint.to_string(),
        token: token.to_hex(),
    })?;
    let path = dir.join(BRIDGE_FILE);
    let staging = dir.join(format!("{BRIDGE_FILE}.{}.tmp", std::process::id()));
    remove_if_present(&staging)?;
    let mut file = create_user_only(&staging)?;
    let written = file.write_all(&text).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = written.and_then(|()| fs::rename(&staging, &path)) {
        let _ = fs::remove_file(&staging);
        return Err(err);
    }
    Ok(path)
}

/// Delete the bridge file, unless another Ottid has written its own since.
pub fn remove_bridge_file(path: &Path, endpoint: &str) {
    match BridgeFile::read(path) {
        Ok(file) if file.endpoint == endpoint => {
            let _ = fs::remove_file(path);
        }
        Ok(_) | Err(BridgeFileError::Missing) => {}
        Err(BridgeFileError::Invalid) => {
            let _ = fs::remove_file(path);
        }
    }
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

// ---- Windows ----

#[cfg(windows)]
pub use self::windows_impl::{connect, create_user_only, prepare_dir, Listener, Stream};

#[cfg(windows)]
mod windows_impl {
    use std::ffi::c_void;
    use std::fs;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::path::Path;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    };
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use crate::agent_bridge::auth::Token;

    /// Every bridge pipe's name starts with this; the rest is random.
    pub const PIPE_PREFIX: &str = r"\\.\pipe\ottid-agent-";

    /// The client's end of the pipe.
    pub type Stream = NamedPipeClient;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The SDDL of a protected DACL with one entry, granting `rights` to
    /// `sid`. Protected (`P`): nothing is inherited from the folder, so no
    /// other account or group gets in.
    pub fn user_only_sddl(sid: &str, rights: &str) -> String {
        format!("D:P(A;;{rights};;;{sid})")
    }

    /// The SID of the user this process runs as, e.g. `S-1-5-21-…-1001`.
    pub fn current_user_sid() -> io::Result<String> {
        // SAFETY: the token handle is closed below; the buffer is sized by
        // the first call and u64-aligned for TOKEN_USER; the string the SID
        // is converted to is freed with LocalFree.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
                .map_err(io::Error::other)?;
            let mut len = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
            let mut buffer = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let read = GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                len,
                &mut len,
            );
            let _ = CloseHandle(token);
            read.map_err(io::Error::other)?;
            let user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(io::Error::other)?;
            let sid = text.to_string().map_err(io::Error::other);
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            sid
        }
    }

    /// A security descriptor built from SDDL, freed on drop.
    struct Descriptor(PSECURITY_DESCRIPTOR);

    impl Descriptor {
        fn user_only(rights: &str) -> io::Result<Self> {
            let sddl = wide(&user_only_sddl(&current_user_sid()?, rights));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: a NUL-terminated SDDL string and an out-parameter.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
            }
            .map_err(io::Error::other)?;
            Ok(Self(descriptor))
        }

        fn attributes(&self) -> SECURITY_ATTRIBUTES {
            SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: self.0 .0,
                bInheritHandle: false.into(),
            }
        }
    }

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }

    /// Create a new file only this user can open. Fails if it exists.
    pub fn create_user_only(path: &Path) -> io::Result<fs::File> {
        let descriptor = Descriptor::user_only("FA")?;
        let attributes = descriptor.attributes();
        let name: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: a NUL-terminated path; `attributes` and its descriptor
        // outlive the call, which copies the descriptor onto the new file.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                Some(&attributes),
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .map_err(|err| io::Error::from_raw_os_error(err.code().0 & 0xFFFF))?;
        // SAFETY: a fresh, owned file handle.
        Ok(unsafe { fs::File::from_raw_handle(handle.0 as RawHandle) })
    }

    /// The directory holds only the bridge file, whose own DACL keeps it to
    /// the user.
    pub fn prepare_dir(dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)
    }

    /// A pipe instance only this user can open, refusing remote clients.
    pub(crate) fn create_instance(name: &str, first: bool) -> io::Result<NamedPipeServer> {
        let descriptor = Descriptor::user_only("GA")?;
        let mut attributes = descriptor.attributes();
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true);
        // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose
        // descriptor outlives the call; CreateNamedPipeW copies it.
        unsafe {
            options.create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
            )
        }
    }

    /// The listening pipe. Each accepted connection takes the waiting
    /// instance, and a new one is opened for the next client.
    pub struct Listener {
        name: String,
        next: NamedPipeServer,
        broken: bool,
    }

    impl Listener {
        pub fn bind(_dir: &Path) -> io::Result<Self> {
            let suffix = Token::generate()?.to_hex();
            let name = format!("{PIPE_PREFIX}{}", &suffix[..32]);
            let next = create_instance(&name, true)?;
            Ok(Self {
                name,
                next,
                broken: false,
            })
        }

        pub fn endpoint(&self) -> &str {
            &self.name
        }

        pub async fn accept(&mut self) -> io::Result<NamedPipeServer> {
            let connected = self.next.connect().await;
            let fresh = match create_instance(&self.name, false) {
                Ok(fresh) => fresh,
                Err(err) => {
                    self.broken = true;
                    return Err(err);
                }
            };
            let server = std::mem::replace(&mut self.next, fresh);
            connected.map(|()| server)
        }

        /// No instance is left to accept on.
        pub fn is_broken(&self) -> bool {
            self.broken
        }
    }

    /// Connect to the bridge pipe, waiting out a busy moment. Only a local
    /// pipe with Ottid's prefix is accepted. The pipe client's default
    /// security quality of service (identification only) keeps the
    /// listener from acting as the hook's user.
    pub async fn connect(endpoint: &str, _bridge_file: &Path) -> io::Result<Stream> {
        let suffix = endpoint
            .strip_prefix(PIPE_PREFIX)
            .ok_or_else(|| io::Error::other("not an Ottid pipe"))?;
        if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::other("not an Ottid pipe"));
        }
        loop {
            match ClientOptions::new().open(endpoint) {
                Ok(client) => return Ok(client),
                Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {}
                Err(err) => return Err(err),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

// ---- Unix ----

#[cfg(unix)]
pub use self::unix_impl::{connect, create_user_only, prepare_dir, Listener, Stream};

#[cfg(unix)]
mod unix_impl {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{
        DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt,
    };
    use std::path::{Path, PathBuf};

    use tokio::net::{UnixListener, UnixStream};

    /// The socket's name in the bridge directory.
    const SOCKET: &str = "agent.sock";

    pub type Stream = UnixStream;

    /// Create the directory 0700, or check that the one there is this
    /// user's and closed to everyone else.
    pub fn prepare_dir(dir: &Path) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let meta = fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Err(io::Error::other("the bridge directory isn't a directory"));
        }
        // SAFETY: geteuid has no preconditions.
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::other(
                "the bridge directory belongs to another user",
            ));
        }
        if meta.mode() & 0o077 != 0 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Create a new 0600 file. Fails if it exists.
    pub fn create_user_only(path: &Path) -> io::Result<fs::File> {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    }

    pub struct Listener {
        path: PathBuf,
        endpoint: String,
        inner: UnixListener,
    }

    impl Listener {
        pub fn bind(dir: &Path) -> io::Result<Self> {
            let path = dir.join(SOCKET);
            // A socket left by an Ottid that didn't exit cleanly.
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if meta.file_type().is_socket() {
                    fs::remove_file(&path)?;
                } else {
                    return Err(io::Error::other("something else is where the socket goes"));
                }
            }
            let inner = UnixListener::bind(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            let endpoint = path
                .to_str()
                .ok_or_else(|| io::Error::other("the socket path isn't UTF-8"))?
                .to_string();
            Ok(Self {
                path,
                endpoint,
                inner,
            })
        }

        pub fn endpoint(&self) -> &str {
            &self.endpoint
        }

        pub async fn accept(&mut self) -> io::Result<UnixStream> {
            self.inner.accept().await.map(|(stream, _)| stream)
        }

        pub fn is_broken(&self) -> bool {
            false
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    /// Connect to the socket, which must sit next to the bridge file.
    pub async fn connect(endpoint: &str, bridge_file: &Path) -> io::Result<Stream> {
        let path = Path::new(endpoint);
        let expected = bridge_file.parent().map(|dir| dir.join(SOCKET));
        if expected.as_deref() != Some(path) {
            return Err(io::Error::other("not the bridge socket"));
        }
        UnixStream::connect(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_file_round_trips_and_is_deleted_only_by_its_writer() {
        let dir = tempfile::tempdir().unwrap();
        let token = Token::generate().unwrap();
        let path = write_bridge_file(dir.path(), "endpoint-a", &token).unwrap();
        assert_eq!(path, dir.path().join(BRIDGE_FILE));
        let read = BridgeFile::read(&path).unwrap();
        assert_eq!(read.endpoint, "endpoint-a");
        assert_eq!(read.token.to_hex(), token.to_hex());

        // A later Ottid wrote its own: an earlier one's exit leaves it.
        write_bridge_file(dir.path(), "endpoint-b", &token).unwrap();
        remove_bridge_file(&path, "endpoint-a");
        assert!(path.exists());
        remove_bridge_file(&path, "endpoint-b");
        assert!(!path.exists());
        // No staging file is left behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_missing_or_foreign_bridge_file_is_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BRIDGE_FILE);
        assert_eq!(
            BridgeFile::read(&path).unwrap_err(),
            BridgeFileError::Missing
        );
        for junk in [
            "not json",
            r#"{"v":1,"endpoint":"x"}"#,
            r#"{"v":1,"endpoint":"x","token":"short"}"#,
            r#"{"v":2,"endpoint":"x","token":"0000000000000000000000000000000000000000000000000000000000000000"}"#,
            r#"{"v":1,"endpoint":"x","token":"0000000000000000000000000000000000000000000000000000000000000000","extra":1}"#,
        ] {
            fs::write(&path, junk).unwrap();
            assert_eq!(
                BridgeFile::read(&path).unwrap_err(),
                BridgeFileError::Invalid,
                "{junk}"
            );
        }
        fs::write(&path, vec![b' '; MAX_BRIDGE_FILE as usize + 1]).unwrap();
        assert_eq!(
            BridgeFile::read(&path).unwrap_err(),
            BridgeFileError::Invalid
        );
    }

    #[test]
    fn the_default_dir_is_under_ottids_own_folder() {
        if let Some(dir) = default_dir() {
            assert!(dir.ends_with(Path::new(IDENTIFIER).join("agent-bridge")));
        }
    }

    #[cfg(windows)]
    mod windows {
        use super::super::windows_impl::{current_user_sid, user_only_sddl, PIPE_PREFIX};
        use super::super::*;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{PCWSTR, PWSTR};
        use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SE_KERNEL_OBJECT,
        };
        use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};

        fn to_sddl(descriptor: PSECURITY_DESCRIPTOR) -> String {
            let mut text = PWSTR::null();
            // SAFETY: a descriptor from Get*SecurityInfo, freed after.
            unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    None,
                )
                .unwrap();
                let sddl = text.to_string().unwrap();
                let _ = LocalFree(Some(HLOCAL(text.0.cast())));
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
                sddl
            }
        }

        fn file_dacl(path: &Path) -> String {
            let name: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: a NUL-terminated path and an out-parameter.
            let status = unsafe {
                GetNamedSecurityInfoW(
                    PCWSTR(name.as_ptr()),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    None,
                    None,
                    &mut descriptor,
                )
            };
            assert!(status.is_ok(), "GetNamedSecurityInfoW: {status:?}");
            to_sddl(descriptor)
        }

        fn handle_dacl(handle: HANDLE) -> String {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: a live handle and an out-parameter.
            let status = unsafe {
                GetSecurityInfo(
                    handle,
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    None,
                    None,
                    Some(&mut descriptor),
                )
            };
            assert!(status.is_ok(), "GetSecurityInfo: {status:?}");
            to_sddl(descriptor)
        }

        /// One protected entry, allowing this user, and nothing else.
        fn assert_user_only(sddl: &str) {
            let sid = current_user_sid().unwrap();
            assert!(sddl.starts_with("D:P"), "not protected: {sddl}");
            assert_eq!(sddl.matches('(').count(), 1, "more than one entry: {sddl}");
            assert!(
                sddl.contains(&format!(";;;{sid})")),
                "not this user: {sddl}"
            );
            assert!(sddl.contains("(A;"), "not an allow entry: {sddl}");
            for broad in ["WD", "AN", "AU", "BU", "BA", "SY"] {
                assert!(
                    !sddl.contains(&format!(";;;{broad})")),
                    "{broad} is granted: {sddl}"
                );
            }
        }

        #[test]
        fn the_sddl_grants_one_sid_and_nothing_else() {
            assert_eq!(
                user_only_sddl("S-1-5-21-1-2-3-1001", "GA"),
                "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)"
            );
            assert!(current_user_sid().unwrap().starts_with("S-1-"));
        }

        #[test]
        fn the_bridge_file_is_readable_by_this_user_only() {
            let dir = tempfile::tempdir().unwrap();
            let token = Token::generate().unwrap();
            let path = write_bridge_file(dir.path(), "x", &token).unwrap();
            assert_user_only(&file_dacl(&path));
            // This user can still read it.
            assert!(BridgeFile::read(&path).is_ok());
        }

        #[test]
        fn the_pipe_admits_this_user_only_and_cannot_be_opened_twice() {
            use std::os::windows::io::AsRawHandle;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let dir = tempfile::tempdir().unwrap();
                let listener = Listener::bind(dir.path()).unwrap();
                let name = listener.endpoint().to_string();
                assert!(name.starts_with(PIPE_PREFIX));
                // Somebody already serving under the name: a first instance
                // is refused, so Ottid can't be made to serve on it.
                assert!(windows_impl::create_instance(&name, true).is_err());
                // This user still connects.
                let client = connect(&name, dir.path())
                    .await
                    .expect("our own account connects");
                let sddl = handle_dacl(HANDLE(client.as_raw_handle()));
                assert_user_only(&sddl);
                drop(client);
                drop(listener);
            });
        }

        #[test]
        fn the_client_connects_only_to_an_ottid_pipe() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let here = Path::new(".");
                for endpoint in [
                    r"\\server\pipe\ottid-agent-00",
                    r"\\.\pipe\something-else",
                    r"\\.\pipe\ottid-agent-",
                    r"\\.\pipe\ottid-agent-..\x",
                    r"C:\Windows\notepad.exe",
                ] {
                    assert!(connect(endpoint, here).await.is_err(), "{endpoint}");
                }
            });
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn the_directory_file_and_socket_are_this_users_only() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("agent-bridge");
            prepare_dir(&dir).unwrap();
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );

            let path = write_bridge_file(&dir, "x", &Token::generate().unwrap()).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );

            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = Listener::bind(&dir).unwrap();
                let socket = Path::new(listener.endpoint());
                assert_eq!(
                    fs::metadata(socket).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            });
        }

        #[test]
        fn an_open_directory_is_closed_again() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("agent-bridge");
            fs::create_dir(&dir).unwrap();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
            prepare_dir(&dir).unwrap();
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }

        #[test]
        fn the_client_connects_only_to_the_socket_next_to_the_bridge_file() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let file = Path::new("/tmp/ottid-test/bridge.json");
                assert!(connect("/tmp/elsewhere/agent.sock", file).await.is_err());
                assert!(connect("relative.sock", file).await.is_err());
            });
        }
    }
}
