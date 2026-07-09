//! Index pipeline passes — OOP abstraction aligned with DeusData/codebase-memory-mcp.
//!
//! Upstream organizes indexing as explicit multi-pass stages (structure, imports,
//! calls, usages, semantic, post). This module mirrors that design with a Rust
//! `IndexPass` trait and shared `PassContext`.

use super::{
    apply_community_properties, build_structure_graph, detect_communities,
    extract_http_client_calls, extract_http_routes, extract_inheritance_edges_with_project,
    link_http_calls, rebuild_call_edges, ImportResolver,
};
use crate::discover::IndexMode;
use crate::error::Result;
use crate::semantic;
use crate::store::{Edge, Store, Symbol};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

/// Shared mutable state for sequential index passes.
pub struct PassContext<'a> {
    pub store: &'a Store,
    pub repo_path: &'a Path,
    pub project_name: &'a str,
    pub mode: IndexMode,
    /// Non-structure symbols extracted from source files.
    pub code_symbols: Vec<Symbol>,
    /// Running edge total contributed by completed passes.
    pub edge_count: usize,
    pub semantic: semantic::SemanticResult,
}

impl<'a> PassContext<'a> {
    pub fn new(
        store: &'a Store,
        repo_path: &'a Path,
        project_name: &'a str,
        mode: IndexMode,
        code_symbols: Vec<Symbol>,
    ) -> Self {
        Self {
            store,
            repo_path,
            project_name,
            mode,
            code_symbols,
            edge_count: 0,
            semantic: semantic::SemanticResult {
                vectors_stored: 0,
                similar_edges: 0,
                semantically_related_edges: 0,
            },
        }
    }

    pub fn symbols_by_file(&self) -> HashMap<String, Vec<Symbol>> {
        self.code_symbols
            .iter()
            .fold(HashMap::new(), |mut acc, sym| {
                acc.entry(sym.file_path.clone())
                    .or_default()
                    .push(sym.clone());
                acc
            })
    }
}

/// Outcome of a single pass (for logging / diagnostics).
#[derive(Debug, Clone, Default)]
pub struct PassOutcome {
    pub symbols_written: usize,
    pub edges_written: usize,
    pub notes: Vec<String>,
}

/// A single indexing stage. Implementations are pure stages over `PassContext`.
pub trait IndexPass: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome>;
}

/// Run a fixed sequence of passes, logging each stage.
pub fn run_passes(ctx: &mut PassContext<'_>, passes: &[&dyn IndexPass]) -> Result<()> {
    for pass in passes {
        let _phase = crate::runtime::profile::PhaseTimer::start(pass.name());
        let outcome = pass.run(ctx)?;
        info!(
            pass = pass.name(),
            symbols = outcome.symbols_written,
            edges = outcome.edges_written,
            "index pass complete"
        );
    }
    Ok(())
}

/// Default full/moderate/fast pass pipeline (structure → edges → semantic → communities).
pub fn default_passes() -> Vec<Box<dyn IndexPass>> {
    vec![
        Box::new(StructurePass),
        Box::new(ImportsPass),
        Box::new(CallsPass),
        Box::new(RoutesPass),
        Box::new(InheritancePass),
        Box::new(SemanticPass),
        Box::new(CommunitiesPass),
    ]
}

// ── Concrete passes ──────────────────────────────────────────────────────────

pub struct StructurePass;

impl IndexPass for StructurePass {
    fn name(&self) -> &'static str {
        "structure"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let file_paths: Vec<String> = ctx.store.list_files()?.into_iter().map(|f| f.path).collect();
        let symbol_qns: Vec<String> = ctx
            .code_symbols
            .iter()
            .map(|s| s.qualified_name.clone())
            .collect();

        ctx.store
            .delete_symbols_by_labels(&["Project", "Folder", "File", "Module"])?;
        let (struct_symbols, struct_edges) = build_structure_graph(
            ctx.project_name,
            ctx.repo_path.to_string_lossy().as_ref(),
            &file_paths,
            &symbol_qns,
        );
        ctx.store.upsert_symbols_batch(&struct_symbols)?;
        ctx.store
            .replace_edges_of_type("CONTAINS", &struct_edges)?;
        ctx.edge_count += struct_edges.len();
        Ok(PassOutcome {
            symbols_written: struct_symbols.len(),
            edges_written: struct_edges.len(),
            notes: vec![],
        })
    }
}

pub struct ImportsPass;

impl IndexPass for ImportsPass {
    fn name(&self) -> &'static str {
        "imports"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let files = ctx.store.list_files()?;
        let known: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let mut contents = HashMap::new();
        for f in &files {
            contents.insert(f.path.clone(), f.content.clone());
        }
        let resolver = ImportResolver::from_project_files(known, &contents);
        let mut import_edges = Vec::new();
        for file in &files {
            import_edges.extend(resolver.extract(&file.path, &file.language, &file.content));
        }
        ctx.store
            .replace_edges_of_type("IMPORTS", &import_edges)?;
        ctx.edge_count += import_edges.len();
        Ok(PassOutcome {
            edges_written: import_edges.len(),
            ..Default::default()
        })
    }
}

pub struct CallsPass;

impl IndexPass for CallsPass {
    fn name(&self) -> &'static str {
        "calls"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let call_edges = rebuild_call_edges(ctx.store, &ctx.code_symbols)?;
        ctx.store.replace_edges_of_type("CALLS", &call_edges)?;
        ctx.edge_count += call_edges.len();
        Ok(PassOutcome {
            edges_written: call_edges.len(),
            ..Default::default()
        })
    }
}

pub struct RoutesPass;

