pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    name TEXT PRIMARY KEY,
    repo_path TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS symbols (
    qualified_name TEXT NOT NULL,
    project TEXT NOT NULL,
    name TEXT NOT NULL,
    label TEXT NOT NULL,
    file_path TEXT NOT NULL,
    line_start INTEGER NOT NULL DEFAULT 1,
    line_end INTEGER NOT NULL DEFAULT 1,
    signature TEXT,
    properties_json TEXT,
    PRIMARY KEY (qualified_name, project)
);

CREATE TABLE IF NOT EXISTS edges (
    src_qn TEXT NOT NULL,
    dst_qn TEXT NOT NULL,
    edge_type TEXT NOT NULL,
    project TEXT NOT NULL,
    properties_json TEXT,
    UNIQUE(src_qn, dst_qn, edge_type, project)
);

CREATE TABLE IF NOT EXISTS files (
    path TEXT NOT NULL,
    project TEXT NOT NULL,
    content TEXT,
    language TEXT,
    line_count INTEGER,
    PRIMARY KEY (path, project)
);

CREATE TABLE IF NOT EXISTS meta (
    key TEXT NOT NULL,
    project TEXT NOT NULL,
    value TEXT,
    PRIMARY KEY (key, project)
);

CREATE TABLE IF NOT EXISTS vectors (
    qualified_name TEXT NOT NULL,
    project TEXT NOT NULL,
    dim INTEGER NOT NULL,
    data BLOB NOT NULL,
    PRIMARY KEY (qualified_name, project)
);

CREATE INDEX IF NOT EXISTS idx_symbols_project ON symbols(project);
CREATE INDEX IF NOT EXISTS idx_symbols_label ON symbols(project, label);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(project, name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(project, file_path);
CREATE INDEX IF NOT EXISTS idx_edges_src ON edges(project, src_qn);
CREATE INDEX IF NOT EXISTS idx_edges_dst ON edges(project, dst_qn);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(project, edge_type);
CREATE INDEX IF NOT EXISTS idx_files_project ON files(project);
CREATE INDEX IF NOT EXISTS idx_vectors_project ON vectors(project);
"#;
