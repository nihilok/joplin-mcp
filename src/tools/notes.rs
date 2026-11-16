use std::fmt::Write as _;

use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};

use crate::error::{JoplinError, Result};
use crate::joplin::{JoplinClient, NOTE_FULL_FIELDS, NOTE_SUMMARY_FIELDS, Note, NoteQuery};
use crate::tools::{
    ToolDefinition, format_timestamp_ms, opt_bool, opt_nonempty_str, opt_str, opt_u32, opt_u64,
    require_str, resolve_notebook_by_name, resolve_tag_by_name,
};

const DEFAULT_LINE_WINDOW: usize = 200;

/// Return tool definitions for notes.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "get_note",
            description: "Retrieve a note by ID with optional section, line-range, TOC-only, or metadata-only display.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to retrieve" },
                    "section": { "type": "string", "description": "Heading number (e.g. '1', '2.1') or heading text to extract" },
                    "start_line": { "type": "integer", "minimum": 1, "description": "1-based start line for sequential reading" },
                    "line_count": { "type": "integer", "minimum": 1, "description": "Number of lines to return from start_line (default: 200)" },
                    "toc_only": { "type": "boolean", "description": "Return only metadata and Markdown table of contents" },
                    "force_full": { "type": "boolean", "description": "Force returning the entire note body" },
                    "metadata_only": { "type": "boolean", "description": "Return only note metadata without body" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "get_note_resources",
            description: "List resources (attachments) on a note, including OCR text for images and PDFs.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID whose resources to list" },
                    "ocr_only": { "type": "boolean", "description": "Only include resources that have OCR text" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "get_links",
            description: "Extract outgoing note links (:/<id>) from a note and find backlinks from other notes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to inspect links for" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "create_note",
            description: "Create a new note in a specified Joplin notebook.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Note title" },
                    "notebook_name": { "type": "string", "description": "Target notebook name or path (e.g. 'Projects/Work')" },
                    "body": { "type": "string", "description": "Markdown body content" },
                    "is_todo": { "type": "boolean", "description": "Create as a to-do item (default: false)" },
                    "todo_completed": { "type": "boolean", "description": "Mark to-do as completed" },
                    "todo_due": { "type": "integer", "description": "Due date as Unix timestamp in milliseconds" }
                },
                "required": ["title", "notebook_name"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "update_note",
            description: "Update note properties (title, body, notebook, todo status, due date). Replaces the entire body if provided.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to update" },
                    "title": { "type": "string", "description": "New note title" },
                    "body": { "type": "string", "description": "New Markdown body (replaces full body)" },
                    "notebook_name": { "type": "string", "description": "Move note to this notebook name or path" },
                    "is_todo": { "type": "boolean", "description": "Convert between note and to-do" },
                    "todo_completed": { "type": "boolean", "description": "Mark to-do completed or uncompleted" },
                    "todo_due": { "type": "integer", "description": "Due date as Unix timestamp in milliseconds (0 clears)" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "edit_note",
            description: "Precision-edit a note's body by replacing an exact string, prepending, or appending.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to edit" },
                    "new_string": { "type": "string", "description": "Replacement text, or text to prepend/append" },
                    "old_string": { "type": "string", "description": "Exact text to replace (omit when using position)" },
                    "replace_all": { "type": "boolean", "description": "Replace all occurrences of old_string (default: false)" },
                    "position": {
                        "type": "string",
                        "enum": ["beginning", "end"],
                        "description": "Insert new_string at 'beginning' or 'end' of note when old_string is omitted"
                    }
                },
                "required": ["note_id", "new_string"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "delete_note",
            description: "Delete a note from Joplin (moves to trash).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to delete" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "find_notes",
            description: "Search notes by query (use '*' for all notes) with optional task, completion, trash, and pagination filters.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search text or '*' for all notes" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results (default: 20)" },
                    "offset": { "type": "integer", "minimum": 0, "description": "Result offset for pagination (default: 0)" },
                    "task": { "type": "boolean", "description": "Filter by to-do items (true) or regular notes (false)" },
                    "completed": { "type": "boolean", "description": "Filter by completion status (when task is true/omitted)" },
                    "trash": { "type": "boolean", "description": "Include/filter trashed notes (default: false)" },
                    "order_by": { "type": "string", "enum": ["updated_time", "created_time", "title"], "description": "Sort field" },
                    "order_dir": { "type": "string", "enum": ["ASC", "DESC"], "description": "Sort direction" }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "find_in_note",
            description: "Search for a regex pattern inside a specific note and return paginated matching lines.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to search within" },
                    "pattern": { "type": "string", "description": "Regular expression pattern" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max matches (default: 20)" },
                    "offset": { "type": "integer", "minimum": 0, "description": "Match offset (default: 0)" },
                    "case_sensitive": { "type": "boolean", "description": "Case-sensitive regex matching (default: true)" },
                    "multiline": { "type": "boolean", "description": "Enable multi-line ^/$ mode (default: false)" },
                    "dotall": { "type": "boolean", "description": "Allow '.' to match newlines (default: false)" }
                },
                "required": ["note_id", "pattern"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "find_notes_with_tag",
            description: "Find all notes with a specific tag, with pagination and task filters.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tag_name": { "type": "string", "description": "Tag name to search for" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results (default: 20)" },
                    "offset": { "type": "integer", "minimum": 0, "description": "Result offset (default: 0)" },
                    "task": { "type": "boolean", "description": "Filter by to-do items" },
                    "completed": { "type": "boolean", "description": "Filter by completion status" },
                    "order_by": { "type": "string", "enum": ["updated_time", "created_time", "title"] },
                    "order_dir": { "type": "string", "enum": ["ASC", "DESC"] }
                },
                "required": ["tag_name"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "find_notes_in_notebook",
            description: "Find all notes in a specific notebook, with pagination and task filters.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "notebook_name": { "type": "string", "description": "Notebook name or path (e.g. 'Projects/Work')" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results (default: 20)" },
                    "offset": { "type": "integer", "minimum": 0, "description": "Result offset (default: 0)" },
                    "task": { "type": "boolean", "description": "Filter by to-do items" },
                    "completed": { "type": "boolean", "description": "Filter by completion status" },
                    "order_by": { "type": "string", "enum": ["updated_time", "created_time", "title"] },
                    "order_dir": { "type": "string", "enum": ["ASC", "DESC"] }
                },
                "required": ["notebook_name"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "get_all_notes",
            description: "Get all notes in Joplin up to a limit.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Max results (default: 20)" },
                    "order_by": { "type": "string", "enum": ["updated_time", "created_time", "title"] },
                    "order_dir": { "type": "string", "enum": ["ASC", "DESC"] }
                },
                "additionalProperties": false
            }),
        },
    ]
}

