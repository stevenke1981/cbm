//! Function/class registry for CALLS resolution (DeusData registry spirit).
//!
//! Priority chain:
//! 1. same_file
//! 2. import_map (symbols reachable via this file's imports)
//! 3. same_directory / same_module prefix
//! 4. unique_name (exactly one global match)
//! 5. unresolved (empty — never pick ambiguous short names)

use crate::store::Symbol;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveStrategy {
    SameFile,
    ImportMap,
    SameDirectory,
    UniqueName,
}

impl ResolveStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SameFile => "same_file",
            Self::ImportMap => "import_map",
            Self::SameDirectory => "same_directory",
            Self::UniqueName => "unique_name",
        }
    }

    pub fn confidence(self) -> &'static str {
        match self {
            Self::SameFile => "high",
            Self::ImportMap => "high",
            Self::SameDirectory => "medium",
            Self::UniqueName => "medium",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub qualified_name: String,
    pub strategy: ResolveStrategy,
}

/// Project-wide callable registry with prioritized resolution.
#[derive(Debug, Clone)]
pub struct FunctionRegistry {
    /// simple_name → list of qualified names
    by_name: HashMap<String, Vec<String>>,
    /// qualified_name → file_path
    file_of: HashMap<String, String>,
}

impl FunctionRegistry {
    pub fn from_symbols(symbols: &[Symbol]) -> Self {
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_of = HashMap::new();

        for sym in symbols {
            if !matches!(
                sym.label.as_str(),
                "Function" | "Method" | "Class" | "Interface"
            ) {
                continue;
            }
            // Prefer callables for name index; classes used for type-ish names too
            if matches!(sym.label.as_str(), "Function" | "Method") {
                by_name
                    .entry(sym.name.clone())
                    .or_default()
                    .push(sym.qualified_name.clone());
            }
            file_of.insert(sym.qualified_name.clone(), sym.file_path.clone());
        }
        // Stable order for determinism
        for list in by_name.values_mut() {
            list.sort();
        }
        Self { by_name, file_of }
    }

    /// Backward-compatible name→qns map (functions only).
    pub fn name_map(&self) -> &HashMap<String, Vec<String>> {
        &self.by_name
    }

    /// Resolve a callee name for a call site in `caller_file`.
    ///
    /// `import_files`: repo-relative files imported by the caller (resolved).
    pub fn resolve(
        &self,
        callee_name: &str,
        caller_file: &str,
        import_files: &HashSet<String>,
    ) -> Vec<Resolution> {
        let Some(candidates) = self.by_name.get(callee_name) else {
            return Vec::new();
        };

        // 1. Same file
        let same_file: Vec<_> = candidates
            .iter()
            .filter(|qn| self.file_of.get(*qn).map(|f| f.as_str()) == Some(caller_file))
            .cloned()
            .collect();
        if !same_file.is_empty() {
            return same_file
                .into_iter()
                .map(|qualified_name| Resolution {
                    qualified_name,
                    strategy: ResolveStrategy::SameFile,
                })
                .collect();
        }

        // 2. Import map: callees defined in imported files
        if !import_files.is_empty() {
            let imported: Vec<_> = candidates
                .iter()
                .filter(|qn| {
                    self.file_of
                        .get(*qn)
                        .is_some_and(|f| import_files.contains(f))
                })
                .cloned()
                .collect();
            if imported.len() == 1 {
                return vec![Resolution {
                    qualified_name: imported[0].clone(),
                    strategy: ResolveStrategy::ImportMap,
                }];
            }
            // Multiple in imports: still prefer them over global ambiguity, take all
            // only if single file of origin among imports
            if !imported.is_empty() {
                let mut files: HashSet<&str> = HashSet::new();
                for qn in &imported {
                    if let Some(f) = self.file_of.get(qn) {
                        files.insert(f.as_str());
                    }
                }
                if files.len() == 1 {
                    return imported
                        .into_iter()
                        .map(|qualified_name| Resolution {
                            qualified_name,
                            strategy: ResolveStrategy::ImportMap,
                        })
                        .collect();
                }
            }
        }

        // 3. Same directory (sibling modules). Skip when both sit at repo root
        // (empty parent dir) — that would over-match every top-level file.
        let caller_dir = parent_dir(caller_file);
        if !caller_dir.is_empty() {
            let same_dir: Vec<_> = candidates
                .iter()
                .filter(|qn| {
                    self.file_of
                        .get(*qn)
                        .is_some_and(|f| parent_dir(f) == caller_dir)
                })
                .cloned()
                .collect();
            if same_dir.len() == 1 {
                return vec![Resolution {
                    qualified_name: same_dir[0].clone(),
                    strategy: ResolveStrategy::SameDirectory,
                }];
            }
        }

        // 4. Globally unique short name
        if candidates.len() == 1 {
            return vec![Resolution {
                qualified_name: candidates[0].clone(),
                strategy: ResolveStrategy::UniqueName,
            }];
        }

        // 5. Ambiguous — refuse
        Vec::new()
    }
}

