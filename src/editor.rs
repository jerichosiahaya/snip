use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// Open `$EDITOR` (then `$VISUAL`, fallback nano) on a temp markdown file seeded with `initial`.
/// Returns the file's final contents and whether it changed.
pub fn edit(initial: &str, prefix: &str) -> io::Result<(String, bool)> {
    let editor = ["EDITOR", "VISUAL"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "nano".into());
    edit_with(&editor, initial, prefix)
}

/// Like [`edit`], with an explicit editor command. The command is run by the shell,
/// so it may carry arguments (`code -w`, `vim -u NONE`), the same as git does.
pub fn edit_with(editor: &str, initial: &str, prefix: &str) -> io::Result<(String, bool)> {
    let file = TempFile::create(prefix, initial)?;
    let status = editor_command(editor, &file.0).status()?;
    if !status.success() {
        return Err(io::Error::other(format!("editor exited with {status}")));
    }
    let final_txt = std::fs::read_to_string(&file.0)?;
    let changed = final_txt != initial;
    Ok((final_txt, changed))
}

#[cfg(unix)]
fn editor_command(editor: &str, path: &Path) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(format!("{editor} \"$1\"")).arg("sh").arg(path);
    cmd
}

#[cfg(not(unix))]
fn editor_command(editor: &str, path: &Path) -> Command {
    let mut parts = editor.split_whitespace();
    let mut cmd = Command::new(parts.next().unwrap_or("notepad"));
    cmd.args(parts).arg(path);
    cmd
}

/// A temp file with a unique name, readable only by the user, removed on drop.
struct TempFile(PathBuf);

impl TempFile {
    fn create(prefix: &str, contents: &str) -> io::Result<TempFile> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        for _ in 0..100 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("{prefix}-{}-{nanos}-{n}.md", std::process::id()));
            // create_new refuses existing paths, including planted symlinks.
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            match opts.open(&path) {
                Ok(mut f) => {
                    let file = TempFile(path);
                    f.write_all(contents.as_bytes())?;
                    return Ok(file);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free temp file name"))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
