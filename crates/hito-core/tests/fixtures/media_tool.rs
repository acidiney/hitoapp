use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn script(directory: &Path, tool: &str, body: &str) -> PathBuf {
    let path = directory.join(format!("test-{tool}"));
    std::fs::write(&path, format!(
        "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then exec /usr/bin/{tool} \"$@\"; fi\n{body}\n",
    )).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

pub fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