impl IndexPass for RoutesPass {
    fn name(&self) -> &'static str {
        "routes"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let by_file = ctx.symbols_by_file();
        let files = ctx.store.list_files()?;
        let mut route_edges: Vec<Edge> = Vec::new();
        let mut client_calls = Vec::new();
        for file in &files {
            let syms = by_file.get(&file.path).map(|s| s.as_slice()).unwrap_or(&[]);
            route_edges.extend(extract_http_routes(
                &file.path,
                &file.language,
                &file.content,
                syms,
            ));
            client_calls.extend(extract_http_client_calls(
                &file.path,
                &file.language,
                &file.content,
                syms,
            ));
        }
        let http_call_edges = link_http_calls(&client_calls, &route_edges);
        ctx.store
            .replace_edges_of_type("HTTP_ROUTE", &route_edges)?;
        ctx.store
            .replace_edges_of_type("HTTP_CALLS", &http_call_edges)?;
        let total = route_edges.len() + http_call_edges.len();
        ctx.edge_count += total;
        Ok(PassOutcome {
            edges_written: total,
            notes: vec![format!(
                "routes={} http_calls={}",
                route_edges.len(),
                http_call_edges.len()
            )],
            ..Default::default()
        })
    }
}

pub struct InheritancePass;

impl IndexPass for InheritancePass {
    fn name(&self) -> &'static str {
        "inheritance"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let by_file = ctx.symbols_by_file();
        // Include structure/class symbols already in store for cross-file parent resolution.
        let project_symbols: Vec<Symbol> = {
            let mut all = ctx.code_symbols.clone();
            if let Ok(stored) = ctx.store.list_symbols() {
                for s in stored {
                    if matches!(s.label.as_str(), "Class" | "Interface")
                        && !all.iter().any(|e| e.qualified_name == s.qualified_name)
                    {
                        all.push(s);
                    }
                }
            }
            all
        };
        let mut inheritance_edges = Vec::new();
        let mut ast_count = 0usize;
        for file in ctx.store.list_files()? {
            let file_syms = by_file
                .get(&file.path)
                .map(|s| s.as_slice())
                .unwrap_or(&[]);
            let before = inheritance_edges.len();
            inheritance_edges.extend(extract_inheritance_edges_with_project(
                &file.path,
                &file.language,
                &file.content,
                file_syms,
                &project_symbols,
            ));
            // Count AST-tagged edges added for this file
            ast_count += inheritance_edges[before..]
                .iter()
                .filter(|e| {
                    e.properties_json
                        .as_ref()
                        .is_some_and(|p| p.contains(r#""method":"ast""#))
                })
                .count();
        }
        ctx.store.replace_edges_of_types(
            &["INHERITS", "IMPLEMENTS", "DECORATES"],
            &inheritance_edges,
        )?;
        ctx.edge_count += inheritance_edges.len();
        Ok(PassOutcome {
            edges_written: inheritance_edges.len(),
            notes: vec![format!("ast_edges={ast_count}")],
            ..Default::default()
        })
    }
}

pub struct SemanticPass;

impl IndexPass for SemanticPass {
    fn name(&self) -> &'static str {
        "semantic"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let semantic = if semantic::should_run(ctx.mode) {
            semantic::run_semantic_pass(ctx.store)?
        } else {
            semantic::SemanticResult {
                vectors_stored: 0,
                similar_edges: 0,
                semantically_related_edges: 0,
            }
        };
        let edges =
            semantic.similar_edges + semantic.semantically_related_edges;
        ctx.edge_count += edges;
        ctx.semantic = semantic.clone();
        Ok(PassOutcome {
            edges_written: edges,
            notes: vec![format!("vectors={}", semantic.vectors_stored)],
            ..Default::default()
        })
    }
}

pub struct CommunitiesPass;

impl IndexPass for CommunitiesPass {
    fn name(&self) -> &'static str {
        "communities"
    }

    fn run(&self, ctx: &mut PassContext<'_>) -> Result<PassOutcome> {
        let all_edges = ctx.store.list_edges()?;
        let community_result = detect_communities(&ctx.code_symbols, &all_edges);
        let mut updated_symbols = ctx.code_symbols.clone();
        apply_community_properties(&mut updated_symbols, &community_result);
        ctx.store.upsert_symbols_batch(&updated_symbols)?;
        ctx.store.set_meta(
            "community_count",
            &community_result.community_count.to_string(),
        )?;
        ctx.store
            .set_meta("community_algo", community_result.algorithm)?;

        // Compact samples for get_architecture (avoids full symbol table scan).
        let mut by_id: HashMap<u32, Vec<String>> = HashMap::new();
        for (qn, id) in &community_result.assignments {
            let entry = by_id.entry(*id).or_default();
            if entry.len() < 5 {
                entry.push(qn.clone());
            }
        }
        let mut ranked: Vec<(u32, usize, Vec<String>)> = by_id
            .into_iter()
            .map(|(id, sample)| {
                let count = community_result
                    .assignments
                    .values()
                    .filter(|&&v| v == id)
                    .count();
                (id, count, sample)
            })
            .collect();
        ranked.sort_by_key(|(_, count, _)| std::cmp::Reverse(*count));
        ranked.truncate(10);
        let samples = ranked
            .iter()
            .map(|(id, count, sample)| format!("{id}:{count}:{}", sample.join(",")))
            .collect::<Vec<_>>()
            .join(";");
        ctx.store.set_meta("community_samples", &samples)?;

        Ok(PassOutcome {
            symbols_written: updated_symbols.len(),
            notes: vec![format!("communities={}", community_result.community_count)],
            ..Default::default()
        })
    }
}
