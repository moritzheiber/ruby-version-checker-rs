//! Docker/OCI registry support for counting available regular Ruby releases.

use std::error::Error;
use std::process;
use std::str::FromStr;

use base64::Engine;
use clap::Args;
use reqwest::header::{HeaderValue, AUTHORIZATION, WWW_AUTHENTICATE};
use reqwest::{Client, Method, Request, StatusCode, Url};
use semver::Version;
use serde::Deserialize;

use crate::client::HttpClient;
use crate::release::is_regular_release;

/// The default registry host used when a reference omits one.
const DEFAULT_REGISTRY: &str = "registry-1.docker.io";

/// A container reference split into registry and repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub registry: String,
    pub repository: String,
}

/// Errors produced while parsing a [`Reference`].
#[derive(Debug, PartialEq, Eq)]
pub enum ReferenceError {
    EmptyRepository,
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReferenceError::EmptyRepository => write!(f, "reference is missing a repository"),
        }
    }
}

impl Error for ReferenceError {}

impl FromStr for Reference {
    type Err = ReferenceError;

    fn from_str(reference: &str) -> Result<Self, Self::Err> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(ReferenceError::EmptyRepository);
        }

        // A leading `host/` segment is the registry when it looks like a host.
        let (registry, remainder) = match reference.split_once('/') {
            Some((head, rest)) if is_registry_host(head) => (head.to_string(), rest),
            _ => (DEFAULT_REGISTRY.to_string(), reference),
        };

        // Drop any trailing `:tag` from the final path segment.
        let repository = match remainder.rsplit_once('/') {
            Some((namespace, name)) => {
                let name = name.split_once(':').map_or(name, |(name, _)| name);
                format!("{namespace}/{name}")
            }
            None => remainder
                .split_once(':')
                .map_or(remainder, |(name, _)| name)
                .to_string(),
        };

        if repository.is_empty() {
            return Err(ReferenceError::EmptyRepository);
        }

        // Docker Hub official images live under the `library/` namespace.
        let repository = if registry == DEFAULT_REGISTRY && !repository.contains('/') {
            format!("library/{repository}")
        } else {
            repository
        };

        Ok(Reference {
            registry,
            repository,
        })
    }
}

/// Whether a reference's first segment denotes a registry host rather than a path.
fn is_registry_host(segment: &str) -> bool {
    segment == "localhost" || segment.contains('.') || segment.contains(':')
}

/// A parsed `WWW-Authenticate: Bearer ...` challenge from a `401` response.
#[derive(Debug, PartialEq, Eq)]
struct BearerChallenge {
    realm: String,
    service: Option<String>,
    scope: Option<String>,
}

/// Parse a Bearer `WWW-Authenticate` challenge; `None` if not a Bearer scheme.
fn parse_www_authenticate(header: &str) -> Option<BearerChallenge> {
    let params = header.trim().strip_prefix("Bearer ")?;

    let mut realm = None;
    let mut service = None;
    let mut scope = None;
    for part in params.split(',') {
        let (key, value) = part.split_once('=')?;
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "realm" => realm = Some(value),
            "service" => service = Some(value),
            "scope" => scope = Some(value),
            _ => {}
        }
    }

    Some(BearerChallenge {
        realm: realm?,
        service,
        scope,
    })
}

/// Registry credentials from the CLI or `REGISTRY_USERNAME`/`REGISTRY_PASSWORD`.
#[derive(Debug, Clone)]
pub struct Credentials {
    username: String,
    password: String,
}

impl Credentials {
    /// Build credentials from optional parts; both are required.
    pub fn new(username: Option<String>, password: Option<String>) -> Option<Self> {
        match (username, password) {
            (Some(username), Some(password)) => Some(Self { username, password }),
            _ => None,
        }
    }
}

/// The `tags/list` response body.
#[derive(Deserialize)]
struct TagList {
    tags: Vec<String>,
}

/// The token endpoint response body.
#[derive(Deserialize)]
struct TokenResponse {
    #[serde(alias = "access_token")]
    token: String,
}

/// The repository tags that resolve to a regular Ruby release, sorted by version.
fn regular_release_tags(tags: &[String], allowed_suffixes: &[String]) -> Vec<String> {
    let mut matched: Vec<(Version, String)> = tags
        .iter()
        .filter_map(|tag| {
            version_from_tag(tag, allowed_suffixes)
                .filter(is_regular_release)
                .map(|version| (version, tag.clone()))
        })
        .collect();
    matched.sort();
    matched.into_iter().map(|(_, tag)| tag).collect()
}