/// Return `true` if `name` is handled by this module.
#[must_use]
pub fn handles(name: &str) -> bool {
    matches!(
        name,
        "get_note"
            | "get_note_resources"
            | "get_links"
            | "create_note"
            | "update_note"
            | "edit_note"
            | "delete_note"
            | "find_notes"
            | "find_in_note"
            | "find_notes_with_tag"
            | "find_notes_in_notebook"
            | "get_all_notes"
    )
}

/// Dispatch a note tool call.
///
/// # Errors
///
/// Returns [`JoplinError`] if arguments are invalid or the API call fails.
pub fn call(client: &JoplinClient, name: &str, args: &Value) -> Result<String> {
    match name {
        "get_note" => get_note(client, args),
        "get_note_resources" => get_note_resources(client, args),
        "get_links" => get_links(client, args),
        "create_note" => create_note(client, args),
        "update_note" => update_note(client, args),
        "edit_note" => edit_note(client, args),
        "delete_note" => delete_note(client, args),
        "find_notes" => find_notes(client, args),
        "find_in_note" => find_in_note(client, args),
        "find_notes_with_tag" => find_notes_with_tag(client, args),
        "find_notes_in_notebook" => find_notes_in_notebook(client, args),
        "get_all_notes" => get_all_notes(client, args),
        _ => Err(JoplinError::InvalidInput(format!("Unknown tool: {name}"))),
    }
}

