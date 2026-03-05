use std::sync::Arc;

use mnema_core::prelude::{LlmProvider, UserSettings};

use super::{LlmClient, LlmConfig, OllamaClient, OpenAiCompatibleClient};

/// LLMエンドポイント設定（環境やユーザー設定に依存しない固定部）
#[derive(Debug, Clone)]
pub struct LlmFactoryConfig {
    pub ollama_base_url: String,
    pub openai_base_url: String,
    pub api_key: Option<String>,
    pub default_planning_model: String,
    pub default_routine_model: String,
}

impl Default for LlmFactoryConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: "http://localhost:11434".into(),
            openai_base_url: "http://localhost:8000".into(), // OpenAI互換サーバの想定
            api_key: None,
            default_planning_model: "gpt-4.1".into(),
            default_routine_model: "gpt-4.1-mini".into(),
        }
    }
}

pub struct LlmFactory {
    cfg: LlmFactoryConfig,
}

impl LlmFactory {
    pub fn new(cfg: LlmFactoryConfig) -> Self {
        Self { cfg }
    }

    pub fn from_env() -> Self {
        let mut cfg = LlmFactoryConfig::default();
        if let Ok(v) = std::env::var("MNEMA_OLLAMA_URL") {
            cfg.ollama_base_url = v;
        }
        if let Ok(v) = std::env::var("MNEMA_OPENAI_URL") {
            cfg.openai_base_url = v;
        }
        if let Ok(v) = std::env::var("MNEMA_OPENAI_API_KEY") {
            cfg.api_key = Some(v);
        }
        Self { cfg }
    }

    /// UserSettings を見て適切なクライアントとデフォルト設定を返す
    pub fn client_for(
        &self,
        settings: &UserSettings,
    ) -> anyhow::Result<(Arc<dyn LlmClient>, LlmConfig)> {
        let (client, model) = match settings.provider {
            LlmProvider::Local => {
                let m = settings
                    .model_for_routine
                    .clone()
                    .unwrap_or_else(|| self.cfg.default_routine_model.clone());
                (
                    Arc::new(OllamaClient::new(&self.cfg.ollama_base_url)) as Arc<dyn LlmClient>,
                    m,
                )
            }
            LlmProvider::OpenAiCompatible => {
                let m = settings
                    .model_for_planning
                    .clone()
                    .unwrap_or_else(|| self.cfg.default_planning_model.clone());
                let key = self
                    .cfg
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .unwrap_or_default();
                (
                    Arc::new(OpenAiCompatibleClient::new(&self.cfg.openai_base_url, key))
                        as Arc<dyn LlmClient>,
                    m,
                )
            }
            LlmProvider::Other(ref _name) => {
                // デフォルトでは OpenAI 互換にフォールバック
                let m = settings
                    .model_for_planning
                    .clone()
                    .unwrap_or_else(|| self.cfg.default_planning_model.clone());
                let key = self
                    .cfg
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .unwrap_or_default();
                (
                    Arc::new(OpenAiCompatibleClient::new(&self.cfg.openai_base_url, key))
                        as Arc<dyn LlmClient>,
                    m,
                )
            }
        };

        let llm_cfg = LlmConfig {
            model,
            temperature: Some(0.7),
            max_tokens: None,
        };

        Ok((client, llm_cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnema_core::prelude::UserId;

    fn base_settings(provider: LlmProvider) -> UserSettings {
        UserSettings {
            user_id: UserId::new(),
            provider,
            model_for_planning: None,
            model_for_routine: None,
            automation: Default::default(),
            weekly_review: Default::default(),
        }
    }

    #[test]
    fn builds_ollama_client() {
        let factory = LlmFactory::new(LlmFactoryConfig {
            ollama_base_url: "http://ollama.local".into(),
            ..Default::default()
        });
        let (client, cfg) = factory
            .client_for(&base_settings(LlmProvider::Local))
            .unwrap();
        assert_eq!(client.provider(), "ollama");
        assert!(!cfg.model.is_empty());
    }

    #[test]
    fn builds_openai_client() {
        let factory = LlmFactory::new(LlmFactoryConfig {
            openai_base_url: "https://api.openai.com".into(),
            api_key: Some("dummy".into()),
            ..Default::default()
        });
        let (client, cfg) = factory
            .client_for(&base_settings(LlmProvider::OpenAiCompatible))
            .unwrap();
        assert_eq!(client.provider(), "openai_compatible");
        assert!(!cfg.model.is_empty());
    }
}
