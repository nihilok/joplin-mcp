use std::fmt::Write as _;
use std::sync::Mutex;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::Agent;

use crate::config::Config;
use crate::error::{JoplinError, Result};

/// Default fields requested for note summaries.
pub const NOTE_SUMMARY_FIELDS: &str =
    "id,title,parent_id,is_todo,todo_completed,todo_due,created_time,updated_time,deleted_time";

/// Default fields requested for full note retrieval.
pub const NOTE_FULL_FIELDS: &str = "id,title,body,parent_id,is_todo,todo_completed,todo_due,created_time,updated_time,deleted_time,source_url,author";

/// Default fields requested for folders (notebooks).
pub const FOLDER_FIELDS: &str = "id,title,parent_id,icon,created_time,updated_time,deleted_time";

/// Default fields requested for tags.
pub const TAG_FIELDS: &str = "id,title,created_time,updated_time";

/// Default fields requested for resources.
pub const RESOURCE_FIELDS: &str =
    "id,title,mime,filename,size,ocr_text,ocr_status,created_time,updated_time";

/// Paginated response envelope from the Joplin Data API.
#[derive(Debug, Clone, Deserialize)]
pub struct Paginated<T> {
    pub items: Vec<T>,
    #[serde(default)]
    pub has_more: bool,
}

/// Joplin note representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub is_todo: u8,
    #[serde(default)]
    pub todo_completed: u64,
    #[serde(default)]
    pub todo_due: u64,
    #[serde(default)]
    pub created_time: u64,
    #[serde(default)]
    pub updated_time: u64,
    #[serde(default)]
    pub deleted_time: u64,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub author: String,
}

/// Joplin notebook (folder) representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub parent_id: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub created_time: u64,
    #[serde(default)]
    pub updated_time: u64,
    #[serde(default)]
    pub deleted_time: u64,
}

/// Joplin tag representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_time: u64,
    #[serde(default)]
    pub updated_time: u64,
}

/// Joplin resource (attachment) representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub ocr_text: String,
    #[serde(default)]
    pub ocr_status: i64,
    #[serde(default)]
    pub created_time: u64,
    #[serde(default)]
    pub updated_time: u64,
}

#[derive(Debug, Clone)]
struct ResolvedEndpoint {
    label: &'static str,
    base_url: String,
    token: String,
}

/// HTTP client for Joplin's local Web Clipper / Data API.
#[derive(Debug)]
pub struct JoplinClient {
    config: Config,
    agent: Agent,
    probe_agent: Agent,
    resolved: Mutex<Option<ResolvedEndpoint>>,
}

/// Query parameters for paginated note listing/search endpoints.
#[derive(Debug, Clone, Copy)]
pub struct NoteQuery<'a> {
    pub fields: &'a str,
    pub page: u32,
    pub limit: u32,
    pub order_by: Option<&'a str>,
    pub order_dir: Option<&'a str>,
    pub include_deleted: bool,
}

