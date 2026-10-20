//! Conditionals and loops (`if`, `match`, `for`, `while`, `loop`) are nested twice at most in a
//! function: a third level goes in a function of its own, named for what it does. An `else if`
//! is the same level as its `if`; closures don't start a new level.

use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

/// Levels of conditionals and loops a function can nest
const MAX_DEPTH: usize = 2;

#[test]
fn test_conditionals_and_loops_are_nested_twice_at_most() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = vec![];
    for dir in [
        "proksi/src",
        "proksi/tests",
        "plugins_api/src",
        "plugin_request_id/src",
    ] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "the sources weren't found");

    let mut found = vec![];
    for file in &files {
        let source = std::fs::read_to_string(file).unwrap();
        let parsed = syn::parse_file(&source).unwrap_or_else(|err| panic!("{file:?}: {err}"));
        let mut nesting = Nesting::default();
        nesting.visit_file(&parsed);
        let name = file.strip_prefix(&root).unwrap_or(file).to_string_lossy();
        for (line, function) in nesting.found {
            found.push(format!("crates/{name}:{line} in `{function}`"));
        }
    }
    assert!(
        found.is_empty(),
        "nested more than {MAX_DEPTH} levels (move the inner part to a function of its own):\n{}",
        found.join("\n")
    );
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

#[derive(Default)]
struct Nesting {
    depth: usize,
    function: String,
    /// Line, and function
    found: Vec<(usize, String)>,
}

impl Nesting {
    /// A conditional or loop at `span`, whose inside `visit_inside` visits
    fn level(&mut self, span: proc_macro2::Span, visit_inside: impl FnOnce(&mut Self)) {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.found.push((span.start().line, self.function.clone()));
        }
        visit_inside(self);
        self.depth -= 1;
    }

    /// A function: its own levels, from none
    fn function(&mut self, name: &syn::Ident, visit_body: impl FnOnce(&mut Self)) {
        let depth = std::mem::take(&mut self.depth);
        let function = std::mem::replace(&mut self.function, name.to_string());
        visit_body(self);
        self.depth = depth;
        self.function = function;
    }

    /// An `if` and its `else if`s, at one level
    fn if_chain(&mut self, expr: &syn::ExprIf) {
        self.visit_expr(&expr.cond);
        self.visit_block(&expr.then_branch);
        let Some((_, otherwise)) = &expr.else_branch else {
            return;
        };
        match otherwise.as_ref() {
            syn::Expr::If(next) => self.if_chain(next),
            otherwise => self.visit_expr(otherwise),
        }
    }
}

impl<'a> Visit<'a> for Nesting {
    fn visit_item_fn(&mut self, item: &'a syn::ItemFn) {
        self.function(&item.sig.ident, |nesting| {
            visit::visit_item_fn(nesting, item)
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'a syn::ImplItemFn) {
        self.function(&item.sig.ident, |nesting| {
            visit::visit_impl_item_fn(nesting, item);
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'a syn::TraitItemFn) {
        self.function(&item.sig.ident, |nesting| {
            visit::visit_trait_item_fn(nesting, item);
        });
    }

    fn visit_expr_if(&mut self, expr: &'a syn::ExprIf) {
        self.level(expr.if_token.span, |nesting| nesting.if_chain(expr));
    }

    fn visit_expr_match(&mut self, expr: &'a syn::ExprMatch) {
        self.level(expr.match_token.span, |nesting| {
            visit::visit_expr_match(nesting, expr);
        });
    }

    fn visit_expr_for_loop(&mut self, expr: &'a syn::ExprForLoop) {
        self.level(expr.for_token.span, |nesting| {
            visit::visit_expr_for_loop(nesting, expr);
        });
    }

    fn visit_expr_while(&mut self, expr: &'a syn::ExprWhile) {
        self.level(expr.while_token.span, |nesting| {
            visit::visit_expr_while(nesting, expr);
        });
    }

    fn visit_expr_loop(&mut self, expr: &'a syn::ExprLoop) {
        self.level(expr.loop_token.span, |nesting| {
            visit::visit_expr_loop(nesting, expr);
        });
    }
}
