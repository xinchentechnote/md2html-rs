//! 纯工具函数：HTML 转义、内联 style 序列化、日期格式化、头尾块剥离、节点取文本。
//! 与 JS 版 utils.js/parse.js 一一对应，core 其余模块只依赖这里。

use markdown::mdast::Node;

/// 节点内全部纯文本（用于标题等）
pub fn node_text(node: &Node) -> String {
    match node {
        Node::Text(t) => t.value.clone(),
        other => other
            .children()
            .map(|children| children.iter().map(node_text).collect::<String>())
            .unwrap_or_default(),
    }
}

/// HTML 文本转义（属性与文本节点通用）
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 样式键值对 → 内联 style 字符串；键直接写 kebab-case，空值跳过
pub fn s(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// 格式化为公众号头卡日期：22 SEP 2026（本地时区）
pub fn format_date() -> String {
    chrono::Local::now()
        .format("%d %b %Y")
        .to_string()
        .to_uppercase()
}

/// 剥离带 data-md2html-block 标记的整块（header/footer），用于 publish 草稿。
/// 按嵌套深度扫描配对的 </section>，不依赖正则贪婪匹配。
pub fn strip_decorations(html: &str, blocks: &[&str]) -> String {
    let mut out = html.to_string();
    for block in blocks {
        let marker = format!("<section data-md2html-block=\"{block}\"");
        if let Some(start) = out.find(&marker) {
            if let Some(end) = find_section_end(&out, start) {
                out = format!("{}{}", &out[..start], &out[end..]);
            }
        }
    }
    out
}

/// 找到 start 位置起始的 <section ...> 的配对闭合标签结束下标（按嵌套深度扫描）
pub fn find_section_end(html: &str, start: usize) -> Option<usize> {
    const OPEN: &str = "<section";
    const CLOSE: &str = "</section>";
    let mut i = html[start..].find('>')? + start + 1;
    let mut depth = 1usize;
    while i < html.len() {
        let open = html[i..].find(OPEN).map(|p| p + i);
        let close = html[i..].find(CLOSE).map(|p| p + i)?;
        match open {
            Some(o) if o < close => {
                depth += 1;
                i = o + OPEN.len();
            }
            _ => {
                depth -= 1;
                let end = close + CLOSE.len();
                if depth == 0 {
                    return Some(end);
                }
                i = end;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_covers_all_five() {
        assert_eq!(esc(r#"a&<>"'"#), "a&amp;&lt;&gt;&quot;&#39;");
    }

    #[test]
    fn s_joins_and_skips_empty() {
        assert_eq!(s(&[("color", "#111"), ("margin", "")]), "color:#111");
    }

    #[test]
    fn strip_removes_only_marked_blocks() {
        let html = "<section data-md2html-block=\"header\" style=\"x\"><section style=\"y\">a</section></section><p>正文</p><section data-md2html-block=\"footer\">三连</section>";
        let stripped = strip_decorations(html, &["header", "footer"]);
        assert!(stripped.contains("正文"));
        assert!(!stripped.contains("data-md2html-block"));
        let only_header = strip_decorations(html, &["header"]);
        assert!(only_header.contains("三连"));
    }

    #[test]
    fn find_section_end_handles_nesting() {
        let html = "<section><section>a</section></section>tail";
        let end = find_section_end(html, 0).unwrap();
        assert_eq!(&html[..end], "<section><section>a</section></section>");
    }
}