/// Extract a version from a tag, stripping leading decoration and allowed suffixes.
fn version_from_tag(tag: &str, allowed_suffixes: &[String]) -> Option<Version> {
    let start = tag.find(|c: char| c.is_ascii_digit())?;
    let rest = &tag[start..];

    let core_len = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let (core, tail) = rest.split_at(core_len);

    if !tail.is_empty() {
        let suffix = tail.strip_prefix('-')?;
        if !allowed_suffixes.iter().any(|allowed| allowed == suffix) {
            return None;
        }
    }

    core.parse().ok()
}

/// Build a bare `GET` request for `url`.
fn get_request(url: &str) -> Result<Request, Box<dyn Error>> {
    Ok(Request::new(Method::GET, Url::parse(url)?))
}

/// A registry client bound to a single repository reference.
pub struct Registry {
    reference: Reference,
    credentials: Option<Credentials>,
}

impl Registry {
    pub fn new(reference: Reference, credentials: Option<Credentials>) -> Self {
        Self {
            reference,
            credentials,
        }
    }

    /// List every tag, completing the OCI bearer-token handshake.
    pub async fn tags<C>(&self, client: &C) -> Result<Vec<String>, Box<dyn Error>>
    where
        C: HttpClient,
    {
        let url = format!(
            "https://{}/v2/{}/tags/list",
            self.reference.registry, self.reference.repository
        );

        let mut response = client.send_request(get_request(&url)?).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let challenge = response
                .headers()
                .get(WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok())
                .and_then(parse_www_authenticate)
                .ok_or("registry did not return a Bearer challenge")?;
            let token = self.fetch_token(&challenge, client).await?;

            let mut request = get_request(&url)?;
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {token}"))?,
            );
            response = client.send_request(request).await?;
        }

        let body = response.error_for_status()?.text().await?;
        let list: TagList = serde_json::from_str(&body)?;
        Ok(list.tags)
    }

    /// The repository tags that resolve to a regular Ruby release.
    pub async fn available_tags<C>(
        &self,
        client: &C,
        allowed_suffixes: &[String],
    ) -> Result<Vec<String>, Box<dyn Error>>
    where
        C: HttpClient,
    {
        let tags = self.tags(client).await?;
        Ok(regular_release_tags(&tags, allowed_suffixes))
    }

    /// Exchange a Bearer challenge for an access token, using Basic auth if present.
    async fn fetch_token<C>(
        &self,
        challenge: &BearerChallenge,
        client: &C,
    ) -> Result<String, Box<dyn Error>>
    where
        C: HttpClient,
    {
        let mut url = Url::parse(&challenge.realm)?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(service) = &challenge.service {
                query.append_pair("service", service);
            }
            if let Some(scope) = &challenge.scope {
                query.append_pair("scope", scope);
            }
        }

        let mut request = Request::new(Method::GET, url);
        if let Some(credentials) = &self.credentials {
            let raw = format!("{}:{}", credentials.username, credentials.password);
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Basic {encoded}"))?,
            );
        }

        let body = client
            .send_request(request)
            .await?
            .error_for_status()?
            .text()
            .await?;
        let token: TokenResponse = serde_json::from_str(&body)?;
        Ok(token.token)
    }
}

/// CLI arguments for the `docker` subcommand.
#[derive(Args)]
pub struct DockerArgs {
    /// Container reference, e.g. `library/ruby` or `ghcr.io/acme/ruby-runtime`.
    reference: String,

    /// Comma-separated tag suffixes to treat as the base version (e.g. `slim,bookworm`).
    #[arg(long, value_delimiter = ',', env = "REGISTRY_ALLOW_SUFFIX")]
    allow_suffix: Vec<String>,

    /// Registry username.
    #[arg(long, env = "REGISTRY_USERNAME")]
    username: Option<String>,

    /// Registry password.
    #[arg(long, env = "REGISTRY_PASSWORD")]
    password: Option<String>,
}

