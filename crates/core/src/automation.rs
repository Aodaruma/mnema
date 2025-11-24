use crate::automation_log::AutomationLog;
use crate::repository::{
    ListRepository, MilestoneRepository, ProjectRepository, StatusRepository, TaskRepository,
    UserSettingsRepository,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationJobKind {
    InboxClassify,
    WeeklyReviewPrep,
    DueDateSuggestion,
    ScheduleGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationJob {
    pub kind: AutomationJobKind,
}

#[derive(Debug, Error)]
pub enum AutomationError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("llm error: {0}")]
    Llm(String),
}

/// LLMに依存する処理を抽象化するための最小トレイト。
#[async_trait::async_trait]
pub trait PromptClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, AutomationError>;
}

/// AutomationService は依存を注入して各ジョブを処理する薄いファサード。
pub struct AutomationService<'a, P: PromptClient> {
    pub tasks: &'a dyn TaskRepository,
    pub projects: &'a dyn ProjectRepository,
    pub lists: &'a dyn ListRepository,
    pub milestones: &'a dyn MilestoneRepository,
    pub statuses: &'a dyn StatusRepository,
    pub user_settings: &'a dyn UserSettingsRepository,
    pub prompt: P,
}

impl<'a, P: PromptClient> AutomationService<'a, P> {
    pub fn new(
        tasks: &'a dyn TaskRepository,
        projects: &'a dyn ProjectRepository,
        lists: &'a dyn ListRepository,
        milestones: &'a dyn MilestoneRepository,
        statuses: &'a dyn StatusRepository,
        user_settings: &'a dyn UserSettingsRepository,
        prompt: P,
    ) -> Self {
        Self {
            tasks,
            projects,
            lists,
            milestones,
            statuses,
            user_settings,
            prompt,
        }
    }

    pub async fn handle(
        &self,
        job: AutomationJob,
    ) -> Result<Option<AutomationLog>, AutomationError> {
        match job.kind {
            AutomationJobKind::InboxClassify => {
                // TODO: implement inbox classification using prompt + repos.
                Ok(None)
            }
            AutomationJobKind::WeeklyReviewPrep => {
                // TODO: gather last 7 days data and prepare review draft.
                Ok(None)
            }
            AutomationJobKind::DueDateSuggestion => {
                // TODO: scan tasks without due dates and suggest ones.
                Ok(None)
            }
            AutomationJobKind::ScheduleGeneration => {
                // TODO: propose a schedule.
                Ok(None)
            }
        }
    }
}

impl From<String> for AutomationError {
    fn from(e: String) -> Self {
        AutomationError::Storage(e)
    }
}
