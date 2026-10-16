//! Workflow Builder chat history: one owner-only JSON file per chat.
//!
//! The page owns the message shape; Rust only checks the id, bounds the size,
//! and reads the summary fields for the history list.

use super::{data_dir, write_private};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

const MAX_CHAT_BYTES: usize = 4 * 1024 * 1024;
const MAX_LISTED: usize = 200;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub updated_at: i64,
}

fn chats_dir() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("builder-chats"))
}

fn chat_path(id: &str) -> Result<PathBuf, String> {
    // The id becomes a file name; only a canonical uuid may.
    let id = uuid::Uuid::parse_str(id).map_err(|_| "Invalid chat id.".to_string())?;
    Ok(chats_dir()?.join(format!("{}.json", id.hyphenated())))
}

#[tauri::command]
pub fn builder_list_chats() -> Vec<ChatSummary> {
    let Ok(entries) = chats_dir().and_then(|dir| std::fs::read_dir(dir).map_err(|e| e.to_string()))
    else {
        return Vec::new();
    };
    let mut chats: Vec<ChatSummary> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<ChatSummary>(&text).ok())
        .collect();
    chats.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    chats.truncate(MAX_LISTED);
    chats
}

#[tauri::command]
pub fn builder_load_chat(id: String) -> Result<Value, String> {
    let text = std::fs::read_to_string(chat_path(&id)?).map_err(|_| "Chat not found.".to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn builder_save_chat(chat: Value) -> Result<(), String> {
    let id = chat
        .get("id")
        .and_then(Value::as_str)
        .ok_or("Chat has no id.")?;
    let path = chat_path(id)?;
    let text = serde_json::to_string(&chat).map_err(|e| e.to_string())?;
    if text.len() > MAX_CHAT_BYTES {
        return Err("This chat is too long to save. Start a new chat.".into());
    }
    write_private(&path, &text)
}

#[tauri::command]
pub fn builder_delete_chat(id: String) -> Result<(), String> {
    match std::fs::remove_file(chat_path(&id)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_ids_must_be_uuids() {
        assert!(chat_path("../../etc/passwd").is_err());
        assert!(chat_path("not-a-uuid").is_err());
        let path = chat_path("61C25329-C153-4327-8D52-F04F32B7903D").unwrap();
        assert!(path.ends_with("builder-chats/61c25329-c153-4327-8d52-f04f32b7903d.json"));
    }

    #[test]
    fn summaries_ignore_messages() {
        let summary: ChatSummary = serde_json::from_str(
            r#"{"id":"x","title":"Digest","agent":"codex","updatedAt":5,"messages":[{"big":true}]}"#,
        )
        .unwrap();
        assert_eq!(summary.title, "Digest");
        assert_eq!(summary.updated_at, 5);
    }
}
