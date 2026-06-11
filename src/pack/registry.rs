//! Registry API 客户端
//!
//! 与 SNAR Registry 服务器通信

use super::http;
use super::json::JsonValue;

/// Registry 客户端
pub struct RegistryClient {
    pub base_url: String,
}

/// 远程包信息
#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub name: String,
    pub description: String,
    pub versions: Vec<String>,
    pub latest: String,
}

impl RegistryClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// 默认 Registry 地址
    pub fn default_url() -> String {
        "http://localhost:7890".to_string()
    }

    /// 获取包信息 (版本列表)
    pub fn get_package(&self, name: &str) -> Result<PackageInfo, String> {
        let url = format!("{}/api/packages/{}", self.base_url, name);
        let resp = http::http_get(&url)?;

        if !resp.is_ok() {
            return Err(format!("Registry: 获取 '{}' 失败 (HTTP {})", name, resp.status));
        }

        let v = JsonValue::parse(&resp.body_str())?;
        let pkg_name = v.get("name").and_then(|v| v.as_str()).unwrap_or(name).to_string();
        let description = v.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let latest = v.get("latest").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let mut versions = Vec::new();
        if let Some(arr) = v.get("versions").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(ver) = item.as_str() {
                    versions.push(ver.to_string());
                }
            }
        }

        Ok(PackageInfo { name: pkg_name, description, versions, latest })
    }

    /// 下载指定版本的 .snar 包
    pub fn download(&self, name: &str, version: &str) -> Result<Vec<u8>, String> {
        let url = format!("{}/api/packages/{}/{}", self.base_url, name, version);
        let resp = http::http_get(&url)?;

        if !resp.is_ok() {
            return Err(format!("Registry: 下载 {}@{} 失败 (HTTP {})", name, version, resp.status));
        }

        Ok(resp.body)
    }

    /// 发布包
    pub fn publish(&self, name: &str, version: &str, metadata: &str, data: &[u8]) -> Result<String, String> {
        // 先用 metadata POST 创建版本
        let url = format!("{}/api/publish", self.base_url);

        // 构建 multipart-like body: metadata + archive
        let payload = JsonValue::Object(vec![
            ("name".to_string(), JsonValue::Str(name.to_string())),
            ("version".to_string(), JsonValue::Str(version.to_string())),
            ("metadata".to_string(), JsonValue::parse(metadata).unwrap_or(JsonValue::Null)),
            ("archive".to_string(), JsonValue::Str(super::archive::hex_encode(data))),
        ]);
        let body = payload.to_json_string();

        let resp = http::http_post(&url, body.as_bytes(), "application/json")?;

        if !resp.is_ok() {
            return Err(format!("Registry: 发布失败 (HTTP {}): {}", resp.status, resp.body_str()));
        }

        Ok(resp.body_str())
    }

    /// 搜索包
    pub fn search(&self, query: &str) -> Result<Vec<PackageInfo>, String> {
        let url = format!("{}/api/search?q={}", self.base_url, http::url_encode(query));
        let resp = http::http_get(&url)?;

        if !resp.is_ok() {
            return Err(format!("Registry: 搜索失败 (HTTP {})", resp.status));
        }

        let v = JsonValue::parse(&resp.body_str())?;
        let mut results = Vec::new();

        if let Some(arr) = v.get("results").and_then(|v| v.as_array()) {
            for item in arr {
                let pkg_name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let desc = item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let latest = item.get("latest").and_then(|v| v.as_str()).unwrap_or("").to_string();
                results.push(PackageInfo {
                    name: pkg_name,
                    description: desc,
                    versions: Vec::new(),
                    latest,
                });
            }
        }

        Ok(results)
    }

    /// 检查 Registry 是否可用
    pub fn ping(&self) -> Result<String, String> {
        let url = format!("{}/api/ping", self.base_url);
        let resp = http::http_get(&url)?;
        if resp.is_ok() {
            Ok("Registry 可用".to_string())
        } else {
            Err(format!("Registry 响应异常 (HTTP {})", resp.status))
        }
    }
}

/// 从环境变量或配置文件获取 Registry URL
pub fn get_registry_url() -> String {
    std::env::var("SNAR_REGISTRY")
        .unwrap_or_else(|_| RegistryClient::default_url())
}
