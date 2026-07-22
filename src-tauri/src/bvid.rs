/// B站 BVID ↔ AID 本地互转（纯算法，零网络开销）
///
/// 与 Python 版 bvid_to_aid 算法完全一致。
/// AV 号到 BV 号的转换不在当前功能范围内，仅实现 BVID → AID。
const _BV_XOR: u64 = 23442827791579;
const _BV_MASK: u64 = 2251799813685247;
const _BV_BASE: u64 = 58;
const _BV_TABLE: &str = "FcwAPNKTMug3GV5Lj7EJnHpWsx4tb8haYeviqBz6rkCy12mUSDQX9RdoZf";

/// BVID → AID（作为字符串返回，B站 API 接受字符串形式的 aid）
pub fn bvid_to_aid(bvid: &str) -> Option<String> {
    if !is_valid_bvid(bvid) {
        return None;
    }
    // 取 BV 后的字符（长度可变，通常为 9-10 位）
    let core = &bvid[3..];
    if core.is_empty() {
        return None;
    }
    let mut chars: Vec<char> = core.chars().collect();
    if chars.len() < 7 {
        return None; // 至少需要 7 个字符才能执行 swap(0,6)
    }
    // swap(0, 6), swap(1, 4)
    chars.swap(0, 6);
    chars.swap(1, 4);

    let reverse_table: std::collections::HashMap<char, u64> = _BV_TABLE
        .chars()
        .enumerate()
        .map(|(i, c)| (c, i as u64))
        .collect();

    let mut tmp: u64 = 0;
    for ch in &chars {
        let idx = reverse_table.get(ch)?;
        tmp = tmp.checked_mul(_BV_BASE)?.checked_add(*idx)?;
    }
    let aid = (tmp & _BV_MASK) ^ _BV_XOR;
    Some(aid.to_string())
}

/// 检查字符串是否为合法的 BVID 格式
pub fn is_valid_bvid(bvid: &str) -> bool {
    bvid.starts_with("BV") && bvid.len() >= 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bvid_to_aid_basic() {
        // 验证算法能正常处理合法格式的 BVID
        let aid = bvid_to_aid("BV17x411w7KC");
        assert!(aid.is_some());
        // 应返回纯数字字符串
        let aid_str = aid.unwrap();
        assert!(aid_str.chars().all(|c| c.is_ascii_digit()));
        assert!(!aid_str.is_empty());
    }

    #[test]
    fn test_bvid_roundtrip_deterministic() {
        // 相同输入应产生相同输出
        let a1 = bvid_to_aid("BV17x411w7KC");
        let a2 = bvid_to_aid("BV17x411w7KC");
        assert_eq!(a1, a2);
    }

    #[test]
    fn test_invalid_bvid_short() {
        assert!(bvid_to_aid("BV123").is_none());
    }

    #[test]
    fn test_invalid_no_bv_prefix() {
        assert!(bvid_to_aid("AV12345678").is_none());
    }

    #[test]
    fn test_is_valid_bvid() {
        assert!(is_valid_bvid("BV17x411w7KC"));
        assert!(!is_valid_bvid("AV123"));
        assert!(!is_valid_bvid("BV12"));
    }
}
