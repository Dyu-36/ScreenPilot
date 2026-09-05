use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::{json, Value};

pub const ZALO_API_BASE: &str = "https://bot-api.zaloplatforms.com";

#[derive(Clone, Debug)]
pub struct ZaloUpdate {
    pub update_id: Option<i64>,
    pub chat_id: String,
    pub text: String,
}

pub fn build_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(150))
        .build()
        .context("không tạo được HTTP client")
}

pub fn delete_webhook(client: &Client, token: &str) {
    let _ = api_call(client, token, "deleteWebhook", &json!({}), Duration::from_secs(15));
}

pub fn get_updates(
    client: &Client,
    token: &str,
    offset: Option<i64>,
    poll_timeout: Duration,
) -> Result<Vec<ZaloUpdate>> {
    let timeout_seconds = poll_timeout.as_secs().max(1);
    let mut body = json!({"timeout": timeout_seconds});
    if let Some(offset) = offset {
        body["offset"] = json!(offset);
    }

    let value = api_call(
        client,
        token,
        "getUpdates",
        &body,
        poll_timeout + Duration::from_secs(20),
    )?;
    Ok(parse_updates(value))
}

pub fn send_message(client: &Client, token: &str, chat_id: &str, text: &str) -> Result<()> {
    let body = json!({
        "chat_id": chat_id,
        "text": text,
    });
    let _ = api_call(client, token, "sendMessage", &body, Duration::from_secs(30))?;
    Ok(())
}

pub fn send_chat_action(client: &Client, token: &str, chat_id: &str, action: &str) {
    let body = json!({
        "chat_id": chat_id,
        "action": action,
    });
    let _ = api_call(client, token, "sendChatAction", &body, Duration::from_secs(10));
}

pub fn send_photo_url(
    client: &Client,
    token: &str,
    chat_id: &str,
    photo_url: &str,
    caption: &str,
) -> Result<()> {
    let body = json!({
        "chat_id": chat_id,
        "photo": photo_url,
        "caption": caption,
    });

    if api_call(client, token, "sendPhoto", &body, Duration::from_secs(45)).is_ok() {
        return Ok(());
    }

    let fallback = json!({
        "chat_id": chat_id,
        "photo": photo_url,
    });
    let _ = api_call(
        client,
        token,
        "sendPhoto",
        &fallback,
        Duration::from_secs(45),
    )?;

    if !caption.trim().is_empty() {
        let _ = send_message(client, token, chat_id, caption);
    }
    Ok(())
}

fn api_call(
    client: &Client,
    token: &str,
    method: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value> {
    let url = format!("{ZALO_API_BASE}/bot{token}/{method}");
    let response = client
        .post(url)
        .timeout(timeout)
        .json(body)
        .send()
        .map_err(|_| anyhow::anyhow!("Zalo {method}: lỗi kết nối"))?;

    let status = response.status();
    let envelope: Value = response
        .json()
        .with_context(|| format!("Zalo {method}: phản hồi JSON không hợp lệ"))?;

    let ok = envelope.get("ok").and_then(Value::as_bool);
    if !status.is_success() || ok == Some(false) {
        let code = envelope
            .get("error_code")
            .and_then(Value::as_i64)
            .unwrap_or(status.as_u16() as i64);

        if method == "getUpdates" && code == 408 {
            return Ok(Value::Array(Vec::new()));
        }

        let description = envelope
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        bail!("Zalo {method} failed ({code}): {description}");
    }

    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

fn parse_updates(value: Value) -> Vec<ZaloUpdate> {
    let values: Vec<Value> = match value {
        Value::Array(items) => items,
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object.remove("updates") {
                items
            } else if let Some(Value::Array(items)) = object.remove("data") {
                items
            } else {
                vec![Value::Object(object)]
            }
        }
        Value::Null => Vec::new(),
        other => vec![other],
    };

    values.into_iter().filter_map(parse_update).collect()
}

fn parse_update(value: Value) -> Option<ZaloUpdate> {
    let object = value.as_object()?;
    let message = object
        .get("message")
        .and_then(Value::as_object)
        .unwrap_or(object);

    let update_id = loose_i64(object.get("update_id")).or_else(|| loose_i64(object.get("id")));

    let chat_id = loose_string(message.get("chat_id"))
        .or_else(|| {
            message
                .get("chat")
                .and_then(Value::as_object)
                .and_then(|chat| loose_string(chat.get("id")))
        })
        .or_else(|| loose_string(object.get("chat_id")))?;

    let text = loose_string(message.get("text"))
        .or_else(|| loose_string(message.get("caption")))
        .or_else(|| loose_string(message.get("message")))
        .unwrap_or_default();

    Some(ZaloUpdate {
        update_id,
        chat_id,
        text,
    })
}

fn loose_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn loose_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_message_update() {
        let updates = parse_updates(json!({
            "updates": [{
                "update_id": 41,
                "message": {
                    "chat": {"id": "chat-7"},
                    "text": "/screenshot"
                }
            }]
        }));

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].update_id, Some(41));
        assert_eq!(updates[0].chat_id, "chat-7");
        assert_eq!(updates[0].text, "/screenshot");
    }

    #[test]
    fn parses_flat_update_with_numeric_chat_id() {
        let updates = parse_updates(json!([{
            "id": "55",
            "chat_id": 12345,
            "text": "/status"
        }]));

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].update_id, Some(55));
        assert_eq!(updates[0].chat_id, "12345");
        assert_eq!(updates[0].text, "/status");
    }
}
