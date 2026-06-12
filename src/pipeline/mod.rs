mod calls;
mod calls_ast;
mod communities;
mod extract;
mod imports;
mod inheritance;
mod routes;
mod structure;

pub use calls::*;
pub use communities::*;
pub use extract::*;
pub use imports::*;
pub use inheritance::*;
pub use routes::*;
pub use structure::*;

use crate::discover::{discover, language_for_path, DiscoveredFile, IndexMode};
use crate::error::Result;
use crate::git;
use crate::persistence;
use crate::project::{normalize_project_name, project_name_from_path};
use crate::semantic;
use crate::store::{Edge, SourceFile, Store, Symbol};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::{info, warn};

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexResult {
    pub success: bool,
    pub project: String,
    pub repo_path: String,
    pub mode: String,
    pub incremental: bool,
    pub files_indexed: usize,
    pub symbols_extracted: usize,
    pub edges_extracted: usize,
    pub semantic_edges: usize,
    pub vectors_stored: usize,
    pub duration_ms: u64,
    pub artifact_path: Option<String>,
    pub restored_from_artifact: bool,
}

pub struct Pipeline {
    mode: IndexMode,
    export_artifact: bool,
}

impl Pipeline {
    pub fn new(mode: IndexMode) -> Self {
        Self {
            mode,
            export_artifact: false,
        }
    }

    pub fn set_export_artifact(mut self, enabled: bool) -> Self {
        self.export_artifact = enabled;
        self
    }

