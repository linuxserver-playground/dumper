//! `dumper files`: a lean file tree of a root filesystem.
//!
//! Run inside the container before init, so it sees the image as shipped. It only calls lstat: no file contents
//! are read or hashed.
//!
//! Output (gzipped JSON): `{"format": "lsio-files/1", "tree": <node>}`. A node is an object with short keys,
//! defaults left out:
//!   n  name ("" for the root)
//!   c  children (present = directory)
//!   s  size in bytes (regular files; left out when 0)
//!   l  symlink target, as stored
//!   h  hardlink: absolute path of the first path with the same inode (no size, it is counted there)
//!   t  "char", "block", "fifo" or "socket" for special files
//!   m  permission bits (st_mode & 0o7777) as a number, left out when 0o755 for directories, 0o644 otherwise,
//!      never written for symlinks
//!   u  uid, g gid, left out when 0
//! Children are sorted by name, bytewise. Mount points other than the root are kept as empty directories
//! (proc, sys, dev) or left out (files Docker bind-mounts, such as /etc/hosts), and so is /.dockerenv.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::Instant;

use flate2::write::GzEncoder;
use flate2::Compression;
use serde::Serialize;

pub const FORMAT: &str = "lsio-files/1";

const USAGE: &str = "usage: dumper files [--root DIR] [-o FILE.json.gz] [--exclude PATH]...

Writes the file tree of DIR (default /) as gzipped JSON to FILE, or to stdout. --exclude leaves out an absolute
path (and everything below it) as seen from DIR; it can be given more than once.";

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Node {
    pub n: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<Vec<Node>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g: Option<u32>,
}

#[derive(Serialize)]
struct Output<'a> {
    format: &'static str,
    tree: &'a Node,
}

#[derive(Default, Debug)]
pub struct Stats {
    pub dirs: u64,
    pub files: u64,
    pub symlinks: u64,
    pub hardlinks: u64,
    pub special: u64,
    pub bytes: u64,
    pub lossy_names: u64,
    pub skipped: u64,
}

/// Mount points other than the root (proc, sys, dev, Docker's /etc/hosts and friends, bind mounts).
fn mount_points() -> HashSet<String> {
    let Ok(text) = fs::read_to_string("/proc/self/mountinfo") else {
        return HashSet::new();
    };
    text.lines()
        .filter_map(|l| l.split(' ').nth(4))
        .map(unescape_mount)
        .filter(|p| p != "/")
        .collect()
}

/// mountinfo escapes space, tab, newline and backslash as \ooo.
fn unescape_mount(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if let Some(v) = s
                .get(i + 1..i + 4)
                .and_then(|o| u8::from_str_radix(o, 8).ok())
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Walker<'a> {
    root: &'a Path,
    mounts: HashSet<String>,
    excludes: HashSet<String>,
    inodes: HashMap<(u64, u64), String>,
    pub stats: Stats,
}

impl<'a> Walker<'a> {
    /// `mounts` and `excludes` are absolute paths as seen from `root`.
    pub fn new(root: &'a Path, mounts: HashSet<String>, excludes: HashSet<String>) -> Self {
        Walker {
            root,
            mounts,
            excludes,
            inodes: HashMap::new(),
            stats: Stats::default(),
        }
    }

    pub fn walk(&mut self) -> Node {
        let mut tree = Node::default();
        self.walk_dir("", &mut tree);
        tree
    }

