//! mdast（含自定义 Block）→ 全内联样式 HTML。
//! 结构约定见原 docs/DESIGN.md 第 6 节：块级一律 <section>，代码逐行 <span>，
//! 布局用内联 flex，无 class/id/<style>——公众号粘贴白名单友好。

use markdown::mdast::{AlignKind, Node};
use markdown::mdast;

use crate::highlight;
use crate::themes::Vars;
use crate::transform::Block;
use crate::utils::{esc, s};

/// 递归上下文：与 JS 版 deriveCtx 的 patch 字段一致
#[derive(Clone, Copy)]
struct Ctx<'a> {
    vars: &'a Vars,
    in_quote: bool,
    tight: bool,
    task_done: bool,
    list_depth: usize,
}

impl<'a> Ctx<'a> {
    fn new(vars: &'a Vars) -> Self {
        Ctx { vars, in_quote: false, tight: false, task_done: false, list_depth: 0 }
    }
}

/// 入口：transform 产物 → 内联 HTML（最外层 root section）
pub fn render(blocks: &[Block], vars: &Vars) -> String {
    let ctx = Ctx::new(vars);
    let inner: String = blocks.iter().map(|b| render_block(b, ctx)).collect();
    let f = &vars.font;
    let c = &vars.colors;
    format!(
        "<section style=\"{}\">{inner}</section>",
        s(&[
            ("font-family", f.family),
            ("font-size", f.base),
            ("color", c.text),
            ("line-height", "1.75"),
            ("padding", "0"),
            ("margin", "0"),
        ])
    )
}

fn render_block(block: &Block, ctx: Ctx) -> String {
    match block {
        Block::HeaderCard { title, date } => header_card(title, date, ctx),
        Block::Heading2 { children, no, label } => heading2(children, no, label.as_deref(), ctx),
        Block::Gallery { images } => gallery(images, ctx),
        Block::ImageCard { image } => image_card(image, ctx),
        Block::FooterCard => footer_card(ctx),
        Block::MermaidBlock { code, original } => mermaid_block(code, original, ctx),
        Block::Mdast(node) => render_mdast_block(node, ctx),
    }
}

