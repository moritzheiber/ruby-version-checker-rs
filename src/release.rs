use std::cmp::Ordering;
use std::error::Error;
use std::ops::Range;
use std::process;

use csv::ReaderBuilder;
use regex::Regex;
use reqwest::{Client, Method, Request, Url};
use semver::Version as SemVerVersion;
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};

const MINOR_RANGE: Range<u64> = 1..99;
const PATCH_RANGE: Range<u64> = 0..99;
const RELEASE_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";

#[derive(Debug, Serialize, Deserialize, Eq, Clone)]
pub struct Release {
    #[serde(rename = "name")]
    #[serde(deserialize_with = "parse_semver_version")]
    version: SemVerVersion,
    url: String,
    sha256: String,
}

fn parse_semver_version<'de, D>(deserializer: D) -> Result<SemVerVersion, D::Error>
where
    D: Deserializer<'de>,
{
    let version: String = String::deserialize(deserializer)?;
    let version = version.strip_prefix("ruby-").unwrap();
    version.parse().map_err(D::Error::custom)
}

impl Ord for Release {
    fn cmp(&self, other: &Self) -> Ordering {
        self.version.cmp(&other.version)
    }
}

impl PartialOrd for Release {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Release {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

impl Release {
    fn valid(&self) -> bool {
        is_regular_release(&self.version) && has_tar_gz_url(&self.url)
    }
}

pub fn is_regular_release(r: &SemVerVersion) -> bool {
    r.major == 3
        && MINOR_RANGE.contains(&r.minor)
        && PATCH_RANGE.contains(&r.patch)
        && r.pre.is_empty()
}

fn has_tar_gz_url(u: &str) -> bool {
    let regex = Regex::new(r"https://.*\.tar\.gz$").unwrap();
    regex.is_match(u)
}

pub async fn parse_data(csv: &str) -> Result<Vec<Release>, Box<dyn Error>> {
    let mut result = vec![];
    let mut csv = ReaderBuilder::new()
        .delimiter(b'\t')
        .from_reader(csv.as_bytes());

    for line in csv.deserialize() {
        let item: Release = match line {
            Ok(release) => release,
            Err(_) => continue,
        };
        if item.valid() {
            result.push(item)
        }
    }
    Ok(result)
}

pub async fn latest_versions(versions: Vec<Release>) -> Vec<Release> {
    let mut releases: Vec<Release> = vec![];
    for number in MINOR_RANGE {
        let mut v = versions.clone();
        v.retain(|r| r.version.minor == number);
        v.sort();
        if let Some(r) = v.last() {
            releases.push(r.to_owned())
        }
    }

    releases
}

/// Run the `check` subcommand, printing the latest regular releases as JSON.
pub async fn run_check() {
    let http = Client::builder().https_only(true).build().unwrap();
    let request = Request::new(Method::GET, Url::parse(RELEASE_URL).unwrap());

    let csv = crate::client::fetch_data(request, &http)
        .await
        .expect("Unable to fetch CSV data from the Ruby server");

    let releases = match parse_data(&csv).await {
        Ok(r) => r,
        Err(err) => {
            eprintln!("Error parsing data: {err}");
            process::exit(1);
        }
    };

    let latest = latest_versions(releases).await;
    let json = serde_json::to_string_pretty(&latest)
        .expect("Unable to serialize releases into JSON structure");
    println!("{json}");
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::test_support::{BAD_VERSIONS, GOOD_VERSIONS};
    use rand::RngExt;
    use std::fs;

    struct Data {
        version: &'static str,
        url: &'static str,
    }

    #[test]
    fn validates_good_version() {
        for version in convert_to_versions(good_data()) {
            assert!(version.valid())
        }

        for version in convert_to_versions(good_and_bad_data_with_bad_urls()) {
            assert!(!version.valid())
        }
    }

    #[test]
    fn only_allows_tar_gz_urls() {
        assert!(has_tar_gz_url(good_url()));

        for url in bad_urls() {
            assert!(!has_tar_gz_url(url))
        }
    }

    #[tokio::test]
    async fn parse_correct_csv() {
        let content = fs::read_to_string("test/fixtures/index.txt").unwrap();
        let releases = parse_data(&content).await.unwrap();
        let first: &Release = releases.first().unwrap();
        assert_eq!(
            first.version,
            SemVerVersion {
                major: 3,
                minor: 1,
                patch: 0,
                pre: semver::Prerelease::new("").unwrap(),
                build: semver::BuildMetadata::EMPTY,
            }
        );

        let latest = latest_versions(releases).await;
        assert_eq!(latest.len(), 3);
        assert_eq!(latest[0].version.minor, 1);
        assert_eq!(latest[1].version.minor, 2);
        assert_eq!(latest[2].version.minor, 3);
        assert_eq!(latest[0].version.patch, 4);
        assert_eq!(latest[1].version.patch, 2);
        assert_eq!(latest[2].version.patch, 0);
    }

    #[tokio::test]
    async fn parse_one_line_correctly() {
        let line = "\
name	url	sha1	sha256	sha512
ruby-3.1.1	https://cache.ruby-lang.org/pub/ruby/3.1/ruby-3.1.1.tar.gz	289cbb9eae338bdaf99e376ac511236e39be83a3	fe6e4782de97443978ddba8ba4be38d222aa24dc3e3f02a6a8e7701c0eeb619d	a60d69d35d6d4ad8926b324a6092f962510183d9759b096ba4ce9db2e254e0f436030c2a62741352efe72aec5ca2329b45edd85cca8ad3254a9c57e3d8f66319
";
        let releases = parse_data(line).await.unwrap();
        let release = releases.first().unwrap();
        assert_eq!(
            release.version,
            SemVerVersion {
                major: 3,
                minor: 1,
                patch: 1,
                pre: semver::Prerelease::default(),
                build: semver::BuildMetadata::EMPTY,
            }
        )
    }

    #[tokio::test]
    async fn returns_latest_versions() {
        let releases = convert_to_versions(good_data());
        let latest = latest_versions(releases).await;
        assert_eq!(latest.len(), 3);
        assert_eq!(latest[0].version.minor, 1);
        assert_eq!(latest[1].version.minor, 2);
        assert_eq!(latest[2].version.minor, 3);
        assert_eq!(latest[0].version.patch, 12);
        assert_eq!(latest[1].version.patch, 11);
        assert_eq!(latest[2].version.patch, 12);
    }

    fn convert_to_versions(data: Vec<Data>) -> Vec<Release> {
        let mut releases = vec![];
        for item in data {
            releases.push(Release {
                version: item.version.parse::<SemVerVersion>().unwrap(),
                url: item.url.to_owned(),
                sha256: "sha256".to_string(),
            })
        }
        releases
    }

    fn good_data() -> Vec<Data> {
        GOOD_VERSIONS
            .iter()
            .map(|&version| Data {
                version,
                url: good_url(),
            })
            .collect()
    }

    fn bad_data() -> Vec<Data> {
        BAD_VERSIONS
            .iter()
            .map(|&version| Data {
                version,
                url: good_url(),
            })
            .collect()
    }

    fn good_and_bad_data_with_bad_urls() -> Vec<Data> {
        let mut data = bad_data();
        data.push(Data {
            version: "3.2.0",
            url: one_bad_url(),
        });

        data
    }

    fn good_url() -> &'static str {
        "https://cache.ruby-lang.org/pub/ruby/3.1/ruby-3.1.4.tar.gz"
    }

    fn one_bad_url() -> &'static str {
        let mut rng = rand::rng();
        let urls = bad_urls();
        let index = rng.random_range(0..urls.len());
        urls[index]
    }

    fn bad_urls() -> Vec<&'static str> {
        vec![
            "https://cache.ruby-lang.org/pub/ruby/3.0/ruby-3.0.2.tar.xz",
            "https://cache.ruby-lang.org/pub/ruby/3.0/ruby-3.0.2.zip",
            "https://cache.ruby-lang.org/pub/ruby/2.7/ruby-2.7.6.tar.bz2",
        ]
    }
}
