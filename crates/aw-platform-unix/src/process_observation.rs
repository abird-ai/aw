//! Trustworthy direct Linux process-group observation.
//!
//! Positive evidence is cheap. A negative is accepted only when the procfs
//! used for enumeration describes this PID namespace and is not PID-filtered.
use std::{
    fs,
    io::{self, BufRead},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};

fn mount_id(file: &fs::File) -> io::Result<u64> {
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid procfs mount ID"))
}

fn state(path: &Path, group: i32) -> io::Result<Option<u8>> {
    let stat = fs::read(path.join("stat"))?;
    // comm is arbitrary bytes and may contain ')'. The last ')' ends it;
    // everything after that is kernel-generated ASCII state/ppid/pgrp/...
    let fields = stat
        .iter()
        .rposition(|byte| *byte == b')')
        .and_then(|index| stat.get(index + 2..));
    let mut fields = fields.unwrap_or_default().split(|byte| *byte == b' ');
    let (Some([state]), Some(pgrp)) = (fields.next(), fields.nth(1)) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "malformed proc stat",
        ));
    };
    let pgrp = std::str::from_utf8(pgrp)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .ok_or(io::ErrorKind::InvalidData)?;
    Ok((pgrp == group).then_some(*state))
}

fn vanished(path: &Path) -> bool {
    matches!(path.try_exists(), Ok(false))
}

fn live(path: &Path, group: i32) -> io::Result<bool> {
    match state(path, group)? {
        None => Ok(false),
        Some(b'X' | b'Z') => {
            for thread in fs::read_dir(path.join("task"))? {
                let path = thread?.path();
                match state(&path, group) {
                    Ok(Some(state)) if state != b'X' && state != b'Z' => return Ok(true),
                    Err(error) if !vanished(&path) => return Err(error),
                    _ => {}
                }
            }
            Ok(false)
        }
        Some(_) => Ok(true),
    }
}

fn require_same_pid_namespace(proc: &Path, own: u32) -> io::Result<()> {
    let status = fs::read(proc.join("self/status"))?;
    let expected = own.to_string();
    let same = match status
        .split(|byte| *byte == b'\n')
        .find_map(|line| line.strip_prefix(b"NStgid:"))
    {
        Some(ids) => ids
            .split(u8::is_ascii_whitespace)
            .filter(|id| !id.is_empty())
            .eq([expected.as_bytes()]),
        None => {
            let init = fs::metadata(proc.join("1/ns/pid"))?;
            let me = fs::metadata(proc.join("self/ns/pid"))?;
            (init.dev(), init.ino()) == (me.dev(), me.ino())
        }
    };
    if same {
        Ok(())
    } else {
        Err(io::Error::other(
            "/proc does not describe the command PID namespace",
        ))
    }
}

fn unfiltered_proc_mount(mounts: impl BufRead, visible_id: u64) -> io::Result<bool> {
    let id = visible_id.to_string();
    for line in mounts.split(b'\n') {
        let line = line?;
        let mut fields = line.split(|byte| *byte == b' ');
        if fields.next() != Some(id.as_bytes()) {
            continue;
        }
        if fields.nth(2) != Some(&b"/"[..])
            || fields.next() != Some(&b"/proc"[..])
            || !fields.any(|field| field == b"-")
            || fields.next() != Some(&b"proc"[..])
        {
            return Ok(false);
        }
        let Some(options) = fields.nth(1) else {
            return Ok(false);
        };
        return Ok(!options.is_empty()
            && options.split(|byte| *byte == b',').all(|option| {
                option
                    .strip_prefix(b"hidepid=")
                    .is_none_or(|mode| matches!(mode, b"0" | b"off"))
            }));
    }
    Ok(false)
}

pub(crate) fn group_has_live_member(group: i32) -> io::Result<bool> {
    let root = fs::File::open("/proc")?;
    let root_path = format!("/proc/self/fd/{}", root.as_raw_fd());
    let proc = Path::new(&root_path);
    require_same_pid_namespace(proc, std::process::id())?;

    let mut incomplete = None;
    for entry in fs::read_dir(proc)? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<i32>().ok())
        else {
            continue;
        };
        // The waited leader is terminal but deliberately unreaped by settlement.
        if pid == group {
            continue;
        }
        // SAFETY: numeric PID query only, no pointers or side effects.
        if unsafe { libc::getpgid(pid) } != group {
            continue;
        }
        let path = entry.path();
        match live(&path, group) {
            Ok(true) => return Ok(true),
            Err(error) if !vanished(&path) => incomplete = Some(error),
            _ => {}
        }
    }

    if let Some(error) = incomplete {
        return Err(error);
    }
    let mounts = io::BufReader::new(fs::File::open(proc.join("self/mountinfo"))?);
    if unfiltered_proc_mount(mounts, mount_id(&root)?)? {
        Ok(false)
    } else {
        Err(io::Error::other("incomplete procfs process visibility"))
    }
}