impl JoplinClient {
    /// Create a new `JoplinClient` from the resolved [`Config`].
    #[must_use]
    pub fn new(config: Config) -> Self {
        let ureq_config = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(config.timeout_secs)))
            .http_status_as_error(false)
            .build();
        let agent = ureq_config.new_agent();

        let probe_config = Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build();
        let probe_agent = probe_config.new_agent();

        Self {
            config,
            agent,
            probe_agent,
            resolved: Mutex::new(None),
        }
    }

    /// Return the active or configured base URL.
    #[must_use]
    pub fn base_url(&self) -> String {
        if self.config.token.is_some() {
            return self.config.base_url.clone();
        }
        if let Ok(guard) = self.resolved.lock()
            && let Some(ep) = guard.as_ref()
        {
            return ep.base_url.clone();
        }
        self.config.base_url.clone()
    }

    fn resolve_endpoint(&self) -> Result<ResolvedEndpoint> {
        if let Some(token) = &self.config.token {
            return Ok(ResolvedEndpoint {
                label: "configured",
                base_url: self.config.base_url.clone(),
                token: token.clone(),
            });
        }

        if let Ok(guard) = self.resolved.lock()
            && let Some(ep) = guard.as_ref()
        {
            return Ok(ep.clone());
        }

        let candidates = self.config.discover_profiles();
        if candidates.is_empty() {
            return Err(JoplinError::Config(
                "No JOPLIN_TOKEN set and no local Joplin settings.json found with api.token."
                    .to_string(),
            ));
        }

        for candidate in &candidates {
            let probe_url = format_url(
                &candidate.base_url,
                "/folders",
                &[("limit", "1")],
                Some(&candidate.token),
            );
            if let Ok(resp) = self.probe_agent.get(&probe_url).call()
                && (200..300).contains(&resp.status().as_u16())
            {
                let ep = ResolvedEndpoint {
                    label: candidate.label,
                    base_url: candidate.base_url.clone(),
                    token: candidate.token.clone(),
                };
                if let Ok(mut guard) = self.resolved.lock() {
                    *guard = Some(ep.clone());
                }
                return Ok(ep);
            }
        }

        let checked: Vec<String> = candidates
            .iter()
            .map(|c| format!("{} ({})", c.label, c.base_url))
            .collect();
        Err(JoplinError::Http(format!(
            "No running Joplin API found. Checked: {}. Start Joplin Desktop (Web Clipper enabled) or `joplin server start`.",
            checked.join(", ")
        )))
    }

    fn clear_cached_endpoint(&self) {
        if self.config.token.is_none()
            && let Ok(mut guard) = self.resolved.lock()
        {
            *guard = None;
        }
    }

    fn build_url(
        &self,
        path: &str,
        params: &[(&str, &str)],
        require_token: bool,
    ) -> Result<String> {
        if require_token {
            let ep = self.resolve_endpoint()?;
            Ok(format_url(&ep.base_url, path, params, Some(&ep.token)))
        } else {
            Ok(format_url(&self.base_url(), path, params, None))
        }
    }

    fn parse_response<T: DeserializeOwned>(
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<T> {
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| JoplinError::Http(format!("Failed reading response body: {e}")))?;

        if !(200..300).contains(&status) {
            if status == 404 {
                return Err(JoplinError::NotFound(text));
            }
            return Err(JoplinError::Api { status, body: text });
        }

        serde_json::from_str(&text)
            .map_err(|e| JoplinError::Json(format!("Failed decoding JSON: {e}: {text}")))
    }

    fn parse_empty_response(mut response: ureq::http::Response<ureq::Body>) -> Result<()> {
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| JoplinError::Http(format!("Failed reading response body: {e}")))?;

        if !(200..300).contains(&status) {
            if status == 404 {
                return Err(JoplinError::NotFound(text));
            }
            return Err(JoplinError::Api { status, body: text });
        }
        Ok(())
    }

    fn execute_with_retry<T, F>(&self, op: F) -> Result<T>
    where
        F: Fn() -> Result<T>,
    {
        match op() {
            Ok(v) => Ok(v),
            Err(err)
                if self.config.token.is_none()
                    && matches!(
                        err,
                        JoplinError::Http(_)
                            | JoplinError::Api {
                                status: 401 | 403,
                                ..
                            }
                    ) =>
            {
                self.clear_cached_endpoint();
                op()
            }
            Err(err) => Err(err),
        }
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str, params: &[(&str, &str)]) -> Result<T> {
        self.execute_with_retry(|| {
            let url = self.build_url(path, params, true)?;
            let resp = self
                .agent
                .get(&url)
                .call()
                .map_err(|e| JoplinError::Http(format!("GET {path} failed: {e}")))?;
            Self::parse_response(resp)
        })
    }

    fn post_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.execute_with_retry(|| {
            let url = self.build_url(path, &[], true)?;
            let resp = self
                .agent
                .post(&url)
                .send_json(body)
                .map_err(|e| JoplinError::Http(format!("POST {path} failed: {e}")))?;
            Self::parse_response(resp)
        })
    }

    fn put_json<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        self.execute_with_retry(|| {
            let url = self.build_url(path, &[], true)?;
            let resp = self
                .agent
                .put(&url)
                .send_json(body)
                .map_err(|e| JoplinError::Http(format!("PUT {path} failed: {e}")))?;
            Self::parse_response(resp)
        })
    }

    fn delete(&self, path: &str) -> Result<()> {
        self.execute_with_retry(|| {
            let url = self.build_url(path, &[], true)?;
            let resp = self
                .agent
                .delete(&url)
                .call()
                .map_err(|e| JoplinError::Http(format!("DELETE {path} failed: {e}")))?;
            Self::parse_empty_response(resp)
        })
    }

    /// Collect all pages for a paginated endpoint.
    fn get_all_pages<T: DeserializeOwned>(
        &self,
        path: &str,
        extra_params: &[(&str, &str)],
    ) -> Result<Vec<T>> {
        let mut all = Vec::new();
        let mut page = 1_u32;

        loop {
            let page_str = page.to_string();
            let mut params = Vec::with_capacity(extra_params.len() + 2);
            params.extend_from_slice(extra_params);
            params.push(("limit", "100"));
            params.push(("page", &page_str));

            let batch: Paginated<T> = self.get_json(path, &params)?;
            all.extend(batch.items);
            if !batch.has_more {
                break;
            }
            page += 1;
        }

        Ok(all)
    }

    /// Ping Joplin Web Clipper API (`GET /ping`).
    ///
    /// # Errors
    ///
    /// Returns [`JoplinError::Http`] or [`JoplinError::Api`] if Joplin is unreachable.
    pub fn ping(&self) -> Result<String> {
        self.execute_with_retry(|| {
            let ep = self.resolve_endpoint()?;
            let url = format_url(&ep.base_url, "/ping", &[], None);
            let mut resp = self
                .agent
                .get(&url)
                .call()
                .map_err(|e| JoplinError::Http(format!("GET /ping failed: {e}")))?;
            let status = resp.status().as_u16();
            let text = resp
                .body_mut()
                .read_to_string()
                .map_err(|e| JoplinError::Http(format!("Failed reading /ping response: {e}")))?;
            if !(200..300).contains(&status) {
                return Err(JoplinError::Api { status, body: text });
            }
            Ok(format!("{text} (profile: {})", ep.label))
        })
    }

    /// List all notebooks (folders).
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn list_notebooks(&self) -> Result<Vec<Folder>> {
        self.get_all_pages("/folders", &[("fields", FOLDER_FIELDS)])
    }

    /// Create a notebook.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn create_notebook(&self, body: &Value) -> Result<Folder> {
        self.post_json("/folders", body)
    }

    /// Update a notebook.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn update_notebook(&self, id: &str, body: &Value) -> Result<Folder> {
        self.put_json(&format!("/folders/{id}"), body)
    }

    /// Delete a notebook (moves to trash).
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn delete_notebook(&self, id: &str) -> Result<()> {
        self.delete(&format!("/folders/{id}"))
    }

    /// Get a single note by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the note is not found or the API request fails.
    pub fn get_note(&self, id: &str, fields: &str) -> Result<Note> {
        self.get_json(&format!("/notes/{id}"), &[("fields", fields)])
    }

    /// Get tags attached to a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn get_note_tags(&self, id: &str) -> Result<Vec<Tag>> {
        self.get_all_pages(&format!("/notes/{id}/tags"), &[("fields", TAG_FIELDS)])
    }

    /// Get resources attached to a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn get_note_resources(&self, id: &str) -> Result<Vec<Resource>> {
        self.get_all_pages(
            &format!("/notes/{id}/resources"),
            &[("fields", RESOURCE_FIELDS)],
        )
    }

    /// Create a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn create_note(&self, body: &Value) -> Result<Note> {
        self.post_json("/notes", body)
    }

    /// Update a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn update_note(&self, id: &str, body: &Value) -> Result<Note> {
        self.put_json(&format!("/notes/{id}"), body)
    }

    /// Delete a note (moves to trash).
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn delete_note(&self, id: &str) -> Result<()> {
        self.delete(&format!("/notes/{id}"))
    }

    fn query_notes_endpoint(
        &self,
        path: &str,
        query: NoteQuery<'_>,
        extra: &[(&str, &str)],
    ) -> Result<Paginated<Note>> {
        let page_s = query.page.to_string();
        let limit_s = query.limit.to_string();
        let mut params = Vec::with_capacity(extra.len() + 6);
        params.extend_from_slice(extra);
        params.push(("fields", query.fields));
        params.push(("page", page_s.as_str()));
        params.push(("limit", limit_s.as_str()));
        if let Some(ob) = query.order_by {
            params.push(("order_by", ob));
        }
        if let Some(od) = query.order_dir {
            params.push(("order_dir", od));
        }
        if query.include_deleted {
            params.push(("include_deleted", "1"));
        }
        self.get_json(path, &params)
    }

    /// Search notes using Joplin's `/search` endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn search_notes(&self, text: &str, query: NoteQuery<'_>) -> Result<Paginated<Note>> {
        self.query_notes_endpoint("/search", query, &[("query", text), ("type", "note")])
    }

    /// List notes using `/notes`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn list_notes(&self, query: NoteQuery<'_>) -> Result<Paginated<Note>> {
        self.query_notes_endpoint("/notes", query, &[])
    }

    /// List all notes across all pages (used for backlinks or full scans).
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn list_all_notes_pages(&self, fields: &str) -> Result<Vec<Note>> {
        self.get_all_pages("/notes", &[("fields", fields)])
    }

    /// List notes in a specific notebook.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn list_notebook_notes(
        &self,
        notebook_id: &str,
        query: NoteQuery<'_>,
    ) -> Result<Paginated<Note>> {
        self.query_notes_endpoint(&format!("/folders/{notebook_id}/notes"), query, &[])
    }

    /// List all tags.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        self.get_all_pages("/tags", &[("fields", TAG_FIELDS)])
    }

    /// Create a tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn create_tag(&self, title: &str) -> Result<Tag> {
        self.post_json("/tags", &serde_json::json!({ "title": title }))
    }

    /// Update a tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn update_tag(&self, id: &str, title: &str) -> Result<Tag> {
        self.put_json(
            &format!("/tags/{id}"),
            &serde_json::json!({ "title": title }),
        )
    }

    /// Delete a tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn delete_tag(&self, id: &str) -> Result<()> {
        self.delete(&format!("/tags/{id}"))
    }

    /// Attach a tag to a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn add_tag_to_note(&self, tag_id: &str, note_id: &str) -> Result<()> {
        let _: Value = self.post_json(
            &format!("/tags/{tag_id}/notes"),
            &serde_json::json!({ "id": note_id }),
        )?;
        Ok(())
    }

    /// Remove a tag from a note.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn remove_tag_from_note(&self, tag_id: &str, note_id: &str) -> Result<()> {
        self.delete(&format!("/tags/{tag_id}/notes/{note_id}"))
    }

    /// List notes with a specific tag.
    ///
    /// # Errors
    ///
    /// Returns an error if the Joplin API request fails.
    pub fn find_notes_with_tag(
        &self,
        tag_id: &str,
        query: NoteQuery<'_>,
    ) -> Result<Paginated<Note>> {
        self.query_notes_endpoint(&format!("/tags/{tag_id}/notes"), query, &[])
    }

    /// Restore a trashed note or notebook (`deleted_time = 0`).
    ///
    /// # Errors
    ///
    /// Returns an error if the item is not found or the API request fails.
    pub fn restore_item(&self, item_id: &str, is_notebook: bool) -> Result<()> {
        let path = if is_notebook {
            format!("/folders/{item_id}")
        } else {
            format!("/notes/{item_id}")
        };
        let _: Value = self.put_json(&path, &serde_json::json!({ "deleted_time": 0 }))?;
        Ok(())
    }
}

fn format_url(base_url: &str, path: &str, params: &[(&str, &str)], token: Option<&str>) -> String {
    let mut url = format!("{base_url}{path}");
    let mut has_query = path.contains('?');

    if let Some(tok) = token {
        push_query_param(&mut url, &mut has_query, "token", tok);
    }

    for (k, v) in params {
        push_query_param(&mut url, &mut has_query, k, v);
    }

    url
}

fn push_query_param(url: &mut String, has_query: &mut bool, key: &str, value: &str) {
    url.push(if *has_query { '&' } else { '?' });
    *has_query = true;
    percent_encode_into(key, url);
    url.push('=');
    percent_encode_into(value, url);
}

fn percent_encode_into(input: &str, out: &mut String) {
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_query_params() {
        let mut out = String::new();
        let mut has_query = false;
        push_query_param(&mut out, &mut has_query, "query", "hello world & more");
        assert_eq!(out, "?query=hello%20world%20%26%20more");
    }
}
