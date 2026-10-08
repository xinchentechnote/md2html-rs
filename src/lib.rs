//! 唯一入口（纯函数）：md 字符串 → { html, theme, meta }。
//! 元数据优先级：调用参数 > frontmatter > 默认值。
//! 头卡/页脚默认不含（公众号标题走后台标题栏）：header / footer 显式开启。
//! CLI 与浏览器 UI 共用，不碰文件系统。

pub mod config;
pub mod frontmatter;
pub mod highlight;
pub mod mermaid;
pub mod open_browser;
pub mod parse;
pub mod preview;
pub mod publish;
pub mod render;
pub mod themes;
pub mod transform;
pub mod ui;
pub mod utils;
pub mod wechat;

use markdown::mdast::Node;

use crate::frontmatter::extract_frontmatter;
use crate::themes::Theme;
use crate::transform::{transform, TransformOptions};

pub const DEFAULT_THEME: &str = themes::DEFAULT_THEME;

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub theme: Option<String>,
    pub title: Option<String>,
    pub date: Option<String>,
    pub header: Option<bool>,
    pub footer: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ArticleMeta {
    pub title: Option<String>,
    pub date: Option<String>,
    pub author: Option<String>,
    pub digest: Option<String>,
    pub cover: Option<String>,
}

pub struct RenderResult {
    pub html: String,
    pub theme: &'static Theme,
    pub meta: ArticleMeta,
}

/// 渲染管线：frontmatter → parse → transform → render
pub fn render_markdown(md: &str, options: &RenderOptions) -> Result<RenderResult, String> {
    let (fm, body) = extract_frontmatter(md);
    let theme_id = options
        .theme
        .as_deref()
        .or_else(|| fm.get("theme"))
        .unwrap_or(DEFAULT_THEME);
    let theme = themes::get_theme(theme_id)?;

    // 与 JS 版一致：fm.title 参与标题兜底链（CLI title > frontmatter title > h1 > 无标题）
    let title = options
        .title
        .clone()
        .or_else(|| fm.get("title").map(String::from));
    let opts = TransformOptions::resolve(
        title.clone(),
        options.date.clone(),
        options.header,
        options.footer,
        &fm,
    );
    let blocks = transform(parse::parse_markdown(body), &opts);
    let html = render::render(&blocks, &theme.vars);

    // 显式 title/frontmatter 优先，h1 仅兜底（header 开启时头卡已按此规则定题）
    let h1_title = blocks.iter().find_map(|b| match b {
        transform::Block::Mdast(Node::Heading(h)) if h.depth == 1 => {
            Some(utils::node_text(&Node::Heading(h.clone())))
        }
        _ => None,
    });
    let header_title = match blocks.first() {
        Some(transform::Block::HeaderCard { title, .. }) => Some(title.clone()),
        _ => None,
    };
    let meta = ArticleMeta {
        title: header_title.or(title).or(h1_title),
        date: blocks.first().and_then(|b| match b {
            transform::Block::HeaderCard { date, .. } => Some(date.clone()),
            _ => None,
        }),
        author: fm.get("author").map(String::from),
        digest: fm.get("digest").map(String::from),
        cover: fm.get("cover").map(String::from),
    };

    Ok(RenderResult { html, theme, meta })
}
