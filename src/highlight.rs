//! 代码高亮：syntect（Sublime 语法引擎）+ 主题 codeTokens 色板。
//! 按主题色板动态构建高亮主题（默认前景 = codeText），逐行输出 token 段，
//! 跨行 token 由 HighlightLines 的解析状态自然延续——行结构在公众号不丢。

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::SyntaxSet;

/// 代码块统一深色卡片（VSCode Dark+ 配色），所有主题共用（与 JS 版 base.js 一致）
const PALETTE: &[(&str, &str)] = &[
    ("keyword", "#C586C0"),
    ("built_in", "#4EC9B0"),
    ("type", "#4EC9B0"),
    ("literal", "#569CD6"),
    ("number", "#B5CEA8"),
    ("string", "#CE9178"),
    ("regexp", "#D16969"),
    ("symbol", "#569CD6"),
    ("title", "#DCDCAA"),
    ("function", "#DCDCAA"),
    ("attr", "#9CDCFE"),
    ("attribute", "#9CDCFE"),
    ("variable", "#9CDCFE"),
    ("comment", "#6A9955"),
    ("doctag", "#6A9955"),
    ("meta", "#608B4E"),
    ("tag", "#569CD6"),
    ("name", "#569CD6"),
    ("section", "#569CD6"),
    ("selector_tag", "#D7BA7D"),
    ("selector_class", "#D7BA7D"),
    ("property", "#9CDCFE"),
    ("params", "#9CDCFE"),
    ("operator", "#D4D4D4"),
    ("punctuation", "#D4D4D4"),
];

/// Sublime 作用域 → 色板键。specific 在后（后匹配覆盖先匹配）：
/// 通用 "string" 先入，"string.regexp" 后入以覆盖为 regexp 色。
/// storage（let/const/function）按 VSCode Dark+ 习惯映射为蓝色字面量色。
const SCOPE_MAP: &[(&str, &str)] = &[
    ("comment", "comment"),
    ("keyword", "keyword"),
    ("storage", "literal"),
    ("constant.numeric", "number"),
    ("constant.language", "literal"),
    ("constant.character.escape", "literal"),
    ("entity.name.function", "function"),
    ("support.function", "function"),
    ("entity.name.type", "type"),
    ("entity.name.class", "type"),
    ("support.type", "type"),
    ("support.class", "type"),
    ("entity.name.tag", "tag"),
    ("entity.other.attribute-name", "attribute"),
    ("variable.language", "literal"),
    ("support.constant", "built_in"),
    ("string.regexp", "regexp"),
    ("string", "string"),
];

fn syntax_set() -> &'static SyntaxSet {
    static SS: OnceLock<SyntaxSet> = OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn hex_color(hex: &str) -> Color {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    Color {
        r: ((v >> 16) & 0xff) as u8,
        g: ((v >> 8) & 0xff) as u8,
        b: (v & 0xff) as u8,
        a: 0xff,
    }
}

fn palette(key: &str) -> Option<&'static str> {
    PALETTE.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn build_theme(code_text: &str) -> Theme {
    let settings = ThemeSettings {
        foreground: Some(hex_color(code_text)),
        ..Default::default()
    };
    Theme {
        name: Some("md2html".into()),
        author: Some("md2html-rs".into()),
        settings,
        scopes: SCOPE_MAP
            .iter()
            .filter_map(|(scope, key)| {
                let selector = scope.parse().ok()?;
                palette(key).map(|color| ThemeItem {
                    scope: selector,
                    style: StyleModifier {
                        foreground: Some(hex_color(color)),
                        background: None,
                        font_style: None,
                    },
                })
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub text: String,
    /// "#rrggbb"；None = 沿用 codeText（行级默认色）
    pub color: Option<String>,
}

/// 逐行高亮：返回每行的 token 段。空行 → 空段列表（渲染层出 &nbsp; 占位）。
/// lang 找不到对应语法时回退纯文本（无着色）。
pub fn highlight(code: &str, lang: Option<&str>, code_text: &str) -> Vec<Vec<Seg>> {
    let ss = syntax_set();
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let theme = build_theme(code_text);
    let mut hl = HighlightLines::new(syntax, &theme);

    let mut lines: Vec<Vec<Seg>> = Vec::new();
    for line in code.split('\n') {
        let mut segs: Vec<Seg> = Vec::new();
        // 必须带行尾换行喂给解析器：否则 // 行注释的解析状态会跨行延续，
        // 把下一行也当成注释（syntect 靠 \n 复位行内状态）
        let line_with_nl = format!("{line}\n");
        let Ok(regions) = hl.highlight_line(&line_with_nl, ss) else {
            continue;
        };
        for (style, text) in regions {
            let text = text.trim_end_matches(['\r', '\n']);
            if text.is_empty() {
                continue;
            }
            let color = seg_color(style.foreground, code_text);
            match segs.last_mut() {
                // 同色相邻段合并，减少 span 数量
                Some(last) if last.color == color => last.text.push_str(text),
                _ => segs.push(Seg { text: text.to_string(), color }),
            }
        }
        lines.push(segs);
    }
    lines
}

fn seg_color(fg: Color, code_text: &str) -> Option<String> {
    if fg.a == 0 {
        return None;
    }
    let hex = format!("#{:02x}{:02x}{:02x}", fg.r, fg.g, fg.b);
    if hex.eq_ignore_ascii_case(code_text) {
        None
    } else {
        Some(hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_comment_and_string_colored() {
        let lines = highlight("// 注释\nconst a = \"s\"\n", Some("js"), "#D4D4D4");
        assert!(lines[0].iter().any(|s| s.color.as_deref() == Some("#6a9955")));
        assert!(lines[1].iter().any(|s| s.color.as_deref() == Some("#ce9178")));
    }

    #[test]
    fn empty_line_gives_empty_segs() {
        let lines = highlight("a\n\nb", Some("text"), "#D4D4D4");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].is_empty());
    }

    #[test]
    fn unknown_lang_falls_back_plain() {
        let lines = highlight("hello", Some("notalang"), "#D4D4D4");
        assert_eq!(lines.len(), 1);
        assert!(lines[0][0].color.is_none());
    }
}
