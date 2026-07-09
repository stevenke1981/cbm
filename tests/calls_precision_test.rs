use cbm::pipeline::{build_name_registry, resolve_calls_with_registry};
use cbm::store::Symbol;
use cbm::symbol_id::qualified_name;

fn function_sym(file: &str, name: &str, line: i64, end: i64) -> Symbol {
    Symbol {
        qualified_name: qualified_name(file, "Function", name, line),
        name: name.into(),
        label: "Function".into(),
        file_path: file.into(),
        line_start: line,
        line_end: end,
        signature: None,
        properties_json: None,
    }
}

fn assert_resolves(language: &str, src: &str, symbols: &[Symbol], caller: &str, callee: &str) {
    let caller_sym = symbols
        .iter()
        .find(|s| s.name == caller)
        .expect("caller symbol");
    let registry = build_name_registry(symbols);
    let edges =
        resolve_calls_with_registry(std::slice::from_ref(caller_sym), src, language, &registry);
    assert!(
        edges
            .iter()
            .any(|e| e.dst_qn.contains(&format!("::{callee}@"))),
        "{language}: expected {caller} -> {callee}, got {edges:?}"
    );
}

#[test]
fn python_resolves_local_call() {
    let symbols = vec![
        function_sym("main.py", "helper", 1, 2),
        function_sym("main.py", "main", 4, 6),
    ];
    let src = "def helper():\n    pass\n\ndef main():\n    helper()\n";
    assert_resolves("python", src, &symbols, "main", "helper");
}

#[test]
fn javascript_resolves_local_call() {
    let symbols = vec![
        function_sym("main.js", "helper", 1, 1),
        function_sym("main.js", "main", 2, 2),
    ];
    let src = "function helper() {}\nfunction main() { helper(); }\n";
    assert_resolves("javascript", src, &symbols, "main", "helper");
}

#[test]
fn go_resolves_local_call() {
    let symbols = vec![
        function_sym("main.go", "helper", 2, 2),
        function_sym("main.go", "main", 3, 3),
    ];
    let src = "package main\nfunc helper() {}\nfunc main() { helper() }\n";
    assert_resolves("go", src, &symbols, "main", "helper");
}

#[test]
fn java_resolves_local_call() {
    let symbols = vec![
        function_sym("App.java", "helper", 2, 2),
        function_sym("App.java", "main", 3, 3),
    ];
    let src = "class App {\n  void helper() {}\n  void main() { helper(); }\n}\n";
    assert_resolves("java", src, &symbols, "main", "helper");
}

#[test]
fn ambiguous_cross_file_call_stays_empty() {
    let symbols = vec![
        function_sym("a.rs", "main", 1, 3),
        function_sym("b.rs", "spawn", 1, 2),
        function_sym("c.rs", "spawn", 1, 2),
    ];
    let src = "fn main() { spawn(); }\n";
    let registry = build_name_registry(&symbols);
    let edges = resolve_calls_with_registry(&symbols[..1], src, "rust", &registry);
    assert!(edges.is_empty());
}

#[test]
fn python_ast_marks_method_metadata() {
    let symbols = vec![
        function_sym("a.py", "main", 1, 2),
        function_sym("a.py", "helper", 4, 5),
    ];
    let src = "def main():\n    helper()\n\ndef helper():\n    pass\n";
    let registry = build_name_registry(&symbols);
    let edges = resolve_calls_with_registry(&symbols[..1], src, "python", &registry);
    assert_eq!(edges.len(), 1);
    assert!(
        edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("ast")),
        "expected AST method metadata for Python"
    );
}

#[test]
fn unsupported_language_uses_regex_fallback() {
    let symbols = vec![
        function_sym("a.rb", "main", 1, 2),
        function_sym("a.rb", "helper", 4, 5),
    ];
    // No Ruby grammar — regex path must still resolve same-file calls.
    let src = "def main\n  helper()\nend\ndef helper\nend\n";
    let registry = build_name_registry(&symbols);
    let edges = resolve_calls_with_registry(&symbols[..1], src, "ruby", &registry);
    assert_eq!(edges.len(), 1);
    assert!(
        edges[0]
            .properties_json
            .as_ref()
            .is_some_and(|p| p.contains("regex")),
        "expected regex fallback for unsupported language"
    );
}
