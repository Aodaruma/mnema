use crate::ids::UserId;
use serde::{Deserialize, Serialize};
use time::Time;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationLevel {
    Off,
    Ask,
    AutoWithReview,
    AutoSilent,
}

impl Default for AutomationLevel {
    fn default() -> Self {
        AutomationLevel::Ask
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationFeature {
    InboxClassify,
    DueSuggestion,
    ScheduleGeneration,
    WeeklyReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationSettings {
    pub inbox_classify: AutomationLevel,
    pub due_suggestion: AutomationLevel,
    pub schedule_generation: AutomationLevel,
    pub weekly_review: AutomationLevel,
}

impl Default for AutomationSettings {
    fn default() -> Self {
        AutomationSettings {
            inbox_classify: AutomationLevel::Ask,
            due_suggestion: AutomationLevel::Ask,
            schedule_generation: AutomationLevel::Ask,
            weekly_review: AutomationLevel::Ask,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyReviewSettings {
    #[serde(with = "time::serde::time::option")]
    pub scheduled_time: Option<Time>,
    pub template: Option<String>,
}

impl Default for WeeklyReviewSettings {
    fn default() -> Self {
        WeeklyReviewSettings {
            scheduled_time: None,
            template: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    Local,
    OpenAiCompatible,
    Other(String),
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::Local
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub user_id: UserId,
    pub provider: LlmProvider,
    pub model_for_planning: Option<String>,
    pub model_for_routine: Option<String>,
    pub automation: AutomationSettings,
    pub weekly_review: WeeklyReviewSettings,
}

impl Default for UserSettings {
    fn default() -> Self {
        UserSettings {
            user_id: UserId::new(),
            provider: LlmProvider::default(),
            model_for_planning: None,
            model_for_routine: None,
            automation: AutomationSettings::default(),
            weekly_review: WeeklyReviewSettings::default(),
        }
    }
}
