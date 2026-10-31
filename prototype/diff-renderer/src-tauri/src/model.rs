// PROTOTYPE — throwaway. Parses a `git diff` of a CPython range into display rows,
// marks Generated files via .gitattributes, sprinkles synthetic comment threads,
// and highlights code rows with syntect (eagerly, or lazily per file).

use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Instant;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, FontStyle, Style, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

pub const FILE: u8 = 0;
pub const HUNK: u8 = 1;
pub const CTX: u8 = 2;
pub const ADD: u8 = 3;
pub const DEL: u8 = 4;
pub const COMMENT: u8 = 5;
pub const NOTE: u8 = 6;

struct Row {
    kind: u8,
    old: u32,
    new: u32,
    text: String,
    html: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct RowOut {
    k: u8,
    o: u32,
    n: u32,
    h: String,
}

#[derive(Serialize, Clone)]
pub struct FileMeta {
    path: String,
    start: usize,
    end: usize,
    adds: u32,
    dels: u32,
    generated: bool,
    binary: bool,
    max_cols: usize,
}

#[derive(Serialize)]
pub struct LoadInfo {
    dataset: String,
    range: String,
    repo: String,
    files: Vec<FileMeta>,
    total_rows: usize,
    comment_rows: Vec<usize>,
    css: String,
    eager: bool,
    git_ms: f64,
    attr_ms: f64,
    parse_ms: f64,
    highlight_ms: f64,
}

pub struct Model {
    rows: Vec<Row>,
    files: Vec<FileMeta>,
    highlighted: Vec<bool>,
    classes: HashMap<(u8, u8, u8), usize>,
}

struct Hl {
    ss: SyntaxSet,
    theme: Theme,
}

fn hl() -> &'static Hl {
    static HL: OnceLock<Hl> = OnceLock::new();
    HL.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        Hl {
            ss: SyntaxSet::load_defaults_newlines(),
            theme: ts.themes["InspiredGitHub"].clone(),
        }
    })
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn range_for(dataset: &str) -> Result<String, String> {
    match dataset {
        "big" => Ok("v3.15.0b4..v3.15.0rc1".into()),
        "huge" => Ok("v3.15.0b1..v3.15.0rc1".into()),
        "custom" => std::env::var("GHREV_PROTO_RANGE")
            .map_err(|_| "set GHREV_PROTO_RANGE=<base>..<head> for the custom dataset".to_string()),
        other => Err(format!("unknown dataset {other}")),
    }
}

