//! Which rulesets may run their `lua_predicate` checks: the trust store (D5).
//!
//! A ruleset is code. The app takes any folder as a project and then runs its
//! rules, so a `lua_predicate` of a ruleset nobody looked at would be a
//! stranger's Lua running with the user's rights. The engine therefore runs a
//! predicate only for a folder the host names (`--trust=<folder>`), and this
//! module decides which folders those are:
//!
//! - an **entry** is a ruleset by its real path (links resolved, so an alias
//!   is the folder it points at and nothing else) plus the SHA-256 of what was
//!   in it when the user said yes, who said it and when;
//! - a ruleset is trusted **only while its content still has that hash**. A
//!   changed file is a different ruleset: it is untrusted again, the window
//!   says "changed" and asks again;
//! - the store is one file in the app's own settings folder, per machine like
//!   the engine path. It is never part of a project, a workspace or a request:
//!   nothing in this module talks to a network, and `agent/` does not read it;
//! - whatever goes wrong on the way to a yes is a no: a store that does not
//!   parse (said aloud, as an error of its own), a ruleset that cannot be read
//!   or resolved, one too large to hash.
//!
//! What the hash covers: every file below the ruleset, by its path relative to
//! it and its bytes, and every link by where it points (a link is not
//! followed: the engine does not trust a rule file that lies outside by its
//! real path either). `.git` folders are left out, they are the history of the
//! content and change on every fetch.
//!
//! What it cannot do: the hash is taken before the engine starts, and the
//! engine reads the files a moment later. Somebody who can write into a
//! trusted ruleset in that moment can still get code run; they can also write
//! anywhere else the user can, so this is the limit of a check in user space,
//! not a hole in it.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::apperr::AppError;

/// The store's file, in the app's settings folder.
pub const STORE_FILE: &str = "rules-trust.json";
/// The only layout this build reads. Another number is "does not parse".
const VERSION: u32 = 1;

/// A ruleset with more files than this is not hashed, so it cannot be trusted.
const MAX_FILES: usize = 20_000;
/// Nor one with more bytes than this.
const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Nor one nested deeper than this.
const MAX_DEPTH: usize = 32;

/// One trusted ruleset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The real path, as shown: forward slashes, no `\\?\`.
    pub path: String,
    /// Of the content at the time of the yes ([`digest`]).
    pub sha256: String,
    /// How many predicate rules it had then; a number for the reader.
    #[serde(default)]
    pub predicates: u64,
    /// The account that said yes.
    #[serde(default)]
    pub by: String,
    /// When, in milliseconds since the epoch.
    #[serde(default)]
    pub at: u64,
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    trusted: Vec<Entry>,
}

/// The trusted rulesets of this machine.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Store {
    entries: Vec<Entry>,
}

impl Store {
    /// The store at `file`. No file is an empty store; a file that cannot be
    /// read, does not parse or is of another version is `err.rules.trustStore`
    /// and the caller trusts nothing.
    pub fn load(file: &Path) -> Result<Store, AppError> {
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Store::default()),
            Err(e) => return Err(unreadable(file, e)),
        };
        let parsed: StoreFile = serde_json::from_str(&text).map_err(|e| unreadable(file, e))?;
        if parsed.version != VERSION {
            return Err(unreadable(
                file,
                format!("version {} (this app reads {VERSION})", parsed.version),
            ));
        }
        Ok(Store {
            entries: parsed.trusted,
        })
    }

    pub fn save(&self, file: &Path) -> Result<(), AppError> {
        let body = serde_json::to_string_pretty(&StoreFile {
            version: VERSION,
            trusted: self.entries.clone(),
        })
        .map_err(|e| AppError::new("err.workspace.serialise").detail(e))?;
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| crate::path_err("err.path.create", dir, e))?;
        }
        crate::write_atomic(file, body + "\n")
            .map_err(|e| crate::path_err("err.path.write", file, e))
    }

    /// The entry of the ruleset whose real path is `real`.
    pub fn find(&self, real: &Path) -> Option<&Entry> {
        let key = key_of(real);
        self.entries
            .iter()
            .find(|e| key_of(Path::new(&e.path)) == key)
    }

    /// Trust `entry`, in place of an older entry of the same ruleset.
    pub fn grant(&mut self, entry: Entry) {
        let key = key_of(Path::new(&entry.path));
        self.entries.retain(|e| key_of(Path::new(&e.path)) != key);
        self.entries.push(entry);
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Forget the ruleset at `real`. False when it was not trusted.
    pub fn revoke(&mut self, real: &Path) -> bool {
        let key = key_of(real);
        let before = self.entries.len();
        self.entries.retain(|e| key_of(Path::new(&e.path)) != key);
        self.entries.len() != before
    }
}

