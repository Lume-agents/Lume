//! Project notes kept as Markdown files, one per note, so any editor or agent can read and change them.
//!
//! Layout under `root`:
//! - `<folder>-<hash>/<slug>.md`: the notes of one project; `.lume-project` in the folder holds the project's full path.
//! - `_sem-projeto/<slug>.md`: notes without a project.
//! - `.trash/`: deleted notes, kept so a deletion can be undone by hand.
//!
//! Each file may start with a small frontmatter (`key: value` lines between `---`). A file written by hand
//! without one is still a note: its title comes from a leading `# ` heading or the file name.

use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, UNIX_EPOCH},
};

use notify::{RecursiveMode, Watcher};

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use crate::domain::SessionNote;

const PROJECT_MARKER: &str = ".lume-project";
const NO_PROJECT_FOLDER: &str = "_sem-projeto";
const TRASH_FOLDER: &str = ".trash";
/// Files larger than this are not taken for notes.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    note: SessionNote,
}

pub struct NoteFiles {
    root: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
    /// Kept alive while Lume watches the folder for edits made outside it.
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

impl NoteFiles {
    pub fn open(root: &Path) -> Result<Self, String> {
        create_private_dir(root)?;
        let root = fs::canonicalize(root).map_err(|error| format!("Could not open the notes folder: {error}"))?;
        let files = Self { root, entries: Mutex::new(HashMap::new()), watcher: Mutex::new(None) };
        files.refresh()?;
        Ok(files)
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Reads the folder again, so edits made outside Lume are seen. True when a note was added, changed or removed.
    pub fn refresh(&self) -> Result<bool, String> {
        let mut entries = self.entries.lock().map_err(|_| "Could not access the notes folder".to_string())?;
        let scanned = scan(&self.root);
        let fingerprint = |entries: &HashMap<String, Entry>| {
            let mut items = entries
                .iter()
                .map(|(id, entry)| (id.clone(), entry.path.clone(), entry.note.revision.clone()))
                .collect::<Vec<_>>();
            items.sort();
            items
        };
        let changed = fingerprint(&entries) != fingerprint(&scanned);
        *entries = scanned;
        Ok(changed)
    }

    /// Calls `on_change` (from another thread) when a note file is added, changed or removed outside Lume.
    /// Lume's own saves are already in the cache, so they do not call it.
    pub fn watch(self: &Arc<Self>, on_change: impl Fn() + Send + 'static) -> Result<(), String> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if event.is_ok() {
                let _ = sender.send(());
            }
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(&self.root, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;
        *self.watcher.lock().map_err(|_| "Could not watch the notes folder".to_string())? = Some(watcher);
        let files = Arc::downgrade(self);
        thread::Builder::new()
            .name("lume-note-files".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    // An editor's save is several events (a temporary file, a rename): wait for them to settle.
                    while receiver.recv_timeout(Duration::from_millis(250)).is_ok() {}
                    let Some(files) = files.upgrade() else { break };
                    if files.refresh().unwrap_or(false) {
                        on_change();
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// The folder of a project's notes, or the folder of every note.
    pub fn folder(&self, project: Option<&str>) -> Result<PathBuf, String> {
        match project.map(str::trim).filter(|project| !project.is_empty()) {
            Some(project) => self.project_dir(Some(project)),
            None => Ok(self.root.clone()),
        }
    }

    /// Saves a note brought from the database, keeping when it was last changed as the file's date.
    pub fn import(&self, note: &SessionNote) -> Result<SessionNote, String> {
        let saved = self.save(note)?;
        let Some(modified) = u64::try_from(note.updated_at).ok().filter(|millis| *millis > 0) else {
            return Ok(saved);
        };
        let mut entries = self.entries.lock().map_err(|_| "Could not access the notes folder".to_string())?;
        let Some(entry) = entries.get_mut(&saved.id) else { return Ok(saved) };
        let time = UNIX_EPOCH + Duration::from_millis(modified);
        if fs::File::options().write(true).open(&entry.path).and_then(|file| file.set_modified(time)).is_ok() {
            entry.note.updated_at = note.updated_at;
        }
        Ok(entry.note.clone())
    }

    pub fn list(&self) -> Result<Vec<SessionNote>, String> {
        Ok(self
            .entries
            .lock()
            .map_err(|_| "Could not access the notes folder".to_string())?
            .values()
            .map(|entry| entry.note.clone())
            .collect())
    }

    /// The note as its file reads now, so a note attached to a prompt carries edits made outside Lume.
    pub fn get(&self, id: &str) -> Result<Option<SessionNote>, String> {
        let mut entries = self.entries.lock().map_err(|_| "Could not access the notes folder".to_string())?;
        let Some(entry) = entries.get(id).cloned() else {
            return Ok(None);
        };
        let project = folder_project(entry.path.parent().unwrap_or(&self.root));
        match read_note(&self.root, &entry.path, project.as_deref()) {
            Some(mut note) => {
                // The id the note was found by stays, even if the file's own id was changed by hand.
                note.id = id.to_string();
                entries.insert(id.to_string(), Entry { path: entry.path, note: note.clone() });
                Ok(Some(note))
            }
            None => {
                entries.remove(id);
                Ok(None)
            }
        }
    }

    /// Writes the note to its file, or to a new one named after its title. Returns the note as saved.
    pub fn save(&self, note: &SessionNote) -> Result<SessionNote, String> {
        let mut entries = self.entries.lock().map_err(|_| "Could not access the notes folder".to_string())?;
        let directory = self.project_dir(note.project.as_deref())?;
        let previous = entries.get(&note.id).map(|entry| entry.path.clone());
        // A note keeps its file, even when renamed, unless it moved to another project.
        let path = match &previous {
            Some(path) if path.parent() == Some(directory.as_path()) => path.clone(),
            _ => free_path(&directory, &slug(&note.title)),
        };
        write_atomically(&path, &serialize(note))?;
        if let Some(previous) = previous.filter(|previous| *previous != path) {
            let _ = fs::remove_file(previous);
        }
        let saved = self.read_saved(&path, note)?;
        entries.insert(saved.id.clone(), Entry { path, note: saved.clone() });
        Ok(saved)
    }

    /// Moves the note's file to `.trash/`. Returns false when no file holds that note.
    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let mut entries = self.entries.lock().map_err(|_| "Could not access the notes folder".to_string())?;
        let Some(entry) = entries.remove(id) else {
            return Ok(false);
        };
        let trash = self.root.join(TRASH_FOLDER);
        create_private_dir(&trash)?;
        let folder = entry
            .path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = entry.path.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let target = trash.join(format!("{folder}-{stem}-{}.md", crate::state::now_millis()));
        fs::rename(&entry.path, &target).map_err(|error| format!("Could not delete the note: {error}"))?;
        Ok(true)
    }

    /// The folder of a project's notes, created with its `.lume-project` marker on first use.
    fn project_dir(&self, project: Option<&str>) -> Result<PathBuf, String> {
        let project = project.map(str::trim).filter(|project| !project.is_empty());
        let directory = match project {
            Some(project) => self.root.join(project_folder_name(project)),
            None => self.root.join(NO_PROJECT_FOLDER),
        };
        if !directory.is_dir() {
            create_private_dir(&directory)?;
        }
        if let Some(project) = project {
            let marker = directory.join(PROJECT_MARKER);
            if !marker.is_file() {
                write_atomically(&marker, project)?;
            }
        }
        Ok(directory)
    }

    /// The note as it reads back from disk, so `updated_at` matches the file.
    fn read_saved(&self, path: &Path, fallback: &SessionNote) -> Result<SessionNote, String> {
        let project = folder_project(path.parent().unwrap_or(&self.root));
        Ok(read_note(&self.root, path, project.as_deref()).unwrap_or_else(|| fallback.clone()))
    }
}

/// `Lume-3f9a2c`: the folder's own name, made safe, and 6 characters of the full path's hash.
fn project_folder_name(project: &str) -> String {
    let name = Path::new(project)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.to_string());
    let mut name = slug(&name);
    if name == "nota" {
        name = "projeto".into();
    }
    let hash = hex(&Sha256::digest(project.as_bytes()));
    format!("{name}-{}", &hash[..6])
}

fn scan(root: &Path) -> HashMap<String, Entry> {
    let mut entries = HashMap::new();
    let mut add = |path: PathBuf, project: Option<&str>| {
        let Some(mut note) = read_note(root, &path, project) else { return };
        // A copied file keeps the id of the original; the copy is told apart by its path.
        if entries.contains_key(&note.id) {
            note.id = path_id(root, &path);
        }
        entries.insert(note.id.clone(), Entry { path, note });
    };
    for path in markdown_files(root) {
        add(path, None);
    }
    let Ok(children) = fs::read_dir(root) else { return entries };
    for child in children.flatten() {
        let directory = child.path();
        if is_hidden(&directory) || !is_plain_dir(&directory) {
            continue;
        }
        let project = folder_project(&directory);
        for path in markdown_files(&directory) {
            add(path, project.as_deref());
        }
    }
    entries
}

/// The visible `.md` files of a folder, without following links.
fn markdown_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(children) = fs::read_dir(directory) else { return Vec::new() };
    let mut files = children
        .flatten()
        .map(|child| child.path())
        .filter(|path| {
            !is_hidden(path)
                && path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_FILE_BYTES)
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn is_plain_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

fn folder_project(directory: &Path) -> Option<String> {
    fs::read_to_string(directory.join(PROJECT_MARKER))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn path_id(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    format!("file:{}", relative.to_string_lossy().replace('\\', "/"))
}

fn read_note(root: &Path, path: &Path, folder_project: Option<&str>) -> Option<SessionNote> {
    let text = fs::read_to_string(path).ok()?;
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default();
    let mut note = parse(&text, path.file_stem()?.to_string_lossy().as_ref());
    if note.id.is_empty() {
        note.id = path_id(root, path);
    }
    if note.project.is_none() {
        note.project = folder_project.map(str::to_string);
    }
    note.updated_at = modified;
    note.revision = Some(hex(&Sha256::digest(text.as_bytes()))[..16].to_string());
    if note.created_at == 0 {
        note.created_at = modified;
    }
    Some(note)
}

/// Reads a note file. `fallback_title` (the file name) names a note that has neither a title nor a heading.
pub fn parse(text: &str, fallback_title: &str) -> SessionNote {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    let mut note = SessionNote { kind: "note".into(), ..SessionNote::default() };
    let body = match split_frontmatter(&text) {
        Some((header, body)) => {
            for line in header.lines() {
                let Some((key, value)) = line.split_once(':') else { continue };
                let value = value.trim();
                match key.trim() {
                    "id" => note.id = scalar(value),
                    "project" => note.project = Some(scalar(value)).filter(|value| !value.is_empty()),
                    "title" => note.title = scalar(value),
                    "kind" => note.kind = if scalar(value) == "plan" { "plan".into() } else { "note".into() },
                    "pinned" => note.pinned = scalar(value) == "true",
                    "tags" => note.tags = list(value),
                    "agent" => note.agent = Some(scalar(value)).filter(|value| !value.is_empty()),
                    "created" => {
                        note.created_at = DateTime::parse_from_rfc3339(&scalar(value))
                            .map(|time| time.timestamp_millis())
                            .unwrap_or_default()
                    }
                    "prompt_activity_id" => note.prompt_activity_id = Some(scalar(value)).filter(|value| !value.is_empty()),
                    "prompt_excerpt" => note.prompt_excerpt = Some(scalar(value)).filter(|value| !value.is_empty()),
                    "source_activity_id" => note.source_activity_id = Some(scalar(value)).filter(|value| !value.is_empty()),
                    _ => {}
                }
            }
            body
        }
        None => text.as_str(),
    };
    note.body = body.trim().to_string();
    if note.title.is_empty() {
        note.title = note
            .body
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("# "))
            .map(|heading| heading.trim().to_string())
            .filter(|heading| !heading.is_empty())
            .unwrap_or_else(|| fallback_title.to_string());
    }
    note
}

/// The note as file text: the frontmatter, a blank line and the body.
pub fn serialize(note: &SessionNote) -> String {
    let mut text = String::from("---\n");
    let mut field = |key: &str, value: &str| text.push_str(&format!("{key}: {}\n", quote(value)));
    field("id", &note.id);
    if let Some(project) = &note.project {
        field("project", project);
    }
    field("title", &note.title);
    if note.kind == "plan" {
        field("kind", "plan");
    }
    if note.pinned {
        field("pinned", "true");
    }
    if let Some(agent) = &note.agent {
        field("agent", agent);
    }
    if let Some(value) = &note.prompt_activity_id {
        field("prompt_activity_id", value);
    }
    if let Some(value) = &note.prompt_excerpt {
        field("prompt_excerpt", value);
    }
    if let Some(value) = &note.source_activity_id {
        field("source_activity_id", value);
    }
    if let Some(created) = DateTime::<Utc>::from_timestamp_millis(note.created_at).filter(|_| note.created_at > 0) {
        field("created", &created.to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    if !note.tags.is_empty() {
        text.push_str(&format!("tags: {}\n", serde_json::to_string(&note.tags).unwrap_or_else(|_| "[]".into())));
    }
    text.push_str("---\n\n");
    text.push_str(note.body.trim());
    text.push('\n');
    text
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    if let Some(body) = rest.strip_prefix("---\n") {
        return Some(("", body));
    }
    let end = rest.find("\n---\n").map(|index| (index, index + 5)).or_else(|| {
        rest.strip_suffix("\n---").map(|header| (header.len(), rest.len()))
    })?;
    Some((&rest[..end.0], &rest[end.1..]))
}

/// A value as written: plain when it reads back the same, JSON-quoted otherwise.
fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value.trim() == value
        && !value.contains(['\n', '\r', '#'])
        && !value.starts_with(['"', '\'', '[', '{', '-', '&', '*', '!', '|', '>', '%', '@', '`']);
    if plain {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_default()
    }
}

fn scalar(value: &str) -> String {
    if value.starts_with('"') {
        if let Ok(text) = serde_json::from_str::<String>(value) {
            return text;
        }
    }
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].replace("''", "'");
    }
    value.to_string()
}

fn list(value: &str) -> Vec<String> {
    if let Ok(items) = serde_json::from_str::<Vec<String>>(value) {
        return items;
    }
    value
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| scalar(item.trim()))
        .filter(|item| !item.is_empty())
        .collect()
}

/// A file name from a title: lowercase ASCII, words joined by `-`, at most 60 characters.
pub fn slug(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars().flat_map(char::to_lowercase) {
        let character = match character {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            character => character,
        };
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 60 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() { "nota".into() } else { slug }
}

/// `name.md`, or `name-2.md`, `name-3.md`... when taken.
fn free_path(directory: &Path, name: &str) -> PathBuf {
    let mut path = directory.join(format!("{name}.md"));
    let mut index = 2;
    while fs::symlink_metadata(&path).is_ok() {
        path = directory.join(format!("{name}-{index}.md"));
        index += 1;
    }
    path
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn create_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("Could not create the notes folder: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Writes a temporary file beside `path` and renames it over, so a crash never leaves half a note.
fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("Could not save the note: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("lume-note-files-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn note(id: &str, title: &str, body: &str, project: Option<&str>) -> SessionNote {
        SessionNote {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            kind: "note".into(),
            project: project.map(str::to_string),
            created_at: 1_794_840_539_000,
            ..SessionNote::default()
        }
    }

    #[test]
    fn a_note_reads_back_as_it_was_written() {
        let mut original = note("project-note:1", "Pendências: v2 #1", "- [ ] a\n\n---\n\nb", Some("/home/a/Lume"));
        original.pinned = true;
        original.tags = vec!["lume".into(), "a, b".into()];
        original.prompt_excerpt = Some("\"quoted\" text".into());
        let parsed = parse(&serialize(&original), "fallback");
        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.title, original.title);
        assert_eq!(parsed.body, original.body);
        assert_eq!(parsed.project, original.project);
        assert_eq!(parsed.tags, original.tags);
        assert_eq!(parsed.prompt_excerpt, original.prompt_excerpt);
        assert!(parsed.pinned);
        assert_eq!(parsed.created_at, original.created_at);
    }

    #[test]
    fn a_file_without_frontmatter_takes_its_title_from_the_heading_or_the_name() {
        let headed = parse("# Ideias\n\ntexto", "arquivo");
        assert_eq!(headed.title, "Ideias");
        assert_eq!(headed.body, "# Ideias\n\ntexto");
        assert_eq!(parse("só texto", "arquivo").title, "arquivo");
        assert_eq!(parse("---\nnão é frontmatter", "arquivo").body, "---\nnão é frontmatter");
    }

    #[test]
    fn slugs_are_plain_ascii() {
        assert_eq!(slug("Pendências do Lume!"), "pendencias-do-lume");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("???"), "nota");
    }

    #[test]
    fn notes_are_saved_listed_renamed_and_moved_to_the_trash() {
        let folder = Folder::new("roundtrip");
        let files = NoteFiles::open(&folder.0).unwrap();
        let saved = files.save(&note("project-note:1", "Pendências", "texto", Some("/home/a/Lume"))).unwrap();
        let path = files.entries.lock().unwrap()["project-note:1"].path.clone();
        assert_eq!(path.file_name().unwrap(), "pendencias.md");
        assert_eq!(folder_project(path.parent().unwrap()).as_deref(), Some("/home/a/Lume"));
        assert!(saved.updated_at > 0);

        // A second note with the same title gets its own file.
        files.save(&note("project-note:2", "Pendências", "outro", Some("/home/a/Lume"))).unwrap();
        assert_eq!(files.entries.lock().unwrap()["project-note:2"].path.file_name().unwrap(), "pendencias-2.md");

        // A new title keeps the file; another project moves it.
        files.save(&note("project-note:1", "Renomeada", "texto", Some("/home/a/Lume"))).unwrap();
        assert_eq!(files.entries.lock().unwrap()["project-note:1"].path, path);
        files.save(&note("project-note:1", "Renomeada", "texto", None)).unwrap();
        assert!(!path.exists());

        files.refresh().unwrap();
        assert_eq!(files.list().unwrap().len(), 2);
        assert!(files.delete("project-note:1").unwrap());
        assert!(!files.delete("project-note:1").unwrap());
        files.refresh().unwrap();
        assert_eq!(files.list().unwrap().len(), 1);
        assert_eq!(fs::read_dir(files.root().join(TRASH_FOLDER)).unwrap().count(), 1);
    }

    #[test]
    fn hand_written_files_and_copies_are_notes_with_their_own_ids() {
        let folder = Folder::new("handwritten");
        let files = NoteFiles::open(&folder.0).unwrap();
        files.save(&note("project-note:1", "Original", "texto", Some("/p"))).unwrap();
        let directory = files.root().join(project_folder_name("/p"));
        fs::copy(directory.join("original.md"), directory.join("copia.md")).unwrap();
        fs::write(directory.join("solta.md"), "# Solta\n\nescrita à mão").unwrap();
        fs::write(directory.join("ignorada.txt"), "não é nota").unwrap();
        files.refresh().unwrap();
        let mut ids = files.list().unwrap().into_iter().map(|note| (note.id, note.project)).collect::<Vec<_>>();
        ids.sort();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|(_, project)| project.as_deref() == Some("/p")));
        assert!(ids.iter().any(|(id, _)| id == "project-note:1"));
        assert!(ids.iter().any(|(id, _)| id.starts_with("file:") && id.ends_with("/solta.md")));
    }

