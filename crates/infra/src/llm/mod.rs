use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmConfig {
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<String>;
    async fn chat(&self, messages: &[ChatMessage], config: &LlmConfig) -> Result<String>;
}

#[derive(Clone)]
pub struct OllamaClient {
    client: Client,
    base_url: String,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
        }
    }

    fn generate_body(prompt: &str, config: &LlmConfig) -> serde_json::Value {
        json!({
            "model": config.model,
            "prompt": prompt,
            "temperature": config.temperature,
            "options": config.max_tokens.map(|t| json!({ "num_predict": t })).unwrap_or(json!({})),
            "stream": false
        })
    }

    fn chat_body(messages: &[ChatMessage], config: &LlmConfig) -> serde_json::Value {
        json!({
            "model": config.model,
            "messages": messages,
            "temperature": config.temperature,
            "options": config.max_tokens.map(|t| json!({ "num_predict": t })).unwrap_or(json!({})),
            "stream": false
        })
    }
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<String> {
        let url = format!("{}/api/generate", self.base_url.trim_end_matches('/'));
        let body = Self::generate_body(prompt, config);
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;
        resp.get("response")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected response from Ollama"))
    }

    async fn chat(&self, messages: &[ChatMessage], config: &LlmConfig) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let body = Self::chat_body(messages, config);
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;
        resp.get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected chat response from Ollama"))
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl OpenAiCompatibleClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let bearer = format!("Bearer {}", self.api_key);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&bearer).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn completion_body(prompt: &str, config: &LlmConfig) -> serde_json::Value {
        json!({
            "model": config.model,
            "prompt": prompt,
            "temperature": config.temperature,
            "max_tokens": config.max_tokens
        })
    }

    fn chat_body(messages: &[ChatMessage], config: &LlmConfig) -> serde_json::Value {
        json!({
            "model": config.model,
            "messages": messages,
            "temperature": config.temperature,
            "max_tokens": config.max_tokens
        })
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatibleClient {
    async fn complete(&self, prompt: &str, config: &LlmConfig) -> Result<String> {
        let url = format!("{}/v1/completions", self.base_url.trim_end_matches('/'));
        let body = Self::completion_body(prompt, config);
        let resp = self
            .client
            .post(url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        resp.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c0| c0.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected response from OpenAI-compatible completion"))
    }

    async fn chat(&self, messages: &[ChatMessage], config: &LlmConfig) -> Result<String> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let body = Self::chat_body(messages, config);
        let resp = self
            .client
            .post(url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        resp.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c0| c0.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("unexpected response from OpenAI-compatible chat"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ollama_generate_body() {
        let cfg = LlmConfig {
            model: "llama3".into(),
            temperature: Some(0.7),
            max_tokens: Some(128),
        };
        let body = OllamaClient::generate_body("hello", &cfg);
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["prompt"], "hello");
        let temp = body["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 1e-6);
        assert_eq!(body["options"]["num_predict"], 128);
    }

    #[test]
    fn build_openai_chat_body() {
        let cfg = LlmConfig {
            model: "gpt-4.1".into(),
            temperature: None,
            max_tokens: None,
        };
        let msgs = vec![
            ChatMessage {
                role: ChatRole::System,
                content: "You are helpful".into(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "Hello".into(),
            },
        ];
        let body = OpenAiCompatibleClient::chat_body(&msgs, &cfg);
        assert_eq!(body["model"], "gpt-4.1");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Hello");
    }
}
