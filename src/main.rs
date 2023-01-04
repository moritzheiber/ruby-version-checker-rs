mod client;
mod release;

use std::{process, str::FromStr};

use reqwest::{Client, Method, Request, Url};

const RELEASE_URL: &str = "https://cache.ruby-lang.org/pub/ruby/index.txt";

#[tokio::main]
async fn main() {
    let mut http = Client::builder().https_only(true).build().unwrap();
    let request = Request::new(Method::GET, Url::from_str(RELEASE_URL).unwrap());

    let csv = client::fetch_data(request, &mut http)
        .await
        .expect("Unable to fetch CSV data from the Ruby server");

    let releases = match release::parse_data(&csv).await {
        Ok(r) => r,
        Err(err) => {
            println!("Error parsing data: {err}");
            process::exit(1);
        }
    };

    let latest_versions = release::latest_versions(releases).await;
    let json = serde_json::to_string_pretty(&latest_versions)
        .expect("Unable to serialize releases into JSON structure");
    println!("{json}")
}
