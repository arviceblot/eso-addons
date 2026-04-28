//! Fetch the top-N ESO addon descriptions and report bbcode tag frequency.
//!
//! Usage:
//!   cargo run -p tools --bin scan_corpus
//!   cargo run -p tools --bin scan_corpus -- 200
//!
//! Caches under `<workspace>/target/scan-corpus-cache/`. The filelist refreshes
//! after 24h; per-addon detail JSON is cached forever (run `cargo clean` to
//! drop everything).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

const FILE_LIST_URL: &str = "https://api.mmoui.com/v3/game/ESO/filelist.json";
const FILE_DETAILS_URL: &str = "https://api.mmoui.com/v3/game/ESO/filedetails/";
const FILELIST_TTL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Deserialize)]
struct FileListItem {
    #[serde(rename = "UID")]
    id: String,
    #[serde(rename = "UIName")]
    name: String,
    #[serde(rename = "UIDownloadTotal")]
    download_total: String,
}

#[derive(Deserialize)]
struct FileDetails {
    #[serde(rename = "UIDescription")]
    description: String,
    #[serde(rename = "UIChangeLog")]
    change_log: String,
}

fn cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("target/scan-corpus-cache")
}

async fn fetch_cached(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    ttl: Option<Duration>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let stale = match (path.exists(), ttl) {
        (false, _) => true,
        (true, None) => false,
        (true, Some(ttl)) => path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age > ttl),
    };
    if stale {
        eprintln!("fetch {url}");
        let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
        std::fs::write(path, &bytes)?;
        return Ok(bytes.to_vec());
    }
    Ok(std::fs::read(path)?)
}

fn count_tags(
    node: &bbcode::Node<'_>,
    addon_id: &str,
    counts: &mut BTreeMap<String, usize>,
    addons: &mut BTreeMap<String, BTreeSet<String>>,
    depth: &mut usize,
    max_depth: &mut usize,
) {
    if let bbcode::Node::Element(e) = node {
        let tag = e.tag.to_ascii_lowercase();
        *counts.entry(tag.clone()).or_default() += 1;
        addons.entry(tag).or_default().insert(addon_id.to_string());
        *depth += 1;
        *max_depth = (*max_depth).max(*depth);
        for c in &e.children {
            count_tags(c, addon_id, counts, addons, depth, max_depth);
        }
        *depth -= 1;
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let top_n: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("top-N must be a number"))
        .unwrap_or(100);

    let cache = cache_dir();
    let details_dir = cache.join("details");
    std::fs::create_dir_all(&details_dir)?;

    let client = reqwest::Client::builder()
        .gzip(true)
        .user_agent(concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let list_bytes = fetch_cached(
        &client,
        FILE_LIST_URL,
        &cache.join("filelist.json"),
        Some(FILELIST_TTL),
    )
    .await?;
    let mut items: Vec<FileListItem> = serde_json::from_slice(&list_bytes)?;
    items.sort_by(|a, b| {
        let a: i64 = a.download_total.parse().unwrap_or(0);
        let b: i64 = b.download_total.parse().unwrap_or(0);
        b.cmp(&a)
    });
    items.truncate(top_n);

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut addons: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut total_bytes = 0usize;
    let mut max_depth = 0usize;

    for (i, item) in items.iter().enumerate() {
        let url = format!("{FILE_DETAILS_URL}{}.json", item.id);
        let path = details_dir.join(format!("{}.json", item.id));
        eprintln!("[{:>3}/{:>3}] {}", i + 1, items.len(), item.name);
        let bytes = fetch_cached(&client, &url, &path, None).await?;
        let arr: Vec<FileDetails> = serde_json::from_slice(&bytes)?;
        let Some(d) = arr.into_iter().next() else {
            continue;
        };
        for src in [&d.description, &d.change_log] {
            total_bytes += src.len();
            let doc = bbcode::parse(src);
            let mut depth = 0usize;
            for node in &doc.children {
                count_tags(node, &item.id, &mut counts, &mut addons, &mut depth, &mut max_depth);
            }
        }
    }

    println!();
    println!("addons:    {}", items.len());
    println!("text size: {} bytes", total_bytes);
    println!("max depth: {}", max_depth);
    println!();
    let mut by_count: Vec<_> = counts.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1));
    for (tag, hits) in by_count.iter().take(40) {
        let n_addons = addons.get(*tag).map_or(0, |s| s.len());
        println!("  {tag:<14} hits={hits:>5}  addons={n_addons:>3}");
    }
    Ok(())
}
