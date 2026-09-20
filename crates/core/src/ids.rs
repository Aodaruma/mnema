use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        // A generated identity must be an explicit operation; implementing
        // `Default` would make accidental ID creation too easy in domain code.
        #[allow(clippy::new_without_default)]
        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

id_type!(TaskId);
id_type!(ProjectId);
id_type!(ListId);
id_type!(StatusId);
id_type!(StatusGroupId);
id_type!(MilestoneId);
id_type!(ScheduleBlockId);
id_type!(UserId);
id_type!(AssistantId);
id_type!(AutomationLogId);
id_type!(LlmMemoryId);
id_type!(HabitId);
id_type!(HabitOccurrenceId);
id_type!(ExternalEventId);
id_type!(CalendarAccountId);
id_type!(SchedulingPolicyId);
id_type!(ManagedCalendarEventId);

/// Stable identity used by the local-first single-user CLI, desktop, and server.
///
/// Keeping this value deterministic lets every frontend address the same Habit,
/// Calendar, and SchedulingPreferences records across restarts.
#[must_use]
pub fn local_user_id() -> UserId {
    UserId(Uuid::from_u128(1))
}