/// Run the `docker` subcommand, printing the available regular releases as JSON.
pub async fn run(args: DockerArgs) {
    let reference: Reference = match args.reference.parse() {
        Ok(reference) => reference,
        Err(err) => {
            eprintln!("Invalid reference: {err}");
            process::exit(1);
        }
    };

    let credentials = Credentials::new(args.username, args.password);
    let registry = Registry::new(reference, credentials);
    let http = Client::builder().https_only(true).build().unwrap();

    let tags = match registry.available_tags(&http, &args.allow_suffix).await {
        Ok(tags) => tags,
        Err(err) => {
            eprintln!("Error querying registry: {err}");
            process::exit(1);
        }
    };

    let json =
        serde_json::to_string_pretty(&tags).expect("Unable to serialize tags into JSON structure");
    println!("{json}");
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::test_support::{BAD_VERSIONS, GOOD_VERSIONS};
    use async_trait::async_trait;
    use http::response::Response as HttpResponse;
    use reqwest::{header, Error as ReqwestError, Request, Response, StatusCode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    // --- Reference parsing ---------------------------------------------------

    #[test]
    fn parses_official_docker_hub_reference() {
        let reference: Reference = "ruby".parse().unwrap();

        assert_eq!(reference.registry, DEFAULT_REGISTRY);
        assert_eq!(reference.repository, "library/ruby");
    }

    #[test]
    fn parses_namespaced_docker_hub_reference() {
        let reference: Reference = "team/ruby-images".parse().unwrap();

        assert_eq!(reference.registry, DEFAULT_REGISTRY);
        assert_eq!(reference.repository, "team/ruby-images");
    }

    #[test]
    fn parses_ghcr_reference() {
        let reference: Reference = "ghcr.io/acme/ruby-runtime".parse().unwrap();

        assert_eq!(reference.registry, "ghcr.io");
        assert_eq!(reference.repository, "acme/ruby-runtime");
    }

    #[test]
    fn parses_custom_registry_with_port() {
        let reference: Reference = "registry.example.com:5000/team/img".parse().unwrap();

        assert_eq!(reference.registry, "registry.example.com:5000");
        assert_eq!(reference.repository, "team/img");
    }

    #[test]
    fn ignores_a_trailing_tag() {
        let reference: Reference = "library/ruby:3.3.6".parse().unwrap();

        assert_eq!(reference.registry, DEFAULT_REGISTRY);
        assert_eq!(reference.repository, "library/ruby");
    }

    #[test]
    fn rejects_empty_reference() {
        assert_eq!(
            "".parse::<Reference>(),
            Err(ReferenceError::EmptyRepository)
        );
    }

    // --- WWW-Authenticate parsing -------------------------------------------

    #[test]
    fn parses_bearer_challenge() {
        let header = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/ruby:pull""#;

        let challenge = parse_www_authenticate(header).unwrap();

        assert_eq!(challenge.realm, "https://auth.docker.io/token");
        assert_eq!(challenge.service.as_deref(), Some("registry.docker.io"));
        assert_eq!(
            challenge.scope.as_deref(),
            Some("repository:library/ruby:pull")
        );
    }

    #[test]
    fn ignores_non_bearer_challenge() {
        assert!(parse_www_authenticate(r#"Basic realm="registry""#).is_none());
    }

    // --- Tag selection -------------------------------------------------------

    #[test]
    fn keeps_only_bare_regular_releases_by_default() {
        let mut tags: Vec<String> = GOOD_VERSIONS
            .iter()
            .chain(BAD_VERSIONS)
            .map(|v| v.to_string())
            .collect();
        tags.push("3.3".to_string()); // not a major.minor.patch version
        tags.push("latest".to_string()); // not a version

        let result = regular_release_tags(&tags, &[]);

        assert_eq!(result, expected_good_tags());
    }

    #[test]
    fn strips_leading_decoration_before_the_version() {
        let decorations = ["ruby-", "ruby_", "v", ""];
        let tags: Vec<String> = GOOD_VERSIONS
            .iter()
            .enumerate()
            .map(|(i, version)| format!("{}{version}", decorations[i % decorations.len()]))
            .collect();

        let result = regular_release_tags(&tags, &[]);

        // Every decorated tag qualifies; the real tag is kept, ordered by version.
        let mut expected = tags.clone();
        expected.sort_by_key(|tag| version_from_tag(tag, &[]).unwrap());
        assert_eq!(result, expected);
    }

    #[test]
    fn lists_tags_with_allowed_suffixes() {
        let allowed = to_strings(&["slim", "bookworm", "ubuntu24.04"]);
        let good_tags: Vec<String> = GOOD_VERSIONS
            .iter()
            .enumerate()
            .map(|(i, version)| format!("{version}-{}", allowed[i % allowed.len()]))
            .collect();
        let mut tags = good_tags.clone();
        // Bad versions (bare non-regular, or disallowed suffixes) stay excluded.
        tags.extend(BAD_VERSIONS.iter().map(|version| version.to_string()));

        let result = regular_release_tags(&tags, &allowed);

        let mut expected = good_tags;
        expected.sort_by_key(|tag| version_from_tag(tag, &allowed).unwrap());
        assert_eq!(result, expected);
    }

    #[test]
    fn includes_base_and_allowed_variant_tags() {
        let base = GOOD_VERSIONS[0];
        let allowed = to_strings(&["slim", "bookworm"]);
        let tags = vec![
            base.to_string(),
            format!("{base}-slim"),
            format!("ruby-{base}-bookworm"),
        ];

        let result = regular_release_tags(&tags, &allowed);

        // All three tags share one version and are all returned, ordered by tag.
        let mut expected = tags.clone();
        expected.sort();
        assert_eq!(result, expected);
    }

    #[test]
    fn excludes_bad_versions_by_default() {
        let mut tags: Vec<String> = BAD_VERSIONS
            .iter()
            .map(|version| version.to_string())
            .collect();
        tags.push("3.3".to_string());
        tags.push("latest".to_string());

        let result = regular_release_tags(&tags, &[]);

        assert!(result.is_empty());
    }

    // --- Credentials ---------------------------------------------------------

    #[test]
    fn credentials_require_both_username_and_password() {
        assert!(Credentials::new(Some("u".into()), Some("p".into())).is_some());
        assert!(Credentials::new(Some("u".into()), None).is_none());
        assert!(Credentials::new(None, Some("p".into())).is_none());
        assert!(Credentials::new(None, None).is_none());
    }

    // --- Registry handshake (mocked HTTP) ------------------------------------

    #[tokio::test]
    async fn tags_completes_anonymous_bearer_handshake() {
        let reference: Reference = "library/ruby".parse().unwrap();
        let registry = Registry::new(reference, None);
        let client = MockRegistry::default();

        let tags = registry.tags(&client).await.unwrap();

        assert!(tags.contains(&GOOD_VERSIONS[0].to_string()));
        assert!(client.tags_served_authenticated.load(Ordering::SeqCst));
        assert!(client.token_authorization.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn tags_sends_basic_auth_when_credentials_present() {
        let reference: Reference = "library/ruby".parse().unwrap();
        let credentials = Credentials {
            username: "user".into(),
            password: "pass".into(),
        };
        let registry = Registry::new(reference, Some(credentials));
        let client = MockRegistry::default();

        registry.tags(&client).await.unwrap();

        // base64("user:pass") == "dXNlcjpwYXNz"
        assert_eq!(
            client.token_authorization.lock().unwrap().as_deref(),
            Some("Basic dXNlcjpwYXNz")
        );
    }

    #[tokio::test]
    async fn available_tags_lists_regular_releases() {
        let reference: Reference = "library/ruby".parse().unwrap();
        let registry = Registry::new(reference, None);
        let client = MockRegistry::default();

        let tags = registry.available_tags(&client, &[]).await.unwrap();

        assert_eq!(tags, expected_good_tags());
    }

    // --- Mock registry -------------------------------------------------------

    /// Mock registry driving the `401` -> token -> authenticated retry handshake.
    #[derive(Default)]
    struct MockRegistry {
        token_authorization: Mutex<Option<String>>,
        tags_served_authenticated: AtomicBool,
    }

    #[async_trait]
    impl HttpClient for MockRegistry {
        async fn send_request(&self, request: Request) -> Result<Response, ReqwestError> {
            let path = request.url().path().to_string();

            if path.ends_with("/tags/list") {
                if request.headers().contains_key(header::AUTHORIZATION) {
                    self.tags_served_authenticated.store(true, Ordering::SeqCst);
                    return Ok(json_response(StatusCode::OK, tags_body()));
                }

                let challenge = r#"Bearer realm="https://auth.example.test/token",service="example",scope="repository:library/ruby:pull""#;
                return Ok(challenge_response(challenge));
            }

            // Token endpoint.
            if let Some(auth) = request.headers().get(header::AUTHORIZATION) {
                *self.token_authorization.lock().unwrap() =
                    Some(auth.to_str().unwrap().to_string());
            }
            Ok(json_response(StatusCode::OK, token_body()))
        }
    }

    /// A `tags/list` body built from the shared version fixtures plus noise.
    fn tags_body() -> String {
        let tags: Vec<String> = GOOD_VERSIONS
            .iter()
            .chain(BAD_VERSIONS)
            .map(|version| version.to_string())
            .chain(["3.3".to_string(), "latest".to_string()])
            .collect();
        serde_json::json!({ "name": "library/ruby", "tags": tags }).to_string()
    }

    fn token_body() -> String {
        serde_json::json!({ "token": "test-bearer-token" }).to_string()
    }

    fn json_response(status: StatusCode, body: String) -> Response {
        Response::from(HttpResponse::builder().status(status).body(body).unwrap())
    }

    fn challenge_response(www_authenticate: &str) -> Response {
        Response::from(
            HttpResponse::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::WWW_AUTHENTICATE, www_authenticate)
                .body(String::new())
                .unwrap(),
        )
    }

    fn to_strings(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| t.to_string()).collect()
    }

    fn expected_good_tags() -> Vec<String> {
        let mut versions: Vec<Version> = GOOD_VERSIONS.iter().map(|v| v.parse().unwrap()).collect();
        versions.sort();
        versions.iter().map(|v| v.to_string()).collect()
    }
}
