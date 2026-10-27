use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::operation::{UndoGroup, UndoOperation, UndoStack};
use crate::selection::{CaretSnapshot, Selection, TextPosition};

/// Read a file. Returns the raw bytes as-is.
pub fn read_file(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path)
}

/// Prepare buffer bytes for saving: strip trailing spaces and tabs from every
/// line and ensure a trailing newline. Works on raw bytes so invalid UTF-8,
/// non-ASCII whitespace such as NBSP, and CRLF line endings survive a save
/// untouched.
pub fn clean_for_write(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 1);
    for (index, line) in data.split(|&b| b == b'\n').enumerate() {
        if index > 0 {
            out.push(b'\n');
        }
        let (body, has_carriage_return) = match line.strip_suffix(b"\r") {
            Some(body) => (body, true),
            None => (line, false),
        };
        let end = body
            .iter()
            .rposition(|&b| b != b' ' && b != b'\t')
            .map_or(0, |last| last + 1);
        out.extend_from_slice(&body[..end]);
        if has_carriage_return {
            out.push(b'\r');
        }
    }
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out
}

/// Write already-cleaned `data` (see `clean_for_write`) to `path`.
///
/// `expected_mtime` is the modification time recorded when the editor last
/// read or wrote the file (`None` when it did not exist); the write is refused
/// if the file on disk differs, so an external edit is never silently
/// overwritten. An existing file is replaced by renaming a fsynced temporary
/// file over it, so a crash or full disk leaves either the old or the new
/// content. Symlinks are followed and the target keeps its mode and owner.
/// When a rename cannot preserve that (a hard-linked file, a directory we
/// cannot create files in, an owner we cannot restore) the file is rewritten
/// in place instead.
pub fn write_file(
    path: &Path,
    data: &[u8],
    expected_mtime: Option<SystemTime>,
) -> io::Result<()> {
    let target = match fs::canonicalize(path) {
        Ok(resolved) => resolved,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if fs::symlink_metadata(path).is_ok() {
                // Dangling symlink: creating through it is the only way to
                // keep the link, and renaming over it would replace it.
                return write_new_file(path, data);
            }
            path.to_path_buf()
        }
        Err(e) => return Err(e),
    };
    if file_modification_time(&target) != expected_mtime {
        return Err(changed_on_disk_error());
    }
    let meta = match fs::metadata(&target) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return write_new_file(&target, data);
        }
        Err(e) => return Err(e),
    };
    if meta.nlink() == 1 && replace_file(&target, data, &meta)? == Replaced::Done {
        return Ok(());
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&target)?;
    file.write_all(data)?;
    file.sync_all()
}

fn write_new_file(path: &Path, data: &[u8]) -> io::Result<()> {
    // `create_new` also catches a file that appeared since the mtime check.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    sync_parent_dir(path)
}

#[derive(PartialEq, Eq)]
enum Replaced {
    Done,
    /// The rename strategy cannot keep the target's owner or location
    /// semantics; the caller must write in place.
    Unavailable,
}

fn replace_file(target: &Path, data: &[u8], meta: &fs::Metadata) -> io::Result<Replaced> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{}.e-tmp-{}", name, std::process::id()));
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Ok(Replaced::Unavailable),
        Err(e) => return Err(e),
    };
    let staged = (|| {
        file.write_all(data)?;
        // chown before chmod: changing the owner clears setuid/setgid bits.
        if std::os::unix::fs::fchown(&file, Some(meta.uid()), Some(meta.gid())).is_err() {
            return Ok(Replaced::Unavailable);
        }
        file.set_permissions(fs::Permissions::from_mode(meta.mode()))?;
        file.sync_all()?;
        fs::rename(&temp, target)?;
        Ok(Replaced::Done)
    })();
    if !matches!(staged, Ok(Replaced::Done)) {
        let _ = fs::remove_file(&temp);
    }
    if matches!(staged, Ok(Replaced::Done)) {
        sync_parent_dir(target)?;
    }
    staged
}

/// Persist a create or rename by fsyncing the containing directory.
fn sync_parent_dir(path: &Path) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    fs::File::open(dir)?.sync_all()
}

/// Per-user state directory `~/.config/e`. There is deliberately no fallback
/// when `HOME` is unset: any shared default such as `/tmp` would expose edit
/// history and lock state to other users.
fn state_dir() -> io::Result<PathBuf> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".config/e")),
        _ => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set, so ~/.config/e cannot be located",
        )),
    }
}

/// Create `dir` (and missing parents) readable only by the owner, and tighten
/// a directory left over from an earlier version that used the default mode.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
}

/// Directory for lock files: `~/.config/e/locks/`
fn locks_dir() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("locks"))
}

/// Encode an absolute file path for use as a filename.
/// `/path/to/file.txt` → `%2Fpath%2Fto%2Ffile.txt`
fn encode_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'/' => out.push_str("%2F"),
            b'%' => out.push_str("%25"),
            _ => out.push(b as char),
        }
    }
    out
}

/// Lock file path: `<locks dir>/<encoded_path>.elock`
fn lock_path_in(locks_dir: &Path, path: &Path) -> PathBuf {
    locks_dir.join(format!("{}.elock", encode_path(path)))
}

/// Resolve a path to absolute. Uses canonicalize if the file exists,
/// otherwise canonicalizes the parent and appends the filename.
fn resolve_absolute(path: &Path) -> PathBuf {
    if let Ok(abs) = path.canonicalize() {
        return abs;
    }
    // File doesn't exist yet — resolve parent
    let parent = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default();
    let abs_parent = parent.canonicalize().unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(parent)
    });
    abs_parent.join(name)
}

#[derive(Debug)]
pub enum LockError {
    /// Another live process holds the lock; `holder_pid` is best effort.
    Held { holder_pid: Option<u32> },
    Io(io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::Held {
                holder_pid: Some(pid),
            } => write!(f, "locked by another e instance (pid {pid})"),
            LockError::Held { holder_pid: None } => write!(f, "locked by another e instance"),
            LockError::Io(e) => write!(f, "cannot create lock file: {e}"),
        }
    }
}

