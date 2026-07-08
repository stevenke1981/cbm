//! Full-pipeline CALLS precision fixtures (Section 7.5).

mod support;

use cbm::discover::IndexMode;
use cbm::pipeline::Pipeline;
use cbm::store::Store;
use support::isolated_cache;
use tempfile::TempDir;

fn calls_edges(store: &Store) -> Vec<cbm::store::Edge> {
    store
        .list_edges_limited(500)
        .unwrap()
        .into_iter()
        .filter(|e| e.edge_type == "CALLS")
        .collect()
}

fn has_call(store: &Store, caller_file: &str, caller: &str, callee: &str) -> bool {
    calls_edges(store).iter().any(|e| {
        e.src_qn
            .contains(&format!("{caller_file}::Function::{caller}@"))
            && e.dst_qn.contains(&format!("::{callee}@"))
    })
}

#[test]
fn python_pipeline_resolves_local_call() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("main.py"),
        "def helper():\n    pass\n\ndef main():\n    helper()\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("py-calls")).unwrap();
    let store = Store::open(&index.project).unwrap();

    assert!(has_call(&store, "main.py", "main", "helper"));
    let edge = calls_edges(&store)
        .into_iter()
        .find(|e| e.dst_qn.contains("helper"))
        .expect("CALLS edge");
    assert!(
        edge.properties_json
            .as_ref()
            .is_some_and(|p| p.contains("regex")),
        "expected regex method metadata"
    );

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn javascript_pipeline_resolves_local_call() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("main.js"),
        "function helper() {}\nfunction main() { helper(); }\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("js-calls")).unwrap();
    let store = Store::open(&index.project).unwrap();
    assert!(has_call(&store, "main.js", "main", "helper"));

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn rust_pipeline_resolves_call_with_ast_metadata() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        "fn helper() {}\nfn main() { helper(); }\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("rs-calls")).unwrap();
    let store = Store::open(&index.project).unwrap();
    assert!(has_call(&store, "lib.rs", "main", "helper"));

    let edge = calls_edges(&store)
        .into_iter()
        .find(|e| e.dst_qn.contains("helper"))
        .expect("CALLS edge");
    assert!(
        edge.properties_json
            .as_ref()
            .is_some_and(|p| p.contains("ast")),
        "expected AST method metadata for Rust"
    );

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn pipeline_skips_ambiguous_cross_file_calls() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() { helper(); }\n").unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn helper() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn helper() {}\n").unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("ambiguous-calls")).unwrap();
    let store = Store::open(&index.project).unwrap();

    let main_calls: Vec<_> = calls_edges(&store)
        .into_iter()
        .filter(|e| e.src_qn.contains("main.rs::Function::main@"))
        .collect();
    assert!(
        main_calls.is_empty(),
        "ambiguous cross-file helper should not link: {main_calls:?}"
    );

    let _ = cbm::store::delete_project_db(&index.project);
}

#[test]
fn nested_python_function_does_not_false_positive_outer() {
    let (_guard, _cache, _) = isolated_cache();
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("nested.py"),
        "def outer():\n    def inner():\n        pass\n    inner()\n",
    )
    .unwrap();

    let pipeline = Pipeline::new(IndexMode::Full);
    let index = pipeline.run(dir.path(), Some("nested-calls")).unwrap();
    let store = Store::open(&index.project).unwrap();

    let edges = calls_edges(&store);
    assert!(
        !edges.iter().any(|e| {
            e.src_qn.contains("outer@") && e.dst_qn.contains("outer@") && e.src_qn != e.dst_qn
        }),
        "outer should not call itself: {edges:?}"
    );

    let _ = cbm::store::delete_project_db(&index.project);
}
