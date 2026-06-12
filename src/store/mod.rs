mod codec;
mod schema;
mod types;

pub use schema::*;
pub use types::*;

use crate::error::{Error, Result};
use crate::project::project_db_path;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::{HashMap, HashSet, VecDeque};

pub struct Store {
    conn: Connection,
    project: String,
}

impl Store {
    pub fn open(project: &str) -> Result<Self> {
        Self::open_with_flags(project, OpenFlags::default())
    }

    pub fn open_readonly(project: &str) -> Result<Self> {
        Self::open_with_flags(
            project,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
    }

    fn open_with_flags(project: &str, flags: OpenFlags) -> Result<Self> {
        let path = project_db_path(project);
        let readonly = flags.contains(OpenFlags::SQLITE_OPEN_READ_ONLY);
        if !readonly {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open_with_flags(&path, flags)?;
        if !flags.contains(OpenFlags::SQLITE_OPEN_READ_ONLY) {
            apply_sqlite_pragmas(&conn)?;
        }
        let store = Self {
            conn,
            project: project.to_string(),
        };
        if !readonly {
            store.init_schema()?;
            store.set_meta("schema_version", "2")?;
        }
        Ok(store)
    }

    pub fn integrity_check(&self) -> Result<String> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(result)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=NORMAL;")?;
        if let Some(size) = sqlite_mmap_size() {
            conn.execute(&format!("PRAGMA mmap_size = {size}"), [])?;
        }
        let store = Self {
            conn,
            project: "memory".to_string(),
        };
        store.init_schema()?;
        Ok(store)
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    pub fn upsert_project(&self, repo_path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO projects (name, repo_path, indexed_at) VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(name) DO UPDATE SET repo_path=excluded.repo_path, indexed_at=datetime('now')",
            params![self.project, repo_path],
        )?;
        Ok(())
    }

    pub fn list_projects() -> Result<Vec<ProjectInfo>> {
        let cache = crate::project::default_cache_dir();
        if !cache.exists() {
            return Ok(vec![]);
        }
        let mut projects = Vec::new();
        for entry in std::fs::read_dir(&cache)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if let Ok(store) = Self::open(&name) {
                if let Ok(info) = store.get_project() {
                    projects.push(info);
                }
            }
        }
        projects.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(projects)
    }