/// Exclusive claim on editing one file, held for the life of the value.
///
/// The claim is an `flock` on the lock file, so the kernel drops it when the
/// process dies by any means; a lock file left behind by a crash is not held
/// and is simply reacquired. The file records the holder's pid for messages.
pub struct FileLock {
    file: fs::File,
    path: PathBuf,
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Unlink before the descriptor closes and releases the flock, and only
        // while the path still names our file: after `break_lock` it names
        // the new owner's file.
        if linked_to(&self.path, &self.file) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn linked_to(path: &Path, file: &fs::File) -> bool {
    match (fs::metadata(path), file.metadata()) {
        (Ok(linked), Ok(ours)) => linked.dev() == ours.dev() && linked.ino() == ours.ino(),
        _ => false,
    }
}

/// Take the lock for `path`, or report who holds it.
pub fn acquire_lock(path: &Path) -> Result<FileLock, LockError> {
    let locks_dir = locks_dir().map_err(LockError::Io)?;
    // The locks directory is created owner-only along with any missing parents;
    // also tighten a state directory left by a version that used the default mode.
    create_private_dir(&locks_dir).map_err(LockError::Io)?;
    create_private_dir(locks_dir.parent().unwrap_or(&locks_dir)).map_err(LockError::Io)?;
    acquire_lock_in(&locks_dir, path)
}

fn acquire_lock_in(locks_dir: &Path, path: &Path) -> Result<FileLock, LockError> {
    let lock = lock_path_in(locks_dir, &resolve_absolute(path));
    // A holder that exits unlinks the file while we may already have it open;
    // locking that orphaned inode would not exclude a newcomer, so recheck the
    // link after locking and retry against the fresh file.
    for _ in 0..8 {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock)
            .map_err(LockError::Io)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                let holder_pid = fs::read_to_string(&lock)
                    .ok()
                    .and_then(|text| text.trim().parse().ok());
                return Err(LockError::Held { holder_pid });
            }
            Err(fs::TryLockError::Error(e)) => return Err(LockError::Io(e)),
        }
        if linked_to(&lock, &file) {
            file.set_len(0).map_err(LockError::Io)?;
            (&file)
                .write_all(format!("{}\n", std::process::id()).as_bytes())
                .map_err(LockError::Io)?;
            return Ok(FileLock { file, path: lock });
        }
    }
    Err(LockError::Io(io::Error::other(
        "lock file kept changing while locking",
    )))
}

/// Unlink the lock file for `path` so a new claim can replace a live holder
/// the user chose to override. The old holder's drop leaves the new file alone.
pub fn break_lock(path: &Path) -> io::Result<()> {
    break_lock_in(&locks_dir()?, path)
}

fn break_lock_in(locks_dir: &Path, path: &Path) -> io::Result<()> {
    match fs::remove_file(lock_path_in(locks_dir, &resolve_absolute(path))) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Check the first 8KB for null bytes → likely binary.
pub fn is_likely_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// File size in bytes.
pub fn file_size(path: &Path) -> io::Result<u64> {
    Ok(fs::metadata(path)?.len())
}

/// File modification time, or None if unavailable.
pub fn file_modification_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

pub(crate) fn changed_on_disk_error() -> io::Error {
    io::Error::other("file changed on disk since it was loaded")
}

// -- persistent undo history ------------------------------------------------
//
// All undo histories stored in a single file: ~/.config/e/undo.bin
// Format: [magic][version][entry_count] then length-prefixed entries. Each
// entry starts with the file path, the file's mtime, and the byte length of the
// text the history applies to; a history is restored only when all three match,
// because its byte offsets mean nothing against any other content.
// On save, non-matching entries are copied as raw bytes (no deserialization).
// On load, entries are scanned linearly; only the matching one is deserialized.

const UNDO_MAGIC: &[u8; 4] = b"eUND";
const UNDO_VERSION: u8 = 3;
const MAX_GROUPS: u32 = 100_000;
const MAX_ENTRIES: u32 = 10_000;

fn undo_db_path() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("undo.bin"))
}

/// Sidecar file whose `flock` serializes access to a database. The database
/// itself is replaced by rename, so a lock on it would be lost with its inode.
fn db_lock_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

/// Rewrite a database under an exclusive lock: `build` receives the current
/// bytes (empty when there is no database yet) and returns the new contents,
/// which replace the file atomically so a crash never leaves a torn database.
fn rewrite_db(db_path: &Path, build: impl FnOnce(&[u8]) -> io::Result<Vec<u8>>) -> io::Result<()> {
    if let Some(parent) = db_path.parent() {
        create_private_dir(parent)?;
    }
    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(db_lock_path(db_path))?;
    lock_file.lock()?;
    let existing = match fs::read(db_path) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let contents = build(&existing)?;
    let mut staged_name = db_path.as_os_str().to_owned();
    staged_name.push(".tmp");
    let staged = PathBuf::from(staged_name);
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&staged)?;
        file.write_all(&contents)?;
        file.sync_all()?;
        fs::rename(&staged, db_path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&staged);
    }
    written
}

