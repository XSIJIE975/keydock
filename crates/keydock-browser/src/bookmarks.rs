use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::detect::{bookmark_files, BookmarkSource};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub url: String,
    pub folder: String,
    pub browser: String,
    pub profile: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkList {
    pub items: Vec<Bookmark>,
    pub sources: usize,
    pub errors: Vec<String>,
}

/// Read every installed browser's bookmark file. Read-only: no writes and no
/// modification timestamps are touched.
pub fn list(keywords: &[String]) -> BookmarkList {
    let mut result = BookmarkList::default();
    let needles: Vec<String> = keywords
        .iter()
        .map(|keyword| keyword.trim().to_lowercase())
        .filter(|keyword| !keyword.is_empty())
        .collect();

    for source in bookmark_files() {
        result.sources += 1;
        match parse_source(&source, &needles) {
            Ok(mut items) => result.items.append(&mut items),
            Err(error) => result
                .errors
                .push(format!("{} ({}): {error}", source.browser, source.profile)),
        }
    }
    result
}

/// Parse one Chromium `Bookmarks` JSON file.
pub fn parse_source(source: &BookmarkSource, needles: &[String]) -> Result<Vec<Bookmark>, String> {
    let bytes = std::fs::read(&source.path).map_err(|error| error.to_string())?;
    parse_bytes(&bytes, &source.browser, &source.profile, needles)
}

pub fn parse_bytes(
    bytes: &[u8],
    browser: &str,
    profile: &str,
    needles: &[String],
) -> Result<Vec<Bookmark>, String> {
    let root: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("JSON: {error}"))?;
    let Some(roots) = root.get("roots").and_then(|value| value.as_object()) else {
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let mut order = 0_usize;
    for (root_name, node) in roots {
        let folder = match root_name.as_str() {
            "bookmark_bar" => "书签栏".to_owned(),
            "other" => "其他书签".to_owned(),
            "synced" => "移动设备".to_owned(),
            other => other.to_owned(),
        };
        if let Some(children) = node.get("children").and_then(|value| value.as_array()) {
            for child in children {
                collect(
                    child, &folder, browser, profile, needles, &mut seen, &mut order, &mut items,
                );
            }
        }
    }
    Ok(items)
}

#[allow(clippy::too_many_arguments)]
fn collect(
    node: &serde_json::Value,
    folder: &str,
    browser: &str,
    profile: &str,
    needles: &[String],
    seen: &mut HashSet<String>,
    order: &mut usize,
    items: &mut Vec<Bookmark>,
) {
    let node_type = node.get("type").and_then(|value| value.as_str());
    match node_type {
        Some("url") => {
            let url = node
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .trim()
                .to_owned();
            if url.is_empty() {
                return;
            }
            let name = node
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .trim()
                .to_owned();
            if !needles.is_empty() {
                let haystack = format!("{} {}", name.to_lowercase(), url.to_lowercase());
                if !needles.iter().any(|needle| haystack.contains(needle)) {
                    return;
                }
            }
            if !seen.insert(url.clone()) {
                return;
            }
            *order += 1;
            items.push(Bookmark {
                id: format!("{browser}:{profile}:{order}"),
                name: if name.is_empty() { url.clone() } else { name },
                url,
                folder: folder.to_owned(),
                browser: browser.to_owned(),
                profile: profile.to_owned(),
            });
        }
        Some("folder") | None => {
            let name = node
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let child_folder = if name.is_empty() {
                folder.to_owned()
            } else if folder.is_empty() {
                name.to_owned()
            } else {
                format!("{folder} / {name}")
            };
            if let Some(children) = node.get("children").and_then(|value| value.as_array()) {
                for child in children {
                    collect(
                        child,
                        &child_folder,
                        browser,
                        profile,
                        needles,
                        seen,
                        order,
                        items,
                    );
                }
            }
        }
        _ => {}
    }
}

/// Locate bookmark sources without parsing (for diagnostics).
pub fn sources() -> Vec<BookmarkSource> {
    bookmark_files()
}

pub fn source_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "roots": {
        "bookmark_bar": {
          "type": "folder",
          "name": "书签栏",
          "children": [
            {"type": "url", "name": "CSAir SSO", "url": "https://sso.csair.com/login"},
            {"type": "folder", "name": "工作", "children": [
              {"type": "url", "name": "SOM", "url": "https://som.csair.com/"}
            ]}
          ]
        },
        "other": {
          "type": "folder",
          "name": "其他书签",
          "children": [
            {"type": "url", "name": "Example", "url": "https://example.com/"}
          ]
        }
      }
    }"#;

    #[test]
    fn parses_nested_folders_and_deduplicates_urls() {
        let items = parse_bytes(SAMPLE.as_bytes(), "Chrome", "Default", &[]).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].folder, "书签栏 / 工作");
        assert_eq!(items[2].name, "Example");
    }

    #[test]
    fn filters_by_keyword_case_insensitively() {
        let items = parse_bytes(SAMPLE.as_bytes(), "Chrome", "Default", &["csair".into()]).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.url.contains("csair")));
    }

    #[test]
    fn empty_keywords_do_not_filter() {
        let items = parse_bytes(SAMPLE.as_bytes(), "Chrome", "Default", &[]).unwrap();
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn duplicate_urls_are_dropped() {
        let duplicate = r#"{"roots":{"bookmark_bar":{"children":[
            {"type":"url","name":"A","url":"https://a.example/"},
            {"type":"url","name":"A2","url":"https://a.example/"}
        ]}}}"#;
        let items = parse_bytes(duplicate.as_bytes(), "Chrome", "Default", &[]).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn malformed_json_is_reported() {
        assert!(parse_bytes(b"{bad", "Chrome", "Default", &[]).is_err());
    }

    #[test]
    fn missing_roots_returns_empty() {
        assert!(parse_bytes(b"{}", "Chrome", "Default", &[])
            .unwrap()
            .is_empty());
    }
}