    pub fn run(&self, repo_path: &Path, project: Option<&str>) -> Result<IndexResult> {
        let start = Instant::now();
        let repo_path = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.to_path_buf());
        let project_name = match project {
            Some(p) => normalize_project_name(p),
            None => project_name_from_path(&repo_path),
        };
        self.run_full(&repo_path, &project_name, start)
    }

    pub fn run_incremental(
        &self,
        repo_path: &Path,
        project: &str,
        changed_files: &[String],
    ) -> Result<IndexResult> {
        let start = Instant::now();
        let repo_path = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.to_path_buf());
        let project_name = normalize_project_name(project);

        if changed_files.is_empty() {
            return Ok(IndexResult {
                success: true,
                project: project_name,
                repo_path: repo_path.to_string_lossy().to_string(),
                mode: format!("{:?}", self.mode).to_lowercase(),
                incremental: true,
                files_indexed: 0,
                symbols_extracted: 0,
                edges_extracted: 0,
                semantic_edges: 0,
                vectors_stored: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                artifact_path: None,
                restored_from_artifact: false,
            });
        }

        info!(
            project = %project_name,
            files = changed_files.len(),
            "starting incremental index"
        );

        let store = Store::open(&project_name)?;
        let git_st = git::status(&repo_path).unwrap_or_default();
        let mut indexed = 0usize;
        let mut symbol_count = 0usize;

        for rel in changed_files {
            let normalized = rel.replace('\\', "/");
            if git_st.deleted_files.iter().any(|d| d == &normalized)
                || !repo_path.join(&normalized).exists()
            {
                store.delete_nodes_by_file(&normalized)?;
                store.delete_file(&normalized)?;
                continue;
            }

            let abs = repo_path.join(&normalized);
            if !abs.is_file() {
                continue;
            }
            let language = language_for_path(&abs).unwrap_or_else(|| "text".into());
            let file = DiscoveredFile {
                path: abs,
                relative_path: normalized,
                language,
            };

            store.delete_nodes_by_file(&file.relative_path)?;
            if let Ok(result) = self.index_file(&file) {
                symbol_count += result.symbols.len();
                store.upsert_file(&result.source_file)?;
                store.upsert_symbols_batch(&result.symbols)?;
                indexed += 1;
            }
        }

        let (edges, semantic) = finalize_index(&store, &repo_path, &project_name, self.mode)?;
        store.upsert_project(repo_path.to_string_lossy().as_ref())?;
        if let Ok(Some(h)) = git::head_sha(&repo_path) {
            store.set_meta("git_head", &h)?;
        }
        store.set_meta("index_mode", &format!("{:?}", self.mode).to_lowercase())?;
        store.set_meta("semantic_enabled", &semantic::is_enabled().to_string())?;
        store.checkpoint()?;
        let artifact_path =
            maybe_export_artifact(&repo_path, &project_name, &store, self.export_artifact)?;

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(IndexResult {
            success: true,
            project: project_name,
            repo_path: repo_path.to_string_lossy().to_string(),
            mode: format!("{:?}", self.mode).to_lowercase(),
            incremental: true,
            files_indexed: indexed,
            symbols_extracted: symbol_count,
            edges_extracted: edges,
            semantic_edges: semantic.similar_edges + semantic.semantically_related_edges,
            vectors_stored: semantic.vectors_stored,
            duration_ms,
            artifact_path: artifact_path.map(|p| p.display().to_string()),
            restored_from_artifact: false,
        })
    }

    /// Use incremental reindex when project exists and git reports dirty files.
    pub fn run_smart(
        &self,
        repo_path: &Path,
        project: Option<&str>,
        incremental: bool,
    ) -> Result<IndexResult> {
        let repo_path = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.to_path_buf());
        let project_name = match project {
            Some(p) => normalize_project_name(p),
            None => project_name_from_path(&repo_path),
        };

        if incremental {
            if let Ok(store) = Store::open(&project_name) {
                if store.get_project().is_ok() {
                    let git_status = git::status(&repo_path)?;
                    if !git_status.changed_files.is_empty() {
                        return self.run_incremental(
                            &repo_path,
                            &project_name,
                            &git_status.changed_files,
                        );
                    }
                }
            }
        }

        self.run(&repo_path, project)
    }

    fn run_full(
        &self,
        repo_path: &Path,
        project_name: &str,
        start: Instant,
    ) -> Result<IndexResult> {
        info!(project = %project_name, path = %repo_path.display(), "starting full index");

        if persistence::try_restore(repo_path, project_name)? {
            let store = Store::open(project_name)?;
            let duration_ms = start.elapsed().as_millis() as u64;
            info!(project = %project_name, "restored graph from compressed artifact");
            return Ok(IndexResult {
                success: true,
                project: project_name.to_string(),
                repo_path: repo_path.to_string_lossy().to_string(),
                mode: format!("{:?}", self.mode).to_lowercase(),
                incremental: false,
                files_indexed: store.count_files().unwrap_or(0) as usize,
                symbols_extracted: store.count_symbols().unwrap_or(0) as usize,
                edges_extracted: 0,
                semantic_edges: 0,
                vectors_stored: 0,
                duration_ms,
                artifact_path: Some(persistence::artifact_path(repo_path).display().to_string()),
                restored_from_artifact: true,
            });
        }

        let _phase = crate::runtime::profile::PhaseTimer::start("discover");
        let files = discover(repo_path, self.mode)?;
        let budget = crate::runtime::budget::MemoryBudget::from_env();
        let store = Store::open(project_name)?;
        store.clear_project_data()?;
        store.upsert_project(repo_path.to_string_lossy().as_ref())?;
        store.set_meta("index_mode", &format!("{:?}", self.mode).to_lowercase())?;
        if let Ok(Some(head)) = git::head_sha(repo_path) {
            store.set_meta("git_head", &head)?;
        }

        let file_results: Vec<FileIndexResult> = files
            .par_iter()
            .filter_map(|file| {
                let size = file.path.metadata().map(|m| m.len() as usize).unwrap_or(0);
                if size > 0 && !budget.try_reserve(size) {
                    warn!(file = %file.relative_path, "skipped file: memory budget exceeded");
                    return None;
                }
                let result = self.index_file(file);
                if size > 0 {
                    budget.release(size);
                }
                match result {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warn!(file = %file.relative_path, error = %e, "index file failed");
                        None
                    }
                }
            })
            .collect();

        let mut all_symbols = Vec::new();
        for result in &file_results {
            all_symbols.extend(result.symbols.clone());
            store.upsert_file(&result.source_file)?;
        }

        store.upsert_symbols_batch(&all_symbols)?;
        let (call_edges, semantic) = finalize_index(&store, repo_path, project_name, self.mode)?;
        store.set_meta("semantic_enabled", &semantic::is_enabled().to_string())?;
        store.checkpoint()?;
        let artifact_path =
            maybe_export_artifact(repo_path, project_name, &store, self.export_artifact)?;

        let duration_ms = start.elapsed().as_millis() as u64;
        info!(
            project = %project_name,
            files = file_results.len(),
            symbols = all_symbols.len(),
            edges = call_edges,
            semantic_edges = semantic.similar_edges + semantic.semantically_related_edges,
            duration_ms,
            artifact = ?artifact_path,
            "index complete"
        );

        Ok(IndexResult {
            success: true,
            project: project_name.to_string(),
            repo_path: repo_path.to_string_lossy().to_string(),
            mode: format!("{:?}", self.mode).to_lowercase(),
            incremental: false,
            files_indexed: file_results.len(),
            symbols_extracted: all_symbols.len(),
            edges_extracted: call_edges,
            semantic_edges: semantic.similar_edges + semantic.semantically_related_edges,
            vectors_stored: semantic.vectors_stored,
            duration_ms,
            artifact_path: artifact_path.map(|p| p.display().to_string()),
            restored_from_artifact: false,
        })
    }

    fn index_file(&self, file: &DiscoveredFile) -> Result<FileIndexResult> {
        let content = std::fs::read_to_string(&file.path)?;
        let line_count = content.lines().count() as i64;
        let source_file = SourceFile {
            path: file.relative_path.clone(),
            content: content.clone(),
            language: file.language.clone(),
            line_count,
        };

        let symbols = extract_symbols(&file.relative_path, &file.language, &content)?;

        Ok(FileIndexResult {
            source_file,
            symbols,
        })
    }
}

