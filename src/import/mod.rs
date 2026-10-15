// Memory import functionality
// Import from OpenClaw and other systems

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::db::memories::NewMemory;
use crate::db::Db;

/// Import source format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    /// OpenClaw daily memory files (memory/YYYY-MM-DD.md)
    OpenClawDaily,
    /// OpenClaw MEMORY.md file
    OpenClawMemory,
    /// OpenClaw USER.md file
    OpenClawUser,
    /// Generic markdown file
    Markdown,
    /// JSON export format
    Json,
    /// Plain text file
    Text,
}

/// A memory to import
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportMemory {
    pub content: String,
    pub memory_type: Option<String>,
    pub importance: Option<u8>,
    pub confidence: Option<f32>,
    pub source_agent: Option<String>,
    pub source_context: Option<String>,
    pub provenance: Option<String>,
}

/// Result of an import operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub source: ImportSource,
    pub total_found: usize,
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub errors: Vec<String>,
}

/// Import memories from a file or directory
pub fn import_from_file(
    db: &Db,
    bank_id: &str,
    path: &Path,
    source: ImportSource,
    default_agent: &str,
) -> Result<ImportResult> {
    let mut result = ImportResult {
        source,
        total_found: 0,
        imported: 0,
        skipped_duplicates: 0,
        errors: Vec::new(),
    };

    let memories = match source {
        ImportSource::OpenClawDaily => parse_openclaw_daily(path)?,
        ImportSource::OpenClawMemory => parse_openclaw_memory(path)?,
        ImportSource::OpenClawUser => parse_openclaw_user(path)?,
        ImportSource::Markdown => parse_markdown(path)?,
        ImportSource::Json => parse_json(path)?,
        ImportSource::Text => parse_text(path)?,
    };

    result.total_found = memories.len();

    for mem in memories {
        // Check for duplicate
        match db.find_duplicate(bank_id, &mem.content) {
            Ok(Some(_existing)) => {
                result.skipped_duplicates += 1;
                continue;
            }
            Ok(None) => {}
            Err(e) => {
                result.errors.push(format!("Duplicate check failed: {}", e));
                continue;
            }
        }

        // Store the memory
        let new_memory = NewMemory {
            content: mem.content,
            memory_type: mem.memory_type.unwrap_or_else(|| "world_fact".to_string()),
            importance: mem.importance.unwrap_or(3),
            confidence: mem.confidence.unwrap_or(1.0),
            source_agent: mem.source_agent.unwrap_or_else(|| default_agent.to_string()),
            source_context: mem.source_context,
            provenance: mem.provenance,
        };

        match db.store_memory(bank_id, new_memory) {
            Ok(_) => result.imported += 1,
            Err(e) => result.errors.push(format!("Store failed: {}", e)),
        }
    }

    Ok(result)
}

/// Parse OpenClaw daily memory files (memory/YYYY-MM-DD.md)
/// Extracts bullet points and sections as individual memories
fn parse_openclaw_daily(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut memories = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Track section headers
        if line.starts_with("## ") || line.starts_with("### ") {
            current_section = line.trim_start_matches('#').trim().to_string();
            continue;
        }

        // Extract bullet points as memories
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = line.trim_start_matches("- ").trim_start_matches("* ").trim();
            if !content.is_empty() && content.len() > 10 {
                memories.push(ImportMemory {
                    content: content.to_string(),
                    memory_type: Some("observation".to_string()),
                    importance: Some(3),
                    confidence: Some(0.8),
                    source_agent: Some("openclaw_import".to_string()),
                    source_context: Some(format!("daily:{}", current_section)),
                    provenance: Some(format!("file:{}", path.display())),
                });
            }
        }
    }

    Ok(memories)
}

/// Parse OpenClaw MEMORY.md file
/// Extracts sections and bullet points as memories
fn parse_openclaw_memory(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut memories = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        // Track section headers
        if line.starts_with("## ") || line.starts_with("### ") {
            current_section = line.trim_start_matches('#').trim().to_string();
            continue;
        }

        // Extract bullet points
        if line.starts_with("- ") || line.starts_with("* ") {
            let content = line.trim_start_matches("- ").trim_start_matches("* ").trim();
            if !content.is_empty() && content.len() > 10 {
                // Determine type from section
                let memory_type = if current_section.to_lowercase().contains("config") {
                    "world_fact"
                } else if current_section.to_lowercase().contains("decision") {
                    "experience"
                } else if current_section.to_lowercase().contains("lesson") {
                    "mental_model"
                } else {
                    "observation"
                };

                memories.push(ImportMemory {
                    content: content.to_string(),
                    memory_type: Some(memory_type.to_string()),
                    importance: Some(4), // Higher importance for curated memories
                    confidence: Some(0.9),
                    source_agent: Some("openclaw_import".to_string()),
                    source_context: Some(format!("memory:{}", current_section)),
                    provenance: Some(format!("file:{}", path.display())),
                });
            }
        }
    }

    Ok(memories)
}

