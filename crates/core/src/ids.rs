use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

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
id_type!(UserId);
id_type!(AssistantId);
id_type!(AutomationLogId);
id_type!(LlmMemoryId);
