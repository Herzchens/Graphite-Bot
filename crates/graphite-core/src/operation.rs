use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationState {
    Pending,
    Committed,
    Cancelled,
    Failed,
    Reversed,
}

impl OperationState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Pending,
                Self::Committed | Self::Cancelled | Self::Failed
            ) | (Self::Committed, Self::Reversed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_state_machine_rejects_invalid_reentry() {
        assert!(OperationState::Pending.can_transition_to(OperationState::Committed));
        assert!(OperationState::Committed.can_transition_to(OperationState::Reversed));
        assert!(!OperationState::Failed.can_transition_to(OperationState::Committed));
        assert!(!OperationState::Committed.can_transition_to(OperationState::Committed));
    }
}
