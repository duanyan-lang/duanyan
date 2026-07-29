//! .hardy 打包/解包格式
//!
//! 自定义归档格式:
//! Header: "Hardy" magic(4) + version(u8) + file_count(u32 LE)
//! 每个文件: name_len(u16 LE) + name(UTF-8) + size(u64 LE) + content
//! 末尾: SHA-256 checksum(32 bytes) 覆盖除 checksum 外的全部数据

use std::fs;
use std::path::Path;

const MAGIC: &[u8; 4] = b"HARD";
const FORMAT_VERSION: u8 = 1;

/// 归档中的文件
#[derive(Debug)]
pub struct ArchiveEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// 打包目录为 .hardy 字节流
pub fn pack(dir: &Path) -> Result<Vec<u8>, String> {
    let mut entries: Vec<ArchiveEntry> = Vec::new();
    collect_files(dir, dir, &mut entries)?;

    if entries.is_empty() {
        return Err("目录为空，无法打包".to_string());
    }

    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(MAGIC);
    buf.push(FORMAT_VERSION);
    buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());

    // Entries
    for entry in &entries {
        let name_bytes = entry.name.as_bytes();
        buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        buf.extend_from_slice(name_bytes);
        buf.extend_from_slice(&(entry.data.len() as u64).to_le_bytes());
        buf.extend_from_slice(&entry.data);
    }

    // Checksum
    let checksum = sha256(&buf);
    buf.extend_from_slice(&checksum);

    Ok(buf)
}

/// 解包 .hardy 字节流到目标目录
pub fn unpack(data: &[u8], target: &Path) -> Result<Vec<String>, String> {
    if data.len() < 4 + 1 + 4 + 32 {
        return Err("Hardy 归档: 数据太短".to_string());
    }

    // Verify checksum
    let payload_len = data.len() - 32;
    let expected = sha256(&data[..payload_len]);
    let actual = &data[payload_len..];
    if expected != actual {
        return Err("Hardy 归档: 校验和不匹配 (文件损坏)".to_string());
    }

    // Header
    if &data[0..4] != MAGIC {
        return Err("Hardy 归档: 无效 magic".to_string());
    }
    let version = data[4];
    if version != FORMAT_VERSION {
        return Err(format!("Hardy 归档: 不支持的版本 {}", version));
    }
    let file_count = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;

    let mut pos = 9;
    let mut names = Vec::new();

    for _ in 0..file_count {
        if pos + 2 > payload_len {
            return Err("Hardy 归档: 截断的文件名长度".to_string());
        }
        let name_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + name_len > payload_len {
            return Err("Hardy 归档: 截断的文件名".to_string());
        }
        let name = std::str::from_utf8(&data[pos..pos + name_len])
            .map_err(|_| "Hardy 归档: 无效 UTF-8 文件名")?
            .to_string();
        pos += name_len;

        if pos + 8 > payload_len {
            return Err("Hardy 归档: 截断的文件大小".to_string());
        }
        let mut size_bytes = [0u8; 8];
        size_bytes.copy_from_slice(&data[pos..pos + 8]);
        let size = u64::from_le_bytes(size_bytes) as usize;
        pos += 8;

        if pos + size > payload_len {
            return Err(format!("Hardy 归档: 文件 '{}' 数据截断", name));
        }
        let file_data = &data[pos..pos + size];
        pos += size;

        // Write file
        let file_path = target.join(&name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败: {}", e))?;
        }
        fs::write(&file_path, file_data)
            .map_err(|e| format!("写入文件 '{}' 失败: {}", name, e))?;

        names.push(name);
    }

    Ok(names)
}

/// 递归收集目录中的文件
fn collect_files(base: &Path, dir: &Path, entries: &mut Vec<ArchiveEntry>) -> Result<(), String> {
    let read_dir = fs::read_dir(dir)
        .map_err(|e| format!("读取目录失败: {}", e))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| format!("目录项错误: {}", e))?;
        let path = entry.path();

        // 跳过隐藏文件和 .hardy 目录
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') { continue; }

        if path.is_dir() {
            collect_files(base, &path, entries)?;
        } else if path.is_file() {
            let rel_path = path.strip_prefix(base)
                .map_err(|_| "路径计算错误")?
                .to_string_lossy()
                .replace('\\', "/");

            let data = fs::read(&path)
                .map_err(|e| format!("读取文件 '{}' 失败: {}", rel_path, e))?;

            entries.push(ArchiveEntry {
                name: rel_path,
                data,
            });
        }
    }
    Ok(())
}

/// 计算文件的 SHA-256 校验和
pub fn file_checksum(path: &Path) -> Result<String, String> {
    let data = fs::read(path)
        .map_err(|e| format!("读取文件失败: {}", e))?;
    Ok(hex_encode(&sha256(&data)))
}

/// 字节数组的 SHA-256
pub fn data_checksum(data: &[u8]) -> String {
    hex_encode(&sha256(data))
}

/// 十六进制编码
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// =============================================================================
// 手写 SHA-256 实现 (零依赖)
// =============================================================================

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Pre-processing: padding
    let bit_len = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[4 * i..4 * i + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = sha256(b"");
        let hex = hex_encode(&hash);
        assert_eq!(hex, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn test_sha256_hello() {
        let hash = sha256(b"hello");
        let hex = hex_encode(&hash);
        assert_eq!(hex, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn test_pack_unpack() {
        let dir = std::env::temp_dir().join("hardy_test_archive");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hello.js"), "console.log(42);").unwrap();

        let data = pack(&dir).unwrap();
        assert!(&data[0..4] == MAGIC);

        let out_dir = std::env::temp_dir().join("hardy_test_unpack");
        let _ = fs::remove_dir_all(&out_dir);
        fs::create_dir_all(&out_dir).unwrap();

        let names = unpack(&data, &out_dir).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "hello.js");

        let content = fs::read_to_string(out_dir.join("hello.js")).unwrap();
        assert_eq!(content, "console.log(42);");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&out_dir);
    }
}
