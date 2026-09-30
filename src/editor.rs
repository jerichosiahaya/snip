use std::process::Command;

/// Open `$EDITOR` (fallback nano) on a temp markdown file seeded with `initial`.
/// Returns the file's final contents and whether it changed.
pub fn edit(initial: &str, file_name: &str) -> std::io::Result<(String, bool)> {
    let dir = std::env::temp_dir();
    let path = dir.join(file_name);
    std::fs::write(&path, initial)?;

    let editor = std::env::var("EDITOR").or_else(|_| std::env::var("VISUAL")).unwrap_or_else(|_| "nano".into());
    let status = Command::new(&editor).arg(&path).status()?;
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("editor exited with {status}"),
        ));
    }
    let final_txt = std::fs::read_to_string(&path)?;
    std::fs::remove_file(&path).ok();
    let changed = final_txt != initial;
    Ok((final_txt, changed))
}