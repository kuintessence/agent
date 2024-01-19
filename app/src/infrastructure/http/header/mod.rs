mod www_authenticate;

pub use self::www_authenticate::{parse as parse_www_authenticate, AuthError};
pub const TASK_ID: &str = "TaskId";
