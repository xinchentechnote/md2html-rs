//! 极简 frontmatter 提取（key: value 字符串/布尔值子集，够用即可，不引 YAML 依赖）。
//! 返回 (meta, body)，meta 键统一小写。

use std::collections::BTreeMap;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    meta: BTreeMap<String, String>,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.meta.get(key).map(String::as_str)
    }

    /// 布尔值语义：'true' / 'false'，其余视为 None
    pub fn bool_of(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some("true") => Some(true),
            Some("false") => Some(false),
            _ => None,
        }
    }
}

/// 剥离文首 `---` 块并解析其中的 key: value 行
pub fn extract_frontmatter(md: &str) -> (Frontmatter, &str) {
    let Some((block_text, rest)) = frontmatter_block(md) else {
        return (Frontmatter::default(), md);
    };
    let mut meta = Frontmatter::default();
    for line in block_text.lines() {
        if let Some((key, value)) = parse_kv(line) {
            meta.meta.insert(key, value);
        }
    }
    (meta, rest)
}

/// 返回 (块内文本, 剩余正文) 与边界；无 frontmatter 时返回 None
fn frontmatter_block(md: &str) -> Option<(&str, &str)> {
    let trimmed = md.strip_prefix('\u{FEFF}').unwrap_or(md);
    let after_dashes = trimmed.strip_prefix("---")?;
    // --- 后必须紧跟换行，否则是分割线/表格语法
    let after_nl = match after_dashes.split_once('\n') {
        Some((first_line, rest)) if first_line.trim_end_matches('\r').is_empty() => rest,
        _ => return None,
    };
    let mut offset = 0usize;
    for line in after_nl.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if bare == "---" {
            // 块内文本是闭合行之前的全部行，正文从闭合行之后开始
            let rest = &after_nl[offset + line.len()..];
            return Some((&after_nl[..offset], rest));
        }
        offset += line.len();
    }
    // 未闭合：与 JS 的非贪婪正则一致地视为无 frontmatter，原文返回
    None
}

/// `key: value` 行解析，键名规则与 JS 一致：[A-Za-z_][A-Za-z0-9_-]*
fn parse_kv(line: &str) -> Option<(String, String)> {
    let (raw_key, raw_value) = line.split_once(':')?;
    let key = raw_key.trim();
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return None,
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    // 与 JS 一致：trim 后剥掉首尾各一个引号
    let value = raw_value.trim();
    let value = value.strip_prefix(['\'', '"']).unwrap_or(value);
    let value = value.strip_suffix(['\'', '"']).unwrap_or(value);
    Some((key.to_ascii_lowercase(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_kv_and_strips_body() {
        let md = "---\ntitle: 你好\ndate: 22 SEP 2026\nfooter: true\nquoted: 'x y'\n---\n\n正文";
        let (fm, body) = extract_frontmatter(md);
        assert_eq!(fm.get("title"), Some("你好"));
        assert_eq!(fm.get("date"), Some("22 SEP 2026"));
        assert_eq!(fm.bool_of("footer"), Some(true));
        assert_eq!(fm.get("quoted"), Some("x y"));
        assert_eq!(body, "\n正文");
    }

    #[test]
    fn no_frontmatter_returns_body_as_is() {
        let (fm, body) = extract_frontmatter("# 标题\n");
        assert!(fm.get("title").is_none());
        assert_eq!(body, "# 标题\n");
    }

    #[test]
    fn unclosed_block_not_treated_as_frontmatter() {
        let md = "---\ntitle: x\n\n正文";
        let (_, body) = extract_frontmatter(md);
        assert_eq!(body, md);
    }

    #[test]
    fn keys_lowercased() {
        let (fm, _) = extract_frontmatter("---\nTitle: a\n---\nb");
        assert_eq!(fm.get("title"), Some("a"));
    }
}