fn unreadable(file: &Path, why: impl std::fmt::Display) -> AppError {
    AppError::new("err.rules.trustStore")
        .with("path", crate::portable(file))
        .detail(why)
}

/// The real path of a ruleset: links resolved, shown with forward slashes.
/// This is what an entry is keyed by and what the engine is told to trust, so
/// a second name for a folder (a junction, a symlink, `..`) is that folder.
pub fn real_path(path: &Path) -> Result<PathBuf, AppError> {
    let real = fs::canonicalize(path).map_err(|e| crate::path_err("err.path.resolve", path, e))?;
    Ok(PathBuf::from(crate::portable(&real)))
}

/// What two spellings of one real path have in common: separators, a trailing
/// one, and on Windows the case.
fn key_of(real: &Path) -> String {
    let text = crate::portable(real);
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

/// True when `file` (a real path) is the ruleset `real` or lies below it.
pub fn within(file: &Path, real: &Path) -> bool {
    let (file, root) = (key_of(file), key_of(real));
    file == root
        || file
            .strip_prefix(&root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The content of a ruleset, as one number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Digest {
    pub sha256: String,
    pub files: u64,
    pub bytes: u64,
}

enum Item {
    File(PathBuf),
    /// Where the link points, as text.
    Link(String),
}

/// The SHA-256 of everything below `real` (see the module text for what
/// "everything" is). A single file is a ruleset of one file.
pub fn digest(real: &Path) -> Result<Digest, AppError> {
    let meta = fs::symlink_metadata(real).map_err(|e| crate::path_err("err.path.read", real, e))?;
    let mut items: Vec<(String, Item)> = Vec::new();
    if meta.is_dir() {
        collect(real, "", 0, &mut items)?;
    } else {
        let name = real
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        items.push((name, Item::File(real.to_path_buf())));
    }
    items.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

    let mut hash = Sha256::new();
    let (mut files, mut bytes) = (0u64, 0u64);
    let mut buf = vec![0u8; 64 * 1024];
    for (rel, item) in &items {
        match item {
            Item::Link(target) => {
                hash.update(b"L");
                hash.update(rel.as_bytes());
                hash.update([0]);
                hash.update(target.as_bytes());
                hash.update([0]);
            }
            Item::File(path) => {
                // The content first, into a hash of its own: the length that goes in front of
                // it is the number of bytes that were read, not what the directory said.
                let mut file =
                    fs::File::open(path).map_err(|e| crate::path_err("err.path.read", path, e))?;
                let mut inner = Sha256::new();
                let mut len = 0u64;
                loop {
                    let n = file
                        .read(&mut buf)
                        .map_err(|e| crate::path_err("err.path.read", path, e))?;
                    if n == 0 {
                        break;
                    }
                    len += n as u64;
                    bytes += n as u64;
                    if bytes > MAX_BYTES {
                        return Err(too_large(real));
                    }
                    inner.update(&buf[..n]);
                }
                hash.update(b"F");
                hash.update(rel.as_bytes());
                hash.update([0]);
                hash.update(len.to_le_bytes());
                hash.update(inner.finalize());
                files += 1;
            }
        }
    }
    Ok(Digest {
        sha256: hex(&hash.finalize()),
        files,
        bytes,
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn too_large(real: &Path) -> AppError {
    AppError::new("err.rules.trustTooLarge")
        .with("path", crate::portable(real))
        .with("files", MAX_FILES)
        .with("mb", MAX_BYTES / (1024 * 1024))
}

fn collect(
    root: &Path,
    rel: &str,
    depth: usize,
    out: &mut Vec<(String, Item)>,
) -> Result<(), AppError> {
    if depth > MAX_DEPTH {
        return Err(too_large(root));
    }
    let dir = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let entries = fs::read_dir(&dir).map_err(|e| crate::path_err("err.path.read", &dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| crate::path_err("err.path.read", &dir, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| crate::path_err("err.path.read", &path, e))?;
        if kind.is_symlink() {
            let target = fs::read_link(&path)
                .map(|t| t.to_string_lossy().into_owned())
                .map_err(|e| crate::path_err("err.path.read", &path, e))?;
            out.push((child, Item::Link(target)));
        } else if kind.is_dir() {
            if name == ".git" {
                continue;
            }
            collect(root, &child, depth + 1, out)?;
        } else {
            out.push((child, Item::File(path)));
        }
        if out.len() > MAX_FILES {
            return Err(too_large(root));
        }
    }
    Ok(())
}

/// Where a ruleset stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// In the store, and its content is what was trusted.
    Trusted,
    /// Never trusted.
    Untrusted,
    /// Trusted once, but the content is no longer that.
    Changed,
    /// It could not be resolved or read, so nothing is known about it.
    Unreadable,
}

/// The verdict on one ruleset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub state: State,
    /// The real path; `None` when it could not be resolved.
    pub real: Option<PathBuf>,
    /// The entry it has or had.
    pub entry: Option<Entry>,
    /// Why it is [`State::Unreadable`].
    pub problem: Option<AppError>,
}

/// Decide about the ruleset at `path` (as the engine names it). `store` is
/// `None` when the store itself could not be read: then nothing is trusted.
pub fn verdict(store: Option<&Store>, path: &Path) -> Verdict {
    let real = match real_path(path) {
        Ok(real) => real,
        Err(e) => {
            return Verdict {
                state: State::Unreadable,
                real: None,
                entry: None,
                problem: Some(e),
            }
        }
    };
    let Some(entry) = store.and_then(|s| s.find(&real)).cloned() else {
        return Verdict {
            state: State::Untrusted,
            real: Some(real),
            entry: None,
            problem: None,
        };
    };
    match digest(&real) {
        Ok(now) if now.sha256 == entry.sha256 => Verdict {
            state: State::Trusted,
            real: Some(real),
            entry: Some(entry),
            problem: None,
        },
        Ok(_) => Verdict {
            state: State::Changed,
            real: Some(real),
            entry: Some(entry),
            problem: None,
        },
        Err(e) => Verdict {
            state: State::Unreadable,
            real: Some(real),
            entry: Some(entry),
            problem: Some(e),
        },
    }
}

/// The account name for an entry's `by`. Only ever shown, never compared.
pub fn account() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fresh_dir;

    fn ruleset(name: &str) -> PathBuf {
        let dir = fresh_dir(name);
        fs::write(
            dir.join("RULES.md"),
            "# rules\n\ncheck = function() return true end\n",
        )
        .unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/more.md"), "more\n").unwrap();
        dir
    }

    fn trust(store: &mut Store, dir: &Path) {
        let real = real_path(dir).unwrap();
        store.grant(Entry {
            path: crate::portable(&real),
            sha256: digest(&real).unwrap().sha256,
            predicates: 1,
            by: "tester".into(),
            at: 1,
        });
    }

    #[test]
    fn a_ruleset_nobody_trusted_is_untrusted() {
        let dir = ruleset("docmap-trust-none");
        let v = verdict(Some(&Store::default()), &dir);
        assert_eq!(v.state, State::Untrusted);
        assert!(v.entry.is_none());
    }

    #[test]
    fn a_trusted_ruleset_stays_trusted_while_its_content_does() {
        let dir = ruleset("docmap-trust-same");
        let mut store = Store::default();
        trust(&mut store, &dir);
        assert_eq!(verdict(Some(&store), &dir).state, State::Trusted);
        // rewriting the same bytes is not a change
        fs::write(dir.join("sub/more.md"), "more\n").unwrap();
        assert_eq!(verdict(Some(&store), &dir).state, State::Trusted);
    }

    #[test]
    fn a_changed_file_revokes_the_trust() {
        let dir = ruleset("docmap-trust-changed");
        let mut store = Store::default();
        trust(&mut store, &dir);
        fs::write(
            dir.join("RULES.md"),
            "# rules\n\ncheck = function() os.exit() end\n",
        )
        .unwrap();
        let v = verdict(Some(&store), &dir);
        assert_eq!(v.state, State::Changed);
        assert!(v.entry.is_some(), "the window says what was trusted before");
    }

    #[test]
    fn a_new_file_a_removed_file_and_a_renamed_file_each_revoke_it() {
        let dir = ruleset("docmap-trust-tree");
        let mut store = Store::default();
        trust(&mut store, &dir);
        fs::write(dir.join("helper.lua"), "return 1\n").unwrap();
        assert_eq!(verdict(Some(&store), &dir).state, State::Changed);
        fs::remove_file(dir.join("helper.lua")).unwrap();
        assert_eq!(verdict(Some(&store), &dir).state, State::Trusted);
        fs::rename(dir.join("sub/more.md"), dir.join("sub/other.md")).unwrap();
        assert_eq!(verdict(Some(&store), &dir).state, State::Changed);
        fs::rename(dir.join("sub/other.md"), dir.join("sub/more.md")).unwrap();
        fs::remove_file(dir.join("sub/more.md")).unwrap();
        assert_eq!(verdict(Some(&store), &dir).state, State::Changed);
    }

    #[test]
    fn moving_bytes_from_one_file_into_the_next_changes_the_hash() {
        // "ab" + "c" and "a" + "bc" must not be the same ruleset
        let one = fresh_dir("docmap-trust-split-1");
        fs::write(one.join("a"), "ab").unwrap();
        fs::write(one.join("b"), "c").unwrap();
        let two = fresh_dir("docmap-trust-split-2");
        fs::write(two.join("a"), "a").unwrap();
        fs::write(two.join("b"), "bc").unwrap();
        assert_ne!(digest(&one).unwrap().sha256, digest(&two).unwrap().sha256);
    }

    #[test]
    fn the_history_folder_is_not_part_of_the_content() {
        let dir = ruleset("docmap-trust-git");
        let before = digest(&dir).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git/FETCH_HEAD"), "x").unwrap();
        assert_eq!(digest(&dir).unwrap(), before);
        assert_eq!(before.files, 2);
    }

    #[test]
    fn another_spelling_of_the_path_is_the_same_ruleset() {
        let dir = ruleset("docmap-trust-spelling");
        let mut store = Store::default();
        trust(&mut store, &dir);
        let dotted = dir.join("sub").join("..");
        assert_eq!(verdict(Some(&store), &dotted).state, State::Trusted);
        #[cfg(windows)]
        {
            let upper = PathBuf::from(dir.to_string_lossy().to_uppercase());
            assert_eq!(verdict(Some(&store), &upper).state, State::Trusted);
        }
    }

    #[test]
    fn an_alias_that_points_elsewhere_does_not_inherit_the_trust() {
        // `trusted` is trusted. Then the name `trusted` is made a link to a folder with other
        // content: the name is the same, the ruleset is not.
        let base = fresh_dir("docmap-trust-alias");
        let trusted = base.join("trusted");
        fs::create_dir_all(&trusted).unwrap();
        fs::write(trusted.join("RULES.md"), "good\n").unwrap();
        let other = base.join("other");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("RULES.md"), "evil\n").unwrap();
        let mut store = Store::default();
        trust(&mut store, &trusted);

        fs::rename(&trusted, base.join("moved")).unwrap();
        if !crate::testutil::dir_link(&trusted, &other) {
            return; // no right to make links on this machine
        }
        let v = verdict(Some(&store), &trusted);
        assert_eq!(
            v.state,
            State::Untrusted,
            "the name now is `other`, which nobody trusted"
        );
        assert!(within(&v.real.unwrap(), &real_path(&other).unwrap()));
    }

    #[test]
    fn a_link_to_a_trusted_ruleset_is_that_ruleset_and_is_keyed_by_its_real_path() {
        let base = fresh_dir("docmap-trust-link");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("RULES.md"), "good\n").unwrap();
        let mut store = Store::default();
        trust(&mut store, &real);
        let link = base.join("link");
        if !crate::testutil::dir_link(&link, &real) {
            return;
        }
        let v = verdict(Some(&store), &link);
        assert_eq!(v.state, State::Trusted);
        // what the engine is told to trust is the real folder, never the link's name
        assert_eq!(key_of(&v.real.unwrap()), key_of(&real_path(&real).unwrap()));
    }

    #[test]
    fn a_link_inside_a_ruleset_is_hashed_by_its_target_not_followed() {
        let base = fresh_dir("docmap-trust-inner-link");
        let dir = base.join("ruleset");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RULES.md"), "r\n").unwrap();
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("big.md"), "outside\n").unwrap();
        let plain = digest(&dir).unwrap();
        if !crate::testutil::dir_link(&dir.join("out"), &outside) {
            return;
        }
        let linked = digest(&dir).unwrap();
        assert_ne!(linked.sha256, plain.sha256, "a new link is a change");
        assert_eq!(linked.files, 1, "what lies behind the link is not read");
        fs::write(outside.join("big.md"), "changed outside\n").unwrap();
        assert_eq!(digest(&dir).unwrap(), linked);
    }

    #[test]
    fn a_store_that_does_not_parse_is_an_error_and_nothing_is_trusted() {
        let dir = ruleset("docmap-trust-broken");
        let file = fresh_dir("docmap-trust-broken-store").join(STORE_FILE);
        let mut store = Store::default();
        trust(&mut store, &dir);
        store.save(&file).unwrap();
        assert_eq!(Store::load(&file).unwrap(), store);

        for broken in ["{ not json", "[]", "{\"version\": 2, \"trusted\": []}", ""] {
            fs::write(&file, broken).unwrap();
            let e = Store::load(&file).unwrap_err();
            assert_eq!(e.key(), "err.rules.trustStore", "{broken}");
            // the caller has no store, and without one nothing is trusted
            assert_eq!(verdict(None, &dir).state, State::Untrusted, "{broken}");
        }
    }

    #[test]
    fn no_store_file_is_an_empty_store() {
        let file = fresh_dir("docmap-trust-nofile").join(STORE_FILE);
        assert_eq!(Store::load(&file).unwrap(), Store::default());
    }

    #[test]
    fn a_ruleset_that_cannot_be_resolved_is_not_trusted() {
        let base = fresh_dir("docmap-trust-gone");
        let dir = base.join("ruleset");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("RULES.md"), "r\n").unwrap();
        let mut store = Store::default();
        trust(&mut store, &dir);
        fs::remove_dir_all(&dir).unwrap();
        let v = verdict(Some(&store), &dir);
        assert_eq!(v.state, State::Unreadable);
        assert!(v.problem.is_some());
    }

    #[test]
    fn granting_again_replaces_the_entry_and_revoking_removes_it() {
        let dir = ruleset("docmap-trust-regrant");
        let mut store = Store::default();
        trust(&mut store, &dir);
        fs::write(dir.join("RULES.md"), "new\n").unwrap();
        trust(&mut store, &dir);
        assert_eq!(store.entries.len(), 1);
        assert_eq!(verdict(Some(&store), &dir).state, State::Trusted);
        let real = real_path(&dir).unwrap();
        assert!(store.revoke(&real));
        assert!(!store.revoke(&real));
        assert_eq!(verdict(Some(&store), &dir).state, State::Untrusted);
    }

    #[test]
    fn a_single_file_is_a_ruleset_of_one_file() {
        let dir = fresh_dir("docmap-trust-file");
        let file = dir.join("RULES.md");
        fs::write(&file, "one\n").unwrap();
        let d = digest(&file).unwrap();
        assert_eq!((d.files, d.bytes), (1, 4));
        fs::write(&file, "two\n").unwrap();
        assert_ne!(digest(&file).unwrap().sha256, d.sha256);
    }

    #[test]
    fn within_is_about_whole_path_segments() {
        let root = Path::new("/a/rules");
        assert!(within(Path::new("/a/rules"), root));
        assert!(within(Path::new("/a/rules/x.md"), root));
        assert!(!within(Path::new("/a/rules-evil/x.md"), root));
        assert!(!within(Path::new("/a"), root));
    }
}
