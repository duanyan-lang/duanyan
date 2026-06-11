//! 手写 HTTP/1.1 客户端 (零依赖)
//!
//! 基于 std::net::TcpStream，仅支持 HTTP (非 HTTPS)
//! Registry 在内网/本机运行足够

use std::io::{Read, Write, BufRead, BufReader};
use std::net::TcpStream;
use std::time::Duration;

/// HTTP 响应
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    pub fn is_ok(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}

/// URL 解析结果
struct UrlParts {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<UrlParts, String> {
    let url = url.trim();
    let without_scheme = if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("https://") {
        return Err("HTTPS 不支持，请使用 HTTP".to_string());
    } else {
        url
    };

    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], without_scheme[i..].to_string()),
        None => (without_scheme, "/".to_string()),
    };

    let (host, port) = match host_port.find(':') {
        Some(i) => {
            let h = &host_port[..i];
            let p = host_port[i + 1..].parse::<u16>()
                .map_err(|_| format!("HTTP: 无效端口 '{}'", &host_port[i + 1..]))?;
            (h.to_string(), p)
        }
        None => (host_port.to_string(), 80),
    };

    Ok(UrlParts { host, port, path })
}

/// 发送 HTTP GET 请求
pub fn http_get(url: &str) -> Result<HttpResponse, String> {
    let parts = parse_url(url)?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        parts.path, parts.host
    );
    send_request(&parts.host, parts.port, request.as_bytes())
}

/// 发送 HTTP POST 请求
pub fn http_post(url: &str, body: &[u8], content_type: &str) -> Result<HttpResponse, String> {
    let parts = parse_url(url)?;
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        parts.path, parts.host, content_type, body.len()
    );
    let mut data = request.into_bytes();
    data.extend_from_slice(body);
    send_request(&parts.host, parts.port, &data)
}

/// 发送 HTTP PUT 请求
pub fn http_put(url: &str, body: &[u8], content_type: &str) -> Result<HttpResponse, String> {
    let parts = parse_url(url)?;
    let request = format!(
        "PUT {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        parts.path, parts.host, content_type, body.len()
    );
    let mut data = request.into_bytes();
    data.extend_from_slice(body);
    send_request(&parts.host, parts.port, &data)
}

/// 发送 HTTP DELETE 请求
pub fn http_delete(url: &str) -> Result<HttpResponse, String> {
    let parts = parse_url(url)?;
    let request = format!(
        "DELETE {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        parts.path, parts.host
    );
    send_request(&parts.host, parts.port, request.as_bytes())
}

fn send_request(host: &str, port: u16, data: &[u8]) -> Result<HttpResponse, String> {
    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("HTTP: 无法连接 {}: {}", addr, e))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| format!("HTTP: 设置超时失败: {}", e))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("HTTP: 设置超时失败: {}", e))?;

    stream.write_all(data)
        .map_err(|e| format!("HTTP: 发送请求失败: {}", e))?;
    stream.flush()
        .map_err(|e| format!("HTTP: 刷新失败: {}", e))?;

    read_response(&mut stream)
}

fn read_response(stream: &mut TcpStream) -> Result<HttpResponse, String> {
    let mut reader = BufReader::new(stream);

    // Read status line
    let mut status_line = String::new();
    reader.read_line(&mut status_line)
        .map_err(|e| format!("HTTP: 读取状态行失败: {}", e))?;

    let status_line = status_line.trim_end();
    // HTTP/1.1 200 OK
    let mut parts = status_line.splitn(3, ' ');
    let _version = parts.next().ok_or("HTTP: 无效状态行")?;
    let status: u16 = parts.next()
        .ok_or("HTTP: 缺少状态码")?
        .parse()
        .map_err(|_| "HTTP: 无效状态码")?;
    let status_text = parts.next().unwrap_or("").to_string();

    // Read headers
    let mut headers = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut chunked = false;

    loop {
        let mut line = String::new();
        reader.read_line(&mut line)
            .map_err(|e| format!("HTTP: 读取头部失败: {}", e))?;
        let line = line.trim_end();
        if line.is_empty() { break; }

        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let val = line[colon + 1..].trim().to_string();
            if key == "content-length" {
                content_length = val.parse().ok();
            }
            if key == "transfer-encoding" && val.to_lowercase().contains("chunked") {
                chunked = true;
            }
            headers.push((key, val));
        }
    }

    // Read body
    let body = if chunked {
        read_chunked(&mut reader)?
    } else if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)
            .map_err(|e| format!("HTTP: 读取 body 失败: {}", e))?;
        buf
    } else {
        // Read until connection close
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)
            .map_err(|e| format!("HTTP: 读取 body 失败: {}", e))?;
        buf
    };

    Ok(HttpResponse { status, status_text, headers, body })
}

fn read_chunked(reader: &mut BufReader<&mut TcpStream>) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line)
            .map_err(|e| format!("HTTP chunked: 读取大小失败: {}", e))?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|_| "HTTP chunked: 无效 chunk 大小")?;
        if size == 0 { break; }

        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk)
            .map_err(|e| format!("HTTP chunked: 读取 chunk 失败: {}", e))?;
        body.extend_from_slice(&chunk);

        // Read trailing \r\n
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)
            .map_err(|e| format!("HTTP chunked: 读取 CRLF 失败: {}", e))?;
    }
    Ok(body)
}

/// URL 编码
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(ch),
            ' ' => out.push_str("%20"),
            _ => {
                for byte in ch.to_string().as_bytes() {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_url() {
        let p = parse_url("http://localhost:8080/api/packages").unwrap();
        assert_eq!(p.host, "localhost");
        assert_eq!(p.port, 8080);
        assert_eq!(p.path, "/api/packages");
    }

    #[test]
    fn test_parse_url_default_port() {
        let p = parse_url("http://example.com/test").unwrap();
        assert_eq!(p.host, "example.com");
        assert_eq!(p.port, 80);
        assert_eq!(p.path, "/test");
    }

    #[test]
    fn test_parse_url_no_path() {
        let p = parse_url("http://localhost:3000").unwrap();
        assert_eq!(p.host, "localhost");
        assert_eq!(p.port, 3000);
        assert_eq!(p.path, "/");
    }

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a+b"), "a%2Bb");
    }

    #[test]
    fn test_https_rejected() {
        assert!(parse_url("https://example.com").is_err());
    }
}
