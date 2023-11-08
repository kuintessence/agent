use serde::{de::DeserializeOwned, Deserialize, Deserializer};

pub trait ParseReply {
    fn status(&self) -> serde_json::Result<StatusCode>;
    fn content<T: DeserializeOwned>(&self) -> Result<T, ParseReplyError>;
    fn error(&self) -> serde_json::Result<ReplyError>;
}

impl ParseReply for [u8] {
    fn status(&self) -> serde_json::Result<StatusCode> {
        serde_json::from_slice::<Status>(self).map(|s| s.status)
    }

    fn content<T: DeserializeOwned>(&self) -> Result<T, ParseReplyError> {
        let reply: Reply<T> = serde_json::from_slice(self)?;
        match reply.content {
            Some(content) => Ok(content),
            None => Err(ReplyError::from(reply).into()),
        }
    }

    fn error(&self) -> serde_json::Result<ReplyError> {
        serde_json::from_slice(self)
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Deserialize)]
pub struct StatusCode(u16);

#[derive(Debug, Deserialize)]
struct Reply<T> {
    status: StatusCode,
    #[serde(default)]
    message: String,
    #[serde(
        deserialize_with = "deserialize_content",
        bound = "T: Deserialize<'de>"
    )]
    content: Option<T>,
}

fn deserialize_content<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(T::deserialize(deserializer).ok())
}

#[derive(Debug, thiserror::Error)]
pub enum ParseReplyError {
    #[error("{0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("{0}")]
    Reply(#[from] ReplyError),
}

#[derive(Debug, thiserror::Error, Deserialize)]
#[error("server error: {status}\t{message}")]
pub struct ReplyError {
    pub status: StatusCode,
    #[serde(default)]
    pub message: String,
}

impl<T> From<Reply<T>> for ReplyError {
    fn from(reply: Reply<T>) -> Self {
        Self {
            status: reply.status,
            message: reply.message,
        }
    }
}

impl StatusCode {
    pub const OK: StatusCode = Self(0);
    pub const FLASH_UPLOAD: StatusCode = Self(100);
    pub const INCOMPLETE_UPLOAD: StatusCode = Self(101);
    pub const INCOMPLETE_OLD_UPLOAD: StatusCode = Self(102);
}

impl std::fmt::Display for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Deserialize)]
struct Status {
    status: StatusCode,
}
