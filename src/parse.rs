//! markdown → mdast（markdown-rs，即 remark/micromark 的 Rust 移植，GFM 开启）。

use markdown::mdast::Node;

pub fn parse_markdown(md: &str) -> Node {
    markdown::to_mdast(md, &markdown::ParseOptions::gfm())
        .expect("markdown 解析不应失败")
}

/// 节点内全部纯文本（用于标题等）
pub fn node_text(node: &Node) -> String {
    crate::utils::node_text(node)
}
