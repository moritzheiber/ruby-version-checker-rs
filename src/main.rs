use std::error::Error;
use std::process;

use csv::ReaderBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};

const RELEASE_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";

#[derive(Debug, Serialize, Deserialize)]
struct Version {
    name: String,
    url: String,
    sha256: String,
}

impl Version {
    fn valid(&self) -> bool {
        is_regular_release(&self.name) && has_tar_gz_url(&self.url)
    }
}

fn is_regular_release(r: &str) -> bool {
    let regex = Regex::new(r"^ruby-3\.\d{1,2}\.\d{1,2}$").unwrap();
    regex.is_match(r)
}

fn has_tar_gz_url(u: &str) -> bool {
    let regex = Regex::new(r"https://.*\.tar\.gz$").unwrap();
    regex.is_match(u)
}

async fn parse_data<'a>(csv: &'a str) -> Result<Vec<Version>, Box<dyn Error>> {
    let mut result = vec![];
    let mut csv = ReaderBuilder::new()
        .delimiter(b'\t')
        .from_reader(csv.as_bytes());

    for line in csv.deserialize() {
        let item: Version = line?;
        if item.valid() {
            result.push(item)
        }
    }
    Ok(result)
}

async fn latest_versions(versions: Vec<Version>) -> Vec<Version> {
    vec![]
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
    use std::fs;

    use crate::{has_tar_gz_url, is_regular_release, latest_versions, parse_data, Version};
    use csv::ReaderBuilder;
    use rand::prelude::*;

    struct Release {
        name: &'static str,
        url: &'static str,
    }

    #[test]
    fn parses_regular_release() {
        let good = good_releases();
        let bad = bad_releases();

        for release in good {
            assert!(is_regular_release(release.name))
        }

        for release in bad {
            assert!(!is_regular_release(release.name))
        }
    }

    #[test]
    fn validates_good_version() {
        for version in convert_to_versions(good_releases()) {
            assert!(version.valid())
        }

        for version in convert_to_versions(good_and_bad_releases_with_bad_urls()) {
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
        let versions = parse_data(&content).await.unwrap();
        let first: &Version = versions.first().unwrap();
        assert_eq!(first.name, "ruby-3.0.0".to_string())
    }

    #[test]
    fn parse_fixture() {
        let mut csv = ReaderBuilder::new()
            .delimiter(b'\t')
            .from_path("test/fixtures/index.txt")
            .unwrap();
        for result in csv.deserialize() {
            let _: Version = result.unwrap();
        }
    }

    #[tokio::test]
    async fn parse_one_line_correctly() {
        let line = "\
name	url	sha1	sha256	sha512
ruby-3.1.1	https://cache.ruby-lang.org/pub/ruby/3.1/ruby-3.1.1.tar.gz	289cbb9eae338bdaf99e376ac511236e39be83a3	fe6e4782de97443978ddba8ba4be38d222aa24dc3e3f02a6a8e7701c0eeb619d	a60d69d35d6d4ad8926b324a6092f962510183d9759b096ba4ce9db2e254e0f436030c2a62741352efe72aec5ca2329b45edd85cca8ad3254a9c57e3d8f66319
";
        let versions = parse_data(line).await.unwrap();
        let version = versions.first().unwrap();
        assert_eq!(version.name, "ruby-3.1.1")
    }

    #[tokio::test]
    async fn returns_latest_versions() {
        let versions = convert_to_versions(good_releases());
        let latest = latest_versions(versions).await;
        assert_eq!(latest.len(), 3)
    }

    fn convert_to_versions(releases: Vec<Release>) -> Vec<Version> {
        let mut versions = vec![];
        for release in releases {
            versions.push(Version {
                name: release.name.to_owned(),
                url: release.url.to_owned(),
                sha256: "sha256".to_string(),
            })
        }
        versions
    }

    fn good_releases() -> Vec<Release> {
        let mut releases = vec![];
        for (name, url) in &[
            ("ruby-3.2.0", good_url()),
            ("ruby-3.2.11", good_url()),
            ("ruby-3.2.2", good_url()),
            ("ruby-3.1.0", good_url()),
            ("ruby-3.1.12", good_url()),
            ("ruby-3.0.5", good_url()),
            ("ruby-3.0.16", good_url()),
        ] {
            releases.push(Release { name, url })
        }

        releases
    }

    fn bad_releases() -> Vec<Release> {
        let mut releases = vec![];
        for (name, url) in &[
            ("ruby-2.7.0", one_bad_url()),
            ("ruby-3.2.0-preview1", one_bad_url()),
            ("ruby-3.2.0-rc2", one_bad_url()),
            ("ruby-3.1.5-something", one_bad_url()),
        ] {
            releases.push(Release { name, url })
        }
        releases
    }

    fn good_and_bad_releases_with_bad_urls() -> Vec<Release> {
        let mut releases = bad_releases();
        releases.push(Release {
            name: "3.2.0",
            url: one_bad_url(),
        });

        releases
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