fn render_mdast_block(node: &Node, ctx: Ctx) -> String {
    match node {
        Node::Heading(h) => match h.depth {
            1 => format!(
                "<section style=\"{}\">{}</section>",
                s(&[
                    ("font-size", ctx.vars.font.title),
                    ("font-weight", "700"),
                    ("color", ctx.vars.colors.heading),
                    ("text-align", "center"),
                    ("margin", "0 0 24px"),
                    ("line-height", "1.4"),
                ]),
                inline_children(&h.children, ctx)
            ),
            // h2 已被 transform 摘成 Heading2；防御性兜底：按 h3 形态渲染
            2 => render_mdast_block(&Node::Heading(mdast::Heading { depth: 3, children: h.children.clone(), position: None }), ctx),
            3 => {
                let c = &ctx.vars.colors;
                format!(
                    "<section style=\"{}\"><span style=\"{}\"></span><span style=\"{}\">{}</span></section>",
                    s(&[("display", "flex"), ("align-items", "center"), ("gap", "8px"), ("margin", "24px 0 12px")]),
                    s(&[("width", "4px"), ("height", "16px"), ("background", c.accent), ("border-radius", "2px"), ("flex-shrink", "0")]),
                    s(&[("font-size", "16px"), ("font-weight", "700"), ("color", c.heading)]),
                    inline_children(&h.children, ctx)
                )
            }
            _ => format!(
                "<section style=\"{}\">{}</section>",
                s(&[
                    ("font-size", "15px"),
                    ("font-weight", "700"),
                    ("color", ctx.vars.colors.accent),
                    ("margin", "18px 0 8px"),
                ]),
                inline_children(&h.children, ctx)
            ),
        },

        Node::Paragraph(p) => {
            let c = &ctx.vars.colors;
            let f = &ctx.vars.font;
            let normal_margin = format!("0 0 {}", ctx.vars.spacing.paragraph);
            if ctx.in_quote {
                return format!(
                    "<p style=\"{}\">{}</p>",
                    s(&[
                        ("font-size", f.base),
                        ("font-weight", "600"),
                        ("color", c.heading),
                        ("line-height", "1.9"),
                        ("margin", "0"),
                        ("text-align", "center"),
                    ]),
                    inline_children(&p.children, ctx)
                );
            }
            format!(
                "<p style=\"{}\">{}</p>",
                s(&[
                    ("font-size", f.base),
                    ("color", if ctx.task_done { c.muted } else { c.text }),
                    ("line-height", "1.75"),
                    ("margin", if ctx.tight { "0" } else { normal_margin.as_str() }),
                    ("text-align", "justify"),
                    ("overflow-wrap", "break-word"),
                ]),
                inline_children(&p.children, ctx)
            )
        }
        Node::Blockquote(b) => {
            let c = &ctx.vars.colors;
            let r = &ctx.vars.radius;
            let quote_ctx = Ctx { in_quote: true, ..ctx };
            let inner: String = b.children.iter().map(|n| render_mdast_block(n, quote_ctx)).collect();
            format!(
                "<section style=\"{}\">{inner}</section>",
                s(&[
                    ("border", &format!("1.5px dashed {}", c.accent)),
                    ("border-radius", r.card),
                    ("padding", "18px 22px"),
                    ("margin", "20px 0"),
                ])
            )
        }

        Node::Code(code) => code_block(&code.value, code.lang.as_deref(), ctx),

        Node::List(list) => {
            let is_task = list
                .children
                .iter()
                .any(|it| matches!(it, Node::ListItem(li) if li.checked.is_some()));
            if is_task {
                task_list(list, ctx)
            } else if list.ordered {
                ordered_list(list, ctx)
            } else {
                unordered_list(list, ctx)
            }
        }

        Node::Table(table) => {
            let c = &ctx.vars.colors;
            let align_of = |i: usize| -> &'static str {
                match table.align.get(i) {
                    Some(AlignKind::Center) => "center",
                    Some(AlignKind::Right) => "right",
                    _ => "left",
                }
            };
            let cell = |cell_node: &Node, i: usize, header: bool| -> String {
                let tag = if header { "th" } else { "td" };
                let children = cell_node.children().map(|cs| cs.as_slice()).unwrap_or(&[]);
                format!(
                    "<{tag} style=\"{}\">{}</{tag}>",
                    s(&[
                        ("padding", "8px 12px"),
                        ("border", &format!("1px solid {}", c.border)),
                        ("text-align", align_of(i)),
                        ("font-size", "14px"),
                        ("color", if header { c.accent } else { c.text }),
                        ("font-weight", if header { "700" } else { "400" }),
                        ("background", if header { c.accent_light } else { "transparent" }),
                        ("overflow-wrap", "break-word"),
                    ]),
                    inline_children(children, ctx)
                )
            };
            let mut rows = table.children.iter();
            let head_row = match rows.next() {
                Some(row) => {
                    let cells: String = row
                        .children()
                        .map(|cs| cs.iter().enumerate().map(|(i, cn)| cell(cn, i, true)).collect())
                        .unwrap_or_default();
                    format!("<thead><tr>{cells}</tr></thead>")
                }
                None => String::new(),
            };
            let body_rows: String = rows
                .map(|row| {
                    let cells: String = row
                        .children()
                        .map(|cs| cs.iter().enumerate().map(|(i, cn)| cell(cn, i, false)).collect())
                        .unwrap_or_default();
                    format!("<tr>{cells}</tr>")
                })
                .collect();
            format!(
                "<section style=\"{}\"><table style=\"{}\">{head_row}<tbody>{body_rows}</tbody></table></section>",
                s(&[("margin", "16px 0"), ("overflow-x", "auto")]),
                s(&[("width", "100%"), ("border-collapse", "collapse")]),
            )
        }

        Node::ThematicBreak(_) => {
            let c = &ctx.vars.colors;
            format!(
                "<section style=\"{}\"><span style=\"{}\"></span></section>",
                s(&[("text-align", "center"), ("margin", "24px 0")]),
                s(&[("display", "inline-block"), ("width", "40px"), ("height", "4px"), ("background", c.accent), ("border-radius", "2px")]),
            )
        }

        // md 里的原生 HTML 片段直接丢弃，避免把未内联的标签带进公众号
        Node::Html(_) => String::new(),

        // 其余（Yaml/Definition/FootnoteDefinition 等）不渲染
        other => other
            .children()
            .map(|children| children.iter().map(|n| render_mdast_block(n, ctx)).collect::<String>())
            .unwrap_or_default(),
    }
}