fn repo_path() -> String {
    std::env::var("GHREV_PROTO_REPO").unwrap_or_else(|_| "X:/Work/cpython/main".into())
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn hunk_starts(line: &str) -> (u32, u32) {
    // "@@ -12,7 +12,8 @@ ..."
    let num = |s: &str| -> u32 {
        s.split(|c| c == ',' || c == ' ')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    let old = line.find(" -").map(|i| num(&line[i + 2..])).unwrap_or(0);
    let new = line.find(" +").map(|i| num(&line[i + 2..])).unwrap_or(0);
    (old, new)
}

struct ParsedFile {
    path: String,
    binary: bool,
    rows: Vec<Row>,
}

fn parse(diff: &[u8]) -> Vec<ParsedFile> {
    let mut files: Vec<ParsedFile> = Vec::new();
    let mut in_header = false;
    let (mut old, mut new) = (0u32, 0u32);
    let row = |kind, old, new, text: &str| Row { kind, old, new, text: text.to_string(), html: None };
    for raw in diff.split(|b| *b == b'\n') {
        let s = String::from_utf8_lossy(raw);
        let s = s.strip_suffix('\r').unwrap_or(&s);
        if let Some(rest) = s.strip_prefix("diff --git ") {
            let path = rest.rsplit(" b/").next().unwrap_or(rest).to_string();
            files.push(ParsedFile { path, binary: false, rows: vec![row(FILE, 0, 0, "")] });
            in_header = true;
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if s.starts_with("@@") {
            in_header = false;
            (old, new) = hunk_starts(s);
            f.rows.push(row(HUNK, 0, 0, s));
        } else if in_header {
            if let Some(p) = s.strip_prefix("+++ b/") {
                f.path = p.to_string();
            } else if let Some(p) = s.strip_prefix("rename to ") {
                f.path = p.to_string();
            } else if s.starts_with("Binary files") {
                f.binary = true;
                f.rows.push(row(NOTE, 0, 0, "Binary file not shown"));
            }
        } else if let Some(t) = s.strip_prefix(' ') {
            f.rows.push(row(CTX, old, new, t));
            old += 1;
            new += 1;
        } else if let Some(t) = s.strip_prefix('+') {
            f.rows.push(row(ADD, 0, new, t));
            new += 1;
        } else if let Some(t) = s.strip_prefix('-') {
            f.rows.push(row(DEL, old, 0, t));
            old += 1;
        } else if s.starts_with('\\') {
            f.rows.push(row(NOTE, 0, 0, s));
        }
    }
    files
}

fn generated_paths(repo: &str, head: &str, paths: &[&str]) -> Result<Vec<bool>, String> {
    let mut child = Command::new("git")
        .args(["-C", repo, "check-attr", &format!("--source={head}"), "--stdin", "generated"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().unwrap();
    let input = paths.join("\n") + "\n";
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    let text = String::from_utf8_lossy(&out.stdout);
    let set: std::collections::HashSet<&str> = text
        .lines()
        .filter_map(|l| l.strip_suffix(": generated: set"))
        .collect();
    Ok(paths.iter().map(|p| set.contains(p)).collect())
}

// Deterministic fake review threads standing in for GitHub's bodyHTML.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

const AUTHORS: &[&str] = &["vstinner", "serhiy-storchaka", "encukou", "markshannon", "picnixz", "ZeroIntensity", "JelleZijlstra"];
const SENTENCES: &[&str] = &[
    "This leaks a reference if <code>PyList_Append()</code> fails.",
    "Can we add a test for the error path?",
    "I'm not sure this is safe under the free-threaded build; the critical section is released before the size check.",
    "Nit: please keep the line under 79 columns.",
    "Why not reuse <code>_PyObject_LookupSpecial()</code> here?",
    "The NEWS entry should mention that this also affects subclasses.",
    "LGTM once the comment is updated.",
    "This changes behaviour for negative values — is that intended? It would be good to document it in <code>Doc/library/</code>.",
    "Could you run the benchmark suite? I'd expect a small slowdown on the fast path.",
];

fn comment_html(rng: &mut Rng) -> String {
    let mut h = String::from("<div class=\"thread\">");
    for c in 0..(1 + rng.next() % 3) {
        let who = rng.pick(AUTHORS);
        h.push_str(&format!("<div class=\"cmt\"><div class=\"who\"><b>{who}</b> <span>{} days ago</span></div>", c + 1 + rng.next() % 9));
        for _ in 0..(1 + rng.next() % 4) {
            h.push_str("<p>");
            for _ in 0..(1 + rng.next() % 3) {
                h.push_str(rng.pick(SENTENCES));
                h.push(' ');
            }
            h.push_str("</p>");
        }
        if rng.next() % 4 == 0 {
            h.push_str("<pre><code>if (res &lt; 0) {\n    Py_DECREF(item);\n    return NULL;\n}</code></pre>");
        }
        h.push_str("</div>");
    }
    h.push_str("</div>");
    h
}

impl Model {
    pub fn load(dataset: &str, eager: bool) -> Result<(Model, LoadInfo), String> {
        let range = range_for(dataset)?;
        let repo = repo_path();
        let head = range.split("..").last().unwrap_or("HEAD").to_string();

        let t = Instant::now();
        let out = Command::new("git")
            .args(["-C", &repo, "diff", "--no-color", "--no-ext-diff", "-M", &range])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let git_ms = ms(t);

        let t = Instant::now();
        let parsed = parse(&out.stdout);
        let parse_ms_1 = ms(t);

        let t = Instant::now();
        let paths: Vec<&str> = parsed.iter().map(|f| f.path.as_str()).collect();
        let generated = generated_paths(&repo, &head, &paths)?;
        let attr_ms = ms(t);

        let t = Instant::now();
        let mut rng = Rng(0x9E3779B97F4A7C15);
        let mut rows = Vec::new();
        let mut files = Vec::new();
        let mut comment_rows = Vec::new();
        for (pf, gen_) in parsed.into_iter().zip(generated) {
            let start = rows.len();
            let (mut adds, mut dels, mut max_cols) = (0, 0, 0);
            for r in pf.rows {
                let code = matches!(r.kind, CTX | ADD | DEL);
                adds += (r.kind == ADD) as u32;
                dels += (r.kind == DEL) as u32;
                max_cols = max_cols.max(r.text.chars().count());
                rows.push(r);
                if code && !gen_ && rng.next() % 140 == 0 {
                    comment_rows.push(rows.len());
                    rows.push(Row { kind: COMMENT, old: 0, new: 0, text: String::new(), html: Some(comment_html(&mut rng)) });
                }
            }
            files.push(FileMeta { path: pf.path, start, end: rows.len(), adds, dels, generated: gen_, binary: pf.binary, max_cols });
        }
        let parse_ms = parse_ms_1 + ms(t);

        let (classes, css) = class_table(&hl().theme);
        let mut model = Model { highlighted: vec![false; files.len()], rows, files, classes };

        let t = Instant::now();
        if eager {
            for f in 0..model.files.len() {
                model.highlight_file(f);
            }
        }
        let highlight_ms = if eager { ms(t) } else { 0.0 };

        let info = LoadInfo {
            dataset: dataset.into(),
            range,
            repo,
            files: model.files.clone(),
            total_rows: model.rows.len(),
            comment_rows,
            css,
            eager,
            git_ms,
            attr_ms,
            parse_ms,
            highlight_ms,
        };
        Ok((model, info))
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn rows_out(&mut self, start: usize, end: usize) -> Vec<RowOut> {
        let end = end.min(self.rows.len());
        // Lazy mode: highlight every file the requested range touches.
        let first = self.files.partition_point(|f| f.end <= start);
        for f in first..self.files.len() {
            if self.files[f].start >= end {
                break;
            }
            self.highlight_file(f);
        }
        self.rows[start..end]
            .iter()
            .map(|r| RowOut { k: r.kind, o: r.old, n: r.new, h: r.html.clone().unwrap_or_default() })
            .collect()
    }

    fn highlight_file(&mut self, f: usize) {
        if self.highlighted[f] {
            return;
        }
        self.highlighted[f] = true;
        let Hl { ss, theme } = hl();
        let (start, end) = (self.files[f].start, self.files[f].end);
        let ext = self.files[f].path.rsplit('.').next().unwrap_or("");
        let syntax: &SyntaxReference = ss.find_syntax_by_extension(ext).unwrap_or_else(|| ss.find_syntax_plain_text());
        // Two parse states approximate old/new file context; hunks are fed back to back.
        let mut new_side = HighlightLines::new(syntax, theme);
        let mut old_side = HighlightLines::new(syntax, theme);
        let classes = &self.classes;
        for r in &mut self.rows[start..end] {
            let html = match r.kind {
                CTX => {
                    let _ = old_side.highlight_line(&format!("{}\n", r.text), ss);
                    spans(&mut new_side, &r.text, ss, classes)
                }
                ADD => spans(&mut new_side, &r.text, ss, classes),
                DEL => spans(&mut old_side, &r.text, ss, classes),
                COMMENT => continue,
                _ => {
                    let mut s = String::new();
                    escape(&r.text, &mut s);
                    s
                }
            };
            r.html = Some(html);
        }
    }
}

fn spans(h: &mut HighlightLines, text: &str, ss: &SyntaxSet, classes: &HashMap<(u8, u8, u8), usize>) -> String {
    let line = format!("{text}\n");
    let mut out = String::with_capacity(text.len() * 2);
    let Ok(tokens) = h.highlight_line(&line, ss) else {
        escape(text, &mut out);
        return out;
    };
    for (style, tok) in tokens {
        let tok = tok.strip_suffix('\n').unwrap_or(tok);
        if tok.is_empty() {
            continue;
        }
        open_span(&style, classes, &mut out);
        escape(tok, &mut out);
        out.push_str("</span>");
    }
    out
}

fn open_span(style: &Style, classes: &HashMap<(u8, u8, u8), usize>, out: &mut String) {
    let Color { r, g, b, .. } = style.foreground;
    let fs = style.font_style;
    let extra = match (fs.contains(FontStyle::BOLD), fs.contains(FontStyle::ITALIC)) {
        (true, true) => " sb si",
        (true, false) => " sb",
        (false, true) => " si",
        _ => "",
    };
    match classes.get(&(r, g, b)) {
        Some(i) => out.push_str(&format!("<span class=\"s{i}{extra}\">")),
        None => out.push_str(&format!("<span class=\"{extra}\" style=\"color:#{r:02x}{g:02x}{b:02x}\">")),
    }
}

fn class_table(theme: &Theme) -> (HashMap<(u8, u8, u8), usize>, String) {
    let mut classes = HashMap::new();
    let mut css = String::from(".sb{font-weight:bold}.si{font-style:italic}");
    let colors = theme.settings.foreground.into_iter().chain(theme.scopes.iter().filter_map(|s| s.style.foreground));
    for Color { r, g, b, .. } in colors {
        let n = classes.len();
        classes.entry((r, g, b)).or_insert_with(|| {
            css.push_str(&format!(".s{n}{{color:#{r:02x}{g:02x}{b:02x}}}"));
            n
        });
    }
    (classes, css)
}

#[cfg(test)]
mod tests {
    #[test]
    fn load_datasets() {
        for d in ["big", "huge"] {
            let (mut m, info) = super::Model::load(d, true).unwrap();
            let generated = info.files.iter().filter(|f| f.generated).count();
            let rows = m.rows_out(0, m.len());
            let bytes: usize = rows.iter().map(|r| r.h.len()).sum();
            eprintln!(
                "{d}: files={} generated={} rows={} comments={} git={:.0}ms attr={:.0}ms parse={:.0}ms highlight={:.0}ms html={:.1}MB",
                info.files.len(), generated, info.total_rows, info.comment_rows.len(),
                info.git_ms, info.attr_ms, info.parse_ms, info.highlight_ms, bytes as f64 / 1e6
            );
        }
    }
}
