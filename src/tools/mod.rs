pub mod notebooks;
pub mod notes;
pub mod tags;
pub mod trash;

use serde::Serialize;
use serde_json::Value;

use crate::error::{JoplinError, Result};
use crate::joplin::{Folder, JoplinClient, Tag};

/// Metadata for a single MCP tool exposed via `tools/list`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Return all MCP tool definitions.
#[must_use]
pub fn all_tools() -> Vec<ToolDefinition> {
    let mut tools = vec![ToolDefinition {
        name: "ping",
        description: "Check connectivity to the local Joplin Web Clipper / Data API.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    }];
    tools.extend(notebooks::definitions());
    tools.extend(notes::definitions());
    tools.extend(tags::definitions());
    tools.extend(trash::definitions());
    tools
}

/// Dispatch an MCP `tools/call` invocation by tool name.
///
/// # Errors
///
/// Returns [`JoplinError`] if the tool name is unknown, arguments are invalid,
/// or the underlying Joplin API request fails.
pub fn dispatch(client: &JoplinClient, name: &str, args: &Value) -> Result<String> {
    match name {
        "ping" => {
            let reply = client.ping()?;
            Ok(format!(
                "Joplin API reachable at {} (response: {reply})",
                client.base_url()
            ))
        }
        _ if notebooks::handles(name) => notebooks::call(client, name, args),
        _ if notes::handles(name) => notes::call(client, name, args),
        _ if tags::handles(name) => tags::call(client, name, args),
        _ if trash::handles(name) => trash::call(client, name, args),
        _ => Err(JoplinError::InvalidInput(format!("Unknown tool: {name}"))),
    }
}

/// Extract a required non-empty string argument.
///
/// # Errors
///
/// Returns [`JoplinError::InvalidInput`] if missing or empty.
pub fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    let val = args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| JoplinError::InvalidInput(format!("Missing required parameter: {key}")))?;
    Ok(val)
}

/// Extract an optional string argument.
#[must_use]
pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

/// Extract an optional non-empty trimmed string argument.
#[must_use]
pub fn opt_nonempty_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Extract an optional boolean argument.
#[must_use]
pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

/// Extract an optional `u32` argument.
#[must_use]
pub fn opt_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

/// Extract an optional `u64` argument.
#[must_use]
pub fn opt_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

/// Extract a required argument that may be either a single string or an array of strings.
///
/// # Errors
///
/// Returns [`JoplinError::InvalidInput`] if missing, empty, or wrong type.
pub fn require_str_or_vec(args: &Value, key: &str) -> Result<Vec<String>> {
    let Some(val) = args.get(key) else {
        return Err(JoplinError::InvalidInput(format!(
            "Missing required parameter: {key}"
        )));
    };

    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(JoplinError::InvalidInput(format!(
                "Parameter '{key}' cannot be empty"
            )));
        }
        return Ok(vec![trimmed.to_string()]);
    }

    if let Some(arr) = val.as_array() {
        let mut out = Vec::with_capacity(arr.len());
        for item in arr {
            let s = item
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    JoplinError::InvalidInput(format!(
                        "All elements in '{key}' must be non-empty strings"
                    ))
                })?;
            out.push(s.to_string());
        }
        if out.is_empty() {
            return Err(JoplinError::InvalidInput(format!(
                "Parameter '{key}' array cannot be empty"
            )));
        }
        return Ok(out);
    }

    Err(JoplinError::InvalidInput(format!(
        "Parameter '{key}' must be a string or array of strings"
    )))
}

/// Resolve a notebook by name or slash-separated path (e.g. `Projects/Work`).
///
/// # Errors
///
/// Returns [`JoplinError::NotFound`] if no matching notebook exists.
pub fn resolve_notebook_by_name(client: &JoplinClient, name_or_path: &str) -> Result<Folder> {
    let notebooks = client.list_notebooks()?;
    let active: Vec<&Folder> = notebooks.iter().filter(|f| f.deleted_time == 0).collect();

    let query = name_or_path.trim().trim_matches('/');
    if query.contains('/') {
        let parts: Vec<&str> = query.split('/').map(str::trim).collect();
        let mut parent_id = String::new();
        let mut current: Option<&Folder> = None;

        for part in parts {
            let found = active
                .iter()
                .find(|f| f.parent_id == parent_id && f.title.eq_ignore_ascii_case(part))
                .copied();
            if let Some(folder) = found {
                parent_id.clone_from(&folder.id);
                current = Some(folder);
            } else {
                current = None;
                break;
            }
        }
        if let Some(folder) = current {
            return Ok(folder.clone());
        }
    }

    if let Some(exact) = active.iter().find(|f| f.title == query) {
        return Ok((*exact).clone());
    }

    if let Some(ci) = active.iter().find(|f| f.title.eq_ignore_ascii_case(query)) {
        return Ok((*ci).clone());
    }

    Err(JoplinError::NotFound(format!(
        "Notebook '{name_or_path}' not found"
    )))
}

/// Resolve a tag by name (exact match first, then case-insensitive).
///
/// # Errors
///
/// Returns [`JoplinError::NotFound`] if no matching tag exists.
pub fn resolve_tag_by_name(client: &JoplinClient, name: &str) -> Result<Tag> {
    let tags = client.list_tags()?;
    let query = name.trim();

    if let Some(exact) = tags.iter().find(|t| t.title == query) {
        return Ok(exact.clone());
    }
    if let Some(ci) = tags.iter().find(|t| t.title.eq_ignore_ascii_case(query)) {
        return Ok(ci.clone());
    }

    Err(JoplinError::NotFound(format!("Tag '{name}' not found")))
}

/// Format a millisecond Unix timestamp as `YYYY-MM-DD HH:MM:SS UTC` without external date crates.
#[must_use]
pub fn format_timestamp_ms(ms: u64) -> String {
    if ms == 0 {
        return "never".to_string();
    }
    let total_secs = ms / 1000;
    let secs_of_day = total_secs % 86_400;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;

    // Howard Hinnant's civil-from-days algorithm
    let days = i64::try_from(total_secs / 86_400).unwrap_or(0) + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02} {hour:02}:{minute:02}:{second:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_unix_epoch_and_known_timestamp() {
        assert_eq!(format_timestamp_ms(0), "never");
        // 2026-01-01 00:00:00 UTC = 1767225600 seconds
        assert_eq!(
            format_timestamp_ms(1_767_225_600_000),
            "2026-01-01 00:00:00 UTC"
        );
    }

    #[test]
    fn require_str_or_vec_accepts_both() {
        let single = serde_json::json!({"id": "abc"});
        assert_eq!(
            require_str_or_vec(&single, "id").ok(),
            Some(vec!["abc".to_string()])
        );

        let multi = serde_json::json!({"id": ["a", "b"]});
        assert_eq!(
            require_str_or_vec(&multi, "id").ok(),
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }
}
