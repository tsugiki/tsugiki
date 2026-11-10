//! PROTOTYPE, throwaway (#365): Go to definition and Definition peek over `git grep` in a mirror. It lives
//! on the `prototype/definition-peek` branch only, to try the look of the peek and of the file view on real
//! PRs. The real lookup is a build ticket, and it rewrites this with tests.

#![allow(clippy::all, clippy::pedantic, clippy::nursery, clippy::restriction)]

use std::ffi::OsStr;
use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use specta::Type;

use crate::diff::HeadLine;
use crate::github::live;
use crate::highlight::{Language, Parsed};

/// Where a symbol may be defined.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProtoHit {
    pub path: String,
    /// Counted from 1.
    pub line: u32,
    pub text: String,
    /// "python", "c", "rest" or "word" for the whole-word fallback.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ProtoHits {
    pub hits: Vec<ProtoHit>,
    /// Whether the hits come from the whole-word fallback.
    pub fallback: bool,
    pub millis: u32,
    /// Whether the 50-hit cap cut the list.
    pub capped: bool,
}

const CAP: usize = 50;
const NAME_END: &str = "([^A-Za-z0-9_]|$)";

fn patterns(word: &str) -> Vec<(&'static str, Vec<String>, Vec<&'static str>)> {
    let s = "[[:space:]]";
    vec![
        (
            "python",
            vec![
                format!("^{s}*(async{s}+)?def{s}+{word}{s}*\\("),
                format!("^{s}*class{s}+{word}{s}*[(:]"),
                format!("^{word}{s}*(:[^=]*)?=([^=]|$)"),
            ],
            vec!["*.py", "*.pyi"],
        ),
        (
            "c",
            vec![
                format!("^{word}\\("),
                format!("^#{s}*define{s}+{word}{NAME_END}"),
                format!("^(typedef{s}+)?(struct|enum|union){s}+{word}{s}*\\{{?{s}*$"),
                format!("^}}{s}*{word}{s}*;"),
                format!("^[A-Za-z_][^;=]*[^A-Za-z0-9_]{word}\\([^;]*\\){s}*\\{{"),
            ],
            vec!["*.c", "*.h"],
        ),
        // Not in ADR 0002's list: here so the prototype finds definitions in Tsugiki's own PRs.
        (
            "rust",
            vec![
                format!("^{s}*(pub(\\([a-z]+\\))?{s}+)?(const{s}+|async{s}+|unsafe{s}+)*fn{s}+{word}{NAME_END}"),
                format!("^{s}*(pub(\\([a-z]+\\))?{s}+)?(struct|enum|trait|type|const|static|mod|union){s}+{word}{NAME_END}"),
                format!("^{s}*macro_rules!{s}*{word}{NAME_END}"),
            ],
            vec!["*.rs"],
        ),
        (
            "ts",
            vec![
                format!("^{s}*(export{s}+)?(default{s}+)?(async{s}+)?function\\*?{s}+{word}{NAME_END}"),
                format!("^{s}*(export{s}+)?(abstract{s}+)?(class|interface|type|enum){s}+{word}{NAME_END}"),
                format!("^{s}*(export{s}+)?(const|let|var){s}+{word}{s}*[:=]"),
                format!("^{s}*(readonly{s}+|static{s}+|async{s}+|get{s}+|set{s}+|#)*{word}{s}*(\\(.*\\)|=)[^;]*\\{{{s}*$"),
            ],
            vec!["*.ts", "*.svelte", "*.js", "*.mjs"],
        ),
        (
            "rest",
            vec![
                format!("^\\.\\.{s}+[a-z]+(:[a-z]+)?::{s}+([^ ]*[^A-Za-z0-9_])?{word}{NAME_END}"),
                format!("^\\.\\.{s}+[a-z]+:[a-z]+::.*[^A-Za-z0-9_]{word}{s}*\\("),
            ],
            vec!["Doc/*.rst"],
        ),
    ]
}

fn grep(mirror: &Path, commit: &str, args: &[String], paths: &[&str]) -> Vec<(String, u32, String)> {
    let mut all: Vec<&OsStr> = vec![OsStr::new("-C"), mirror.as_os_str(), OsStr::new("grep")];
    all.extend(args.iter().map(OsStr::new));
    all.push(OsStr::new(commit));
    all.push(OsStr::new("--"));
    all.extend(paths.iter().map(OsStr::new));
    // git grep fails when nothing matches.
    let out = live::read_git(&all).unwrap_or_default();
    out.lines()
        .filter_map(|line| {
            // <commit>:<path>:<line>:<text>
            let rest = line.strip_prefix(commit)?.strip_prefix(':')?;
            let (path, rest) = rest.split_once(':')?;
            let (number, text) = rest.split_once(':')?;
            Some((path.to_owned(), number.parse().ok()?, text.trim().to_owned()))
        })
        .collect()
}

fn dir(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn ext(path: &str) -> &str {
    path.rsplit_once('.').map_or("", |(_, ext)| ext)
}

/// The definitions of `word` at `commit`, ranked from the file `from`.
pub fn definitions(mirror: &Path, commit: &str, word: &str, from: &str) -> ProtoHits {
    let started = Instant::now();
    let valid = !word.is_empty()
        && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !word.starts_with(|c: char| c.is_ascii_digit());
    let mut hits = Vec::new();
    let mut fallback = false;
    if valid {
        for (kind, regexes, paths) in patterns(word) {
            let mut args = vec!["-n".to_owned(), "-E".to_owned(), "-I".to_owned()];
            for regex in regexes {
                args.push("-e".to_owned());
                args.push(regex);
            }
            for (path, line, text) in grep(mirror, commit, &args, &paths) {
                hits.push(ProtoHit { path, line, text, kind: kind.to_owned() });
            }
        }
        if hits.is_empty() {
            fallback = true;
            let args = ["-n", "-I", "-w", "-F", "--max-count=3", "-e", word].map(str::to_owned);
            for (path, line, text) in grep(mirror, commit, &args, &[]) {
                hits.push(ProtoHit { path, line, text, kind: "word".to_owned() });
            }
        }
    }
    let rank = |hit: &ProtoHit| {
        if hit.path == from {
            0
        } else if dir(&hit.path) == dir(from) {
            1
        } else if ext(&hit.path) == ext(from) {
            2
        } else {
            3
        }
    };
    hits.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.path.cmp(&b.path)).then(a.line.cmp(&b.line)));
    let capped = hits.len() > CAP;
    hits.truncate(CAP);
    ProtoHits { hits, fallback, millis: started.elapsed().as_millis() as u32, capped }
}

/// The whole file `path` at `commit`, highlighted.
pub fn file(mirror: &Path, commit: &str, path: &str) -> Option<Vec<HeadLine>> {
    let name = format!("{commit}:{path}");
    let args = [OsStr::new("-C"), mirror.as_os_str(), OsStr::new("show"), OsStr::new(&name)];
    let text = live::read_git(&args).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let count = lines.len() as u32;
    let spans = Language::of(path)
        .and_then(|language| Parsed::parse(language, text.clone()))
        .map(|parsed| parsed.spans(1..count + 1));
    Some(
        lines
            .iter()
            .enumerate()
            .map(|(at, line)| HeadLine {
                text: (*line).to_owned(),
                spans: spans.as_ref().and_then(|all| all.get(at).cloned()).unwrap_or_default(),
            })
            .collect(),
    )
}