fn format_note_metadata(note: &Note) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Title: {}", note.title);
    let _ = writeln!(out, "ID: {}", note.id);
    let _ = writeln!(out, "Notebook ID: {}", note.parent_id);
    if note.is_todo == 1 {
        let status = if note.todo_completed > 0 {
            "completed"
        } else {
            "open"
        };
        let _ = writeln!(out, "Type: to-do ({status})");
        if note.todo_due > 0 {
            let _ = writeln!(out, "Due: {}", format_timestamp_ms(note.todo_due));
        }
    }
    let _ = writeln!(
        out,
        "Created: {} | Updated: {}",
        format_timestamp_ms(note.created_time),
        format_timestamp_ms(note.updated_time)
    );
    if !note.source_url.is_empty() {
        let _ = writeln!(out, "Source URL: {}", note.source_url);
    }
    if note.deleted_time > 0 {
        let _ = writeln!(out, "Deleted: {}", format_timestamp_ms(note.deleted_time));
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Heading {
    level: usize,
    number: String,
    title: String,
    line_idx: usize,
}

fn extract_headings(lines: &[&str]) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut counts = [0_usize; 6];
    let mut in_code_block = false;

    for (idx, raw) in lines.iter().enumerate() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block || !trimmed.starts_with('#') {
            continue;
        }
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if hashes == 0 || hashes > 6 {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            continue;
        }
        let title = rest.trim().trim_end_matches('#').trim().to_string();
        if title.is_empty() {
            continue;
        }

        let level = hashes;
        counts[level - 1] += 1;
        for c in &mut counts[level..] {
            *c = 0;
        }
        let number = counts[..level]
            .iter()
            .filter(|&&n| n > 0)
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(".");

        headings.push(Heading {
            level,
            number,
            title,
            line_idx: idx,
        });
    }
    headings
}

fn extract_section<'a>(lines: &[&'a str], query: &str) -> Option<(Heading, Vec<&'a str>)> {
    let headings = extract_headings(lines);
    let q = query.trim();
    let pos = headings
        .iter()
        .position(|h| h.number == q || h.title.eq_ignore_ascii_case(q))
        .or_else(|| {
            let q_lower = q.to_lowercase();
            headings
                .iter()
                .position(|h| h.title.to_lowercase().contains(&q_lower))
        })?;

    let target = &headings[pos];
    let start = target.line_idx;
    let end = headings[pos + 1..]
        .iter()
        .find(|h| h.level <= target.level)
        .map_or(lines.len(), |h| h.line_idx);

    Some((target.clone(), lines[start..end].to_vec()))
}

fn get_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let note = client.get_note(note_id, NOTE_FULL_FIELDS)?;
    let mut out = format_note_metadata(&note);

    let metadata_only = opt_bool(args, "metadata_only").unwrap_or(false);
    if metadata_only {
        return Ok(out);
    }

    let lines: Vec<&str> = note.body.lines().collect();
    let _ = writeln!(out, "Lines: {}", lines.len());

    let toc_only = opt_bool(args, "toc_only").unwrap_or(false);
    if toc_only {
        let headings = extract_headings(&lines);
        out.push_str("\nTable of Contents:\n");
        if headings.is_empty() {
            out.push_str("(no headings found)\n");
        } else {
            for h in headings {
                let indent = "  ".repeat(h.level.saturating_sub(1));
                let _ = writeln!(
                    out,
                    "{indent}{} {} (line {})",
                    h.number,
                    h.title,
                    h.line_idx + 1
                );
            }
        }
        return Ok(out);
    }

    if let Some(section_query) = opt_nonempty_str(args, "section") {
        let Some((heading, section_lines)) = extract_section(&lines, section_query) else {
            return Err(JoplinError::NotFound(format!(
                "Section '{section_query}' not found in note '{note_id}'"
            )));
        };
        let _ = writeln!(
            out,
            "\n--- Section {} {} (line {}) ---\n{}",
            heading.number,
            heading.title,
            heading.line_idx + 1,
            section_lines.join("\n")
        );
        return Ok(out);
    }

    let force_full = opt_bool(args, "force_full").unwrap_or(false);
    let start_line = opt_u32(args, "start_line");
    let line_count = opt_u32(args, "line_count");

    if !force_full && (start_line.is_some() || line_count.is_some()) {
        let start = start_line.unwrap_or(1).max(1) as usize - 1;
        let count = line_count.map_or(DEFAULT_LINE_WINDOW, |c| c.max(1) as usize);
        if start >= lines.len() {
            let _ = writeln!(out, "\n(start_line {} is past end of note)", start + 1);
            return Ok(out);
        }
        let end = (start + count).min(lines.len());
        let _ = writeln!(
            out,
            "\n--- Lines {}-{} of {} ---\n{}",
            start + 1,
            end,
            lines.len(),
            lines[start..end].join("\n")
        );
        return Ok(out);
    }

    let _ = writeln!(out, "\n---\n{}", note.body);
    Ok(out)
}