/// Parse OpenClaw USER.md file
/// Extracts user preferences and profile facts
fn parse_openclaw_user(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut memories = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with("#") {
            continue;
        }

        // Extract directives (lines starting with -)
        if line.starts_with("- ") {
            let content = line.trim_start_matches("- ").trim();
            if !content.is_empty() && content.len() > 10 {
                memories.push(ImportMemory {
                    content: content.to_string(),
                    memory_type: Some("world_fact".to_string()),
                    importance: Some(4),
                    confidence: Some(0.95), // User directives are high confidence
                    source_agent: Some("openclaw_import".to_string()),
                    source_context: Some("user_profile".to_string()),
                    provenance: Some(format!("file:{}", path.display())),
                });
            }
        }
    }

    Ok(memories)
}

/// Parse generic markdown file
/// Extracts headers and bullet points
fn parse_markdown(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut memories = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ") {
            current_section = line.trim_start_matches('#').trim().to_string();
            continue;
        }

        if line.starts_with("- ") || line.starts_with("* ") {
            let content = line.trim_start_matches("- ").trim_start_matches("* ").trim();
            if !content.is_empty() && content.len() > 10 {
                memories.push(ImportMemory {
                    content: content.to_string(),
                    memory_type: Some("observation".to_string()),
                    importance: Some(3),
                    confidence: Some(0.7),
                    source_agent: Some("import".to_string()),
                    source_context: Some(current_section.clone()),
                    provenance: Some(format!("file:{}", path.display())),
                });
            }
        }
    }

    Ok(memories)
}

/// Parse JSON export format
/// Expects an array of ImportMemory objects
fn parse_json(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let memories: Vec<ImportMemory> = serde_json::from_str(&content)
        .with_context(|| "failed to parse JSON import file")?;

    Ok(memories)
}

/// Parse plain text file
/// Each non-empty line becomes a memory
fn parse_text(path: &Path) -> Result<Vec<ImportMemory>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let mut memories = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.len() < 10 {
            continue;
        }

        memories.push(ImportMemory {
            content: line.to_string(),
            memory_type: Some("observation".to_string()),
            importance: Some(2),
            confidence: Some(0.6),
            source_agent: Some("import".to_string()),
            source_context: None,
            provenance: Some(format!("file:{}", path.display())),
        });
    }

    Ok(memories)
}

/// Import from OpenClaw workspace directory
/// Scans memory/ directory and imports all daily files
pub fn import_openclaw_workspace(
    db: &Db,
    bank_id: &str,
    workspace_path: &Path,
    default_agent: &str,
) -> Result<Vec<ImportResult>> {
    let mut results = Vec::new();

    // Import MEMORY.md
    let memory_md = workspace_path.join("MEMORY.md");
    if memory_md.exists() {
        let result = import_from_file(
            db,
            bank_id,
            &memory_md,
            ImportSource::OpenClawMemory,
            default_agent,
        )?;
        results.push(result);
    }

    // Import USER.md
    let user_md = workspace_path.join("USER.md");
    if user_md.exists() {
        let result = import_from_file(
            db,
            bank_id,
            &user_md,
            ImportSource::OpenClawUser,
            default_agent,
        )?;
        results.push(result);
    }

    // Import daily memory files
    let memory_dir = workspace_path.join("memory");
    if memory_dir.exists() && memory_dir.is_dir() {
        let entries = std::fs::read_dir(&memory_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md") {
                let result = import_from_file(
                    db,
                    bank_id,
                    &path,
                    ImportSource::OpenClawDaily,
                    default_agent,
                )?;
                results.push(result);
            }
        }
    }

    Ok(results)
}

/// Export memories to JSON format
pub fn export_to_json(db: &Db, bank_id: &str, output_path: &Path) -> Result<usize> {
    let memories = db.all_memories(bank_id)?;

    let export: Vec<ImportMemory> = memories
        .iter()
        .map(|m| ImportMemory {
            content: m.content.clone(),
            memory_type: Some(m.memory_type.clone()),
            importance: Some(m.importance),
            confidence: Some(m.confidence),
            source_agent: Some(m.source_agent.clone()),
            source_context: m.source_context.clone(),
            provenance: m.provenance.clone(),
        })
        .collect();

    let json = serde_json::to_string_pretty(&export)?;
    std::fs::write(output_path, json)?;

    Ok(export.len())
}
