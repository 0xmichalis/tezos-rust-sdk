use {
    crate::error::Error,
    async_trait::async_trait,
    serde::{de::DeserializeOwned, Serialize},
};

#[async_trait]
pub trait Http {
    fn new(rpc_endpoint: String) -> Self;

    fn change_rpc_endpoint(&mut self, rpc_endpoint: String);

    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, Error>;

    async fn get_with_query<T: DeserializeOwned, Q: Serialize + ?Sized + Sync>(
        &self,
        url: &str,
        query: &Q,
    ) -> Result<T, Error>;

    async fn post<B: Serialize + Sync, T: DeserializeOwned, Q: Serialize + Sync>(
        &self,
        url: &str,
        body: &B,
        query: Option<&Q>,
    ) -> Result<T, Error>;

    async fn patch<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        url: &str,
        body: Option<&B>,
    ) -> Result<T, Error>;

    async fn delete<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        url: &str,
        body: Option<&B>,
    ) -> Result<T, Error>;
}

#[cfg(feature = "http")]
pub mod default {
    use crate::models::error::RpcError;

    use super::*;
    use reqwest::{header::CONTENT_TYPE, Client, Response};
    use std::time::Duration;

    const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

    async fn read_bounded(mut response: Response) -> Result<Vec<u8>, Error> {
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(Error::ResponseTooLarge {
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(Error::ResponseTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    #[derive(Debug)]
    pub struct HttpClient {
        rpc_endpoint: String,
        client: Client,
    }

    impl HttpClient {
        fn url(&self, path: &str) -> String {
            format!("{}{}", self.rpc_endpoint, path)
        }

        async fn handle_response<T: DeserializeOwned>(
            &self,
            response: Response,
        ) -> Result<T, Error> {
            if response.status() != 200 {
                let is_json = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| {
                        value.split(';').next().is_some_and(|mime| {
                            mime.trim().eq_ignore_ascii_case("application/json")
                        })
                    });
                let body = read_bounded(response).await?;
                if is_json {
                    let errors: Vec<RpcError> = serde_json::from_slice(&body)?;
                    return Err(Error::RpcErrors(errors.into()));
                }
                return Err(Error::RpcErrorPlain {
                    description: String::from_utf8_lossy(&body).into_owned(),
                });
            }

            let body = read_bounded(response).await?;
            Ok(serde_json::from_slice(&body)?)
        }
    }

    #[cfg(test)]
    mod tests {
        use httpmock::prelude::*;

        use super::*;

        #[tokio::test]
        async fn error_without_content_type_is_returned_instead_of_panicking() {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(GET).path("/unavailable");
                then.status(503).body("temporarily unavailable");
            });

            let client = HttpClient::new(server.base_url());
            let result = client.get::<serde_json::Value>("/unavailable").await;
            match result {
                Err(Error::RpcErrorPlain { description }) => {
                    assert_eq!(description, "temporarily unavailable");
                }
                other => panic!("expected plain RPC error, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn json_error_with_charset_is_parsed() {
            let server = MockServer::start();
            server.mock(|when, then| {
                when.method(GET).path("/unavailable");
                then.status(503)
                    .header("content-type", "application/json; charset=utf-8")
                    .body(r#"[{"kind":"temporary","id":"node.unavailable"}]"#);
            });

            let client = HttpClient::new(server.base_url());
            let result = client.get::<serde_json::Value>("/unavailable").await;
            assert!(matches!(result, Err(Error::RpcErrors(_))));
        }

        #[tokio::test]
        async fn oversized_response_body_is_rejected() {
            let server = MockServer::start();
            let oversized = vec![b'a'; MAX_RESPONSE_BYTES + 1];
            server.mock(|when, then| {
                when.method(GET).path("/huge");
                then.status(200)
                    .header("content-type", "application/json")
                    .body(oversized);
            });

            let client = HttpClient::new(server.base_url());
            match client.get::<serde_json::Value>("/huge").await {
                Err(Error::ResponseTooLarge { limit }) => assert_eq!(limit, MAX_RESPONSE_BYTES),
                other => panic!("expected response too large error, got {other:?}"),
            }
        }
    }

    #[cfg(feature = "http")]
    #[async_trait]
    impl Http for HttpClient {
        /// Creates an Http client that will be used to send requests to the specified node.
        fn new(rpc_endpoint: String) -> Self {
            let client = Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .read_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(300))
                .build()
                .expect("static client configuration");
            Self {
                rpc_endpoint,
                client,
            }
        }

        fn change_rpc_endpoint(&mut self, rpc_endpoint: String) {
            self.rpc_endpoint = rpc_endpoint;
        }

        /// Convenience method to make a `GET` request to a URL.
        async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, Error> {
            self.handle_response(self.client.get(self.url(url)).send().await?)
                .await
        }

        /// Convenience method to make a `GET` request with query parameters to a URL.
        async fn get_with_query<T: DeserializeOwned, Q: Serialize + ?Sized + Sync>(
            &self,
            url: &str,
            query: &Q,
        ) -> Result<T, Error> {
            self.handle_response(self.client.get(self.url(url)).query(query).send().await?)
                .await
        }

        /// Convenience method to make a `POST` request to a URL.
        async fn post<B: Serialize + Sync, T: DeserializeOwned, Q: Serialize + Sync>(
            &self,
            url: &str,
            body: &B,
            query: Option<&Q>,
        ) -> Result<T, Error> {
            self.handle_response(
                self.client
                    .post(self.url(url))
                    .query(&query)
                    .json(body)
                    .send()
                    .await?,
            )
            .await
        }

        /// Convenience method to make a `PATCH` request to a URL.
        async fn patch<B: Serialize + Sync, T: DeserializeOwned>(
            &self,
            url: &str,
            body: Option<&B>,
        ) -> Result<T, Error> {
            let mut req = self.client.patch(self.url(url));

            if let Some(json) = body {
                req = req.json(json);
            }

            self.handle_response(req.send().await?).await
        }

        /// Convenience method to make a `DELETE` request to a URL.
        async fn delete<B: Serialize + Sync, T: DeserializeOwned>(
            &self,
            url: &str,
            body: Option<&B>,
        ) -> Result<T, Error> {
            let mut req = self.client.delete(self.url(url));

            if let Some(json) = body {
                req = req.json(json);
            }

            self.handle_response(req.send().await?).await
        }
    }
}