    pub fn get_project(&self) -> Result<ProjectInfo> {
        self.conn
            .query_row(
                "SELECT name, repo_path, indexed_at FROM projects WHERE name = ?1",
                params![self.project],
                |row| {
                    Ok(ProjectInfo {
                        name: row.get(0)?,
                        repo_path: row.get(1)?,
                        indexed_at: row.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::ProjectNotFound(self.project.clone()))
    }

    pub fn delete_project(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE project = ?1",
            params![self.project],
        )?;
        self.conn.execute(
            "DELETE FROM symbols WHERE project = ?1",
            params![self.project],
        )?;
        self.conn.execute(
            "DELETE FROM files WHERE project = ?1",
            params![self.project],
        )?;
        self.clear_vectors()?;
        self.conn
            .execute("DELETE FROM meta WHERE project = ?1", params![self.project])?;
        self.conn.execute(
            "DELETE FROM projects WHERE name = ?1",
            params![self.project],
        )?;
        Ok(())
    }

    pub fn clear_project_data(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE project = ?1",
            params![self.project],
        )?;
        self.conn.execute(
            "DELETE FROM symbols WHERE project = ?1",
            params![self.project],
        )?;
        self.conn.execute(
            "DELETE FROM files WHERE project = ?1",
            params![self.project],
        )?;
        self.clear_vectors()?;
        Ok(())
    }

    pub fn upsert_symbol(&self, sym: &Symbol) -> Result<()> {
        self.conn.execute(
            "INSERT INTO symbols (qualified_name, project, name, label, file_path, line_start, line_end, signature, properties_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(qualified_name, project) DO UPDATE SET
               name=excluded.name, label=excluded.label, file_path=excluded.file_path,
               line_start=excluded.line_start, line_end=excluded.line_end,
               signature=excluded.signature, properties_json=excluded.properties_json",
            params![
                sym.qualified_name,
                self.project,
                sym.name,
                sym.label,
                sym.file_path,
                sym.line_start,
                sym.line_end,
                sym.signature,
                sym.properties_json,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_symbols_batch(&self, symbols: &[Symbol]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for sym in symbols {
            tx.execute(
                "INSERT INTO symbols (qualified_name, project, name, label, file_path, line_start, line_end, signature, properties_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(qualified_name, project) DO UPDATE SET
                   name=excluded.name, label=excluded.label, file_path=excluded.file_path,
                   line_start=excluded.line_start, line_end=excluded.line_end,
                   signature=excluded.signature, properties_json=excluded.properties_json",
                params![
                    sym.qualified_name,
                    self.project,
                    sym.name,
                    sym.label,
                    sym.file_path,
                    sym.line_start,
                    sym.line_end,
                    sym.signature,
                    sym.properties_json,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn insert_edge(&self, edge: &Edge) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO edges (src_qn, dst_qn, edge_type, project, properties_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                edge.src_qn,
                edge.dst_qn,
                edge.edge_type,
                self.project,
                edge.properties_json,
            ],
        )?;
        Ok(())
    }

    pub fn insert_edges_batch(&self, edges: &[Edge]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for edge in edges {
            tx.execute(
                "INSERT OR IGNORE INTO edges (src_qn, dst_qn, edge_type, project, properties_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    edge.src_qn,
                    edge.dst_qn,
                    edge.edge_type,
                    self.project,
                    edge.properties_json,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_file(&self, file: &SourceFile) -> Result<()> {
        let stored_content = codec::maybe_compress(&file.content);
        self.conn.execute(
            "INSERT INTO files (path, project, content, language, line_count)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path, project) DO UPDATE SET
               content=excluded.content, language=excluded.language, line_count=excluded.line_count",
            params![
                file.path,
                self.project,
                stored_content,
                file.language,
                file.line_count,
            ],
        )?;
        Ok(())
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (key, project, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(key, project) DO UPDATE SET value=excluded.value",
            params![key, self.project, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let val = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1 AND project = ?2",
                params![key, self.project],
                |row| row.get(0),
            )
            .optional()?;
        Ok(val)
    }

    pub fn checkpoint(&self) -> Result<()> {
        let _: (i32, i32, i32) =
            self.conn
                .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        Ok(())
    }

    pub fn checkpoint_truncate(&self) -> Result<()> {
        let _: (i32, i32, i32) =
            self.conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?;
        Ok(())
    }

    pub fn count_symbols(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM symbols WHERE project = ?1",
                params![self.project],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn count_files(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE project = ?1",
                params![self.project],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn find_symbol(&self, qn: &str) -> Result<Option<Symbol>> {
        self.conn
            .query_row(
                "SELECT qualified_name, name, label, file_path, line_start, line_end, signature, properties_json
                 FROM symbols WHERE qualified_name = ?1 AND project = ?2",
                params![qn, self.project],
                symbol_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn search(&self, filter: &SearchFilter) -> Result<SearchResult> {
        let mut stmt = self.conn.prepare(
            "SELECT qualified_name, name, label, file_path, line_start, line_end, signature, properties_json
             FROM symbols WHERE project = ?1 ORDER BY qualified_name",
        )?;
        let all: Vec<Symbol> = stmt
            .query_map(params![self.project], symbol_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut filtered: Vec<Symbol> = all
            .iter()
            .filter(|sym| matches_filter(sym, filter))
            .cloned()
            .collect();

        if needs_graph_filter(filter) {
            let graph = EdgeGraph::from_edges(&self.list_edges()?);
            let direction = normalize_direction(filter.direction.as_deref());
            let relationship = filter.relationship.as_deref();

            if let Some(rel) = relationship {
                filtered.retain(|sym| graph.participates(&sym.qualified_name, rel, direction));
            }

            if filter.min_degree.is_some() || filter.max_degree.is_some() {
                filtered.retain(|sym| {
                    let degree = graph.degree(&sym.qualified_name, relationship, direction);
                    filter.min_degree.is_none_or(|min| degree >= min)
                        && filter.max_degree.is_none_or(|max| degree <= max)
                });
            }

            if filter.exclude_entry_points {
                let rel = relationship.unwrap_or("CALLS");
                filtered.retain(|sym| graph.inbound_degree(&sym.qualified_name, rel) > 0);
            }

            if filter.include_connected {
                let rel = relationship.unwrap_or("CALLS");
                let by_qn: HashMap<String, Symbol> = all
                    .iter()
                    .map(|s| (s.qualified_name.clone(), s.clone()))
                    .collect();
                let mut expanded: HashSet<String> =
                    filtered.iter().map(|s| s.qualified_name.clone()).collect();
                let mut connected = filtered.clone();
                for sym in &filtered {
                    for neighbor in graph.neighbors(&sym.qualified_name, rel) {
                        if expanded.insert(neighbor.clone()) {
                            if let Some(n) = by_qn.get(&neighbor) {
                                connected.push(n.clone());
                            }
                        }
                    }
                }
                connected.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
                filtered = connected;
            }
        }

        let total = filtered.len();
        let has_more = filter.offset + filter.limit < total;
        let symbols = filtered
            .into_iter()
            .skip(filter.offset)
            .take(filter.limit)
            .collect();

        Ok(SearchResult {
            symbols,
            total,
            limit: filter.limit,
            offset: filter.offset,
            has_more,
        })
    }

    pub fn trace_path(&self, start_qn: &str, direction: &str, depth: usize) -> Result<TraceResult> {
        let start = self
            .find_symbol(start_qn)?
            .ok_or_else(|| Error::SymbolNotFound(start_qn.to_string()))?;

        let mut visited = HashSet::new();
        let mut nodes = vec![start.clone()];
        let mut edges = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back((start_qn.to_string(), 0));
        visited.insert(start_qn.to_string());

        while let Some((qn, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }
            let edge_rows = match direction {
                "outbound" | "out" => self.edges_from(&qn)?,
                "inbound" | "in" => self.edges_to(&qn)?,
                _ => {
                    let mut all = self.edges_from(&qn)?;
                    all.extend(self.edges_to(&qn)?);
                    all
                }
            };
            for edge in edge_rows {
                edges.push(edge.clone());
                let neighbor = if edge.src_qn == qn {
                    &edge.dst_qn
                } else {
                    &edge.src_qn
                };
                if visited.insert(neighbor.clone()) {
                    if let Some(sym) = self.find_symbol(neighbor)? {
                        nodes.push(sym);
                        queue.push_back((neighbor.clone(), d + 1));
                    }
                }
            }
        }

        Ok(TraceResult {
            start: start_qn.to_string(),
            direction: direction.to_string(),
            depth,
            nodes,
            edges,
        })
    }

    fn edges_from(&self, qn: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_qn, dst_qn, edge_type, properties_json FROM edges
             WHERE src_qn = ?1 AND project = ?2",
        )?;
        let edges = stmt
            .query_map(params![qn, self.project], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    fn edges_to(&self, qn: &str) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_qn, dst_qn, edge_type, properties_json FROM edges
             WHERE dst_qn = ?1 AND project = ?2",
        )?;
        let edges = stmt
            .query_map(params![qn, self.project], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    pub fn get_snippet(&self, qn: &str) -> Result<CodeSnippet> {
        let sym = self
            .find_symbol(qn)?
            .ok_or_else(|| Error::SymbolNotFound(qn.to_string()))?;
        let stored: String = self
            .conn
            .query_row(
                "SELECT content FROM files WHERE path = ?1 AND project = ?2",
                params![sym.file_path, self.project],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let content = codec::maybe_decompress(&stored);
        let lines: Vec<&str> = content.lines().collect();
        let start = sym.line_start.saturating_sub(1) as usize;
        let end = sym.line_end.min(lines.len() as i64) as usize;
        let snippet = if start < end && !lines.is_empty() {
            lines[start..end].join("\n")
        } else {
            content
        };
        Ok(CodeSnippet {
            symbol: sym,
            snippet,
        })
    }

    pub fn search_code(&self, pattern: &str, limit: usize) -> Result<Vec<CodeMatch>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, content, language FROM files WHERE project = ?1")?;
        let mut matches = Vec::new();
        let rows = stmt.query_map(params![self.project], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (path, stored, language) = row?;
            let content = read_file_content(&stored);
            if !content.contains(pattern) {
                continue;
            }
            let line_number = content
                .lines()
                .position(|line| line.contains(pattern))
                .map(|i| i + 1)
                .unwrap_or(1);
            matches.push(CodeMatch {
                path,
                language,
                line_number,
                preview: content
                    .lines()
                    .nth(line_number.saturating_sub(1))
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect(),
            });
            if matches.len() >= limit {
                break;
            }
        }
        Ok(matches)
    }

    pub fn query_select(&self, sql: &str) -> Result<QueryResult> {
        validate_readonly_select(sql)?;

        let mut stmt = self.conn.prepare(sql.trim())?;
        let col_count = stmt.column_count();
        let columns: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
            .collect();
        let rows = stmt
            .query_map([], |row| {
                let mut map = HashMap::new();
                for (i, col) in columns.iter().enumerate() {
                    let val: rusqlite::types::Value = row.get(i)?;
                    map.insert(col.clone(), value_to_json(val));
                }
                Ok(map)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(QueryResult { columns, rows })
    }

    pub fn get_schema(&self) -> GraphSchema {
        let mut schema = GraphSchema::standard();
        if let Ok(mut edge_stmt) = self.conn.prepare(
            "SELECT edge_type FROM edges WHERE project = ?1 GROUP BY edge_type ORDER BY edge_type",
        ) {
            if let Ok(rows) =
                edge_stmt.query_map(params![self.project], |row| row.get::<_, String>(0))
            {
                schema.implemented_edge_types = rows.filter_map(|r| r.ok()).collect();
            }
        }
        schema
    }

    pub fn get_architecture(&self) -> Result<ArchitectureSummary> {
        let symbol_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE project = ?1",
            params![self.project],
            |row| row.get(0),
        )?;
        let edge_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE project = ?1",
            params![self.project],
            |row| row.get(0),
        )?;
        let file_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE project = ?1",
            params![self.project],
            |row| row.get(0),
        )?;

        let mut label_stmt = self.conn.prepare(
            "SELECT label, COUNT(*) FROM symbols WHERE project = ?1 GROUP BY label ORDER BY COUNT(*) DESC",
        )?;
        let labels = label_stmt
            .query_map(params![self.project], |row| {
                Ok(LabelCount {
                    label: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut edge_stmt = self.conn.prepare(
            "SELECT edge_type, COUNT(*) FROM edges WHERE project = ?1 GROUP BY edge_type ORDER BY COUNT(*) DESC",
        )?;
        let edge_types = edge_stmt
            .query_map(params![self.project], |row| {
                Ok(EdgeTypeCount {
                    edge_type: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let top_functions = self.search(&SearchFilter {
            label: Some("Function".into()),
            limit: 10,
            ..Default::default()
        })?;

        let all_symbols = self.list_symbols()?;
        let mut community_counts: HashMap<u32, (usize, Vec<String>)> = HashMap::new();
        for sym in &all_symbols {
            let Some(props) = sym.properties_json.as_ref() else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(props) else {
                continue;
            };
            let Some(id) = v.get("community_id").and_then(|x| x.as_u64()) else {
                continue;
            };
            let entry = community_counts.entry(id as u32).or_insert((0, Vec::new()));
            entry.0 += 1;
            if entry.1.len() < 5 {
                entry.1.push(sym.qualified_name.clone());
            }
        }
        let community_count = community_counts.len();
        let mut top_communities: Vec<CommunitySummary> = community_counts
            .into_iter()
            .map(|(id, (symbol_count, sample_symbols))| CommunitySummary {
                id,
                symbol_count,
                sample_symbols,
            })
            .collect();
        top_communities.sort_by_key(|b| std::cmp::Reverse(b.symbol_count));
        top_communities.truncate(10);

        Ok(ArchitectureSummary {
            project: self.project.clone(),
            symbol_count: symbol_count as usize,
            edge_count: edge_count as usize,
            file_count: file_count as usize,
            labels,
            edge_types,
            top_functions: top_functions.symbols,
            community_count,
            top_communities,
        })
    }

    pub fn index_status(&self) -> Result<IndexStatus> {
        let project = self.get_project().ok();
        let mode = self
            .get_meta("index_mode")?
            .unwrap_or_else(|| "full".into());
        let arch = self.get_architecture().ok();
        let vector_count = self.count_vectors().unwrap_or(0) as usize;
        let semantic_enabled = self
            .get_meta("semantic_enabled")?
            .map(|v| matches!(v.as_str(), "true" | "1"))
            .unwrap_or(false);
        Ok(IndexStatus {
            project: self.project.clone(),
            indexed: project.is_some(),
            repo_path: project.as_ref().map(|p| p.repo_path.clone()),
            indexed_at: project.as_ref().map(|p| p.indexed_at.clone()),
            mode,
            symbol_count: arch.as_ref().map(|a| a.symbol_count).unwrap_or(0),
            edge_count: arch.as_ref().map(|a| a.edge_count).unwrap_or(0),
            file_count: arch.as_ref().map(|a| a.file_count).unwrap_or(0),
            vector_count,
            semantic_enabled,
        })
    }

    pub fn get_adr(&self) -> Result<Option<String>> {
        self.get_meta("adr")
    }

    pub fn set_adr(&self, content: &str) -> Result<()> {
        if content.len() > 8000 {
            return Err(Error::InvalidArgument("ADR max length is 8000".into()));
        }
        self.set_meta("adr", content)
    }

    pub fn delete_nodes_by_file(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM vectors WHERE project = ?1 AND qualified_name IN (
                SELECT qualified_name FROM symbols WHERE file_path = ?2 AND project = ?1
             )",
            params![self.project, file_path],
        )?;
        self.conn.execute(
            "DELETE FROM edges WHERE project = ?1 AND (src_qn IN (
                SELECT qualified_name FROM symbols WHERE file_path = ?2 AND project = ?1
             ) OR dst_qn IN (
                SELECT qualified_name FROM symbols WHERE file_path = ?2 AND project = ?1
             ))",
            params![self.project, file_path],
        )?;
        self.conn.execute(
            "DELETE FROM symbols WHERE file_path = ?1 AND project = ?2",
            params![file_path, self.project],
        )?;
        Ok(())
    }

    pub fn clear_vectors(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM vectors WHERE project = ?1",
            params![self.project],
        )?;
        Ok(())
    }

    pub fn upsert_vector(&self, qn: &str, dim: i32, data: &[i8]) -> Result<()> {
        let blob: Vec<u8> = data.iter().map(|&b| b as u8).collect();
        self.conn.execute(
            "INSERT INTO vectors (qualified_name, project, dim, data) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(qualified_name, project) DO UPDATE SET dim=excluded.dim, data=excluded.data",
            params![qn, self.project, dim, blob],
        )?;
        Ok(())
    }

    pub fn count_vectors(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM vectors WHERE project = ?1",
                params![self.project],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn list_vector_entries(&self) -> Result<Vec<VectorEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT v.qualified_name, v.data, s.name, s.label, s.file_path
             FROM vectors v
             JOIN symbols s ON s.qualified_name = v.qualified_name AND s.project = v.project
             WHERE v.project = ?1",
        )?;
        let rows = stmt
            .query_map(params![self.project], |row| {
                let qn: String = row.get(0)?;
                let data: Vec<u8> = row.get(1)?;
                let name: String = row.get(2)?;
                let label: String = row.get(3)?;
                let file_path: String = row.get(4)?;
                let stored: Vec<i8> = data.iter().map(|&b| b as i8).collect();
                Ok((qn, stored, name, label, file_path))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_file(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM files WHERE path = ?1 AND project = ?2",
            params![file_path, self.project],
        )?;
        Ok(())
    }

    pub fn delete_edges_by_type(&self, edge_type: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM edges WHERE project = ?1 AND edge_type = ?2",
            params![self.project, edge_type],
        )?;
        Ok(())
    }

    pub fn delete_symbols_by_labels(&self, labels: &[&str]) -> Result<()> {
        for label in labels {
            self.conn.execute(
                "DELETE FROM symbols WHERE project = ?1 AND label = ?2",
                params![self.project, label],
            )?;
        }
        Ok(())
    }

    pub fn count_edges_by_type(&self, edge_type: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE project = ?1 AND edge_type = ?2",
                params![self.project, edge_type],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn list_symbols(&self) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT qualified_name, name, label, file_path, line_start, line_end, signature, properties_json
             FROM symbols WHERE project = ?1",
        )?;
        let rows = stmt
            .query_map(params![self.project], symbol_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn ingest_traces(&self, traces: &[(String, String)]) -> Result<usize> {
        let mut count = 0usize;
        for (src, dst) in traces {
            self.conn.execute(
                "INSERT INTO edges (src_qn, dst_qn, edge_type, project)
                 VALUES (?1, ?2, 'RUNTIME_TRACE', ?3)
                 ON CONFLICT(src_qn, dst_qn, edge_type, project) DO NOTHING",
                params![src, dst, self.project],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn list_edges(&self) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_qn, dst_qn, edge_type, properties_json FROM edges WHERE project = ?1",
        )?;
        let rows = stmt
            .query_map(params![self.project], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_edges_limited(&self, limit: usize) -> Result<Vec<Edge>> {
        let mut stmt = self.conn.prepare(
            "SELECT src_qn, dst_qn, edge_type, properties_json FROM edges
             WHERE project = ?1 LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![self.project, limit as i64], edge_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn list_files(&self) -> Result<Vec<SourceFile>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, content, language, line_count FROM files WHERE project = ?1")?;
        let rows = stmt
            .query_map(params![self.project], |row| {
                let stored: String = row.get(1)?;
                Ok(SourceFile {
                    path: row.get(0)?,
                    content: read_file_content(&stored),
                    language: row.get(2)?,
                    line_count: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn apply_sqlite_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    if let Some(size) = sqlite_mmap_size() {
        conn.execute(&format!("PRAGMA mmap_size = {size}"), [])?;
    }
    Ok(())
}

fn sqlite_mmap_size() -> Option<i64> {
    for key in ["CBRLM_SQLITE_MMAP_SIZE", "CBM_SQLITE_MMAP_SIZE"] {
        if let Ok(v) = std::env::var(key) {
            if let Ok(n) = v.parse::<i64>() {
                return Some(n);
            }
        }
    }
    None
}

fn read_file_content(stored: &str) -> String {
    codec::maybe_decompress(stored)
}

struct EdgeGraph {
    outbound: HashMap<(String, String), usize>,
    inbound: HashMap<(String, String), usize>,
    outbound_neighbors: HashMap<(String, String), HashSet<String>>,
    inbound_neighbors: HashMap<(String, String), HashSet<String>>,
}

impl EdgeGraph {
    fn from_edges(edges: &[Edge]) -> Self {
        let mut outbound: HashMap<(String, String), usize> = HashMap::new();
        let mut inbound: HashMap<(String, String), usize> = HashMap::new();
        let mut outbound_neighbors: HashMap<(String, String), HashSet<String>> = HashMap::new();
        let mut inbound_neighbors: HashMap<(String, String), HashSet<String>> = HashMap::new();

        for edge in edges {
            let rel = edge.edge_type.clone();
            *outbound
                .entry((edge.src_qn.clone(), rel.clone()))
                .or_default() += 1;
            *inbound
                .entry((edge.dst_qn.clone(), rel.clone()))
                .or_default() += 1;
            outbound_neighbors
                .entry((edge.src_qn.clone(), rel.clone()))
                .or_default()
                .insert(edge.dst_qn.clone());
            inbound_neighbors
                .entry((edge.dst_qn.clone(), rel))
                .or_default()
                .insert(edge.src_qn.clone());
        }

        Self {
            outbound,
            inbound,
            outbound_neighbors,
            inbound_neighbors,
        }
    }

    fn outbound_degree(&self, qn: &str, relationship: &str) -> usize {
        self.outbound
            .get(&(qn.to_string(), relationship.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn inbound_degree(&self, qn: &str, relationship: &str) -> usize {
        self.inbound
            .get(&(qn.to_string(), relationship.to_string()))
            .copied()
            .unwrap_or(0)
    }

    fn degree(&self, qn: &str, relationship: Option<&str>, direction: &str) -> usize {
        match relationship {
            Some(rel) => match direction {
                "outbound" | "out" => self.outbound_degree(qn, rel),
                "inbound" | "in" => self.inbound_degree(qn, rel),
                _ => self.outbound_degree(qn, rel) + self.inbound_degree(qn, rel),
            },
            None => match direction {
                "outbound" | "out" => self
                    .outbound
                    .iter()
                    .filter(|((q, _), _)| q == qn)
                    .map(|(_, c)| *c)
                    .sum(),
                "inbound" | "in" => self
                    .inbound
                    .iter()
                    .filter(|((q, _), _)| q == qn)
                    .map(|(_, c)| *c)
                    .sum(),
                _ => {
                    self.outbound
                        .iter()
                        .filter(|((q, _), _)| q == qn)
                        .map(|(_, c)| *c)
                        .sum::<usize>()
                        + self
                            .inbound
                            .iter()
                            .filter(|((q, _), _)| q == qn)
                            .map(|(_, c)| *c)
                            .sum::<usize>()
                }
            },
        }
    }

    fn participates(&self, qn: &str, relationship: &str, direction: &str) -> bool {
        self.degree(qn, Some(relationship), direction) > 0
    }

    fn neighbors(&self, qn: &str, relationship: &str) -> HashSet<String> {
        let key = (qn.to_string(), relationship.to_string());
        let mut out = HashSet::new();
        if let Some(dsts) = self.outbound_neighbors.get(&key) {
            out.extend(dsts.iter().cloned());
        }
        if let Some(srcs) = self.inbound_neighbors.get(&key) {
            out.extend(srcs.iter().cloned());
        }
        out
    }
}

fn needs_graph_filter(filter: &SearchFilter) -> bool {
    filter.relationship.is_some()
        || filter.min_degree.is_some()
        || filter.max_degree.is_some()
        || filter.include_connected
        || filter.exclude_entry_points
}

fn normalize_direction(direction: Option<&str>) -> &str {
    match direction.unwrap_or("any") {
        "outbound" | "out" => "outbound",
        "inbound" | "in" => "inbound",
        _ => "any",
    }
}

fn matches_filter(sym: &Symbol, filter: &SearchFilter) -> bool {
    if let Some(label) = &filter.label {
        if &sym.label != label {
            return false;
        }
    }
    if let Some(query) = &filter.query {
        let q = query.to_lowercase();
        let hay = format!(
            "{} {} {}",
            sym.name,
            sym.qualified_name,
            sym.signature.as_deref().unwrap_or("")
        )
        .to_lowercase();
        if !hay.contains(&q) {
            return false;
        }
    }
    if let Some(pattern) = &filter.name_pattern {
        if !regex_match(pattern, &sym.name) {
            return false;
        }
    }
    if let Some(pattern) = &filter.qn_pattern {
        if !regex_match(pattern, &sym.qualified_name) {
            return false;
        }
    }
    if let Some(pattern) = &filter.file_pattern {
        if !glob_match(pattern, &sym.file_path) {
            return false;
        }
    }
    true
}

fn regex_match(pattern: &str, value: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(value))
        .unwrap_or(false)
}

fn glob_match(pattern: &str, value: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|p| p.matches(value))
        .unwrap_or(false)
}

fn validate_readonly_select(sql: &str) -> Result<()> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.is_empty() {
        return Err(Error::QueryNotAllowed("empty query".into()));
    }
    if !trimmed.to_uppercase().starts_with("SELECT") {
        return Err(Error::QueryNotAllowed(
            "only SELECT queries are allowed".into(),
        ));
    }
    if trimmed.contains(';') {
        return Err(Error::QueryNotAllowed(
            "multiple statements are not allowed".into(),
        ));
    }
    let stripped = strip_sql_string_literals(trimmed);
    let upper = stripped.to_uppercase();
    for kw in [
        "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "ATTACH", "DETACH", "PRAGMA",
        "REPLACE", "TRUNCATE",
    ] {
        if contains_sql_keyword(&upper, kw) {
            return Err(Error::QueryNotAllowed(format!(
                "mutating or privileged keyword forbidden: {kw}"
            )));
        }
    }
    Ok(())
}

fn strip_sql_string_literals(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                out.push(' ');
                while let Some(ch) = chars.next() {
                    if ch == '\'' {
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
            }
            '"' => {
                out.push(' ');
                for ch in chars.by_ref() {
                    if ch == '"' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn contains_sql_keyword(haystack: &str, keyword: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| token == keyword)
}

fn symbol_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        qualified_name: row.get(0)?,
        name: row.get(1)?,
        label: row.get(2)?,
        file_path: row.get(3)?,
        line_start: row.get(4)?,
        line_end: row.get(5)?,
        signature: row.get(6)?,
        properties_json: row.get(7)?,
    })
}

fn edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Edge> {
    Ok(Edge {
        src_qn: row.get(0)?,
        dst_qn: row.get(1)?,
        edge_type: row.get(2)?,
        properties_json: row.get(3)?,
    })
}

fn value_to_json(val: rusqlite::types::Value) -> serde_json::Value {
    match val {
        rusqlite::types::Value::Null => serde_json::Value::Null,
        rusqlite::types::Value::Integer(i) => serde_json::json!(i),
        rusqlite::types::Value::Real(f) => serde_json::json!(f),
        rusqlite::types::Value::Text(s) => serde_json::Value::String(s),
        rusqlite::types::Value::Blob(b) => {
            serde_json::Value::String(format!("<blob {} bytes>", b.len()))
        }
    }
}

pub fn delete_project_db(project: &str) -> Result<()> {
    let path = project_db_path(project);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_returns_indexed_symbols() {
        let store = Store::open_memory().unwrap();
        store
            .upsert_symbol(&Symbol {
                qualified_name: "a.rs::main".into(),
                name: "main".into(),
                label: "Function".into(),
                file_path: "a.rs".into(),
                line_start: 1,
                line_end: 3,
                signature: None,
                properties_json: None,
            })
            .unwrap();

        assert_eq!(store.count_symbols().unwrap(), 1);
        assert!(store.find_symbol("a.rs::main").unwrap().is_some());

        let result = store.search(&SearchFilter::default()).unwrap();
        assert_eq!(result.total, 1, "search count mismatch");
        assert_eq!(result.symbols.len(), 1, "search rows mismatch");
    }

    #[test]
    fn roundtrips_compressed_file_content() {
        let store = Store::open_memory().unwrap();
        let content = "x = 1\n".repeat(400);
        store
            .upsert_file(&SourceFile {
                path: "big.py".into(),
                content: content.clone(),
                language: "python".into(),
                line_count: 400,
            })
            .unwrap();
        assert_eq!(store.list_files().unwrap()[0].content, content);
        assert!(!store.search_code("x = 1", 5).unwrap().is_empty());
    }

    #[test]
    fn search_filters_by_relationship_and_degree() {
        let store = Store::open_memory().unwrap();
        let mk = |qn: &str, name: &str| Symbol {
            qualified_name: qn.into(),
            name: name.into(),
            label: "Function".into(),
            file_path: "a.rs".into(),
            line_start: 1,
            line_end: 2,
            signature: None,
            properties_json: None,
        };
        store
            .upsert_symbol(&mk("a.rs::Function::main@L1", "main"))
            .unwrap();
        store
            .upsert_symbol(&mk("a.rs::Function::a@L3", "a"))
            .unwrap();
        store
            .upsert_symbol(&mk("a.rs::Function::helper@L5", "helper"))
            .unwrap();
        store
            .insert_edges_batch(&[
                Edge {
                    src_qn: "a.rs::Function::main@L1".into(),
                    dst_qn: "a.rs::Function::a@L3".into(),
                    edge_type: "CALLS".into(),
                    properties_json: None,
                },
                Edge {
                    src_qn: "a.rs::Function::a@L3".into(),
                    dst_qn: "a.rs::Function::helper@L5".into(),
                    edge_type: "CALLS".into(),
                    properties_json: None,
                },
            ])
            .unwrap();

        let calls = store
            .search(&SearchFilter {
                relationship: Some("CALLS".into()),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(calls.total, 3);

        let entry_points = store
            .search(&SearchFilter {
                relationship: Some("CALLS".into()),
                exclude_entry_points: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert!(entry_points.symbols.iter().any(|s| s.name == "helper"));
        assert!(!entry_points.symbols.iter().any(|s| s.name == "main"));

        let hub = store
            .search(&SearchFilter {
                relationship: Some("CALLS".into()),
                min_degree: Some(2),
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert!(hub.symbols.iter().any(|s| s.name == "a"));
    }
}