fn parent_dir(path: &str) -> String {
    let p = path.replace('\\', "/");
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

fn file_path_from_file_qn(qn: &str) -> Option<String> {
    // "{path}::File::{path}"
    let (path, _rest) = qn.split_once("::File::")?;
    Some(path.replace('\\', "/"))
}

/// Parse import → local file paths using ImportResolver-style relative resolution.
pub fn parse_import_files(
    file_path: &str,
    language: &str,
    content: &str,
    known_files: &HashSet<String>,
) -> HashSet<String> {
    use super::imports::ImportResolver;
    let resolver = ImportResolver::new(known_files.iter().cloned());
    let edges = resolver.extract(file_path, language, content);
    let mut out = HashSet::new();
    for e in edges {
        if let Some(p) = file_path_from_file_qn(&e.dst_qn) {
            if known_files.contains(&p) {
                out.insert(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fun(file: &str, name: &str, line: i64) -> Symbol {
        Symbol {
            qualified_name: format!("{file}::Function::{name}@L{line}"),
            name: name.into(),
            label: "Function".into(),
            file_path: file.into(),
            line_start: line,
            line_end: line + 1,
            signature: None,
            properties_json: None,
        }
    }

    #[test]
    fn prefers_same_file() {
        let reg = FunctionRegistry::from_symbols(&[
            fun("a.rs", "helper", 1),
            fun("b.rs", "helper", 1),
        ]);
        let r = reg.resolve("helper", "a.rs", &HashSet::new());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].strategy, ResolveStrategy::SameFile);
        assert!(r[0].qualified_name.contains("a.rs"));
    }

    #[test]
    fn uses_import_map_for_cross_file() {
        let reg = FunctionRegistry::from_symbols(&[
            fun("src/main.js", "main", 1),
            fun("src/util.js", "helper", 1),
            fun("src/other.js", "helper", 1),
        ]);
        let mut imports = HashSet::new();
        imports.insert("src/util.js".into());
        let r = reg.resolve("helper", "src/main.js", &imports);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].strategy, ResolveStrategy::ImportMap);
        assert!(r[0].qualified_name.contains("util.js"));
    }

    #[test]
    fn refuses_ambiguous_without_import() {
        let reg = FunctionRegistry::from_symbols(&[
            fun("a.rs", "helper", 1),
            fun("b.rs", "helper", 1),
        ]);
        let r = reg.resolve("helper", "c.rs", &HashSet::new());
        assert!(r.is_empty());
    }

    #[test]
    fn unique_name_cross_file() {
        let reg = FunctionRegistry::from_symbols(&[fun("lib.rs", "only_one", 1)]);
        let r = reg.resolve("only_one", "main.rs", &HashSet::new());
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].strategy, ResolveStrategy::UniqueName);
    }
}
