//! Small, portable notes kept alongside the vault's other files.

use std::{fs, io, path::Path};

use anyhow::{Context, Result};

pub fn load_home_note(vault: &Path) -> Result<String> {
    match fs::read_to_string(vault.join("notes/home.md")) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).context("メモを読み込めませんでした"),
    }
}

pub fn save_home_note(vault: &Path, text: &str) -> Result<()> {
    let directory = vault.join("notes");
    fs::create_dir_all(&directory).context("メモの保存先を作成できませんでした")?;
    let temporary = directory.join(format!(".home-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::write(&temporary, text)?;
        fs::rename(&temporary, directory.join("home.md"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.context("メモを保存できませんでした")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_survive_reopening_and_are_isolated_per_vault() -> Result<()> {
        let first = tempfile::tempdir()?;
        let second = tempfile::tempdir()?;
        assert_eq!(load_home_note(first.path())?, "");
        save_home_note(first.path(), "明日の準備\n- 資料を確認")?;
        assert_eq!(load_home_note(first.path())?, "明日の準備\n- 資料を確認");
        assert_eq!(load_home_note(second.path())?, "");
        save_home_note(first.path(), "更新したメモ")?;
        assert_eq!(load_home_note(first.path())?, "更新したメモ");
        save_home_note(first.path(), "")?;
        assert_eq!(load_home_note(first.path())?, "");
        Ok(())
    }

    #[test]
    fn unreadable_content_is_reported_instead_of_being_treated_as_empty() -> Result<()> {
        let vault = tempfile::tempdir()?;
        fs::create_dir(vault.path().join("notes"))?;
        fs::write(vault.path().join("notes/home.md"), [0xff, 0xfe])?;
        assert!(load_home_note(vault.path()).is_err());
        Ok(())
    }
}