fn header_card(title: &str, date: &str, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let r = &ctx.vars.radius;
    format!(
        "<section data-md2html-block=\"header\" style=\"{}\"><section style=\"{}\">{}</section><section style=\"{}\">{}</section><section style=\"{}\"></section></section>",
        // data-md2html-block 标记供「复制到公众号」与 publish 剥离头尾用（公众号粘贴时会剥未知属性，无副作用）
        s(&[
            ("background", &format!("linear-gradient(135deg, {}, #FFFFFF)", c.accent_light)),
            ("border-radius", r.card),
            ("padding", "28px 20px 24px"),
            ("text-align", "center"),
            ("margin-bottom", "28px"),
        ]),
        s(&[("font-size", "12px"), ("letter-spacing", "2px"), ("color", c.muted)]),
        esc(date),
        s(&[
            ("font-size", f.title),
            ("font-weight", "700"),
            ("color", c.heading),
            ("margin", "12px 0 14px"),
            ("line-height", "1.4"),
        ]),
        esc(title),
        s(&[("width", "36px"), ("height", "4px"), ("background", c.accent), ("border-radius", "2px"), ("margin", "0 auto")]),
    )
}

fn heading2(children: &[Node], no: &str, label: Option<&str>, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let label_html = label
        .map(|l| {
            format!(
                "<span style=\"{}\">{}</span>",
                s(&[("font-size", "12px"), ("letter-spacing", "2px"), ("color", c.muted), ("text-transform", "uppercase")]),
                esc(l)
            )
        })
        .unwrap_or_default();
    format!(
        "<section style=\"{}\"><section style=\"{}\"><span style=\"{}\">{no}</span>{label_html}</section><section style=\"{}\">{}</section></section>",
        s(&[("margin", "32px 0 16px")]),
        s(&[("display", "flex"), ("align-items", "baseline"), ("gap", "8px")]),
        s(&[("font-size", "30px"), ("font-weight", "800"), ("color", c.accent), ("line-height", "1")]),
        s(&[("font-size", f.h2), ("font-weight", "700"), ("color", c.heading), ("margin-top", "4px")]),
        inline_children(children, ctx),
    )
}

fn code_block(value: &str, lang: Option<&str>, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let r = &ctx.vars.radius;
    let lines = highlight::highlight(value, lang, c.code_text);
    let body: String = lines
        .iter()
        .map(|segs| {
            let inner = if segs.is_empty() {
                "&nbsp;".to_string()
            } else {
                segs.iter()
                    .map(|seg| match &seg.color {
                        Some(color) => format!("<span style=\"color:{color}\">{}</span>", esc(&seg.text)),
                        None => esc(&seg.text),
                    })
                    .collect::<String>()
            };
            format!(
                "<span style=\"{}\">{inner}</span>",
                s(&[("display", "block"), ("white-space", "pre-wrap"), ("word-break", "break-all"), ("color", c.code_text)])
            )
        })
        .collect();
    format!(
        "<section style=\"{}\">{body}</section>",
        s(&[
            ("background", c.code_bg),
            ("border-radius", r.card),
            ("padding", "16px"),
            ("margin", "16px 0"),
            ("font-family", f.mono),
            ("font-size", f.code),
            ("line-height", "1.7"),
            ("overflow-x", "auto"),
        ])
    )
}

fn ordered_list(list: &mdast::List, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let start = list.start.unwrap_or(1);
    let items: String = list
        .children
        .iter()
        .enumerate()
        .map(|(i, item)| {
            format!(
                "<section style=\"{}\"><span style=\"{}\">{}</span><section style=\"{}\">{}</section></section>",
                s(&[
                    ("display", "flex"),
                    ("gap", "10px"),
                    ("align-items", "flex-start"),
                    ("background", c.card_bg),
                    ("border-radius", "8px"),
                    ("padding", "12px 14px"),
                    ("margin-bottom", "8px"),
                ]),
                s(&[
                    ("flex-shrink", "0"),
                    ("width", "20px"),
                    ("height", "20px"),
                    ("line-height", "20px"),
                    ("border-radius", "50%"),
                    ("background", c.accent),
                    ("color", "#FFFFFF"),
                    ("font-size", "12px"),
                    ("font-weight", "700"),
                    ("text-align", "center"),
                ]),
                start + i as u32,
                s(&[("flex", "1"), ("min-width", "0"), ("font-size", f.base), ("color", c.text), ("line-height", "1.7")]),
                list_body(item, ctx),
            )
        })
        .collect();
    list_wrapper(&items, "12px 0 4px", ctx)
}