fn maybe_export_artifact(
    repo_path: &Path,
    project: &str,
    store: &Store,
    enabled: bool,
) -> Result<Option<std::path::PathBuf>> {
    if !enabled {
        return Ok(None);
    }
    let path = persistence::export_artifact(repo_path, project, store)?;
    Ok(Some(path))
}

fn finalize_index(
    store: &Store,
    repo_path: &Path,
    project_name: &str,
    mode: IndexMode,
) -> Result<(usize, semantic::SemanticResult)> {
    let _phase = crate::runtime::profile::PhaseTimer::start("finalize_index");
    let code_symbols: Vec<Symbol> = store
        .list_symbols()?
        .into_iter()
        .filter(|s| !matches!(s.label.as_str(), "Project" | "Folder" | "File" | "Module"))
        .collect();
    let file_paths: Vec<String> = store.list_files()?.into_iter().map(|f| f.path).collect();
    let symbol_qns: Vec<String> = code_symbols
        .iter()
        .map(|s| s.qualified_name.clone())
        .collect();

    store.delete_symbols_by_labels(&["Project", "Folder", "File", "Module"])?;
    let (struct_symbols, struct_edges) = build_structure_graph(
        project_name,
        repo_path.to_string_lossy().as_ref(),
        &file_paths,
        &symbol_qns,
    );
    store.upsert_symbols_batch(&struct_symbols)?;

    store.delete_edges_by_type("CONTAINS")?;
    store.insert_edges_batch(&struct_edges)?;

    let mut import_edges = Vec::new();
    for file in store.list_files()? {
        import_edges.extend(extract_import_edges(
            &file.path,
            &file.language,
            &file.content,
        ));
    }
    store.delete_edges_by_type("IMPORTS")?;
    store.insert_edges_batch(&import_edges)?;

    let symbols_by_file: HashMap<String, Vec<Symbol>> =
        code_symbols.iter().fold(HashMap::new(), |mut acc, sym| {
            acc.entry(sym.file_path.clone())
                .or_default()
                .push(sym.clone());
            acc
        });

    let call_edges = rebuild_call_edges(store, &code_symbols)?;
    store.delete_edges_by_type("CALLS")?;
    store.insert_edges_batch(&call_edges)?;

    let mut route_edges = Vec::new();
    for file in store.list_files()? {
        if let Some(syms) = symbols_by_file.get(&file.path) {
            route_edges.extend(extract_http_routes(
                &file.path,
                &file.language,
                &file.content,
                syms,
            ));
        }
    }
    store.delete_edges_by_type("HTTP_ROUTE")?;
    store.insert_edges_batch(&route_edges)?;

    let mut inheritance_edges = Vec::new();
    for file in store.list_files()? {
        if let Some(syms) = symbols_by_file.get(&file.path) {
            inheritance_edges.extend(extract_inheritance_edges(
                &file.path,
                &file.language,
                &file.content,
                syms,
            ));
        }
    }
    for edge_type in ["INHERITS", "IMPLEMENTS", "DECORATES"] {
        store.delete_edges_by_type(edge_type)?;
    }
    store.insert_edges_batch(&inheritance_edges)?;

    let mut edge_count = struct_edges.len()
        + import_edges.len()
        + call_edges.len()
        + route_edges.len()
        + inheritance_edges.len();

    let semantic = if semantic::should_run(mode) {
        semantic::run_semantic_pass(store)?
    } else {
        semantic::SemanticResult {
            vectors_stored: 0,
            similar_edges: 0,
            semantically_related_edges: 0,
        }
    };
    edge_count += semantic.similar_edges + semantic.semantically_related_edges;

    let all_edges = store.list_edges()?;
    let community_result = detect_communities(&code_symbols, &all_edges);
    let mut updated_symbols = code_symbols.clone();
    apply_community_properties(&mut updated_symbols, &community_result);
    store.upsert_symbols_batch(&updated_symbols)?;
    store.set_meta(
        "community_count",
        &community_result.community_count.to_string(),
    )?;

    Ok((edge_count, semantic))
}

