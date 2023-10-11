use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use reqwest::header::{HeaderValue, AUTHORIZATION, WWW_AUTHENTICATE};
use reqwest::{Client, Request, Response, StatusCode};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_middleware::{Middleware, Next};
use task_local_extensions::Extensions;
use url::Url;

use crate::infrastructure::http::authorization::Bearer;
use crate::infrastructure::http::header::{parse_www_authenticate, AuthError};
use crate::infrastructure::service::keycloak;

use super::TimeoutMiddleware;

const EXPIRED_SIGNATURE: &str = "ExpiredSignature";

pub struct AuthMiddleware {
    token: ArcSwap<InnerState>,
    url: Url,
    client_id: String,
    client: ClientWithMiddleware,
}

#[derive(Debug)]
struct InnerState {
    access_token: Bearer,
    refresh_token: String,
}

#[async_trait::async_trait]
impl Middleware for AuthMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let token = HeaderValue::from_str(self.token.load().access_token.as_str())
            .expect("Access token contains not only visable ASCII characters");
        req.headers_mut().insert(AUTHORIZATION, token);
        let resp = next.run(req, extensions).await?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            if let Some(w3auth) = resp
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|s| s.to_str().ok())
                .and_then(parse_www_authenticate)
            {
                if w3auth.error == AuthError::InvalidToken {
                    if w3auth.error_description != EXPIRED_SIGNATURE {
                        panic!(
                            "There is an unrecoverable error with token, please rerun agent: {:?}",
                            w3auth.error_description
                        );
                    }
                    self.refresh().await?;
                }
            }
        }

        Ok(resp)
    }
}

impl AuthMiddleware {
    pub fn new(
        url: Url,
        client_id: impl Into<String>,
        access_token: &str,
        refresh_token: String,
        refresh_timeout: Duration,
    ) -> Self {
        Self {
            token: ArcSwap::from(Arc::new({
                InnerState {
                    access_token: Bearer::new(access_token),
                    refresh_token,
                }
            })),
            url,
            client_id: client_id.into(),
            client: ClientBuilder::new(Client::new())
                .with(TimeoutMiddleware::new(refresh_timeout))
                .build(),
        }
    }
}

impl AuthMiddleware {
    async fn refresh(&self) -> reqwest_middleware::Result<()> {
        let grant_info = match keycloak::refresh_token(
            &self.client,
            self.url.clone(),
            &self.client_id,
            &self.token.load().refresh_token,
        )
        .await
        {
            Ok(info) => info,
            Err(e) => {
                tracing::error!(cause = %e, "Refresh token failed");
                return Err(e);
            }
        };

        self.token.store(Arc::new(InnerState {
            access_token: Bearer::new(&grant_info.access_token),
            refresh_token: grant_info.refresh_token,
        }));

        Ok(())
    }
}