fn get_note_resources(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let ocr_only = opt_bool(args, "ocr_only").unwrap_or(false);
    let resources = client.get_note_resources(note_id)?;

    let filtered: Vec<_> = resources
        .into_iter()
        .filter(|r| !ocr_only || !r.ocr_text.trim().is_empty())
        .collect();

    if filtered.is_empty() {
        return Ok(format!("No matching resources on note '{note_id}'."));
    }

    let mut out = format!("Resources on note '{note_id}' ({}):\n", filtered.len());
    for res in filtered {
        let name = if res.filename.is_empty() {
            &res.title
        } else {
            &res.filename
        };
        let _ = writeln!(
            out,
            "- {} (id: {}, mime: {}, size: {} bytes)",
            name, res.id, res.mime, res.size
        );
        if !res.ocr_text.trim().is_empty() {
            let _ = writeln!(out, "  OCR:\n  {}", res.ocr_text.trim());
        }
    }
    Ok(out)
}

fn extract_note_link_ids(body: &str) -> Vec<String> {
    let Ok(re) = Regex::new(r":/([0-9a-fA-F]{32})") else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for cap in re.captures_iter(body) {
        if let Some(m) = cap.get(1) {
            let id = m.as_str().to_lowercase();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

fn get_links(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let note = client.get_note(note_id, NOTE_FULL_FIELDS)?;
    let outgoing_ids = extract_note_link_ids(&note.body);

    let all_notes = client.list_all_notes_pages("id,title,body,deleted_time")?;
    let mut out = format!(
        "Links for '{}' (id: {}):\n\nOutgoing links:\n",
        note.title, note.id
    );

    if outgoing_ids.is_empty() {
        out.push_str("- (none)\n");
    } else {
        for target_id in &outgoing_ids {
            if let Some(target) = all_notes
                .iter()
                .find(|n| n.id.eq_ignore_ascii_case(target_id))
            {
                let _ = writeln!(out, "- {} (id: {})", target.title, target.id);
            } else {
                let _ = writeln!(out, "- [unknown or resource] (id: {target_id})");
            }
        }
    }

    out.push_str("\nBacklinks:\n");
    let needle = format!(":/{}", note.id.to_lowercase());
    let mut backlink_count = 0_usize;
    for candidate in &all_notes {
        if candidate.deleted_time > 0 || candidate.id == note.id {
            continue;
        }
        if candidate.body.to_lowercase().contains(&needle) {
            let _ = writeln!(out, "- {} (id: {})", candidate.title, candidate.id);
            backlink_count += 1;
        }
    }
    if backlink_count == 0 {
        out.push_str("- (none)\n");
    }

    Ok(out)
}

fn create_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let title = require_str(args, "title")?;
    let notebook_name = require_str(args, "notebook_name")?;
    let folder = resolve_notebook_by_name(client, notebook_name)?;

    let mut payload = Map::new();
    payload.insert("title".to_string(), Value::String(title.to_string()));
    payload.insert("parent_id".to_string(), Value::String(folder.id));

    if let Some(body) = opt_str(args, "body") {
        payload.insert("body".to_string(), Value::String(body.to_string()));
    }
    if let Some(is_todo) = opt_bool(args, "is_todo") {
        payload.insert(
            "is_todo".to_string(),
            Value::Number(u8::from(is_todo).into()),
        );
    }
    if let Some(completed) = opt_bool(args, "todo_completed") {
        let ts = if completed { current_unix_ms() } else { 0 };
        payload.insert("todo_completed".to_string(), Value::Number(ts.into()));
    }
    if let Some(due) = opt_u64(args, "todo_due") {
        payload.insert("todo_due".to_string(), Value::Number(due.into()));
    }

    let created = client.create_note(&Value::Object(payload))?;
    Ok(format!(
        "Created note '{}' (id: {}) in notebook '{}'.",
        created.title, created.id, folder.title
    ))
}

fn update_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let mut payload = Map::new();

    if let Some(title) = opt_nonempty_str(args, "title") {
        payload.insert("title".to_string(), Value::String(title.to_string()));
    }
    if let Some(body) = opt_str(args, "body") {
        payload.insert("body".to_string(), Value::String(body.to_string()));
    }
    if let Some(notebook_name) = opt_nonempty_str(args, "notebook_name") {
        let folder = resolve_notebook_by_name(client, notebook_name)?;
        payload.insert("parent_id".to_string(), Value::String(folder.id));
    }
    if let Some(is_todo) = opt_bool(args, "is_todo") {
        payload.insert(
            "is_todo".to_string(),
            Value::Number(u8::from(is_todo).into()),
        );
    }
    if let Some(completed) = opt_bool(args, "todo_completed") {
        let ts = if completed { current_unix_ms() } else { 0 };
        payload.insert("todo_completed".to_string(), Value::Number(ts.into()));
    }
    if let Some(due) = opt_u64(args, "todo_due") {
        payload.insert("todo_due".to_string(), Value::Number(due.into()));
    }

    if payload.is_empty() {
        return Err(JoplinError::InvalidInput(
            "Provide at least one field to update".to_string(),
        ));
    }

    let updated = client.update_note(note_id, &Value::Object(payload))?;
    Ok(format!(
        "Updated note '{}' (id: {}).",
        updated.title, updated.id
    ))
}

fn apply_body_edit(
    body: &str,
    new_string: &str,
    old_string: Option<&str>,
    replace_all: bool,
    position: Option<&str>,
) -> Result<(String, String)> {
    match (old_string, position) {
        (Some(old), None) => {
            if old.is_empty() {
                return Err(JoplinError::InvalidInput(
                    "old_string cannot be empty".to_string(),
                ));
            }
            let count = body.matches(old).count();
            if count == 0 {
                return Err(JoplinError::NotFound(
                    "old_string not found in note body".to_string(),
                ));
            }
            if !replace_all && count > 1 {
                return Err(JoplinError::InvalidInput(format!(
                    "old_string matched {count} times; pass replace_all=true or provide more context"
                )));
            }
            let updated = if replace_all {
                body.replace(old, new_string)
            } else {
                body.replacen(old, new_string, 1)
            };
            let replaced = if replace_all { count } else { 1 };
            Ok((updated, format!("Replaced {replaced} occurrence(s).")))
        }
        (None, Some("beginning")) => {
            let updated = if body.is_empty() {
                new_string.to_string()
            } else {
                format!("{new_string}\n{body}")
            };
            Ok((updated, "Prepended to beginning of note.".to_string()))
        }
        (None, Some("end")) => {
            let updated = if body.is_empty() {
                new_string.to_string()
            } else if body.ends_with('\n') {
                format!("{body}{new_string}")
            } else {
                format!("{body}\n{new_string}")
            };
            Ok((updated, "Appended to end of note.".to_string()))
        }
        (None, Some(other)) => Err(JoplinError::InvalidInput(format!(
            "Invalid position '{other}': expected 'beginning' or 'end'"
        ))),
        (Some(_), Some(_)) => Err(JoplinError::InvalidInput(
            "Provide either old_string or position, not both".to_string(),
        )),
        (None, None) => Err(JoplinError::InvalidInput(
            "Provide either old_string or position ('beginning' | 'end')".to_string(),
        )),
    }
}

fn edit_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let new_string = opt_str(args, "new_string").ok_or_else(|| {
        JoplinError::InvalidInput("Missing required parameter: new_string".to_string())
    })?;
    let old_string = opt_str(args, "old_string");
    let replace_all = opt_bool(args, "replace_all").unwrap_or(false);
    let position = opt_nonempty_str(args, "position");

    let note = client.get_note(note_id, "id,title,body")?;
    let (new_body, summary) =
        apply_body_edit(&note.body, new_string, old_string, replace_all, position)?;

    let mut payload = Map::new();
    payload.insert("body".to_string(), Value::String(new_body));
    let updated = client.update_note(note_id, &Value::Object(payload))?;

    Ok(format!(
        "Edited note '{}' (id: {}). {summary}",
        updated.title, updated.id
    ))
}

