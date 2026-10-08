//! Fonts the person imports for the interface or the code and terminal text. Files are copied into the
//! app data folder, checked by their magic bytes, and handed back to the webview as raw bytes so each
//! window can register them with `FontFace`. Nothing leaves the computer.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{ipc::Response, AppHandle, Manager};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FONTS: usize = 20;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomFont {
    pub id: String,
    pub name: String,
    pub file: String,
}

fn directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("fonts"))
        .map_err(|error| error.to_string())
}

fn read_index(directory: &Path) -> Vec<CustomFont> {
    fs::read_to_string(directory.join("index.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_index(directory: &Path, fonts: &[CustomFont]) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let text = serde_json::to_string_pretty(fonts).map_err(|error| error.to_string())?;
    fs::write(directory.join("index.json"), text).map_err(|error| error.to_string())
}

/// The font container formats browsers accept, recognised by their first bytes.
fn extension(bytes: &[u8]) -> Option<&'static str> {
    match bytes.get(0..4)? {
        b"wOF2" => Some("woff2"),
        b"wOFF" => Some("woff"),
        b"OTTO" => Some("otf"),
        [0, 1, 0, 0] | b"true" => Some("ttf"),
        _ => None,
    }
}

fn slug(name: &str) -> String {
    let mut text = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            text.push(character.to_ascii_lowercase());
        } else if !text.ends_with('-') && !text.is_empty() {
            text.push('-');
        }
    }
    let text = text.trim_matches('-').chars().take(40).collect::<String>();
    if text.is_empty() {
        "font".into()
    } else {
        text
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
}

#[tauri::command]
pub fn list_custom_fonts(app: AppHandle) -> Result<Vec<CustomFont>, String> {
    let directory = directory(&app)?;
    Ok(read_index(&directory)
        .into_iter()
        .filter(|font| directory.join(&font.file).is_file())
        .collect())
}

#[tauri::command]
pub fn import_custom_font(app: AppHandle, path: String) -> Result<CustomFont, String> {
    let source = PathBuf::from(&path);
    let metadata = fs::metadata(&source).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Choose a font file".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("The font file is larger than 8 MB".into());
    }
    let bytes = fs::read(&source).map_err(|error| error.to_string())?;
    let extension = extension(&bytes).ok_or("This is not a TTF, OTF, WOFF, or WOFF2 font")?;
    let directory = directory(&app)?;
    let mut fonts = read_index(&directory);
    if fonts.len() >= MAX_FONTS {
        return Err("Remove an imported font before adding another".into());
    }
    let stem = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("font");
    let base = slug(stem);
    let mut id = base.clone();
    let mut counter = 2;
    while fonts.iter().any(|font| font.id == id) {
        id = format!("{base}-{counter}");
        counter += 1;
    }
    let file = format!("{id}.{extension}");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    fs::write(directory.join(&file), &bytes).map_err(|error| error.to_string())?;
    let font = CustomFont {
        id,
        name: stem.chars().take(60).collect(),
        file,
    };
    fonts.push(font.clone());
    write_index(&directory, &fonts)?;
    Ok(font)
}

#[tauri::command]
pub fn read_custom_font(app: AppHandle, id: String) -> Result<Response, String> {
    if !valid_id(&id) {
        return Err("Unknown font".into());
    }
    let directory = directory(&app)?;
    let font = read_index(&directory)
        .into_iter()
        .find(|font| font.id == id)
        .ok_or("Unknown font")?;
    fs::read(directory.join(font.file))
        .map(Response::new)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn remove_custom_font(app: AppHandle, id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Unknown font".into());
    }
    let directory = directory(&app)?;
    let mut fonts = read_index(&directory);
    if let Some(position) = fonts.iter().position(|font| font.id == id) {
        let font = fonts.remove(position);
        let _ = fs::remove_file(directory.join(font.file));
        write_index(&directory, &fonts)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_font_containers_by_their_first_bytes() {
        assert_eq!(extension(b"wOF2\0\0"), Some("woff2"));
        assert_eq!(extension(b"wOFF\0\0"), Some("woff"));
        assert_eq!(extension(b"OTTO\0\0"), Some("otf"));
        assert_eq!(extension(&[0, 1, 0, 0, 9]), Some("ttf"));
        assert_eq!(extension(b"%PDF-1"), None);
        assert_eq!(extension(b"ab"), None);
    }

    #[test]
    fn identifiers_are_safe_file_names() {
        assert_eq!(slug("Fira Code Retina!"), "fira-code-retina");
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("日本語"), "font");
        assert!(valid_id("fira-code-2"));
        assert!(!valid_id("../x"));
        assert!(!valid_id("A"));
    }
}
