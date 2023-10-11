mod www_authenticate;

pub use self::www_authenticate::{parse as parse_www_authenticate, AuthError, W3Authenticate};
pub const TASK_ID: &str = "TaskId";