fn delete_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    client.delete_note(note_id)?;
    Ok(format!("Deleted note '{note_id}' (moved to trash)."))
}

fn filter_notes(
    notes: Vec<Note>,
    task: Option<bool>,
    completed: Option<bool>,
    trash: Option<bool>,
) -> Vec<Note> {
    notes
        .into_iter()
        .filter(|n| {
            if let Some(want_trash) = trash
                && (n.deleted_time > 0) != want_trash
            {
                return false;
            }
            if let Some(want_task) = task
                && (n.is_todo == 1) != want_task
            {
                return false;
            }
            if let Some(want_done) = completed
                && (n.is_todo != 1 || (n.todo_completed > 0) != want_done)
            {
                return false;
            }
            true
        })
        .collect()
}

fn format_note_list(label: &str, notes: &[Note], offset: u32, has_more: bool) -> String {
    if notes.is_empty() {
        return format!("No {label} found (offset: {offset}).");
    }
    let mut out = format!("{label} (showing {}, offset {offset}):\n", notes.len());
    for note in notes {
        let todo_tag = if note.is_todo == 1 {
            if note.todo_completed > 0 {
                " [x]"
            } else {
                " [ ]"
            }
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "-{} {} (id: {}, notebook: {}, updated: {})",
            todo_tag,
            note.title,
            note.id,
            note.parent_id,
            format_timestamp_ms(note.updated_time)
        );
    }
    if has_more {
        let next_offset = offset + u32::try_from(notes.len()).unwrap_or(0);
        let _ = writeln!(
            out,
            "\nMore results available (next offset: {next_offset})."
        );
    }
    out
}

