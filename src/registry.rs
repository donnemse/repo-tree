use reqwest::Error;
use serde::Deserialize;
use serde_json::Value;
use std::error::Error as StdError;
use std::sync::Mutex;
use lazy_static::lazy_static;
use std::collections::HashMap;
use crate::ui::CompatibilityRow;

lazy_static! {
    static ref REGISTRY_URL: Mutex<String> = Mutex::new("http://172.16.88.137:30353/v2/".to_string());
}

pub fn set_registry_url(url: &str) {
    let mut registry_url = REGISTRY_URL.lock().unwrap();
    *registry_url = format!("{}/v2/", url.trim_end_matches('/'));
}

// 현재 설정된 URL을 반환하는 함수
pub fn get_registry_url() -> String {
    REGISTRY_URL.lock().unwrap().clone()
}

#[derive(Debug, Deserialize)]
struct CatalogResponse {
    repositories: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    // Manifest 구조체에 필요한 필드를 정의하세요
    // 예: schemaVersion, config, layers 등
}
// JSON 데이터를 받아오는 manifest 구조체 정의
pub async fn fetch_manifest(image: &str, tag: &str) -> Result<String, Box<dyn StdError>> {
    let url = format!("{}{}/manifests/{}", get_registry_url(), image, tag);
    let client = reqwest::Client::new();

    // API 호출
    let resp = client.get(&url).send().await?;

    // JSON 데이터를 Value 형태로 직접 반환
    let manifest: Value = resp.json().await?;
    Ok(serde_json::to_string_pretty(&manifest)?) // 포맷된 JSON 문자열 반환
}


pub async fn fetch_images() -> Result<Vec<String>, Error> {
    let url = format!("{}{}", get_registry_url(), "_catalog");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    // `CatalogResponse`로 응답을 디코딩합니다.
    let catalog: CatalogResponse = resp.json().await?;
    Ok(catalog.repositories)
}

pub async fn fetch_tags(image: &str) -> Result<Vec<String>, Error> {
    let url = format!("{}{}/tags/list", get_registry_url(), image);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await?;

    let tags_response: TagsResponse = resp.json().await?;
    Ok(tags_response.tags.unwrap_or_else(Vec::new))
}

/// Docker 이미지 이름을 1뎁스와 2뎁스 부분으로 분리합니다.
/// 예: "xxxx/yyyy/zzzz" -> ("xxxx", "yyyy/zzzz")
fn split_image_depths(image_name: &str) -> (&str, &str) {
    let parts: Vec<&str> = image_name.splitn(2, '/').collect();
    if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        (image_name, "") // 형식이 다를 경우 전체를 1뎁스로 간주
    }
}

pub fn group_images_by_depth(images: Vec<String>) -> HashMap<String, Vec<String>> {
    let mut grouped_images: HashMap<String, Vec<String>> = HashMap::new();

    for image in images {
        let (depth1, depth2) = split_image_depths(&image);
        grouped_images.entry(depth1.to_string()).or_default().push(depth2.to_string());
    }

    grouped_images
}

pub fn parse_v1compatibility_fields(manifest: &Value) -> (Vec<CompatibilityRow>, String) {
    let empty_vec = vec![];
    let history = manifest.get("history").and_then(|h| h.as_array()).unwrap_or(&empty_vec);
    let mut table_data = Vec::new();

    for entry in history {
        if let Some(v1compat_str) = entry.get("v1Compatibility").and_then(|v| v.as_str()) {
            if let Ok(v1compat_json) = serde_json::from_str::<Value>(v1compat_str) {
                // 필요한 필드 추출
                let id = v1compat_json.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let parent = v1compat_json.get("parent").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let os = v1compat_json.get("os").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let created = v1compat_json.get("created").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let cmd = v1compat_json.get("container_config")
                    .and_then(|cc| cc.get("Cmd"))
                    .and_then(|cmd_array| {
                        cmd_array.as_array().map(|array| {
                            array.iter()
                                .filter_map(|v| v.as_str()) // 배열의 각 요소를 문자열로 변환
                                .collect::<Vec<&str>>()
                                .join("\n") // 공백으로 결합
                        })
                    })
                    .unwrap_or_default();
                let config = v1compat_json.get("config").map(|v| v.to_string()).unwrap_or_default();

                // 필드 값을 CompatibilityRow 형식으로 저장
                table_data.push(CompatibilityRow {
                    id: id.chars().take(8).collect(),
                    parent: parent.chars().take(8).collect(),
                    os,
                    created,
                    cmd,
                    config,
                });
            }
        }
    }

    let full_json_string = serde_json::to_string_pretty(&manifest).unwrap_or_default();

    (table_data, full_json_string)
}