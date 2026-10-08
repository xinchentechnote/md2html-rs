//! AST 结构性预处理，产出增强块（自定义 Block）：
//!   HeaderCard   文首标题卡（由首个 h1 摘出，无 h1 时用 title 兜底）
//!   Heading2     h2 自动编号（01/02/…）+ 可选英文小标签
//!   Gallery      同段落连续 ≥2 张图片合并的图集
//!   ImageCard    单张顶层图片
//!   FooterCard   文末三连页脚
//!   MermaidBlock mermaid 代码块（渲染层给"代码卡+标记"兜底形态，后处理换成图片）
//! 编号/图集必须在 AST 层做：公众号剥离 class 后 CSS 计数器与 grid 全部失效。

use crate::frontmatter::Frontmatter;
use crate::utils::{format_date, node_text};
use markdown::mdast::Node;

#[derive(Debug, Clone)]
pub enum Block {
    Mdast(Node),
    HeaderCard { title: String, date: String },
    Heading2 { children: Vec<Node>, no: String, label: Option<String> },
    Gallery { images: Vec<Node> },
    ImageCard { image: markdown::mdast::Image },
    FooterCard,
    MermaidBlock { code: String, original: markdown::mdast::Code },
}

#[derive(Debug, Clone)]
pub struct TransformOptions {
    pub title: Option<String>,
    pub date: Option<String>,
    /// 文首标题卡开关（复制按钮/publish 会剥离；--no-header 时不渲染，h1 留在正文）
    pub header: bool,
    /// 页脚默认不追加：--footer / frontmatter footer:true 开启
    pub footer: bool,
}

impl Default for TransformOptions {
    fn default() -> Self {
        TransformOptions { title: None, date: None, header: true, footer: false }
    }
}

/// 从 frontmatter + 显式参数合成 TransformOptions（优先级：显式参数 > frontmatter > 默认）
impl TransformOptions {
    pub fn resolve(
        title: Option<String>,
        date: Option<String>,
        header: Option<bool>,
        footer: Option<bool>,
        fm: &Frontmatter,
    ) -> Self {
        let fm_bool = |key: &str| fm.bool_of(key);
        TransformOptions {
            title,
            date,
            header: header.or_else(|| fm_bool("header")).unwrap_or(true),
            footer: footer.or_else(|| fm_bool("footer")).unwrap_or(false),
        }
    }
}

pub fn transform(tree: Node, options: &TransformOptions) -> Vec<Block> {
    let Node::Root(root) = tree else {
        return Vec::new();
    };
    let mut children = root.children;

    // 1. 标题卡：HTML 默认包含（复制按钮/publish 时会剥离）；
    //    --no-header / frontmatter header:false 时整体不渲染，h1 留在正文
    let h1_index = children
        .iter()
        .position(|n| matches!(n, Node::Heading(h) if h.depth == 1));
    if options.header {
        let mut title = options.title.clone();
        if let Some(i) = h1_index {
            let h1 = children.remove(i);
            if title.is_none() {
                title = Some(node_text(&h1));
            }
        }
        let date = options.date.clone().unwrap_or_else(format_date);
        let mut blocks: Vec<Block> = Vec::with_capacity(children.len() + 1);
        blocks.push(Block::HeaderCard {
            title: match title.filter(|t| !t.is_empty()) {
                Some(t) => t,
                None => "无标题".to_string(),
            },
            date,
        });
        blocks.extend(children.into_iter().map(Block::Mdast));
        return finish(blocks, options);
    }

    let blocks: Vec<Block> = children.into_iter().map(Block::Mdast).collect();
    finish(blocks, options)
}

/// 2~5 步：h2 编号、图集分组、页脚、mermaid 标记（header 之后统一走这里）
fn finish(blocks: Vec<Block>, options: &TransformOptions) -> Vec<Block> {
    let mut blocks = blocks;

    // 2. h2 自动编号；首个词为纯 ASCII 时拆为英文小标签
    let mut counter = 0usize;
    for block in blocks.iter_mut() {
        let Block::Mdast(Node::Heading(h)) = block else { continue };
        if h.depth != 2 {
            continue;
        }
        counter += 1;
        let mut label = None;
        if let Some(Node::Text(first)) = h.children.first_mut() {
            if let Some((head, rest)) = split_ascii_label(&first.value) {
                label = Some(head.to_string());
                first.value = rest.to_string();
            }
        }
        let children = std::mem::take(&mut h.children);
        *block = Block::Heading2 {
            children,
            no: format!("{counter:02}"),
            label,
        };
    }

    // 3. 纯图片段落 → 图集/图卡。空行是分组边界：只有同一段落内连续书写的
    //    多张图片合并为 gallery；单个图片段落（含跨空行的）独立为 imageCard
    let grouped: Vec<Block> = blocks
        .into_iter()
        .map(|block| match block {
            Block::Mdast(Node::Paragraph(p)) => {
                let has_image = p.children.iter().any(|c| matches!(c, Node::Image(_)));
                let images_only = p.children.iter().all(|c| match c {
                    Node::Image(_) => true,
                    Node::Text(t) => t.value.trim().is_empty(),
                    _ => false,
                });
                let images: Vec<Node> = p
                    .children
                    .iter()
                    .filter(|c| matches!(c, Node::Image(_)))
                    .cloned()
                    .collect();
                if has_image && images_only {
                    if images.len() >= 2 {
                        Block::Gallery { images }
                    } else if let Some(Node::Image(img)) = images.into_iter().next() {
                        Block::ImageCard { image: img }
                    } else {
                        Block::Mdast(Node::Paragraph(p))
                    }
                } else {
                    Block::Mdast(Node::Paragraph(p))
                }
            }
            other => other,
        })
        .collect();

    // 4. 页脚默认不追加：--footer / frontmatter footer:true 开启（开启即视为内容，复制/发布保留）
    let mut grouped = grouped;
    if options.footer {
        grouped.push(Block::FooterCard);
    }

    // 5. mermaid 代码块 → MermaidBlock（渲染层给"代码卡+标记"兜底形态，后处理换成图片）
    grouped
        .into_iter()
        .map(|block| match block {
            Block::Mdast(Node::Code(code)) => {
                let lang = code.lang.as_deref().unwrap_or("").split_whitespace().next();
                if lang == Some("mermaid") {
                    Block::MermaidBlock { code: code.value.clone(), original: code }
                } else {
                    Block::Mdast(Node::Code(code))
                }
            }
            other => other,
        })
        .collect()
}