fn find_notes(client: &JoplinClient, args: &Value) -> Result<String> {
    let query = require_str(args, "query")?;
    let limit = opt_u32(args, "limit").unwrap_or(20).clamp(1, 100);
    let offset = opt_u32(args, "offset").unwrap_or(0);
    let task = opt_bool(args, "task");
    let completed = opt_bool(args, "completed");
    let trash = opt_bool(args, "trash").unwrap_or(false);
    let order_by = opt_nonempty_str(args, "order_by");
    let order_dir = opt_nonempty_str(args, "order_dir");

    let page = (offset / limit) + 1;
    let note_query = NoteQuery {
        fields: NOTE_SUMMARY_FIELDS,
        page,
        limit,
        order_by,
        order_dir,
        include_deleted: trash,
    };
    let res = if query == "*" {
        client.list_notes(note_query)?
    } else {
        client.search_notes(query, note_query)?
    };

    let filtered = filter_notes(res.items, task, completed, Some(trash));
    Ok(format_note_list(
        &format!("Search results for '{query}'"),
        &filtered,
        offset,
        res.has_more,
    ))
}

fn find_in_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let pattern = require_str(args, "pattern")?;
    let limit = opt_u32(args, "limit").unwrap_or(20).clamp(1, 100) as usize;
    let offset = opt_u32(args, "offset").unwrap_or(0) as usize;
    let case_sensitive = opt_bool(args, "case_sensitive").unwrap_or(true);
    let multiline = opt_bool(args, "multiline").unwrap_or(false);
    let dotall = opt_bool(args, "dotall").unwrap_or(false);

    let re = RegexBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .multi_line(multiline)
        .dot_matches_new_line(dotall)
        .build()
        .map_err(|e| JoplinError::InvalidInput(format!("Invalid regex pattern: {e}")))?;

    let note = client.get_note(note_id, "id,title,body")?;
    let mut matches = Vec::new();
    for (idx, line) in note.body.lines().enumerate() {
        if re.is_match(line) {
            matches.push((idx + 1, line));
        }
    }

    if matches.is_empty() {
        return Ok(format!(
            "No matches for /{pattern}/ in '{}' (id: {note_id}).",
            note.title
        ));
    }

    let total = matches.len();
    let page_matches: Vec<_> = matches.into_iter().skip(offset).take(limit).collect();
    let mut out = format!(
        "Matches in '{}' ({} total, showing {} from offset {offset}):\n",
        note.title,
        total,
        page_matches.len()
    );
    for (line_no, text) in page_matches {
        let _ = writeln!(out, "L{line_no}: {text}");
    }
    Ok(out)
}