/// Read a database under a shared lock. `Ok(None)` when none exists. Writers
/// create the lock file before the database, so a missing lock file means no
/// writer has ever run and there is nothing to race with.
fn read_db(db_path: &Path) -> io::Result<Option<Vec<u8>>> {
    let _lock = match fs::File::open(db_lock_path(db_path)) {
        Ok(file) => {
            file.lock_shared()?;
            Some(file)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    match fs::read(db_path) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Refuse to replace a database written by a newer version of the editor: an
/// older binary would otherwise destroy history it cannot read. Anything else
/// (missing, foreign, or older) is replaced.
fn check_db_not_newer(existing: &[u8], magic: &[u8; 4], version: u8) -> io::Result<()> {
    if existing.len() > 4 && &existing[0..4] == magic && existing[4] > version {
        return Err(io::Error::other(
            "state file was written by a newer version of e; not overwriting it",
        ));
    }
    Ok(())
}

// Binary encoding helpers

fn write_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn write_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn read_u8(data: &[u8], pos: &mut usize) -> Option<u8> {
    if *pos >= data.len() {
        return None;
    }
    let v = data[*pos];
    *pos += 1;
    Some(v)
}

fn read_u32(data: &[u8], pos: &mut usize) -> Option<u32> {
    if *pos + 4 > data.len() {
        return None;
    }
    let v = u32::from_le_bytes(data[*pos..*pos + 4].try_into().ok()?);
    *pos += 4;
    Some(v)
}

fn read_u64(data: &[u8], pos: &mut usize) -> Option<u64> {
    if *pos + 8 > data.len() {
        return None;
    }
    let v = u64::from_le_bytes(data[*pos..*pos + 8].try_into().ok()?);
    *pos += 8;
    Some(v)
}

fn read_i64(data: &[u8], pos: &mut usize) -> Option<i64> {
    if *pos + 8 > data.len() {
        return None;
    }
    let v = i64::from_le_bytes(data[*pos..*pos + 8].try_into().ok()?);
    *pos += 8;
    Some(v)
}

fn serialize_selection(buf: &mut Vec<u8>, selection: Selection) {
    write_u32(buf, selection.anchor.line as u32);
    write_u32(buf, selection.anchor.column as u32);
    write_u32(buf, selection.cursor.line as u32);
    write_u32(buf, selection.cursor.column as u32);
}

fn deserialize_selection(data: &[u8], pos: &mut usize) -> Option<Selection> {
    let anchor_line = read_u32(data, pos)? as usize;
    let anchor_col = read_u32(data, pos)? as usize;
    let cursor_line = read_u32(data, pos)? as usize;
    let cursor_col = read_u32(data, pos)? as usize;
    Some(Selection {
        anchor: TextPosition::new(anchor_line, anchor_col),
        cursor: TextPosition::new(cursor_line, cursor_col),
    })
}

fn serialize_snapshot(buf: &mut Vec<u8>, snapshot: &CaretSnapshot) {
    write_u32(buf, snapshot.selections.len() as u32);
    write_u32(buf, snapshot.primary as u32);
    for selection in &snapshot.selections {
        serialize_selection(buf, *selection);
    }
}

fn deserialize_snapshot(data: &[u8], pos: &mut usize) -> Option<CaretSnapshot> {
    let count = read_u32(data, pos)? as usize;
    if count > MAX_GROUPS as usize {
        return None;
    }
    let primary = read_u32(data, pos)? as usize;
    let mut selections = Vec::with_capacity(count);
    for _ in 0..count {
        selections.push(deserialize_selection(data, pos)?);
    }
    Some(CaretSnapshot {
        selections,
        primary,
    })
}

fn serialize_group(buf: &mut Vec<u8>, group: &UndoGroup) {
    serialize_snapshot(buf, &group.carets_before);
    serialize_snapshot(buf, &group.carets_after);
    write_u32(buf, group.operations.len() as u32);
    for op in &group.operations {
        match op {
            UndoOperation::Insert { byte_offset, bytes } => {
                write_u8(buf, 0);
                write_u64(buf, *byte_offset as u64);
                write_u32(buf, bytes.len() as u32);
                buf.extend_from_slice(bytes.as_ref());
            }
            UndoOperation::Delete { byte_offset, bytes } => {
                write_u8(buf, 1);
                write_u64(buf, *byte_offset as u64);
                write_u32(buf, bytes.len() as u32);
                buf.extend_from_slice(bytes.as_ref());
            }
        }
    }
}

fn deserialize_group(data: &[u8], pos: &mut usize) -> Option<UndoGroup> {
    let carets_before = deserialize_snapshot(data, pos)?;
    let carets_after = deserialize_snapshot(data, pos)?;
    let op_count = read_u32(data, pos)?;
    if op_count > MAX_GROUPS {
        return None;
    }
    let mut operations = Vec::with_capacity(op_count as usize);
    for _ in 0..op_count {
        let kind = read_u8(data, pos)?;
        let op_pos = read_u64(data, pos)? as usize;
        let data_len = read_u32(data, pos)? as usize;
        if *pos + data_len > data.len() {
            return None;
        }
        let op_data: Arc<[u8]> = Arc::from(&data[*pos..*pos + data_len]);
        *pos += data_len;
        let op = match kind {
            0 => UndoOperation::Insert {
                byte_offset: op_pos,
                bytes: op_data,
            },
            1 => UndoOperation::Delete {
                byte_offset: op_pos,
                bytes: op_data,
            },
            _ => return None,
        };
        operations.push(op);
    }
    Some(UndoGroup {
        operations,
        carets_before,
        carets_after,
    })
}

fn deserialize_groups(data: &[u8], pos: &mut usize) -> Option<Vec<UndoGroup>> {
    let count = read_u32(data, pos)?;
    if count > MAX_GROUPS {
        return None;
    }
    let mut groups = Vec::with_capacity(count as usize);
    for _ in 0..count {
        groups.push(deserialize_group(data, pos)?);
    }
    Some(groups)
}

/// Identity of the file content an undo entry applies to.
struct EntryHeader<'a> {
    path: &'a [u8],
    mtime_secs: i64,
    mtime_nanos: u32,
    file_len: u64,
}

/// Read the header fields at the start of an entry body, leaving `pos` just
/// past them at the serialized undo and redo stacks.
fn read_entry_header<'a>(data: &'a [u8], pos: &mut usize) -> Option<EntryHeader<'a>> {
    let path_len = read_u32(data, pos)? as usize;
    if *pos + path_len > data.len() {
        return None;
    }
    let path = &data[*pos..*pos + path_len];
    *pos += path_len;
    let mtime_secs = read_i64(data, pos)?;
    let mtime_nanos = read_u32(data, pos)?;
    let file_len = read_u64(data, pos)?;
    Some(EntryHeader {
        path,
        mtime_secs,
        mtime_nanos,
        file_len,
    })
}

fn mtime_parts(mtime: SystemTime) -> (i64, u32) {
    let duration = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (duration.as_secs() as i64, duration.subsec_nanos())
}

/// Check if an entry still describes the file on disk.
fn entry_matches_disk(header: &EntryHeader) -> bool {
    let Ok(path) = std::str::from_utf8(header.path) else {
        return false;
    };
    let Ok(meta) = fs::metadata(path) else {
        return false; // file deleted
    };
    let Ok(mtime) = meta.modified() else {
        return false;
    };
    mtime_parts(mtime) == (header.mtime_secs, header.mtime_nanos) && meta.len() == header.file_len
}

