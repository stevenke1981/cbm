mod support;

use cbrlm::hooks::extract_token;
use cbrlm::project::{normalize_project_name, project_db_path};
use cbrlm::store::{Store, Symbol};
use support::isolated_cache;

#[test]
fn hook_augment_query_finds_indexed_symbols() {
    let (_guard, _cache, cache_path) = isolated_cache();
    let project = normalize_project_name("hook-test");
    let store = Store::open(&project).unwrap();
    store
        .upsert_symbol(&Symbol {
            qualified_name: "app.auth.handleAuth".into(),
            name: "handleAuth".into(),
            label: "Function".into(),
            file_path: "src/auth.rs".into(),
            line_start: 1,
            line_end: 10,
            signature: Some("fn handleAuth()".into()),
            properties_json: None,
        })
        .unwrap();
    assert!(project_db_path(&project).starts_with(&cache_path));

    let pattern = ".*handleAuth.*";
    let result = store
        .search(&cbrlm::store::SearchFilter {
            name_pattern: Some(pattern.into()),
            limit: 5,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].name, "handleAuth");
}

#[test]
fn extract_token_integration_cases() {
    assert_eq!(extract_token("grep handleAuth"), Some("handleAuth".into()));
    assert_eq!(extract_token("**/*.tsx"), None);
}