    fn walk_dir(&mut self, rel: &str, node: &mut Node) {
        let dir = if rel.is_empty() {
            self.root.to_path_buf()
        } else {
            self.root.join(&rel[1..])
        };
        let mut names: Vec<OsString> = match fs::read_dir(&dir) {
            Ok(read) => read.flatten().map(|e| e.file_name()).collect(),
            Err(err) => {
                eprintln!("dumper: cannot read {}: {err}", dir.display());
                Vec::new()
            }
        };
        names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        let mut children = Vec::with_capacity(names.len());
        for raw in names {
            let name = match raw.to_str() {
                Some(s) => s.to_string(),
                None => {
                    self.stats.lossy_names += 1;
                    raw.to_string_lossy().into_owned()
                }
            };
            let path = format!("{rel}/{name}");
            if path == "/.dockerenv" || self.excludes.contains(&path) {
                self.stats.skipped += 1;
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(dir.join(&raw)) else {
                continue;
            };
            let ft = meta.file_type();
            let is_mount = self.mounts.contains(&path);
            if is_mount && !ft.is_dir() {
                self.stats.skipped += 1;
                continue;
            }
            let mut child = Node {
                n: name,
                ..Default::default()
            };
            let mode = meta.mode() & 0o7777;
            child.u = (meta.uid() != 0).then_some(meta.uid());
            child.g = (meta.gid() != 0).then_some(meta.gid());
            if ft.is_dir() {
                self.stats.dirs += 1;
                child.m = (mode != 0o755).then_some(mode);
                if is_mount {
                    self.stats.skipped += 1;
                    child.c = Some(Vec::new());
                } else {
                    self.walk_dir(&path, &mut child);
                }
            } else if ft.is_symlink() {
                self.stats.symlinks += 1;
                child.l = fs::read_link(dir.join(&raw))
                    .ok()
                    .map(|t| t.to_string_lossy().into_owned());
            } else {
                child.m = (mode != 0o644).then_some(mode);
                if ft.is_file() {
                    let first = (meta.nlink() > 1).then(|| {
                        self.inodes
                            .entry((meta.dev(), meta.ino()))
                            .or_insert_with(|| path.clone())
                            .clone()
                    });
                    match first {
                        Some(target) if target != path => {
                            self.stats.hardlinks += 1;
                            child.h = Some(target);
                        }
                        _ => {
                            self.stats.files += 1;
                            self.stats.bytes += meta.size();
                            child.s = (meta.size() != 0).then_some(meta.size());
                        }
                    }
                } else {
                    self.stats.special += 1;
                    child.t = Some(if ft.is_char_device() {
                        "char"
                    } else if ft.is_block_device() {
                        "block"
                    } else if ft.is_fifo() {
                        "fifo"
                    } else {
                        "socket"
                    });
                }
            }
            children.push(child);
        }
        node.c = Some(children);
    }
}

/// The gzipped document for a tree.
pub fn write_tree(tree: &Node, sink: impl Write) -> io::Result<()> {
    let mut gz = GzEncoder::new(BufWriter::with_capacity(1 << 20, sink), Compression::new(6));
    serde_json::to_writer(
        &mut gz,
        &Output {
            format: FORMAT,
            tree,
        },
    )?;
    gz.finish()?.flush()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (mut root, mut out, mut excludes) = (PathBuf::from("/"), None::<PathBuf>, HashSet::new());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-o" | "--output" => out = Some(it.next().ok_or(USAGE)?.into()),
            "--root" => root = it.next().ok_or(USAGE)?.into(),
            "--exclude" => {
                let path = it.next().ok_or(USAGE)?;
                excludes.insert(path.trim_end_matches('/').to_string());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}\n\n{USAGE}")),
        }
    }
    let start = Instant::now();
    // Mount points describe this process's root, so they only apply when walking it.
    let mounts = if root == Path::new("/") {
        mount_points()
    } else {
        HashSet::new()
    };
    let mut walker = Walker::new(&root, mounts, excludes);
    let tree = walker.walk();
    let walked = start.elapsed();
    let written = match &out {
        Some(path) => fs::File::create(path).and_then(|f| write_tree(&tree, f)),
        None => write_tree(&tree, io::stdout().lock()),
    };
    written.map_err(|err| format!("cannot write the tree: {err}"))?;
    let s = &walker.stats;
    eprintln!(
        "dumper files: {} dirs, {} files ({} bytes), {} symlinks, {} hardlinks, {} special, {} skipped, \
         {} non-UTF-8 names; walk {:.2}s, total {:.2}s",
        s.dirs,
        s.files,
        s.bytes,
        s.symlinks,
        s.hardlinks,
        s.special,
        s.skipped,
        s.lossy_names,
        walked.as_secs_f64(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("dumper-test-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn child<'a>(node: &'a Node, name: &str) -> &'a Node {
        node.c
            .as_ref()
            .unwrap()
            .iter()
            .find(|c| c.n == name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn fixture(name: &str) -> TempDir {
        let dir = TempDir::new(name);
        let root = &dir.0;
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        fs::write(root.join("usr/bin/tool"), b"#!/bin/sh\n").unwrap();
        fs::set_permissions(root.join("usr/bin/tool"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(root.join("usr/bin/empty"), b"").unwrap();
        fs::set_permissions(
            root.join("usr/bin/empty"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        fs::hard_link(root.join("usr/bin/tool"), root.join("usr/bin/tool2")).unwrap();
        symlink("usr/bin", root.join("bin")).unwrap();
        fs::create_dir_all(root.join("skip/me")).unwrap();
        fs::set_permissions(root.join("usr"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(root.join("usr/bin"), fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    fn walk(root: &Path, excludes: &[&str]) -> (Node, Stats) {
        let mut walker = Walker::new(
            root,
            HashSet::new(),
            excludes
                .iter()
                .map(|s| s.to_string())
                .collect::<HashSet<_>>(),
        );
        let tree = walker.walk();
        (tree, walker.stats)
    }

    #[test]
    fn records_types_sizes_and_defaults() {
        let dir = fixture("types");
        let (tree, stats) = walk(&dir.0, &[]);
        let bin = child(child(&tree, "usr"), "bin");
        let tool = child(bin, "tool");
        assert_eq!(tool.s, Some(10));
        assert_eq!(tool.m, Some(0o755));
        let empty = child(bin, "empty");
        assert_eq!((empty.s, empty.m), (None, None));
        assert_eq!(child(bin, "tool2").h.as_deref(), Some("/usr/bin/tool"));
        assert_eq!(child(&tree, "bin").l.as_deref(), Some("usr/bin"));
        assert_eq!(child(&tree, "usr").m, None);
        assert_eq!(
            (stats.files, stats.hardlinks, stats.symlinks, stats.bytes),
            (2, 1, 1, 10)
        );
    }

    #[test]
    fn children_are_sorted_and_excludes_apply() {
        let dir = fixture("exclude");
        let (tree, stats) = walk(&dir.0, &["/skip"]);
        let names: Vec<&str> = tree
            .c
            .as_ref()
            .unwrap()
            .iter()
            .map(|c| c.n.as_str())
            .collect();
        assert_eq!(names, ["bin", "usr"]);
        let bin: Vec<&str> = child(child(&tree, "usr"), "bin")
            .c
            .as_ref()
            .unwrap()
            .iter()
            .map(|c| c.n.as_str())
            .collect();
        assert_eq!(bin, ["empty", "tool", "tool2"]);
        assert_eq!(stats.skipped, 1);
    }

    #[test]
    fn mount_points_are_empty_directories() {
        let dir = fixture("mounts");
        let mounts: HashSet<String> = ["/skip".to_string(), "/usr/bin/empty".to_string()].into();
        let mut walker = Walker::new(&dir.0, mounts, HashSet::new());
        let tree = walker.walk();
        assert_eq!(child(&tree, "skip").c.as_deref(), Some(&[][..]));
        assert!(child(child(&tree, "usr"), "bin")
            .c
            .as_ref()
            .unwrap()
            .iter()
            .all(|c| c.n != "empty"));
    }

    #[test]
    fn output_is_gzipped_json_with_format() {
        let dir = fixture("output");
        let (tree, _) = walk(&dir.0, &[]);
        let mut buf = Vec::new();
        write_tree(&tree, &mut buf).unwrap();
        let mut text = String::new();
        io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&buf[..]), &mut text).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(doc["format"], FORMAT);
        assert_eq!(doc["tree"]["n"], "");
    }

    #[test]
    fn unescapes_mountinfo_paths() {
        assert_eq!(unescape_mount("/a\\040b"), "/a b");
        assert_eq!(unescape_mount("/plain"), "/plain");
        assert_eq!(unescape_mount("/trailing\\04"), "/trailing\\04");
    }
}
