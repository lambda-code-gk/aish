use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Normal,
    Collaborative,
}

impl ExecutionMode {
    pub fn from_legacy_flag(collaborative: bool) -> Self {
        if collaborative {
            Self::Collaborative
        } else {
            Self::Normal
        }
    }

    pub fn is_collaborative(self) -> bool {
        self == Self::Collaborative
    }
}

impl From<ExecutionMode> for aibe_protocol::ExecutionMode {
    fn from(value: ExecutionMode) -> Self {
        match value {
            ExecutionMode::Normal => Self::Normal,
            ExecutionMode::Collaborative => Self::Collaborative,
        }
    }
}
