use std::path::Path;

pub(super) fn unreliable_for_native_events(path: &Path) -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        let mounts = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
        classify(&mounts, path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn classify(mountinfo: &str, path: &Path) -> Option<&'static str> {
    let mut best: Option<(usize, &str)> = None;
    for line in mountinfo.lines() {
        let Some((before, after)) = line.split_once(" - ") else {
            continue;
        };
        let Some(mount) = before.split(' ').nth(4) else {
            continue;
        };
        let mount = unescape(mount);
        let Some(fs_type) = after.split(' ').next() else {
            continue;
        };
        if path.starts_with(&mount) && best.is_none_or(|(length, _)| mount.len() >= length) {
            best = Some((mount.len(), fs_type));
        }
    }
    let (_, fs_type) = best?;
    match fs_type {
        "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "afs" | "9p" | "ncpfs" | "vboxsf" => {
            Some("network_volume")
        }
        "vfat" | "exfat" | "msdos" | "fuseblk" => Some("removable_volume"),
        "virtiofs" => Some("virtual_volume"),
        other if other.starts_with("fuse.") => Some("virtual_volume"),
        _ => None,
    }
}

fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && let Some(digits) = field.get(index + 1..index + 4)
            && let Ok(value) = u8::from_str_radix(digits, 8)
        {
            out.push(value);
            index += 4;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::classify;
    use std::path::Path;

    const MOUNTS: &str = "\
24 1 0:21 / / rw - overlay overlay rw
30 24 0:40 / /music rw - nfs4 server:/music rw
31 24 0:41 / /media/usb\\040drive rw - vfat /dev/sdb1 rw
32 24 0:42 / /scratch rw - ext4 /dev/sda1 rw
";

    #[test]
    fn the_longest_mount_decides_the_volume_kind() {
        assert_eq!(classify(MOUNTS, Path::new("/music/jazz")), Some("network_volume"));
        assert_eq!(
            classify(MOUNTS, Path::new("/media/usb drive/set")),
            Some("removable_volume")
        );
        assert_eq!(classify(MOUNTS, Path::new("/scratch/x")), None);
        assert_eq!(classify(MOUNTS, Path::new("/home/x")), None);
    }
}
