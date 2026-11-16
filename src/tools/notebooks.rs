use std::collections::HashMap;
use std::fmt::Write as _;

use serde_json::{Map, Value};

use crate::error::{JoplinError, Result};
use crate::joplin::{Folder, JoplinClient};
use crate::tools::{
    ToolDefinition, format_timestamp_ms, opt_bool, opt_nonempty_str, require_str,
    resolve_notebook_by_name,
};

/// Return tool definitions for notebooks.
#[must_use]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_notebooks",
            description: "List all notebooks/folders in Joplin as a hierarchy.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "verbose": {
                        "type": "boolean",
                        "description": "Also return icon, parent_id, and timestamps (default: false)"
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "create_notebook",
            description: "Create a new notebook (folder) in Joplin.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Notebook title" },
                    "parent_name": { "type": "string", "description": "Optional parent notebook name or path" },
                    "emoji": { "type": "string", "description": "Optional emoji icon for the notebook" }
                },
                "required": ["title"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "update_notebook",
            description: "Update an existing notebook's title, emoji icon, or parent.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "notebook_id": { "type": "string", "description": "Notebook ID to update" },
                    "title": { "type": "string", "description": "New notebook title" },
                    "emoji": { "type": "string", "description": "New emoji icon (empty string clears it)" },
                    "parent_name": { "type": "string", "description": "New parent notebook name (empty string moves to top-level)" }
                },
                "required": ["notebook_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "delete_notebook",
            description: "Delete a notebook from Joplin (moves to trash).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "notebook_id": { "type": "string", "description": "Notebook ID to delete" }
                },
                "required": ["notebook_id"],
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
        "list_notebooks" | "create_notebook" | "update_notebook" | "delete_notebook"
    )
}

/// Dispatch a notebook tool call.
///
/// # Errors
///
/// Returns [`JoplinError`] if arguments are invalid or the API call fails.
pub fn call(client: &JoplinClient, name: &str, args: &Value) -> Result<String> {
    match name {
        "list_notebooks" => list_notebooks(client, args),
        "create_notebook" => create_notebook(client, args),
        "update_notebook" => update_notebook(client, args),
        "delete_notebook" => delete_notebook(client, args),
        _ => Err(JoplinError::InvalidInput(format!("Unknown tool: {name}"))),
    }
}

fn list_notebooks(client: &JoplinClient, args: &Value) -> Result<String> {
    let verbose = opt_bool(args, "verbose").unwrap_or(false);
    let folders: Vec<Folder> = client
        .list_notebooks()?
        .into_iter()
        .filter(|f| f.deleted_time == 0)
        .collect();

    if folders.is_empty() {
        return Ok("No notebooks found.".to_string());
    }

    let mut children: HashMap<&str, Vec<&Folder>> = HashMap::new();
    for folder in &folders {
        children
            .entry(folder.parent_id.as_str())
            .or_default()
            .push(folder);
    }
    for list in children.values_mut() {
        list.sort_by_key(|a| a.title.to_lowercase());
    }

    let mut out = format!("Notebooks ({}):\n", folders.len());
    format_tree("", 0, &children, verbose, &mut out);
    Ok(out)
}

fn format_tree(
    parent_id: &str,
    depth: usize,
    children: &HashMap<&str, Vec<&Folder>>,
    verbose: bool,
    out: &mut String,
) {
    let Some(list) = children.get(parent_id) else {
        return;
    };
    let indent = "  ".repeat(depth);
    for folder in list {
        let _ = writeln!(out, "{indent}- {} (id: {})", folder.title, folder.id);
        if verbose {
            if !folder.icon.is_empty() {
                let _ = writeln!(out, "{indent}  icon: {}", folder.icon);
            }
            if !folder.parent_id.is_empty() {
                let _ = writeln!(out, "{indent}  parent_id: {}", folder.parent_id);
            }
            let _ = writeln!(
                out,
                "{indent}  created: {} | updated: {}",
                format_timestamp_ms(folder.created_time),
                format_timestamp_ms(folder.updated_time)
            );
        }
        format_tree(&folder.id, depth + 1, children, verbose, out);
    }
}

fn create_notebook(client: &JoplinClient, args: &Value) -> Result<String> {
    let title = require_str(args, "title")?;
    let mut payload = Map::new();
    payload.insert("title".to_string(), Value::String(title.to_string()));

    if let Some(parent_name) = opt_nonempty_str(args, "parent_name") {
        let parent = resolve_notebook_by_name(client, parent_name)?;
        payload.insert("parent_id".to_string(), Value::String(parent.id));
    }

    if let Some(emoji) = opt_nonempty_str(args, "emoji") {
        payload.insert(
            "icon".to_string(),
            serde_json::json!({ "type": 1, "emoji": emoji })
                .to_string()
                .into(),
        );
    }

    let created = client.create_notebook(&Value::Object(payload))?;
    Ok(format!(
        "Created notebook '{}' (id: {}).",
        created.title, created.id
    ))
}

fn update_notebook(client: &JoplinClient, args: &Value) -> Result<String> {
    let notebook_id = require_str(args, "notebook_id")?;
    let mut payload = Map::new();

    if let Some(title) = opt_nonempty_str(args, "title") {
        payload.insert("title".to_string(), Value::String(title.to_string()));
    }

    if let Some(emoji_val) = args.get("emoji").and_then(Value::as_str) {
        if emoji_val.trim().is_empty() {
            payload.insert("icon".to_string(), Value::String(String::new()));
        } else {
            payload.insert(
                "icon".to_string(),
                Value::String(
                    serde_json::json!({ "type": 1, "emoji": emoji_val.trim() }).to_string(),
                ),
            );
        }
    }

    if let Some(parent_val) = args.get("parent_name").and_then(Value::as_str) {
        if parent_val.trim().is_empty() {
            payload.insert("parent_id".to_string(), Value::String(String::new()));
        } else {
            let parent = resolve_notebook_by_name(client, parent_val)?;
            payload.insert("parent_id".to_string(), Value::String(parent.id));
        }
    }

    if payload.is_empty() {
        return Err(JoplinError::InvalidInput(
            "Provide at least one field to update: title, emoji, or parent_name".to_string(),
        ));
    }

    let updated = client.update_notebook(notebook_id, &Value::Object(payload))?;
    Ok(format!(
        "Updated notebook '{}' (id: {}).",
        updated.title, updated.id
    ))
}

fn delete_notebook(client: &JoplinClient, args: &Value) -> Result<String> {
    let notebook_id = require_str(args, "notebook_id")?;
    client.delete_notebook(notebook_id)?;
    Ok(format!(
        "Deleted notebook '{notebook_id}' (moved to trash)."
    ))
}
