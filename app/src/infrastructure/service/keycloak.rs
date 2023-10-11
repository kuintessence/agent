use reqwest::{Client, IntoUrl, RequestBuilder};
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct GrantInfo {
    pub access_token: String,
    pub refresh_token: String,
}

pub async fn login(
    client: &Client,
    url: impl IntoUrl,
    client_id: &str,
) -> reqwest::Result<LoginInfo> {
    client
        .post(url)
        .form(&[("client_id", client_id)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

pub fn grant_request(
    client: &Client,
    url: impl IntoUrl,
    client_id: &str,
    device_code: &str,
) -> RequestBuilder {
    client.post(url).form(&GrantParams {
        grant_type: "urn:ietf:params:oauth:grant-type:device_code",
        client_id,
        device_code,
    })
}

pub async fn refresh_token(
    client: &ClientWithMiddleware,
    url: impl IntoUrl,
    client_id: &str,
    rtoken: &str,
) -> reqwest_middleware::Result<GrantInfo> {
    client
        .post(url)
        .form(&RefreshTokenParams {
            grant_type: "refresh_token",
            client_id,
            refresh_token: rtoken,
        })
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .map_err(|e| e.into())
}

#[derive(Debug, Deserialize)]
pub struct LoginInfo {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
}

#[derive(Debug, Serialize)]
struct GrantParams<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    device_code: &'a str,
}

#[derive(Debug, Serialize)]
struct RefreshTokenParams<'a> {
    grant_type: &'static str,
    client_id: &'a str,
    refresh_token: &'a str,
}