/// Scan entries in a db blob, collecting raw byte ranges of entries whose path
/// does NOT match `exclude_path` and whose mtime is still valid.
fn collect_kept_entries<'a>(data: &'a [u8], exclude_path: &[u8]) -> (Vec<&'a [u8]>, u32) {
    let mut kept = Vec::new();
    let header_len = 4 + 1 + 4; // magic + version + entry_count
    if data.len() < header_len || &data[0..4] != UNDO_MAGIC || data[4] != UNDO_VERSION {
        return (kept, 0);
    }
    let mut pos = 5usize;
    let count = match read_u32(data, &mut pos) {
        Some(c) => c.min(MAX_ENTRIES),
        None => return (kept, 0),
    };
    for _ in 0..count {
        let entry_len = match read_u32(data, &mut pos) {
            Some(l) => l as usize,
            None => break,
        };
        if pos + entry_len > data.len() {
            break;
        }
        let entry_body = &data[pos..pos + entry_len];
        pos += entry_len;
        let mut header_pos = pos - entry_len;
        if let Some(header) = read_entry_header(data, &mut header_pos)
            && header.path != exclude_path
            && entry_matches_disk(&header)
        {
            kept.push(entry_body);
        }
    }
    let kept_count = kept.len() as u32;
    (kept, kept_count)
}

/// Save undo history to disk. `file_len` is the byte length of the text the
/// history applies to; it must equal the file's length on disk, so callers
/// persist only when the buffer and the file hold the same bytes.
pub fn save_undo_history(
    file_path: &Path,
    undo_stack: &UndoStack,
    file_len: u64,
) -> io::Result<()> {
    save_undo_history_to(&undo_db_path()?, file_path, undo_stack, file_len)
}

fn save_undo_history_to(
    db_path: &Path,
    file_path: &Path,
    undo_stack: &UndoStack,
    file_len: u64,
) -> io::Result<()> {
    let abs_path = resolve_absolute(file_path);
    let abs_str = abs_path.to_string_lossy();
    let path_bytes = abs_str.as_bytes();

    let (mtime_secs, mtime_nanos) = mtime_parts(fs::metadata(file_path)?.modified()?);

    // Serialize new entry body. Load rejects stacks past these limits, so the
    // oldest groups are dropped here instead of writing an unreadable entry.
    let mut entry = Vec::new();
    write_u32(&mut entry, path_bytes.len() as u32);
    entry.extend_from_slice(path_bytes);
    write_i64(&mut entry, mtime_secs);
    write_u32(&mut entry, mtime_nanos);
    write_u64(&mut entry, file_len);
    let (undo, redo) = undo_stack.stacks();
    for stack in [undo, redo] {
        let stack = &stack[stack.len().saturating_sub(MAX_GROUPS as usize)..];
        write_u32(&mut entry, stack.len() as u32);
        for group in stack {
            if group.operations.len() > MAX_GROUPS as usize
                || group.carets_before.selections.len() > MAX_GROUPS as usize
                || group.carets_after.selections.len() > MAX_GROUPS as usize
            {
                return Err(io::Error::other("undo group too large to persist"));
            }
            serialize_group(&mut entry, group);
        }
    }
    if u32::try_from(entry.len()).is_err() {
        return Err(io::Error::other("undo history too large to persist"));
    }

    rewrite_db(db_path, |existing| {
        check_db_not_newer(existing, UNDO_MAGIC, UNDO_VERSION)?;
        // Keep fresh entries for other paths, newest last, within the load limit.
        let (kept, _) = collect_kept_entries(existing, path_bytes);
        let kept = &kept[kept.len().saturating_sub(MAX_ENTRIES as usize - 1)..];

        let mut buf = Vec::new();
        buf.extend_from_slice(UNDO_MAGIC);
        write_u8(&mut buf, UNDO_VERSION);
        write_u32(&mut buf, kept.len() as u32 + 1);
        for entry_body in kept {
            write_u32(&mut buf, entry_body.len() as u32);
            buf.extend_from_slice(entry_body);
        }
        write_u32(&mut buf, entry.len() as u32);
        buf.extend_from_slice(&entry);
        Ok(buf)
    })
}

/// Load undo history from disk. A missing database or a history for different
/// file content (mtime or length mismatch) leaves `undo_stack` untouched.
/// `buffer_len` is the byte length of the text just loaded into the buffer.
pub fn load_undo_history(
    file_path: &Path,
    undo_stack: &mut UndoStack,
    buffer_len: u64,
) -> io::Result<()> {
    load_undo_history_from(&undo_db_path()?, file_path, undo_stack, buffer_len)
}

fn load_undo_history_from(
    db_path: &Path,
    file_path: &Path,
    undo_stack: &mut UndoStack,
    buffer_len: u64,
) -> io::Result<()> {
    if let Some(data) = read_db(db_path)? {
        restore_undo_entry(&data, file_path, undo_stack, buffer_len);
    }
    Ok(())
}

fn restore_undo_entry(
    data: &[u8],
    file_path: &Path,
    undo_stack: &mut UndoStack,
    buffer_len: u64,
) -> Option<()> {
    let abs_path = resolve_absolute(file_path);
    let abs_str = abs_path.to_string_lossy();
    let target_path = abs_str.as_bytes();

    if data.len() < 9 || &data[0..4] != UNDO_MAGIC {
        return None;
    }
    let mut pos = 4usize;
    let version = read_u8(data, &mut pos)?;
    if version != UNDO_VERSION {
        return None;
    }

    let count = read_u32(data, &mut pos)?;
    for _ in 0..count.min(MAX_ENTRIES) {
        let entry_len = read_u32(data, &mut pos)? as usize;
        let entry_start = pos;
        if pos + entry_len > data.len() {
            return None;
        }

        let header = read_entry_header(data, &mut pos)?;
        if header.path == target_path {
            if header.file_len != buffer_len || !entry_matches_disk(&header) {
                return None;
            }
            let undo = deserialize_groups(data, &mut pos)?;
            let redo = deserialize_groups(data, &mut pos)?;
            undo_stack.restore(undo, redo);
            return Some(());
        }

        // Skip this entry
        pos = entry_start + entry_len;
    }
    None
}

// -- cursor position persistence --------------------------------------------
//
// Stores last cursor position per file in: ~/.config/e/cursor.bin
// Format: [magic:4 "eCUR"][version:1][entry_count:4] then entries:
//   [path_len:4][path_bytes][line:4][column:4]
// No mtime validation — cursor position is clamped to buffer bounds on load.
// Stale entries for deleted files are pruned on write.

const CURSOR_MAGIC: &[u8; 4] = b"eCUR";
const CURSOR_VERSION: u8 = 1;
const CURSOR_MAX_ENTRIES: u32 = 10_000;

