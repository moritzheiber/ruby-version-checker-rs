use async_trait::async_trait;
use reqwest::{Error, Request, Response};

pub async fn fetch_data<C>(request: Request, client: &mut C) -> Result<String, Error>
where
    C: HttpClient,
{
    client.send_request(request).await.map(|r| r.text())?.await
}

#[async_trait]
pub trait HttpClient {
    async fn send_request(&mut self, request: Request) -> Result<Response, Error>;
}

#[async_trait]
impl HttpClient for reqwest::Client {
    async fn send_request(&mut self, request: Request) -> Result<Response, Error> {
        self.execute(request).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use http::response::Response as HttpResponse;
    use reqwest::{Method, StatusCode, Url};
    use std::{fs, str::FromStr};

    struct MockClient {}

    #[async_trait]
    impl HttpClient for MockClient {
        async fn send_request(&mut self, _request: Request) -> Result<Response, Error> {
            let content = fs::read_to_string("test/fixtures/index.txt").unwrap();
            let response = HttpResponse::builder()
                .status(StatusCode::OK)
                .body(content)
                .unwrap();
            Ok(Response::from(response))
        }
    }

    #[tokio::test]
    async fn fetch_raw_data() {
        let mut client = MockClient {};
        let url = Url::from_str("https://some.url").unwrap();
        let request = Request::new(Method::GET, url);
        let data = fetch_data(request, &mut client).await.unwrap();
        let releases = crate::release::parse_data(&data).await.unwrap();

        assert!(releases.first().is_some());
    }
}