    #[test]
    fn an_edit_outside_lume_is_noticed_and_lumes_own_save_is_not() {
        let folder = Folder::new("watch");
        let files = Arc::new(NoteFiles::open(&folder.0).unwrap());
        let saved = files.save(&note("project-note:1", "Vigiada", "antes", Some("/p"))).unwrap();
        assert!(!files.refresh().unwrap(), "Lume's own save is already known");

        let (sender, receiver) = mpsc::channel();
        files.watch(move || { let _ = sender.send(()); }).unwrap();
        files.save(&note("project-note:1", "Vigiada", "pelo Lume", Some("/p"))).unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(900)).is_err(), "no event for Lume's own save");

        let path = files.entries.lock().unwrap()["project-note:1"].path.clone();
        fs::write(&path, fs::read_to_string(&path).unwrap().replace("pelo Lume", "pelo editor")).unwrap();
        receiver.recv_timeout(Duration::from_secs(5)).expect("an edit outside Lume is noticed");
        let current = files.get("project-note:1").unwrap().unwrap();
        assert_eq!(current.body, "pelo editor");
        assert_ne!(current.revision, saved.revision);
    }

    #[test]
    fn an_imported_note_keeps_its_date() {
        let folder = Folder::new("import");
        let files = NoteFiles::open(&folder.0).unwrap();
        let mut old = note("project-note:9", "Antiga", "texto", Some("/p"));
        old.updated_at = 1_700_000_000_000;
        assert_eq!(files.import(&old).unwrap().updated_at, 1_700_000_000_000);
        files.refresh().unwrap();
        assert_eq!(files.get("project-note:9").unwrap().unwrap().updated_at, 1_700_000_000_000);
    }

    #[cfg(unix)]
    #[test]
    fn links_out_of_the_folder_are_not_read() {
        let folder = Folder::new("links");
        let outside = Folder::new("links-outside");
        fs::create_dir_all(&outside.0).unwrap();
        fs::write(outside.0.join("segredo.md"), "segredo").unwrap();
        let files = NoteFiles::open(&folder.0).unwrap();
        std::os::unix::fs::symlink(outside.0.join("segredo.md"), files.root().join("link.md")).unwrap();
        std::os::unix::fs::symlink(&outside.0, files.root().join("pasta")).unwrap();
        files.refresh().unwrap();
        assert!(files.list().unwrap().is_empty());
    }
}
