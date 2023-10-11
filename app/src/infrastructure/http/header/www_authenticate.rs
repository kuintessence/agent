use std::borrow::Cow;

const REALM: &str = "CO-COM";

#[derive(Debug, PartialEq, Eq)]
pub struct W3Authenticate {
    pub realm: Cow<'static, str>,
    pub error: AuthError,
    pub error_description: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    /// HTTP 400 (Bad Request)
    InvalidRequest,
    /// HTTP 401 (Unauthorized)
    InvalidToken,
    /// HTTP 403 (Forbidden)
    InsufficientScope,
}

pub fn parse(s: &str) -> Option<W3Authenticate> {
    let mut pairs = s.split(',');

    let realm = pair(pairs.next()?, "Bearer realm", |v| {
        Some(if v == REALM {
            Cow::from(REALM)
        } else {
            Cow::from(v.to_owned())
        })
    })?;
    let error = pair(pairs.next()?, "error", |v| {
        Some(match v {
            "invalid_request" => AuthError::InvalidRequest,
            "invalid_token" => AuthError::InvalidToken,
            "insufficient_scope" => AuthError::InsufficientScope,
            _ => return None,
        })
    })?;
    let error_description = pair(pairs.next()?, "error_description", |v| Some(v.to_owned()))?;

    Some(W3Authenticate {
        realm,
        error,
        error_description,
    })
}

fn pair<T, F>(pair: &str, expected_key: &str, f: F) -> Option<T>
where
    F: FnOnce(&str) -> Option<T>,
{
    let (k, v) = pair.split_once('=')?;
    if k != expected_key {
        return None;
    }
    f(v.trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{parse, AuthError, W3Authenticate, REALM};

    #[test]
    fn test_success() {
        let s =
            r#"Bearer realm="CO-COM",error="invalid_token",error_description="ExpiredSignature""#;
        let expected = W3Authenticate {
            realm: Cow::Borrowed(REALM),
            error: AuthError::InvalidToken,
            error_description: String::from("ExpiredSignature"),
        };
        assert_eq!(Some(expected), parse(s));
    }

    #[test]
    fn test_fail() {
        let s = r#"Bearer realm="CO-COM",error="foobar",error_description="other""#;
        assert!(parse(s).is_none());
    }
}
