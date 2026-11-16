use serde_json::Value;

use crate::error::{JoplinError, Result};
use crate::joplin::JoplinClient;
use crate::tools::{ToolDefinition, opt_nonempty_str, require_str};

/// Return tool definitions for trash operations.
#[must_use]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "restore_from_trash",
        description: "Restore a note or notebook from Joplin's trash.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "item_id": { "type": "string", "description": "Note or notebook ID to restore" },
                "item_type": {
                    "type": "string",
                    "enum": ["note", "notebook"],
                    "description": "Type of item to restore (auto-detected if omitted)"
                }
            },
            "required": ["item_id"],
            "additionalProperties": false
        }),
    }]
}

/// Return `true` if `name` is handled by this module.
#[must_use]
pub fn handles(name: &str) -> bool {
    name == "restore_from_trash"
}

/// Dispatch a trash tool call.
///
/// # Errors
///
/// Returns [`JoplinError`] if arguments are invalid or the API call fails.
pub fn call(client: &JoplinClient, name: &str, args: &Value) -> Result<String> {
    if name != "restore_from_trash" {
        return Err(JoplinError::InvalidInput(format!("Unknown tool: {name}")));
    }

    let item_id = require_str(args, "item_id")?;
    let item_type = opt_nonempty_str(args, "item_type");

    match item_type {
        Some("note") => {
            client.restore_item(item_id, false)?;
            Ok(format!("Restored note '{item_id}' from trash."))
        }
        Some("notebook") => {
            client.restore_item(item_id, true)?;
            Ok(format!("Restored notebook '{item_id}' from trash."))
        }
        Some(other) => Err(JoplinError::InvalidInput(format!(
            "Invalid item_type '{other}': expected 'note' or 'notebook'"
        ))),
        None => match client.restore_item(item_id, false) {
            Ok(()) => Ok(format!("Restored note '{item_id}' from trash.")),
            Err(JoplinError::NotFound(_)) => {
                client.restore_item(item_id, true)?;
                Ok(format!("Restored notebook '{item_id}' from trash."))
            }
            Err(e) => Err(e),
        },
    }
}
