use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanTodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
}
