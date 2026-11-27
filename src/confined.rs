//! `jig confined`: judge runner confinement by what the kernel refuses
//! (REQ-027). Environment labels such as ZWINGER_PROFILE are never evidence:
//! any earlier step can write them through GITHUB_ENV.

use std::path::Path;

/// One probe's outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    /// The kernel refused with EACCES or EPERM although Unix permissions allow it.
    Denied(&'static str),
    /// The operation succeeded: the job is not confined.
    Allowed,
    /// The probe cannot judge (target missing, permissions already deny it,
    /// another errno); the reason is printable and names no path.
    Unjudged(String),
}

#[derive(Debug)]
pub struct Report {
    pub write: Probe,
    pub read: Probe,
}

impl Report {
    /// 0 confined, 1 not confined, 2 could not judge.
    pub fn exit(&self) -> u8 {
        match (&self.write, &self.read) {
            (Probe::Allowed, _) | (_, Probe::Allowed) => 1,
            (Probe::Denied(_), Probe::Denied(_)) => 0,
            _ => 2,
        }
    }
}

#[cfg(unix)]
fn denial(e: &std::io::Error) -> Option<&'static str> {
    // Landlock refuses with EACCES. On Linux EPERM also comes from an
    // immutable or append-only attribute, which is not confinement, so it is
    // not counted there; elsewhere (the macOS sandbox) EPERM is the refusal.
    match e.raw_os_error() {
        Some(13) => Some("EACCES"),
        Some(1) if !cfg!(target_os = "linux") => Some("EPERM"),
        _ => None,
    }
}

#[cfg(unix)]
fn errno_name(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(n) => format!("errno {n} ({})", e.kind()),
        None => e.kind().to_string(),
    }
}

/// Whether Unix permissions let `uid` do `bit` (4 read, 2 write, 1 search)
/// on `m`, decided only where the answer is exact: a target the job's own
/// user owns (a POSIX ACL's owner entry is the owner bits). Root gets no
/// bypass: without CAP_DAC_OVERRIDE (a container) it is refused too. For
/// anyone else, group bits, named-user ACL entries and supplementary groups
/// would matter, and a refusal from them must not read as confinement, so
/// such a target is not judged at all.
#[cfg(unix)]
fn dac_allows(m: &std::fs::Metadata, uid: u32, bit: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    m.uid() == uid && m.mode() & (bit << 6) != 0
}

/// Positive control: a file created in the granted dir must be writable and
/// readable back. Returns the effective uid (the new file's owner).
#[cfg(unix)]
pub fn control(granted: &Path) -> Result<u32, String> {
    use std::io::{Read, Write};
    use std::os::unix::fs::MetadataExt;
    let p = granted.join(format!(".jig-confined-control-{}", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
        .map_err(|e| {
            format!(
                "positive control: cannot create a file in the granted directory: {}",
                errno_name(&e)
            )
        })?;
    let r = (|| {
        f.write_all(b"jig")
            .map_err(|e| format!("positive control: write failed: {}", errno_name(&e)))?;
        drop(f);
        let mut s = String::new();
        std::fs::File::open(&p)
            .and_then(|mut f| f.read_to_string(&mut s))
            .map_err(|e| format!("positive control: read back failed: {}", errno_name(&e)))?;
        if s != "jig" {
            return Err("positive control: read back other content".into());
        }
        std::fs::metadata(&p)
            .map(|m| m.uid())
            .map_err(|e| format!("positive control: stat failed: {}", errno_name(&e)))
    })();
    let _ = std::fs::remove_file(&p);
    r
}

/// Try to create a new file at `target` (a unique name is appended).
#[cfg(unix)]
pub fn probe_write(target: &Path, uid: u32) -> Probe {
    let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Probe::Unjudged("write target has no parent directory".into());
    };
    let pm = match std::fs::metadata(parent) {
        Ok(m) => m,
        // A failed stat is never counted: an ancestor without search
        // permission refuses it too, and Landlock does not restrict stat.
        Err(e) => {
            return Probe::Unjudged(format!("write target's directory: {}", errno_name(&e)));
        }
    };
    if !pm.is_dir() {
        return Probe::Unjudged("write target's parent is not a directory".into());
    }
    // Write plus search on the directory: without both, DAC already refuses.
    if !(dac_allows(&pm, uid, 2) && dac_allows(&pm, uid, 1)) {
        return Probe::Unjudged(
            "Unix permissions do not plainly allow writing the target's directory \
             (not owned by the job's user with owner write and search); a refusal \
             there proves nothing"
                .into(),
        );
    }
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}", std::process::id()));
    let p = parent.join(name);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&p)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&p);
            Probe::Allowed
        }
        Err(e) => match denial(&e) {
            Some(d) => Probe::Denied(d),
            None => Probe::Unjudged(format!("write probe: {}", errno_name(&e))),
        },
    }
}

/// Try to open `target` for reading (a directory is listed).
#[cfg(unix)]
pub fn probe_read(target: &Path, uid: u32) -> Probe {
    let m = match std::fs::metadata(target) {
        Ok(m) => m,
        // As for the write target: a failed stat is never counted.
        Err(e) => {
            return Probe::Unjudged(format!("read target: {}", errno_name(&e)));
        }
    };
    let need = if m.is_dir() { [4, 1] } else { [4, 4] };
    if !need.iter().all(|b| dac_allows(&m, uid, *b)) {
        return Probe::Unjudged(
            "Unix permissions do not plainly allow reading the target (not owned by \
             the job's user with owner read); a refusal there proves nothing"
                .into(),
        );
    }
    let r = if m.is_dir() {
        std::fs::read_dir(target).map(|_| ())
    } else {
        std::fs::File::open(target).map(|_| ())
    };
    match r {
        Ok(()) => Probe::Allowed,
        Err(e) => match denial(&e) {
            Some(d) => Probe::Denied(d),
            None => Probe::Unjudged(format!("read probe: {}", errno_name(&e))),
        },
    }
}

#[cfg(unix)]
pub fn check(granted: &Path, write: &Path, read: &Path) -> Result<Report, String> {
    let uid = control(granted)?;
    Ok(Report {
        write: probe_write(write, uid),
        read: probe_read(read, uid),
    })
}

#[cfg(not(unix))]
pub fn check(_: &Path, _: &Path, _: &Path) -> Result<Report, String> {
    Err("jig confined runs only on a Unix runner".into())
}

pub fn line(name: &str, p: &Probe) -> String {
    match p {
        Probe::Denied(d) => format!("{name} probe: refused by the kernel ({d}): confined"),
        Probe::Allowed => format!("{name} probe: succeeded: NOT confined"),
        Probe::Unjudged(why) => format!("{name} probe: cannot judge: {why}"),
    }
}
