/// 对 B站 APP 端 API 请求参数进行签名（模拟 BiliDroid 客户端）
///
/// 算法与 Python 版 _app_sign 完全一致：
/// 1. 添加 appkey + ts
/// 2. 按 key 字母序排序
/// 3. URL 编码每个 key=value（safe=''）
/// 4. 用 & 拼接后追加 appsec
/// 5. MD5 哈希得到 sign
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// B站 Android HD 客户端的 appkey 与 appsec
const APP_KEY: &str = "dfca71928277209b";
const APP_SEC: &str = "b5475a8825547a4fc26c7d518eaaa02e";

/// 对参数进行签名，返回包含 appkey / ts / sign 的完整参数 Map
pub fn sign_params(params: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut signed = params.clone();

    // 注入 appkey 与当前时间戳（秒）
    signed.insert("appkey".to_string(), APP_KEY.to_string());
    signed.insert(
        "ts".to_string(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    );

    // 按 key 字母序排序后拼接
    let raw = signed
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // MD5(raw + appsec)
    let sign = md5::compute(format!("{}{}", raw, APP_SEC));
    signed.insert("sign".to_string(), format!("{:x}", sign));

    signed
}

/// 便捷函数：从普通 HashMap 转为签名后的参数
pub fn sign_from_map(params: std::collections::HashMap<String, String>) -> BTreeMap<String, String> {
    let ordered: BTreeMap<String, String> = params.into_iter().collect();
    sign_params(&ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_has_required_keys() {
        let mut params = BTreeMap::new();
        params.insert("vmid".to_string(), "12345".to_string());
        params.insert("ps".to_string(), "20".to_string());
        params.insert("pn".to_string(), "1".to_string());

        let signed = sign_params(&params);

        assert!(signed.contains_key("appkey"));
        assert!(signed.contains_key("ts"));
        assert!(signed.contains_key("sign"));
        assert_eq!(signed.get("vmid"), Some(&"12345".to_string()));
    }

    #[test]
    fn test_sign_deterministic() {
        let mut params = BTreeMap::new();
        params.insert("a".to_string(), "1".to_string());
        params.insert("b".to_string(), "2".to_string());

        let s1 = sign_params(&params);
        let s2 = sign_params(&params);

        assert_eq!(s1.get("sign"), s2.get("sign"));
    }

    #[test]
    fn test_sign_url_encoding() {
        // 验证 URL 编码行为：特殊字符应被百分号编码
        let encoded_a = urlencoding::encode("a=1");
        assert!(encoded_a.contains("%3D"), "= 应被编码为 %3D");
    }
}
