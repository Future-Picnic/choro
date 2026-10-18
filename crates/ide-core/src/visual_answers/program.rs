//! Bounded structural preflight before a visual answer is persisted. The
//! frontend's actual OpenUI parser still validates component props and data.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, ensure, Context, Result};

use super::{VisualDocument, VisualKind};

/// Checked against the generated renderer catalog by the app's protocol tests.
pub const VISUAL_COMPONENT_NAMES: &[&str] = &[
    "Answer",
    "CategoryChart",
    "DataTable",
    "DesignOptions",
    "Metric",
    "MetricGroup",
    "Note",
    "Section",
    "TrendChart",
];

const DESIGN_EXAMPLE: &str = "root = Answer([choices])\nchoices = DesignOptions()";
const MAX_STATEMENTS: usize = 200;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 10_000;

#[derive(Debug)]
enum Expr {
    Literal,
    List(Vec<Expr>),
    Object(Vec<Expr>),
    Reference(String),
    Call(String, Vec<Expr>),
}

struct Parser<'a> {
    source: &'a str,
    pos: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.pos).copied()
    }

    fn trivia(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if self.peek() == Some(b'#') || self.source[self.pos..].starts_with("//") {
                while self.peek().is_some_and(|c| c != b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn take(&mut self, ch: u8) -> bool {
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn require(&mut self, ch: u8) -> Result<()> {
        self.trivia();
        ensure!(
            self.take(ch),
            "Expected '{}' at byte {}",
            ch as char,
            self.pos
        );
        Ok(())
    }

    fn identifier(&mut self) -> Result<String> {
        self.trivia();
        let start = self.pos;
        ensure!(self.peek().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_'),
            "Expected a statement name or component at byte {}; use OpenUI Lang, not HTML, Markdown or executable expressions", self.pos);
        self.pos += 1;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            self.pos += 1;
        }
        Ok(self.source[start..self.pos].into())
    }

    fn string(&mut self) -> Result<()> {
        let start = self.pos;
        self.require(b'"')?;
        while let Some(ch) = self.peek() {
            self.pos += 1;
            if ch == b'\\' {
                ensure!(self.peek().is_some(), "Unfinished string at byte {start}");
                self.pos += 1;
            } else if ch == b'"' {
                serde_json::from_str::<String>(&self.source[start..self.pos])
                    .context("Use double-quoted strings with JSON escaping")?;
                return Ok(());
            }
        }
        bail!("Unfinished string at byte {start}")
    }

    fn sequence(&mut self, close: u8, depth: usize) -> Result<Vec<Expr>> {
        let mut values = Vec::new();
        self.trivia();
        while !self.take(close) {
            values.push(self.expr(depth + 1)?);
            self.trivia();
            if self.take(close) {
                break;
            }
            self.require(b',')?;
            self.trivia();
        }
        Ok(values)
    }

    fn expr(&mut self, depth: usize) -> Result<Expr> {
        self.trivia();
        self.nodes += 1;
        ensure!(
            depth <= MAX_DEPTH && self.nodes <= MAX_NODES,
            "Visual program is too complex; reduce nesting or split the answer"
        );
        match self.peek() {
            Some(b'"') => {
                self.string()?;
                Ok(Expr::Literal)
            }
            Some(b'[') => {
                self.pos += 1;
                Ok(Expr::List(self.sequence(b']', depth)?))
            }
            Some(b'{') => {
                self.pos += 1;
                let mut values = Vec::new();
                self.trivia();
                while !self.take(b'}') {
                    self.string()?;
                    self.require(b':')?;
                    values.push(self.expr(depth + 1)?);
                    self.trivia();
                    if self.take(b'}') {
                        break;
                    }
                    self.require(b',')?;
                    self.trivia();
                }
                Ok(Expr::Object(values))
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || b"-+.eE".contains(&c))
                {
                    self.pos += 1;
                }
                let value: serde_json::Value = serde_json::from_str(&self.source[start..self.pos])
                    .context("Invalid numeric literal in visual program")?;
                ensure!(value.is_number(), "Expected a numeric literal");
                Ok(Expr::Literal)
            }
            _ => {
                let name = self.identifier()?;
                // Look ahead without consuming the statement's newline.
                let after_name = self.pos;
                self.trivia();
                if self.take(b'(') {
                    Ok(Expr::Call(name, self.sequence(b')', depth)?))
                } else {
                    self.pos = after_name;
                    Ok(if matches!(name.as_str(), "true" | "false" | "null") {
                        Expr::Literal
                    } else {
                        Expr::Reference(name)
                    })
                }
            }
        }
    }

    fn program(&mut self) -> Result<BTreeMap<String, Expr>> {
        let mut statements = BTreeMap::new();
        self.trivia();
        while self.peek().is_some() {
            ensure!(
                statements.len() < MAX_STATEMENTS,
                "Use at most 200 statements"
            );
            let name = self.identifier()?;
            self.require(b'=')?;
            let value = self.expr(0)?;
            ensure!(
                !statements.contains_key(&name),
                "Statement {name} is defined more than once"
            );
            statements.insert(name, value);
            let end = self.pos;
            self.trivia();
            let newline = self.source[end..self.pos].contains('\n');
            ensure!(
                self.peek().is_none() || self.take(b';') || newline,
                "Put each visual-program statement on its own line at byte {}",
                self.pos
            );
            self.trivia();
        }
        Ok(statements)
    }
}

fn visit(
    expr: &Expr,
    statements: &BTreeMap<String, Expr>,
    active: &mut BTreeSet<String>,
    used: &mut BTreeSet<String>,
    components: &mut BTreeSet<String>,
    depth: usize,
) -> Result<()> {
    ensure!(
        depth <= 256,
        "Visual program references are nested too deeply; simplify the answer"
    );
    match expr {
        Expr::Reference(name) => {
            ensure!(
                !active.contains(name),
                "Circular visual-program reference: {name}"
            );
            if used.insert(name.clone()) {
                let next = statements
                    .get(name)
                    .with_context(|| format!("Undefined visual-program reference: {name}"))?;
                active.insert(name.clone());
                visit(next, statements, active, used, components, depth + 1)?;
                active.remove(name);
            }
        }
        Expr::Call(name, children) => {
            ensure!(VISUAL_COMPONENT_NAMES.contains(&name.as_str()),
                "Unknown component {name}. Available components: {}. Use Note(\"text\") for text, not Text or TextContent", VISUAL_COMPONENT_NAMES.join(", "));
            components.insert(name.clone());
            for child in children {
                visit(child, statements, active, used, components, depth + 1)?;
            }
        }
        Expr::List(children) | Expr::Object(children) => {
            for child in children {
                visit(child, statements, active, used, components, depth + 1)?;
            }
        }
        Expr::Literal => {}
    }
    Ok(())
}

fn resolve<'a>(expr: &'a Expr, statements: &'a BTreeMap<String, Expr>) -> &'a Expr {
    // visit() has already rejected undefined and cyclic references.
    match expr {
        Expr::Reference(name) => resolve(&statements[name], statements),
        other => other,
    }
}

pub(super) fn validate(document: &VisualDocument) -> Result<()> {
    let result: Result<()> = (|| {
        let statements = Parser {
            source: &document.openui_lang,
            pos: 0,
            nodes: 0,
        }
        .program()?;
        let root = statements
            .get("root")
            .context("The program must define root = Answer([...])")?;
        let mut used = BTreeSet::from(["root".into()]);
        let mut components = BTreeSet::new();
        visit(
            root,
            &statements,
            &mut BTreeSet::from(["root".into()]),
            &mut used,
            &mut components,
            0,
        )?;
        let Expr::Call(name, args) = resolve(root, &statements) else {
            bail!("The root must be Answer([...])")
        };
        ensure!(
            name == "Answer"
                && args.len() == 1
                && matches!(resolve(&args[0], &statements), Expr::List(_)),
            "The root must be Answer([...]) with one component list"
        );
        let unused: Vec<_> = statements
            .keys()
            .filter(|name| !used.contains(*name))
            .cloned()
            .collect();
        ensure!(
            unused.is_empty(),
            "Statements not used by root: {}",
            unused.join(", ")
        );
        if document.kind == VisualKind::DesignOptions || !document.options.is_empty() {
            ensure!(
                components.contains("DesignOptions"),
                "Design alternatives must be displayed with DesignOptions() inside Answer([...])"
            );
        }
        ensure!(
            !components.contains("DesignOptions") || (2..=4).contains(&document.options.len()),
            "DesignOptions requires two to four alternatives in document.options"
        );
        Ok(())
    })();
    result.map_err(|error| anyhow::anyhow!("Invalid openui_lang: {error:#}. Correct the program and retry visual_present; nothing was published. Design-choice example:\n{DESIGN_EXAMPLE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_examples_match_the_actual_frontend_parser_contract() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("program_cases.json")).unwrap();
        for case in cases {
            let doc: VisualDocument = serde_json::from_value(case["document"].clone()).unwrap();
            assert_eq!(
                doc.validate().is_ok(),
                case["valid"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    fn document(source: &str) -> VisualDocument {
        serde_json::from_value(serde_json::json!({"title":"Example", "summary":"Example",
            "kind":"summary", "openui_lang":source}))
        .unwrap()
    }

    #[test]
    fn rejects_unknown_components_without_mistaking_quoted_text_for_code() {
        for source in [
            "root = Text(\"Choose\")",
            "root = Answer([x])\nx = TextContent(\"Choose\")",
        ] {
            let error = format!("{:#}", validate(&document(source)).unwrap_err());
            assert!(error.contains("Unknown component"));
            assert!(error.contains(DESIGN_EXAMPLE));
        }
        validate(&document(
            "root = Answer([note])\nnote = Note(\"Text(\\\"not code\\\") — café\")",
        ))
        .unwrap();
    }

    #[test]
    fn rejects_broken_roots_references_cycles_and_unused_statements() {
        for source in [
            "x = Note(\"x\")",
            "root = Note(\"x\")",
            "root = Answer(\"x\")",
            "root = Answer([missing])",
            "root = Answer([x])\nx = root",
            "root = Answer([])\nx = Note(\"unused\")",
            "root = Answer([])\nroot = Answer([])",
            "root = Answer([Note(\"unfinished\")",
        ] {
            assert!(validate(&document(source)).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_hoisted_nested_components_literals_and_multiline_lists() {
        validate(&document("# chart\nroot = Answer([\nsection,\n])\nsection = Section(\"Trends\", [chart, note])\nchart = TrendChart(\"days\", \"date\", [\"a\"], \"Trend\")\nnote = Note(\"Missing is not zero\", \"caution\")")).unwrap();
    }

    #[test]
    fn options_cannot_be_saved_without_a_reachable_design_component() {
        let mut doc = document("root = Answer([Note(\"Options\")])");
        doc.kind = VisualKind::DesignOptions;
        doc.options = serde_json::from_value(serde_json::json!([
            {"id":"a","title":"A","description":"","html":"<b>A</b>"},
            {"id":"b","title":"B","description":"","html":"<b>B</b>"}]))
        .unwrap();
        assert!(validate(&doc).is_err());
        doc.openui_lang = DESIGN_EXAMPLE.into();
        validate(&doc).unwrap();
        doc.openui_lang =
            "root = Answer([choices])\nchoices = DesignOptions(\"section\", 720, 1024)".into();
        validate(&doc).unwrap();
    }

    #[test]
    fn bounds_deep_programs_before_recursing_without_limit() {
        let source = format!("root = Answer([{}0{}])", "[".repeat(70), "]".repeat(70));
        assert!(validate(&document(&source)).is_err());
    }
}
