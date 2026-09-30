use std::ops::Range;
use std::path::{Path, PathBuf};

use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use anyhow::{Context as _, Result};
use ignore::{DirEntry, WalkBuilder};
use regex::{Regex, RegexBuilder};

pub const MAX_SEARCH_RESULTS: usize = 1_000;
pub const MAX_SEARCH_FILE_BYTES: u64 = 2 * 1024 * 1024;

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "__pycache__",
    "coverage",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentSearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl ContentSearchQuery {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentSearchResult {
    pub path: PathBuf,
    pub rel_path: String,
    pub line_number: usize,
    pub column: usize,
    pub line_preview: String,
    pub match_ranges: Vec<Range<usize>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentSearchSummary {
    pub results: Vec<ContentSearchResult>,
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub truncated: bool,
}

enum ContentMatcher {
    Literal {
        search: AhoCorasick,
        whole_word: bool,
    },
    Regex {
        regex: Regex,
        whole_word: bool,
    },
}

impl ContentMatcher {
    fn new(query: &ContentSearchQuery) -> Result<Self> {
        if query.regex {
            let regex = RegexBuilder::new(&query.text)
                .case_insensitive(!query.case_sensitive)
                .build()
                .with_context(|| format!("invalid regex: {}", query.text))?;
            return Ok(Self::Regex {
                regex,
                whole_word: query.whole_word,
            });
        }

        if !query.case_sensitive && !query.text.is_ascii() {
            let regex = RegexBuilder::new(&regex::escape(&query.text))
                .case_insensitive(true)
                .build()
                .with_context(|| format!("invalid search text: {}", query.text))?;
            return Ok(Self::Regex {
                regex,
                whole_word: query.whole_word,
            });
        }

        let search = AhoCorasickBuilder::new()
            .ascii_case_insensitive(!query.case_sensitive)
            .build([query.text.as_str()])
            .context("failed to build search matcher")?;
        Ok(Self::Literal {
            search,
            whole_word: query.whole_word,
        })
    }

    fn ranges_in_line(&self, line: &str) -> Vec<Range<usize>> {
        match self {
            Self::Literal { search, whole_word } => search
                .find_iter(line.as_bytes())
                .map(|mat| mat.start()..mat.end())
                .filter(|range| !*whole_word || is_whole_word_match(line, range))
                .collect(),
            Self::Regex { regex, whole_word } => regex
                .find_iter(line)
                .map(|mat| mat.start()..mat.end())
                .filter(|range| !*whole_word || is_whole_word_match(line, range))
                .collect(),
        }
    }
}

pub fn search_project_content(
    root: impl AsRef<Path>,
    query: &ContentSearchQuery,
) -> Result<ContentSearchSummary> {
    let root = root.as_ref();
    if query.is_empty() {
        return Ok(ContentSearchSummary::default());
    }

    let matcher = ContentMatcher::new(query)?;
    let mut summary = ContentSearchSummary::default();
    let mut match_count = 0;
    let mut walker = WalkBuilder::new(root);
    walker
        .standard_filters(true)
        .hidden(false)
        .require_git(false)
        .filter_entry(|entry| !is_skipped_dir(entry));

    for entry in walker.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                summary.files_skipped += 1;
                continue;
            }
        };

        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        if entry.file_name().to_string_lossy() == ".DS_Store" {
            summary.files_skipped += 1;
            continue;
        }

        let path = entry.into_path();
        let Ok(metadata) = std::fs::metadata(&path) else {
            summary.files_skipped += 1;
            continue;
        };
        if metadata.len() > MAX_SEARCH_FILE_BYTES {
            summary.files_skipped += 1;
            continue;
        }

        let Ok(bytes) = std::fs::read(&path) else {
            summary.files_skipped += 1;
            continue;
        };
        if bytes.contains(&0) {
            summary.files_skipped += 1;
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            summary.files_skipped += 1;
            continue;
        };

        summary.files_scanned += 1;
        scan_text_file(
            root,
            &path,
            &content,
            &matcher,
            &mut summary,
            &mut match_count,
        );
        if summary.truncated {
            break;
        }
    }

    Ok(summary)
}

fn scan_text_file(
    root: &Path,
    path: &Path,
    content: &str,
    matcher: &ContentMatcher,
    summary: &mut ContentSearchSummary,
    match_count: &mut usize,
) {
    let rel_path = relative_path(root, path);
    for (line_ix, line) in content.lines().enumerate() {
        let mut ranges = matcher.ranges_in_line(line);
        if ranges.is_empty() {
            continue;
        }

        let remaining = MAX_SEARCH_RESULTS.saturating_sub(*match_count);
        if remaining == 0 {
            summary.truncated = true;
            return;
        }
        if ranges.len() > remaining {
            ranges.truncate(remaining);
            summary.truncated = true;
        }

        let column = ranges
            .first()
            .map(|range| line[..range.start].chars().count() + 1)
            .unwrap_or(1);
        *match_count += ranges.len();
        summary.results.push(ContentSearchResult {
            path: path.to_path_buf(),
            rel_path: rel_path.clone(),
            line_number: line_ix + 1,
            column,
            line_preview: line.to_string(),
            match_ranges: ranges,
        });

        if summary.truncated {
            return;
        }
    }
}