fn unordered_list(list: &mdast::List, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let items: String = list
        .children
        .iter()
        .map(|item| {
            format!(
                "<section style=\"{}\"><span style=\"{}\"></span><section style=\"{}\">{}</section></section>",
                s(&[("display", "flex"), ("gap", "8px"), ("align-items", "flex-start"), ("margin-bottom", "6px")]),
                s(&[("flex-shrink", "0"), ("width", "6px"), ("height", "6px"), ("border-radius", "50%"), ("background", c.accent), ("margin-top", "9px")]),
                s(&[("flex", "1"), ("min-width", "0"), ("font-size", f.base), ("color", c.text), ("line-height", "1.7")]),
                list_body(item, ctx),
            )
        })
        .collect();
    list_wrapper(&items, "10px 0 4px", ctx)
}

fn task_list(list: &mdast::List, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let f = &ctx.vars.font;
    let items: String = list
        .children
        .iter()
        .filter_map(|node| match node {
            Node::ListItem(item) => Some(format!(
                "<section style=\"{}\"><span style=\"{}\">{}</span><section style=\"{}\">{body}</section></section>",
                s(&[("display", "flex"), ("gap", "8px"), ("align-items", "flex-start"), ("margin-bottom", "6px")]),
                s(&[
                    ("flex-shrink", "0"),
                    ("color", if item.checked == Some(true) { c.accent } else { c.muted }),
                    ("font-size", f.base),
                    ("line-height", "1.7"),
                ]),
                if item.checked == Some(true) { "☑" } else { "☐" },
                s(&[
                    ("flex", "1"),
                    ("min-width", "0"),
                    ("font-size", f.base),
                    ("color", if item.checked == Some(true) { c.muted } else { c.text }),
                    ("line-height", "1.7"),
                    ("text-decoration", if item.checked == Some(true) { "line-through" } else { "none" }),
                ]),
                body = {
                    let checked = item.checked == Some(true);
                    let item_ctx = Ctx { tight: true, task_done: checked, ..ctx };
                    item.children.iter().map(|n| render_mdast_block(n, item_ctx)).collect::<String>()
                },
            )),
            _ => None,
        })
        .collect();
    list_wrapper(&items, "10px 0 4px", ctx)
}

fn list_body(item: &Node, ctx: Ctx) -> String {
    let inner = Ctx { tight: true, list_depth: ctx.list_depth + 1, ..ctx };
    let Some(Node::ListItem(li)) = Some(item) else { return String::new() };
    li.children.iter().map(|n| render_mdast_block(n, inner)).collect()
}

fn list_wrapper(items: &str, margin: &str, ctx: Ctx) -> String {
    let mut pairs = vec![("margin", margin)];
    if ctx.list_depth > 0 {
        pairs.push(("margin-left", "1.5em"));
    }
    format!("<section style=\"{}\">{items}</section>", s(&pairs))
}

fn image_card(image: &mdast::Image, ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let caption = if image.alt.is_empty() {
        String::new()
    } else {
        format!(
            "<section style=\"{}\">— {} —</section>",
            s(&[("font-size", "12px"), ("color", c.muted), ("margin-top", "6px"), ("text-align", "center")]),
            esc(&image.alt)
        )
    };
    format!(
        "<section style=\"{}\"><img src=\"{}\" alt=\"{}\" style=\"{}\">{caption}</section>",
        s(&[("margin", "16px 0")]),
        esc(&image.url),
        esc(&image.alt),
        s(&[("width", "100%"), ("border-radius", "8px"), ("display", "block")]),
    )
}

fn gallery(images: &[Node], _ctx: Ctx) -> String {
    let rows: Vec<&[Node]> = images.chunks(2).collect();
    let body: String = rows
        .iter()
        .map(|row| {
            let imgs: String = row
                .iter()
                .filter_map(|img| match img {
                    Node::Image(i) => {
                        Some(format!(
                            "<img src=\"{}\" alt=\"{}\" style=\"{}\">",
                            esc(&i.url),
                            esc(&i.alt),
                            s(&[("flex", "1"), ("width", "0"), ("min-width", "0"), ("height", "auto"), ("border-radius", "8px"), ("display", "block")]),
                        ))
                    }
                    _ => None,
                })
                .collect();
            format!("<section style=\"{}\">{imgs}</section>", s(&[("display", "flex"), ("gap", "6px"), ("margin-bottom", "6px")]))
        })
        .collect();
    format!("<section style=\"{}\">{body}</section>", s(&[("margin", "16px 0")]))
}

