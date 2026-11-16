use std::fmt::Write as _;

use serde_json::Value;

use crate::error::{JoplinError, Result};
use crate::joplin::JoplinClient;
use crate::tools::{
    ToolDefinition, format_timestamp_ms, opt_bool, require_str, require_str_or_vec,
    resolve_tag_by_name,
};

/// Return tool definitions for tags.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_tags",
            description: "List all tags in Joplin.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "verbose": {
                        "type": "boolean",
                        "description": "Also return creation and update timestamps (default: false)"
                    }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "create_tag",
            description: "Create a new tag in Joplin.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Tag title" }
                },
                "required": ["title"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "update_tag",
            description: "Rename an existing tag in Joplin.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tag_id": { "type": "string", "description": "Tag ID to update" },
                    "title": { "type": "string", "description": "New tag title" }
                },
                "required": ["tag_id", "title"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "delete_tag",
            description: "Delete a tag from Joplin.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tag_id": { "type": "string", "description": "Tag ID to delete" }
                },
                "required": ["tag_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "get_tags_by_note",
            description: "Get all tags attached to a specific note.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": { "type": "string", "description": "Note ID to get tags from" }
                },
                "required": ["note_id"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "tag_note",
            description: "Add one or more tags to one or more notes (creates tags if they don't exist).",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ],
                        "description": "Note ID or list of note IDs"
                    },
                    "tag_name": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ],
                        "description": "Tag name or list of tag names"
                    }
                },
                "required": ["note_id", "tag_name"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "untag_note",
            description: "Remove one or more tags from one or more notes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "note_id": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ],
                        "description": "Note ID or list of note IDs"
                    },
                    "tag_name": {
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ],
                        "description": "Tag name or list of tag names"
                    }
                },
                "required": ["note_id", "tag_name"],
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
        "list_tags"
            | "create_tag"
            | "update_tag"
            | "delete_tag"
            | "get_tags_by_note"
            | "tag_note"
            | "untag_note"
    )
}

/// Dispatch a tag tool call.
///
/// # Errors
///
/// Returns [`JoplinError`] if arguments are invalid or the API call fails.
pub fn call(client: &JoplinClient, name: &str, args: &Value) -> Result<String> {
    match name {
        "list_tags" => list_tags(client, args),
        "create_tag" => create_tag(client, args),
        "update_tag" => update_tag(client, args),
        "delete_tag" => delete_tag(client, args),
        "get_tags_by_note" => get_tags_by_note(client, args),
        "tag_note" => tag_note(client, args),
        "untag_note" => untag_note(client, args),
        _ => Err(JoplinError::InvalidInput(format!("Unknown tool: {name}"))),
    }
}

fn list_tags(client: &JoplinClient, args: &Value) -> Result<String> {
    let verbose = opt_bool(args, "verbose").unwrap_or(false);
    let mut tags = client.list_tags()?;
    if tags.is_empty() {
        return Ok("No tags found.".to_string());
    }
    tags.sort_by_key(|a| a.title.to_lowercase());

    let mut out = format!("Tags ({}):\n", tags.len());
    for tag in tags {
        let _ = writeln!(out, "- {} (id: {})", tag.title, tag.id);
        if verbose {
            let _ = writeln!(
                out,
                "  created: {} | updated: {}",
                format_timestamp_ms(tag.created_time),
                format_timestamp_ms(tag.updated_time)
            );
        }
    }
    Ok(out)
}

fn create_tag(client: &JoplinClient, args: &Value) -> Result<String> {
    let title = require_str(args, "title")?;
    let tag = client.create_tag(title)?;
    Ok(format!("Created tag '{}' (id: {}).", tag.title, tag.id))
}

fn update_tag(client: &JoplinClient, args: &Value) -> Result<String> {
    let tag_id = require_str(args, "tag_id")?;
    let title = require_str(args, "title")?;
    let tag = client.update_tag(tag_id, title)?;
    Ok(format!("Updated tag '{}' (id: {}).", tag.title, tag.id))
}

fn delete_tag(client: &JoplinClient, args: &Value) -> Result<String> {
    let tag_id = require_str(args, "tag_id")?;
    client.delete_tag(tag_id)?;
    Ok(format!("Deleted tag '{tag_id}'."))
}

fn get_tags_by_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_id = require_str(args, "note_id")?;
    let tags = client.get_note_tags(note_id)?;
    if tags.is_empty() {
        return Ok(format!("Note '{note_id}' has no tags."));
    }
    let mut out = format!("Tags on note '{note_id}' ({}):\n", tags.len());
    for tag in tags {
        let _ = writeln!(out, "- {} (id: {})", tag.title, tag.id);
    }
    Ok(out)
}

fn tag_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_ids = require_str_or_vec(args, "note_id")?;
    let tag_names = require_str_or_vec(args, "tag_name")?;

    let mut resolved_tags = Vec::with_capacity(tag_names.len());
    for name in &tag_names {
        let tag = match resolve_tag_by_name(client, name) {
            Ok(existing) => existing,
            Err(JoplinError::NotFound(_)) => client.create_tag(name)?,
            Err(e) => return Err(e),
        };
        resolved_tags.push(tag);
    }

    for note_id in &note_ids {
        for tag in &resolved_tags {
            client.add_tag_to_note(&tag.id, note_id)?;
        }
    }

    Ok(format!(
        "Tagged {} note(s) with {} tag(s): {}.",
        note_ids.len(),
        resolved_tags.len(),
        resolved_tags
            .iter()
            .map(|t| t.title.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn untag_note(client: &JoplinClient, args: &Value) -> Result<String> {
    let note_ids = require_str_or_vec(args, "note_id")?;
    let tag_names = require_str_or_vec(args, "tag_name")?;

    let mut resolved_tags = Vec::with_capacity(tag_names.len());
    for name in &tag_names {
        resolved_tags.push(resolve_tag_by_name(client, name)?);
    }

    for note_id in &note_ids {
        for tag in &resolved_tags {
            client.remove_tag_from_note(&tag.id, note_id)?;
        }
    }

    Ok(format!(
        "Removed {} tag(s) from {} note(s).",
        resolved_tags.len(),
        note_ids.len()
    ))
}