fn cursor_db_path() -> io::Result<PathBuf> {
    Ok(state_dir()?.join("cursor.bin"))
}

/// Save cursor position for a file.
pub fn save_cursor_position(file_path: &Path, cursor: TextPosition) -> io::Result<()> {
    save_cursor_position_to(&cursor_db_path()?, file_path, cursor)
}

fn save_cursor_position_to(
    db_path: &Path,
    file_path: &Path,
    cursor: TextPosition,
) -> io::Result<()> {
    let abs_path = resolve_absolute(file_path);
    let abs_str = abs_path.to_string_lossy();
    let path_bytes = abs_str.as_bytes();

    // Build new entry
    let mut entry = Vec::new();
    write_u32(&mut entry, path_bytes.len() as u32);
    entry.extend_from_slice(path_bytes);
    write_u32(&mut entry, cursor.line as u32);
    write_u32(&mut entry, cursor.column as u32);

    rewrite_db(db_path, |existing| {
        check_db_not_newer(existing, CURSOR_MAGIC, CURSOR_VERSION)?;
        // Keep entries for other paths (prune deleted files)
        let kept = collect_cursor_entries(existing, path_bytes);
        let kept = &kept[kept.len().saturating_sub(CURSOR_MAX_ENTRIES as usize - 1)..];

        let mut buf = Vec::new();
        buf.extend_from_slice(CURSOR_MAGIC);
        write_u8(&mut buf, CURSOR_VERSION);
        write_u32(&mut buf, kept.len() as u32 + 1);
        for e in kept {
            buf.extend_from_slice(e);
        }
        // Length-prefix our new entry
        write_u32(&mut buf, entry.len() as u32);
        buf.extend_from_slice(&entry);
        Ok(buf)
    })
}

/// Load cursor position for a file. Returns None if not found.
pub fn load_cursor_position(file_path: &Path) -> Option<TextPosition> {
    load_cursor_position_from(&cursor_db_path().ok()?, file_path)
}

fn load_cursor_position_from(db_path: &Path, file_path: &Path) -> Option<TextPosition> {
    let data = read_db(db_path).ok()??;
    let data = data.as_slice();
    let abs_path = resolve_absolute(file_path);
    let abs_str = abs_path.to_string_lossy();
    let target_path = abs_str.as_bytes();

    if data.len() < 9 || &data[0..4] != CURSOR_MAGIC {
        return None;
    }
    let mut pos = 4usize;
    let version = read_u8(data, &mut pos)?;
    if version != CURSOR_VERSION {
        return None;
    }
    let count = read_u32(data, &mut pos)?;
    for _ in 0..count.min(CURSOR_MAX_ENTRIES) {
        let entry_len = read_u32(data, &mut pos)? as usize;
        let entry_start = pos;
        if pos + entry_len > data.len() {
            return None;
        }
        let path_len = read_u32(data, &mut pos)? as usize;
        if pos + path_len > data.len() {
            return None;
        }
        let entry_path = &data[pos..pos + path_len];
        pos += path_len;

        if entry_path == target_path {
            let line = read_u32(data, &mut pos)? as usize;
            let column = read_u32(data, &mut pos)? as usize;
            return Some(TextPosition::new(line, column));
        }
        pos = entry_start + entry_len;
    }
    None
}

