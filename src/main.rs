use std::cmp::Ordering;
use std::error::Error;
use std::ops::Range;
use std::process;

use csv::ReaderBuilder;
use regex::Regex;
use semver::Version as SemVerVersion;
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};

const RELEASE_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";
const VERSION_RANGE: Range<u64> = 0..99;

#[derive(Debug, Serialize, Deserialize, Eq, Clone)]
struct Release {
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

fn is_regular_release(r: &SemVerVersion) -> bool {
    r.major == 3
        && VERSION_RANGE.contains(&r.minor)
        && VERSION_RANGE.contains(&r.patch)
        && r.pre.is_empty()
}

fn has_tar_gz_url(u: &str) -> bool {
    let regex = Regex::new(r"https://.*\.tar\.gz$").unwrap();
    regex.is_match(u)
}

async fn parse_data<'a>(csv: &'a str) -> Result<Vec<Release>, Box<dyn Error>> {
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

async fn latest_versions(versions: Vec<Release>) -> Vec<Release> {
    let mut releases: Vec<Release> = vec![];
    for number in VERSION_RANGE {
        let mut v = versions.clone();
        v.retain(|r| r.version.minor == number);
        v.sort();
        if let Some(r) = v.last() {
            releases.push(r.to_owned())
        }
    }

    releases
}

#[tokio::main]
async fn main() {
    if let Err(err) = parse_data("csv").await {
        println!("Error parsing data: {}", err);
        process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use rand::prelude::*;
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
                minor: 0,
                patch: 0,
                pre: semver::Prerelease::new("").unwrap(),
                build: semver::BuildMetadata::EMPTY,
            }
        );

        let latest = latest_versions(releases).await;
        assert_eq!(latest.len(), 3);
        assert_eq!(latest[0].version.minor, 0);
        assert_eq!(latest[1].version.minor, 1);
        assert_eq!(latest[2].version.minor, 2);
        assert_eq!(latest[0].version.patch, 5);
        assert_eq!(latest[1].version.patch, 3);
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
        assert_eq!(latest[0].version.minor, 0);
        assert_eq!(latest[1].version.minor, 1);
        assert_eq!(latest[2].version.minor, 2);
        assert_eq!(latest[0].version.patch, 16);
        assert_eq!(latest[1].version.patch, 12);
        assert_eq!(latest[2].version.patch, 11);
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
        let mut releases = vec![];
        for (version, url) in &[
            ("3.2.0", good_url()),
            ("3.2.11", good_url()),
            ("3.2.2", good_url()),
            ("3.1.0", good_url()),
            ("3.1.12", good_url()),
            ("3.0.5", good_url()),
            ("3.0.16", good_url()),
        ] {
            releases.push(Data { version, url })
        }

        releases
    }

    fn bad_data() -> Vec<Data> {
        let mut data = vec![];
        for (version, url) in &[
            ("2.7.0", one_bad_url()),
            ("3.2.0-preview1", one_bad_url()),
            ("3.2.0-rc2", one_bad_url()),
            ("3.1.5-something", one_bad_url()),
        ] {
            data.push(Data { version, url })
        }
        data
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
        "https://cache.ruby-lang.org/pub/ruby/3.0/ruby-3.0.2.tar.gz"
    }

    fn one_bad_url() -> &'static str {
        let mut rng = rand::thread_rng();
        let urls = bad_urls();
        let index = rng.gen_range(0..urls.len());
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
