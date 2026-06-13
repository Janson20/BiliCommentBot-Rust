/// 响应体解压模块
///
/// 虽然 reqwest 默认已处理 gzip/deflate/brotli，
/// 但 B站 某些 API 响应可能返回 zlib 压缩（无 gzip header），
/// 需要手动处理 edge case。
use anyhow::{Context, Result};

/// 尝试解压可能被 zlib 压缩的字节数据
///
/// reqwest 默认会自动解压 Content-Encoding: gzip/deflate 的响应，
/// 但某些 B站 API 返回的原始字节是 raw deflate (zlib) 格式，
/// 缺少 header 导致 reqwest 不解压。
/// 此函数作为 fallback 尝试手动解压。
pub fn try_decompress_zlib(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .context("zlib 解压失败")?;
    Ok(decompressed)
}

/// 将字节数据尝试多种方式转为 UTF-8 字符串
pub fn bytes_to_string(data: &[u8]) -> Result<String> {
    // 先尝试直接 UTF-8 解码
    if let Ok(s) = std::str::from_utf8(data) {
        return Ok(s.to_string());
    }
    // 尝试 gzip 解压
    if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        return Ok(String::from_utf8_lossy(&decompressed).to_string());
    }
    // 尝试 zlib 解压
    if let Ok(decompressed) = try_decompress_zlib(data) {
        return Ok(String::from_utf8_lossy(&decompressed).to_string());
    }
    // Fallback: lossy 解码
    Ok(String::from_utf8_lossy(data).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zlib_roundtrip() {
        let original = b"{\"code\":0,\"message\":\"ok\"}";
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = try_decompress_zlib(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_bytes_to_string_plain() {
        let data = b"hello world";
        let s = bytes_to_string(data).unwrap();
        assert_eq!(s, "hello world");
    }
}