fn find_notes_with_tag(client: &JoplinClient, args: &Value) -> Result<String> {
    let tag_name = require_str(args, "tag_name")?;
    let tag = resolve_tag_by_name(client, tag_name)?;
    let limit = opt_u32(args, "limit").unwrap_or(20).clamp(1, 100);
    let offset = opt_u32(args, "offset").unwrap_or(0);
    let task = opt_bool(args, "task");
    let completed = opt_bool(args, "completed");
    let order_by = opt_nonempty_str(args, "order_by");
    let order_dir = opt_nonempty_str(args, "order_dir");

    let page = (offset / limit) + 1;
    let res = client.find_notes_with_tag(
        &tag.id,
        NoteQuery {
            fields: NOTE_SUMMARY_FIELDS,
            page,
            limit,
            order_by,
            order_dir,
            include_deleted: false,
        },
    )?;
    let filtered = filter_notes(res.items, task, completed, Some(false));
    Ok(format_note_list(
        &format!("Notes tagged '{}'", tag.title),
        &filtered,
        offset,
        res.has_more,
    ))
}

fn find_notes_in_notebook(client: &JoplinClient, args: &Value) -> Result<String> {
    let notebook_name = require_str(args, "notebook_name")?;
    let folder = resolve_notebook_by_name(client, notebook_name)?;
    let limit = opt_u32(args, "limit").unwrap_or(20).clamp(1, 100);
    let offset = opt_u32(args, "offset").unwrap_or(0);
    let task = opt_bool(args, "task");
    let completed = opt_bool(args, "completed");
    let order_by = opt_nonempty_str(args, "order_by");
    let order_dir = opt_nonempty_str(args, "order_dir");

    let page = (offset / limit) + 1;
    let res = client.list_notebook_notes(
        &folder.id,
        NoteQuery {
            fields: NOTE_SUMMARY_FIELDS,
            page,
            limit,
            order_by,
            order_dir,
            include_deleted: false,
        },
    )?;
    let filtered = filter_notes(res.items, task, completed, Some(false));
    Ok(format_note_list(
        &format!("Notes in notebook '{}'", folder.title),
        &filtered,
        offset,
        res.has_more,
    ))
}

fn get_all_notes(client: &JoplinClient, args: &Value) -> Result<String> {
    let limit = opt_u32(args, "limit").unwrap_or(20).clamp(1, 100);
    let order_by = opt_nonempty_str(args, "order_by");
    let order_dir = opt_nonempty_str(args, "order_dir");

    let res = client.list_notes(NoteQuery {
        fields: NOTE_SUMMARY_FIELDS,
        page: 1,
        limit,
        order_by,
        order_dir,
        include_deleted: false,
    })?;
    let filtered = filter_notes(res.items, None, None, Some(false));
    Ok(format_note_list("All notes", &filtered, 0, res.has_more))
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_toc_and_sections() {
        let md = "# Intro\nHello\n## Sub\nWorld\n# Next\nBye";
        let lines: Vec<&str> = md.lines().collect();
        let headings = extract_headings(&lines);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].number, "1");
        assert_eq!(headings[1].number, "1.1");
        assert_eq!(headings[2].number, "2");

        let sec = extract_section(&lines, "1");
        assert!(sec.is_some());
        if let Some((_, sec_lines)) = sec {
            assert_eq!(sec_lines, vec!["# Intro", "Hello", "## Sub", "World"]);
        }
    }

    #[test]
    fn extracts_joplin_note_links() {
        let body =
            "See [one](:/0123456789abcdef0123456789abcdef) and :/0123456789ABCDEF0123456789ABCDEF";
        let ids = extract_note_link_ids(body);
        assert_eq!(ids, vec!["0123456789abcdef0123456789abcdef"]);
    }

    #[test]
    fn applies_body_edits_accurately() {
        let res = apply_body_edit("foo bar baz", "qux", Some("bar"), false, None);
        assert!(res.is_ok());
        if let Ok((body, _)) = res {
            assert_eq!(body, "foo qux baz");
        }

        let dup = apply_body_edit("a a", "b", Some("a"), false, None);
        assert!(dup.is_err());

        let append = apply_body_edit("hello", "world", None, false, Some("end"));
        assert!(append.is_ok());
        if let Ok((body, _)) = append {
            assert_eq!(body, "hello\nworld");
        }
    }
}