/// `## WHY 为什么` → ("WHY", "为什么")；规则与 JS 正则
/// `^([A-Za-z][A-Za-z0-9+#./-]*)\s+(.+)$` 一致
fn split_ascii_label(value: &str) -> Option<(&str, &str)> {
    let first = value.chars().next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = first.len_utf8();
    for c in value[end..].chars() {
        if c.is_ascii_alphanumeric() || "+#./-".contains(c) {
            end += c.len_utf8();
        } else {
            break;
        }
    }
    let rest = &value[end..];
    let ws = rest.len() - rest.trim_start().len();
    if ws == 0 || rest.len() == ws {
        return None;
    }
    Some((&value[..end], &rest[ws..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_markdown;

    fn blocks(md: &str) -> Vec<Block> {
        transform(parse_markdown(md), &TransformOptions::default())
    }

    fn heading2_meta(md: &str) -> Vec<(String, Option<String>)> {
        blocks(md)
            .into_iter()
            .filter_map(|b| match b {
                Block::Heading2 { no, label, .. } => Some((no, label)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn h2_numbering_and_label_split() {
        let meta = heading2_meta("## WHY 为什么\n\n## 纯中文标题");
        assert_eq!(
            meta,
            vec![
                ("01".into(), Some("WHY".into())),
                ("02".into(), None),
            ]
        );
        // 标签拆走后，标题文本只剩中文
        let h2 = blocks("## WHY 为什么")
            .into_iter()
            .find_map(|b| match b {
                Block::Heading2 { children, .. } => Some(children),
                _ => None,
            })
            .unwrap();
        let text = h2.iter().map(node_text).collect::<String>();
        assert_eq!(text, "为什么");
    }

    #[test]
    fn gallery_groups_same_paragraph_images() {
        let found = blocks("![a](u1)\n![b](u2)\n![c](u3)");
        let n = found
            .iter()
            .filter(|b| matches!(b, Block::Gallery { images } if images.len() == 3))
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn single_image_paragraph_becomes_image_card() {
        let found = blocks("![a](u1)");
        assert!(found.iter().any(|b| matches!(b, Block::ImageCard { .. })));
        assert!(!found.iter().any(|b| matches!(b, Block::Gallery { .. })));
    }

    #[test]
    fn blank_line_is_group_boundary() {
        let found = blocks("![a](u1)\n\n![b](u2)\n\n![c](u3)");
        assert_eq!(
            found.iter().filter(|b| matches!(b, Block::ImageCard { .. })).count(),
            3
        );
        assert!(!found.iter().any(|b| matches!(b, Block::Gallery { .. })));
    }

    #[test]
    fn header_by_default_footer_off() {
        let t = blocks("# 标题\n\n正文");
        assert!(matches!(t.first(), Some(Block::HeaderCard { .. })));
        assert!(!t.iter().any(|b| matches!(b, Block::FooterCard)));

        let opts = TransformOptions { footer: true, ..Default::default() };
        let t = transform(parse_markdown("# 标题\n\n正文"), &opts);
        assert!(matches!(t.last(), Some(Block::FooterCard)));

        let opts = TransformOptions { header: false, ..Default::default() };
        let t = transform(parse_markdown("# 标题\n\n正文"), &opts);
        assert!(!t.iter().any(|b| matches!(b, Block::HeaderCard { .. })));
        assert!(t
            .iter()
            .any(|b| matches!(b, Block::Mdast(Node::Heading(h)) if h.depth == 1)));
    }

    #[test]
    fn mermaid_code_block_marked() {
        let found = blocks("```mermaid\ngraph LR\nA-->B\n```\n\n```js\nconst a = 1\n```");
        assert!(found.iter().any(|b| matches!(b, Block::MermaidBlock { .. })));
        assert!(found
            .iter()
            .any(|b| matches!(b, Block::Mdast(Node::Code(c)) if c.lang.as_deref() == Some("js"))));
    }

    #[test]
    fn resolve_options_priority_cli_over_frontmatter() {
        let (fm, _) = crate::frontmatter::extract_frontmatter(
            "---\ntitle: fm标题\ntheme: moyu\nfooter: true\n---\n正文",
        );
        let o = TransformOptions::resolve(Some("CLI标题".into()), None, None, None, &fm);
        assert_eq!(o.title.as_deref(), Some("CLI标题"));
        assert!(o.footer);
        let o2 = TransformOptions::resolve(None, None, Some(false), Some(false), &fm);
        assert!(!o2.footer);
        assert!(!o2.header);
    }
}