fn is_skipped_dir(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    if !entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    SKIPPED_DIRS.contains(&name.as_ref())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn is_whole_word_match(line: &str, range: &Range<usize>) -> bool {
    let prev = line[..range.start].chars().next_back();
    let next = line[range.end..].chars().next();
    !prev.is_some_and(is_word_char) && !next.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(root: &Path, path: &str, content: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn query(text: &str) -> ContentSearchQuery {
        ContentSearchQuery {
            text: text.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn plain_search_returns_line_results() {
        let temp = TempDir::new().unwrap();
        write(
            temp.path(),
            "src/main.rs",
            b"fn main() {}\nlet main_name = main();\n",
        );

        let result = search_project_content(temp.path(), &query("main")).unwrap();

        assert_eq!(result.files_scanned, 1);
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].rel_path, "src/main.rs");
        assert_eq!(result.results[0].line_number, 1);
        assert_eq!(result.results[0].column, 4);
        assert_eq!(result.results[1].match_ranges.len(), 2);
    }

    #[test]
    fn case_sensitivity_is_configurable() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "a.txt", b"Alpha\nalpha\n");

        let insensitive = search_project_content(temp.path(), &query("alpha")).unwrap();
        assert_eq!(insensitive.results.len(), 2);

        let sensitive = search_project_content(
            temp.path(),
            &ContentSearchQuery {
                text: "alpha".into(),
                case_sensitive: true,
                whole_word: false,
                regex: false,
            },
        )
        .unwrap();
        assert_eq!(sensitive.results.len(), 1);
        assert_eq!(sensitive.results[0].line_number, 2);
    }

    #[test]
    fn unicode_case_insensitive_falls_back_to_regex() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "a.txt", "שלום\n".as_bytes());

        let result = search_project_content(temp.path(), &query("שלום")).unwrap();

        assert_eq!(result.results.len(), 1);
    }

    #[test]
    fn whole_word_filters_embedded_matches() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "a.txt", b"cat scatter cat_1 cat\n");

        let result = search_project_content(
            temp.path(),
            &ContentSearchQuery {
                text: "cat".into(),
                case_sensitive: false,
                whole_word: true,
                regex: false,
            },
        )
        .unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].match_ranges, vec![0..3, 18..21]);
    }

    #[test]
    fn regex_search_and_invalid_regex() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "a.txt", b"abc-123\nabc-xyz\n");

        let result = search_project_content(
            temp.path(),
            &ContentSearchQuery {
                text: r"abc-\d+".into(),
                case_sensitive: true,
                whole_word: false,
                regex: true,
            },
        )
        .unwrap();
        assert_eq!(result.results.len(), 1);

        let error = search_project_content(
            temp.path(),
            &ContentSearchQuery {
                text: "[".into(),
                case_sensitive: false,
                whole_word: false,
                regex: true,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid regex"));
    }

    #[test]
    fn skips_binary_large_and_ignored_files() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), ".gitignore", b"ignored.txt\n");
        write(temp.path(), "ok.txt", b"needle\n");
        write(temp.path(), "ignored.txt", b"needle\n");
        write(temp.path(), "target/generated.txt", b"needle\n");
        write(temp.path(), "binary.bin", b"needle\0needle\n");
        write(
            temp.path(),
            "large.txt",
            vec![b'a'; MAX_SEARCH_FILE_BYTES as usize + 1].as_slice(),
        );

        let result = search_project_content(temp.path(), &query("needle")).unwrap();

        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].rel_path, "ok.txt");
        assert!(result.files_skipped >= 2);
    }

    #[test]
    fn caps_results_globally() {
        let temp = TempDir::new().unwrap();
        let content = (0..(MAX_SEARCH_RESULTS + 10))
            .map(|_| "needle\n")
            .collect::<String>();
        write(temp.path(), "many.txt", content.as_bytes());

        let result = search_project_content(temp.path(), &query("needle")).unwrap();

        let matches = result
            .results
            .iter()
            .map(|result| result.match_ranges.len())
            .sum::<usize>();
        assert_eq!(matches, MAX_SEARCH_RESULTS);
        assert!(result.truncated);
    }
}
