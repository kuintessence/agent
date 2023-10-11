use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Reply<T> {
    status: u16,
    content: Option<T>,
    message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("server error: {status}\t{message}")]
pub struct ReplyError {
    pub status: u16,
    pub message: String,
}

impl<T> Reply<T> {
    #[allow(dead_code)]
    #[inline]
    pub const fn status(&self) -> u16 {
        self.status
    }

    #[inline]
    pub const fn is_ok(&self) -> bool {
        self.status == 200
    }

    /// Turn Reply into Result
    pub fn ok(self) -> Result<T, ReplyError> {
        if self.is_ok() {
            Ok(self.content.expect("`content` is `null` or `undefined` when status is OK"))
        } else {
            Err(ReplyError {
                status: self.status,
                message: self
                    .message
                    .expect("`message` is `null` or `undefined` when status isn't OK"),
            })
        }
    }
}