fn rebuild_call_edges(store: &Store, code_symbols: &[Symbol]) -> Result<Vec<Edge>> {
    let registry = build_name_registry(code_symbols);
    let files = store.list_files()?;
    let symbols_by_file: HashMap<String, Vec<Symbol>> =
        code_symbols.iter().fold(HashMap::new(), |mut acc, sym| {
            acc.entry(sym.file_path.clone())
                .or_default()
                .push(sym.clone());
            acc
        });

    let mut edges = Vec::new();
    for file in files {
        if let Some(symbols) = symbols_by_file.get(&file.path) {
            edges.extend(resolve_calls_with_registry(
                symbols,
                &file.content,
                &file.language,
                &registry,
            ));
        }
    }
    Ok(edges)
}

#[derive(Debug)]
struct FileIndexResult {
    source_file: SourceFile,
    symbols: Vec<Symbol>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::test_lock;
    use std::path::Path;

    fn with_isolated_cache() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
        let guard = test_lock::acquire();
        let dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("CBRLM_CACHE_DIR", dir.path());
        (guard, dir)
    }

    fn init_git_repo(path: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t.com"])
            .current_dir(path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    #[test]
    fn persists_symbols_to_store() {
        let (_guard, _cache) = with_isolated_cache();
        let repo = tempfile::TempDir::new().unwrap();
        std::fs::write(
            repo.path().join("main.rs"),
            "fn main() { foo(); }\nfn foo() {}\n",
        )
        .unwrap();

        let pipeline = Pipeline::new(IndexMode::Full);
        let result = pipeline.run(repo.path(), Some("pipeline-persist")).unwrap();
        assert!(result.symbols_extracted > 0);

        let store = Store::open(&result.project).unwrap();
        assert!(store.count_symbols().unwrap() > 0);
    }

    #[test]
    fn incremental_updates_changed_file() {
        let (_guard, _cache) = with_isolated_cache();
        let repo = tempfile::TempDir::new().unwrap();
        init_git_repo(repo.path());
        std::fs::write(repo.path().join("lib.rs"), "pub fn old() {}\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo.path())
            .output()
            .unwrap();

        let pipeline = Pipeline::new(IndexMode::Full);
        let full = pipeline.run(repo.path(), Some("incr-test")).unwrap();
        assert!(full.symbols_extracted >= 1);

        std::fs::write(repo.path().join("lib.rs"), "pub fn new_name() {}\n").unwrap();
        let incr = pipeline
            .run_incremental(repo.path(), &full.project, &["lib.rs".into()])
            .unwrap();
        assert_eq!(incr.files_indexed, 1);

        let store = Store::open(&full.project).unwrap();
        let search = store
            .search(&crate::store::SearchFilter {
                query: Some("new_name".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(search.symbols.iter().any(|s| s.name == "new_name"));
    }

    #[test]
    fn semantic_pass_stores_vectors_and_edges() {
        let (_guard, _cache) = with_isolated_cache();
        std::env::set_var("CBRLM_SEMANTIC_ENABLED", "1");

        let repo = tempfile::TempDir::new().unwrap();
        std::fs::write(
            repo.path().join("a.rs"),
            "pub fn fetch_user(id: u64) {}\npub fn fetch_user_profile(id: u64) {}\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join("b.rs"),
            "pub fn load_user_data(id: u64) {}\n",
        )
        .unwrap();

        let pipeline = Pipeline::new(IndexMode::Full);
        let result = pipeline.run(repo.path(), Some("semantic-test")).unwrap();
        assert!(result.vectors_stored >= 2, "expected vectors");

        let store = Store::open(&result.project).unwrap();
        assert!(store.count_vectors().unwrap() >= 2);

        std::env::remove_var("CBRLM_SEMANTIC_ENABLED");
    }
}