fn footer_card(ctx: Ctx) -> String {
    let c = &ctx.vars.colors;
    let cards: String = [("👍", "点赞"), ("👀", "在看"), ("🔁", "转发")]
        .iter()
        .map(|(emoji, label)| {
            format!(
                "<section style=\"{}\"><section style=\"{}\">{emoji}</section><section style=\"{}\">{label}</section></section>",
                s(&[("flex", "1"), ("background", c.card_bg), ("border-radius", "8px"), ("padding", "12px 0")]),
                s(&[("font-size", "20px"), ("line-height", "1")]),
                s(&[("font-size", "12px"), ("color", c.muted), ("margin-top", "6px")]),
            )
        })
        .collect();
    format!(
        "<section data-md2html-block=\"footer\" style=\"{}\"><section style=\"{}\">— 有收获的话，「三连」支持一下，下期再见 —</section><section style=\"{}\">{cards}</section></section>",
        s(&[("margin", "36px 0 8px"), ("text-align", "center")]),
        s(&[("font-size", "14px"), ("color", c.muted), ("margin-bottom", "12px")]),
        s(&[("display", "flex"), ("gap", "10px")]),
    )
}

/// mermaid 兜底形态：主题色代码卡 + base64 标记；inlineMermaid 后处理成功时整块替换为图片
fn mermaid_block(code: &str, original: &mdast::Code, ctx: Ctx) -> String {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(code.as_bytes());
    format!(
        "<section data-md2html-mermaid=\"{b64}\">{}</section>",
        code_block(&original.value, original.lang.as_deref(), ctx)
    )
}

fn inline_children(nodes: &[Node], ctx: Ctx) -> String {
    nodes.iter().map(|n| inline_node(n, ctx)).collect()
}

fn inline_node(node: &Node, ctx: Ctx) -> String {
    match node {
        Node::Text(t) => esc(&t.value),
        Node::Strong(st) => {
            let c = &ctx.vars.colors;
            let style = if ctx.in_quote {
                s(&[
                    ("font-weight", "700"),
                    ("color", c.heading),
                    ("background", &format!("linear-gradient(180deg, transparent 62%, {} 62%)", c.highlight)),
                ])
            } else {
                s(&[("font-weight", "700"), ("color", c.accent)])
            };
            format!("<span style=\"{style}\">{}</span>", inline_children(&st.children, ctx))
        }
        Node::Emphasis(e) => format!("<em style=\"font-style:italic\">{}</em>", inline_children(&e.children, ctx)),
        Node::InlineCode(code) => {
            let c = &ctx.vars.colors;
            let f = &ctx.vars.font;
            let r = &ctx.vars.radius;
            format!(
                "<code style=\"{}\">{}</code>",
                s(&[
                    ("background", c.accent_light),
                    ("color", c.accent),
                    ("padding", "2px 6px"),
                    ("border-radius", r.tag),
                    ("font-size", f.code),
                    ("font-family", f.mono),
                ]),
                esc(&code.value)
            )
        }
        // 公众号对多数个人号剥 <a>：降级为主题色文字，样式不丢
        Node::Link(l) => format!(
            "<span style=\"{}\">{}</span>",
            s(&[("color", ctx.vars.colors.accent), ("text-decoration", "underline")]),
            inline_children(&l.children, ctx)
        ),
        Node::Image(img) => format!(
            "<img src=\"{}\" alt=\"{}\" style=\"max-width:100%;border-radius:4px;vertical-align:middle\">",
            esc(&img.url),
            esc(&img.alt)
        ),
        Node::Delete(d) => format!(
            "<span style=\"{}\">{}</span>",
            s(&[("text-decoration", "line-through"), ("color", ctx.vars.colors.muted)]),
            inline_children(&d.children, ctx)
        ),
        Node::Break(_) => "<br>".to_string(),
        Node::Html(_) => String::new(),
        // 行内节点兜底：未知类型有 children 则继续行内渲染
        other => other
            .children()
            .map(|children| inline_children(children, ctx))
            .unwrap_or_default(),
    }
}