/// Collect kept entries from existing cursor db, excluding `exclude_path`
/// and pruning entries for files that no longer exist.
/// Returns length-prefixed entry blobs.
fn collect_cursor_entries<'a>(data: &'a [u8], exclude_path: &[u8]) -> Vec<&'a [u8]> {
    let mut kept = Vec::new();
    let header_len = 4 + 1 + 4; // magic + version + entry_count
    if data.len() < header_len || &data[0..4] != CURSOR_MAGIC || data[4] != CURSOR_VERSION {
        return kept;
    }
    let mut pos = 5usize;
    let count = match read_u32(data, &mut pos) {
        Some(c) => c.min(CURSOR_MAX_ENTRIES),
        None => return kept,
    };
    for _ in 0..count {
        let len_start = pos;
        let entry_len = match read_u32(data, &mut pos) {
            Some(l) => l as usize,
            None => break,
        };
        if pos + entry_len > data.len() {
            break;
        }
        let entry_end = pos + entry_len;
        // Peek at path
        let mut p = pos;
        let path_len = match read_u32(data, &mut p) {
            Some(l) => l as usize,
            None => {
                pos = entry_end;
                continue;
            }
        };
        if p + path_len > data.len() {
            break;
        }
        let entry_path = &data[p..p + path_len];

        if entry_path != exclude_path
            && let Ok(path_str) = std::str::from_utf8(entry_path)
            && std::path::Path::new(path_str).exists()
        {
            kept.push(&data[len_start..entry_end]);
        }
        pos = entry_end;
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::Selection;

    // -- clean_for_write ----------------------------------------------------

    #[test]
    fn test_strip_trailing_whitespace() {
        let result = clean_for_write(b"hello   \nworld  \n");
        assert_eq!(result, b"hello\nworld\n");
    }

    #[test]
    fn test_ensure_trailing_newline() {
        let result = clean_for_write(b"hello");
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_already_has_trailing_newline() {
        let result = clean_for_write(b"hello\n");
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_strip_tabs_at_end() {
        let result = clean_for_write(b"hello\t\t\n");
        assert_eq!(result, b"hello\n");
    }

    #[test]
    fn test_preserves_leading_whitespace() {
        let result = clean_for_write(b"  hello\n");
        assert_eq!(result, b"  hello\n");
    }

    #[test]
    fn test_empty_lines_preserved() {
        let result = clean_for_write(b"a\n\nb\n");
        assert_eq!(result, b"a\n\nb\n");
    }

    #[test]
    fn test_only_whitespace_lines() {
        // Blank lines are kept, including leading ones.
        let result = clean_for_write(b"   \n  \n");
        assert_eq!(result, b"\n\n");
    }

    // -- is_likely_binary ---------------------------------------------------

    #[test]
    fn test_text_not_binary() {
        assert!(!is_likely_binary(b"hello world\nfoo bar\n"));
    }

    #[test]
    fn test_binary_with_null() {
        assert!(is_likely_binary(b"hello\x00world"));
    }

    #[test]
    fn test_binary_null_at_start() {
        assert!(is_likely_binary(b"\x00hello"));
    }

    #[test]
    fn test_empty_not_binary() {
        assert!(!is_likely_binary(b""));
    }

    #[test]
    fn test_binary_null_past_8kb_not_detected() {
        let mut data = vec![b'a'; 8193];
        data[8192] = 0; // null after 8KB
        assert!(!is_likely_binary(&data));
    }

    #[test]
    fn test_binary_null_within_8kb() {
        let mut data = vec![b'a'; 8192];
        data[8191] = 0; // null at end of 8KB check range
        assert!(is_likely_binary(&data));
    }

    // -- read_file / write_file (integration) -------------------------------

    fn save_cleaned(path: &Path, data: &[u8]) -> io::Result<()> {
        write_file(
            path,
            &clean_for_write(data),
            file_modification_time(path),
        )
    }

    #[test]
    fn test_clean_for_write_preserves_invalid_utf8_crlf_and_nbsp() {
        let data = b"a\xff\xfeb\r\nline2\xc2\xa0\r\ntail  \t\r\n";
        assert_eq!(
            clean_for_write(data),
            b"a\xff\xfeb\r\nline2\xc2\xa0\r\ntail\r\n"
        );
    }

    #[test]
    fn test_read_write_roundtrip() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join("test.txt");

        save_cleaned(&path, b"hello\nworld\n").unwrap();
        assert_eq!(read_file(&path).unwrap(), b"hello\nworld\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_refuses_file_created_since_load() {
        let dir = scratch_dir("write_created");
        let path = dir.join("test.txt");
        fs::write(&path, b"theirs\n").unwrap();

        assert!(write_file(&path, b"mine\n", None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"theirs\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_keeps_mode_and_follows_symlink() {
        let dir = scratch_dir("write_symlink");
        let real = dir.join("real.txt");
        let link = dir.join("link.txt");
        fs::write(&real, b"old\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        save_cleaned(&link, b"new\n").unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new\n");
        assert_eq!(fs::metadata(&real).unwrap().mode() & 0o7777, 0o640);
        let leftovers: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(leftovers.len(), 2, "temporary file left behind");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_hard_linked_file_stays_linked() {
        let dir = scratch_dir("write_hardlink");
        let path = dir.join("a.txt");
        let other = dir.join("b.txt");
        fs::write(&path, b"old\n").unwrap();
        fs::hard_link(&path, &other).unwrap();

        save_cleaned(&path, b"new\n").unwrap();

        assert_eq!(fs::read(&other).unwrap(), b"new\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_creates_new_file() {
        let dir = scratch_dir("write_new");
        let path = dir.join("fresh.txt");

        write_file(&path, b"hi\n", None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hi\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_size() {
        let dir = std::env::temp_dir().join("e_test_size");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");

        fs::write(&path, b"12345").unwrap();
        assert_eq!(file_size(&path).unwrap(), 5);

        let _ = fs::remove_dir_all(&dir);
    }

    // -- undo history persistence -------------------------------------------

    fn snap(pos: TextPosition) -> CaretSnapshot {
        CaretSnapshot {
            selections: vec![Selection::caret(pos)],
            primary: 0,
        }
    }

    /// The tests' files are written before saving, so the history applies to
    /// whatever length the file has on disk.
    fn save_for_test(db: &Path, path: &Path, stack: &UndoStack) {
        let len = fs::metadata(path).unwrap().len();
        save_undo_history_to(db, path, stack, len).unwrap();
    }

    fn load_for_test(db: &Path, path: &Path, stack: &mut UndoStack) {
        let len = fs::metadata(path).unwrap().len();
        load_undo_history_from(db, path, stack, len).unwrap();
    }

    fn make_test_stack() -> UndoStack {
        let mut stack = UndoStack::new();
        stack.record(
            UndoOperation::Insert {
                byte_offset: 0,
                bytes: Arc::from(b"hello".as_ref()),
            },
            snap(TextPosition::new(0, 0)),
            snap(TextPosition::new(0, 5)),
        );
        stack.seal();
        stack.record(
            UndoOperation::Delete {
                byte_offset: 3,
                bytes: Arc::from(b"lo".as_ref()),
            },
            snap(TextPosition::new(0, 5)),
            snap(TextPosition::new(0, 3)),
        );
        stack.seal();
        stack
    }

    #[test]
    fn test_undo_history_roundtrip() {
        let dir = std::env::temp_dir().join("e_test_undo_rt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hel").unwrap();

        let stack = make_test_stack();
        save_for_test(&db, &path, &stack);

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert_eq!(undo.len(), 2);
        assert!(redo.is_empty());

        assert_eq!(undo[0].carets_before, snap(TextPosition::new(0, 0)));
        assert_eq!(undo[0].carets_after, snap(TextPosition::new(0, 5)));
        assert_eq!(undo[0].operations.len(), 1);
        assert_eq!(undo[1].carets_before, snap(TextPosition::new(0, 5)));
        assert_eq!(undo[1].carets_after, snap(TextPosition::new(0, 3)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_with_redo() {
        let dir = std::env::temp_dir().join("e_test_undo_redo");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hel").unwrap();

        let mut stack = make_test_stack();
        stack.undo(|_| {});
        save_for_test(&db, &path, &stack);

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert_eq!(undo.len(), 1);
        assert_eq!(redo.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_mtime_mismatch() {
        let dir = std::env::temp_dir().join("e_test_undo_mtime");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();

        let stack = make_test_stack();
        save_for_test(&db, &path, &stack);

        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&path, b"changed").unwrap();

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_rejects_different_buffer_length() {
        let dir = scratch_dir("undo_len");
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"abc   \nxyz").unwrap();

        // Saved after a cleaned save: offsets refer to a longer buffer than
        // the file now holds, with the mtime unchanged.
        save_undo_history_to(&db, &path, &make_test_stack(), 10).unwrap();

        let mut loaded = UndoStack::new();
        load_undo_history_from(&db, &path, &mut loaded, 7).unwrap();
        assert!(loaded.stacks().0.is_empty());
        load_undo_history_from(&db, &path, &mut loaded, 10).unwrap();
        assert_eq!(
            loaded.stacks().0.len(),
            make_test_stack().stacks().0.len()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_save_keeps_database_of_newer_version() {
        let dir = scratch_dir("undo_newer");
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();
        let mut newer = Vec::new();
        newer.extend_from_slice(UNDO_MAGIC);
        newer.push(UNDO_VERSION + 1);
        newer.extend_from_slice(b"future payload");
        fs::write(&db, &newer).unwrap();

        let err = save_undo_history_to(&db, &path, &make_test_stack(), 5).unwrap_err();
        assert!(err.to_string().contains("newer version"));
        assert_eq!(fs::read(&db).unwrap(), newer);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_corrupt_db() {
        let dir = std::env::temp_dir().join("e_test_undo_corrupt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();
        fs::write(&db, b"garbage data here").unwrap();

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_empty_stacks() {
        let dir = std::env::temp_dir().join("e_test_undo_empty");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();

        let stack = UndoStack::new();
        save_for_test(&db, &path, &stack);

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_bad_magic() {
        let dir = std::env::temp_dir().join("e_test_undo_magic");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();
        fs::write(&db, b"BADMagic").unwrap();

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_bad_version() {
        let dir = std::env::temp_dir().join("e_test_undo_ver");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();

        let mut data = Vec::new();
        data.extend_from_slice(UNDO_MAGIC);
        data.push(99);
        fs::write(&db, &data).unwrap();

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_multiple_files() {
        let dir = std::env::temp_dir().join("e_test_undo_multi");
        let _ = fs::create_dir_all(&dir);
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let db = dir.join("undo.bin");
        fs::write(&path_a, b"aaa").unwrap();
        fs::write(&path_b, b"bbb").unwrap();

        let stack_a = make_test_stack();
        save_for_test(&db, &path_a, &stack_a);

        let mut stack_b = UndoStack::new();
        stack_b.record(
            UndoOperation::Insert {
                byte_offset: 0,
                bytes: Arc::from(b"x".as_ref()),
            },
            snap(TextPosition::new(0, 0)),
            snap(TextPosition::new(0, 1)),
        );
        stack_b.seal();
        save_for_test(&db, &path_b, &stack_b);

        // Both should be independently loadable
        let mut loaded_a = UndoStack::new();
        load_for_test(&db, &path_a, &mut loaded_a);
        assert_eq!(loaded_a.stacks().0.len(), 2);

        let mut loaded_b = UndoStack::new();
        load_for_test(&db, &path_b, &mut loaded_b);
        assert_eq!(loaded_b.stacks().0.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_update_replaces_entry() {
        let dir = std::env::temp_dir().join("e_test_undo_replace");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("undo.bin");
        fs::write(&path, b"hello").unwrap();

        let stack = make_test_stack();
        save_for_test(&db, &path, &stack);

        // Save again with different stack — should replace, not duplicate
        let mut stack2 = UndoStack::new();
        stack2.record(
            UndoOperation::Insert {
                byte_offset: 0,
                bytes: Arc::from(b"x".as_ref()),
            },
            snap(TextPosition::new(0, 0)),
            snap(TextPosition::new(0, 1)),
        );
        stack2.seal();
        save_for_test(&db, &path, &stack2);

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        assert_eq!(loaded.stacks().0.len(), 1); // new stack, not old

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_prunes_stale_entries() {
        let dir = std::env::temp_dir().join("e_test_undo_prune");
        let _ = fs::create_dir_all(&dir);
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let db = dir.join("undo.bin");
        fs::write(&path_a, b"aaa").unwrap();
        fs::write(&path_b, b"bbb").unwrap();

        // Save undo for both files
        let stack = make_test_stack();
        save_for_test(&db, &path_a, &stack);
        save_for_test(&db, &path_b, &stack);
        let size_before = fs::metadata(&db).unwrap().len();

        // Externally modify file_a (invalidates its mtime)
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&path_a, b"modified").unwrap();

        // Save file_b again — should prune file_a's stale entry
        save_for_test(&db, &path_b, &stack);
        let size_after = fs::metadata(&db).unwrap().len();
        assert!(
            size_after < size_before,
            "db should shrink after pruning stale entry"
        );

        // file_a's entry should be gone
        let mut loaded = UndoStack::new();
        load_for_test(&db, &path_a, &mut loaded);
        assert!(loaded.stacks().0.is_empty());

        // file_b's entry should still work
        let mut loaded_b = UndoStack::new();
        load_for_test(&db, &path_b, &mut loaded_b);
        assert_eq!(loaded_b.stacks().0.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_undo_history_prunes_deleted_file() {
        let dir = std::env::temp_dir().join("e_test_undo_deleted");
        let _ = fs::create_dir_all(&dir);
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let db = dir.join("undo.bin");
        fs::write(&path_a, b"aaa").unwrap();
        fs::write(&path_b, b"bbb").unwrap();

        let stack = make_test_stack();
        save_for_test(&db, &path_a, &stack);
        save_for_test(&db, &path_b, &stack);

        // Delete file_a
        fs::remove_file(&path_a).unwrap();

        // Saving file_b should prune the deleted file_a entry
        save_for_test(&db, &path_b, &stack);

        // Re-create file_a — its old undo entry should be gone
        fs::write(&path_a, b"aaa").unwrap();
        let mut loaded = UndoStack::new();
        load_for_test(&db, &path_a, &mut loaded);
        assert!(loaded.stacks().0.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("e_test_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_break_lock_replaces_holder_and_old_drop_keeps_new_lock() {
        let dir = scratch_dir("lock_break");
        let locks = dir.join("locks");
        fs::create_dir_all(&locks).unwrap();
        let path = dir.join("test.txt");
        fs::write(&path, b"hello").unwrap();

        let old = acquire_lock_in(&locks, &path).unwrap();
        break_lock_in(&locks, &path).unwrap();
        let new = acquire_lock_in(&locks, &path).unwrap();
        drop(old);
        assert!(matches!(
            acquire_lock_in(&locks, &path),
            Err(LockError::Held { .. })
        ));
        drop(new);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_clean_for_write() {
        let result = clean_for_write(b"hello   \nworld  ");
        assert_eq!(result, b"hello\nworld\n");
    }

    #[test]
    fn test_clean_for_write_empty() {
        let result = clean_for_write(b"");
        assert_eq!(result, b"\n");
    }

    #[test]
    fn test_encode_path() {
        let path = std::path::Path::new("/tmp/test/file.txt");
        let encoded = encode_path(path);
        assert!(encoded.contains("%2F"));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_lock_path_contains_elock() {
        let path = std::path::Path::new("/tmp/test.txt");
        let lp = lock_path_in(Path::new("/state/locks"), path);
        let s = lp.to_string_lossy();
        assert!(s.ends_with(".elock"));
        assert!(s.contains("locks"));
    }

    #[test]
    fn test_file_mtime_exists() {
        let dir = std::env::temp_dir().join("e_test_mtime_exists");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        fs::write(&path, b"hello").unwrap();
        assert!(file_modification_time(&path).is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_mtime_nonexistent() {
        let path = std::path::Path::new("/tmp/e_nonexistent_file_mtime_test.txt");
        assert!(file_modification_time(path).is_none());
    }

    #[test]
    fn test_encode_path_percent() {
        let path = Path::new("/tmp/test%file");
        let encoded = encode_path(path);
        assert!(encoded.contains("%25"));
    }

    #[test]
    fn test_resolve_absolute_new_file() {
        // resolve_absolute for a file that doesn't exist yet
        let dir = std::env::temp_dir().join("e_test_resolve");
        let _ = fs::create_dir_all(&dir);
        let new_file = dir.join("brand_new.txt");
        let result = resolve_absolute(&new_file);
        assert!(result.is_absolute());
        assert!(result.to_string_lossy().contains("brand_new.txt"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_helpers_truncated() {
        // Test read functions with truncated data
        let empty: &[u8] = &[];
        let mut pos = 0;
        assert!(read_u8(empty, &mut pos).is_none());
        assert!(read_u32(empty, &mut pos).is_none());
        assert!(read_u64(empty, &mut pos).is_none());
        assert!(read_i64(empty, &mut pos).is_none());
    }

    #[test]
    fn test_deserialize_corrupt_data() {
        // Craft a minimal valid-looking undo db entry that's truncated
        let mut data = Vec::new();
        // Path length + path
        write_u32(&mut data, 4);
        data.extend_from_slice(b"test");
        // mtime secs + nanos
        write_i64(&mut data, 0);
        write_u32(&mut data, 0);
        // Undo group count
        write_u32(&mut data, 1);
        // Truncated here — group data missing
        let mut pos = 0;
        let result = deserialize_groups(&data[data.len() - 4..], &mut pos);
        // Should get Some with 1 group, but group deserialization will fail
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn test_entry_header_truncated() {
        let data = [0u8; 4]; // Just a path length, no actual path
        let mut pos = 0;
        let result = read_entry_header(&data, &mut pos);
        // path_len = 0, then the mtime and length fields are missing → should fail
        assert!(result.is_none());
    }

    #[test]
    fn test_undo_history_no_db_file() {
        let dir = std::env::temp_dir().join("e_test_undo_nodb");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("nonexistent.bin");
        fs::write(&path, b"hello").unwrap();

        let mut loaded = UndoStack::new();
        load_for_test(&db, &path, &mut loaded);
        let (undo, redo) = loaded.stacks();
        assert!(undo.is_empty());
        assert!(redo.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    // -- cursor position persistence -------------------------------------------

    #[test]
    fn test_cursor_position_roundtrip() {
        let dir = std::env::temp_dir().join("e_test_cursor_rt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("cursor.bin");
        fs::write(&path, b"hello\nworld\n").unwrap();

        save_cursor_position_to(&db, &path, TextPosition::new(1, 3)).unwrap();
        let loaded = load_cursor_position_from(&db, &path);
        assert_eq!(loaded, Some(TextPosition::new(1, 3)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cursor_position_update() {
        let dir = std::env::temp_dir().join("e_test_cursor_upd");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("cursor.bin");
        fs::write(&path, b"hello").unwrap();

        save_cursor_position_to(&db, &path, TextPosition::new(0, 2)).unwrap();
        save_cursor_position_to(&db, &path, TextPosition::new(5, 10)).unwrap();
        let loaded = load_cursor_position_from(&db, &path);
        assert_eq!(loaded, Some(TextPosition::new(5, 10)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cursor_position_multiple_files() {
        let dir = std::env::temp_dir().join("e_test_cursor_multi");
        let _ = fs::create_dir_all(&dir);
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let db = dir.join("cursor.bin");
        fs::write(&path_a, b"aaa").unwrap();
        fs::write(&path_b, b"bbb").unwrap();

        save_cursor_position_to(&db, &path_a, TextPosition::new(0, 1)).unwrap();
        save_cursor_position_to(&db, &path_b, TextPosition::new(3, 7)).unwrap();

        assert_eq!(
            load_cursor_position_from(&db, &path_a),
            Some(TextPosition::new(0, 1))
        );
        assert_eq!(
            load_cursor_position_from(&db, &path_b),
            Some(TextPosition::new(3, 7))
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cursor_position_no_db() {
        let dir = std::env::temp_dir().join("e_test_cursor_nodb");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("nonexistent.bin");
        fs::write(&path, b"hello").unwrap();

        assert_eq!(load_cursor_position_from(&db, &path), None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cursor_position_prunes_deleted_files() {
        let dir = std::env::temp_dir().join("e_test_cursor_prune");
        let _ = fs::create_dir_all(&dir);
        let path_a = dir.join("a.txt");
        let path_b = dir.join("b.txt");
        let db = dir.join("cursor.bin");
        fs::write(&path_a, b"aaa").unwrap();
        fs::write(&path_b, b"bbb").unwrap();

        save_cursor_position_to(&db, &path_a, TextPosition::new(0, 1)).unwrap();
        save_cursor_position_to(&db, &path_b, TextPosition::new(3, 7)).unwrap();

        // Delete file_a
        fs::remove_file(&path_a).unwrap();

        // Saving file_b should prune file_a's entry
        save_cursor_position_to(&db, &path_b, TextPosition::new(4, 0)).unwrap();

        // Re-create file_a — old cursor entry should be gone
        fs::write(&path_a, b"aaa").unwrap();
        assert_eq!(load_cursor_position_from(&db, &path_a), None);
        assert_eq!(
            load_cursor_position_from(&db, &path_b),
            Some(TextPosition::new(4, 0))
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cursor_position_corrupt_db() {
        let dir = std::env::temp_dir().join("e_test_cursor_corrupt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        let db = dir.join("cursor.bin");
        fs::write(&path, b"hello").unwrap();
        fs::write(&db, b"garbage").unwrap();

        assert_eq!(load_cursor_position_from(&db, &path), None);

        let _ = fs::remove_dir_all(&dir);
    }
}
