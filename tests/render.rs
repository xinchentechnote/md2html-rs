//! 快照 + 渲染行为集成测试。
//! 快照是纯正文片段，头部补 charset 注释防浏览器打开乱码；
//! UPDATE_SNAPSHOTS=1 cargo test 重新生成。
//! 快照不入库（.gitignore）：首次 cargo test 自动生成本地基准。

use std::fs;
use std::path::Path;

use md2html_rs::themes::list_themes;
use md2html_rs::{render_markdown, RenderOptions};
use md2html_rs::utils::strip_decorations;

const HERE: &str = env!("CARGO_MANIFEST_DIR");
const SNAPSHOT_HEAD: &str = "<!-- md2html 回归快照（UTF-8）· 本文件是测试基准，日常预览请用 md2html 生成 -->\n<meta charset=\"utf-8\">\n";

fn sample() -> String {
    fs::read_to_string(Path::new(HERE).join("tests/fixtures/sample.md")).unwrap()
}

fn snapshot(name: &str, html: &str) {
    let dir = Path::new(HERE).join("tests/__snapshots__");
    let file = dir.join(format!("{name}.html"));
    let content = format!("{SNAPSHOT_HEAD}{html}");
    if !file.exists() || std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, content).unwrap();
        return;
    }
    // 归一化行尾：抵御 Windows checkout 的 autocrlf
    let baseline = fs::read_to_string(&file).unwrap().replace("\r\n", "\n");
    assert_eq!(content, baseline, "快照不一致: {name}（UPDATE_SNAPSHOTS=1 cargo test 更新）");
}

fn render(md: &str, theme: &str, header: Option<bool>, footer: Option<bool>) -> md2html_rs::RenderResult {
    render_markdown(
        md,
        &RenderOptions {
            theme: Some(theme.to_string()),
            date: Some("22 SEP 2026".to_string()),
            header,
            footer,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn snapshots_all_themes() {
    let md = sample();
    for t in list_themes() {
        let result = render(&md, t.id, None, None);
        snapshot(&format!("render-{}", t.id), &result.html);
    }
}

#[test]
fn output_has_no_class_id_style_script() {
    let result = render(&sample(), "moyu", None, Some(false));
    for bad in ["class=", "id=", "<style", "<script"] {
        assert!(!result.html.contains(bad), "正文不允许出现 {bad}");
    }
    assert!(result.html.contains("style=\""));
}

#[test]
fn code_block_line_spans_keep_colors() {
    let result = render("```js\n// 注释\nconst a = \"s\"\n```", "moyu", None, Some(false));
    assert!(result.html.contains("display:block"));
    assert!(result.html.contains("#6a9955"), "注释应有高亮色");
    assert!(result.html.contains("#ce9178"), "字符串应有高亮色");
}

#[test]
fn tables_keep_real_table_form() {
    let wide = render("| a | b | c | d |\n| --- | --- | --- | --- |\n| 1 | 2 | 3 | 4 |", "moyu", Some(false), Some(false));
    assert!(wide.html.contains("<table"), "4 列表格仍是 table（不做卡片降级）");
    assert!(wide.html.contains("<th"), "表头单元格存在");
    let narrow = render("| a | b | c |\n| --- | --- | --- |\n| 1 | 2 | 3 |", "moyu", Some(false), Some(false));
    assert!(narrow.html.contains("<table style"), "3 列表格保留 table");
}

#[test]
fn long_words_get_break_protection() {
    let result = render("文字 https://a-very-long-example-url.test/with/many/segments", "moyu", None, Some(false));
    assert!(result.html.contains("overflow-wrap:break-word"));
}

#[test]
fn nested_lists_render_all_levels() {
    let result = render("- a\n  - b\n    - c", "moyu", None, Some(false));
    assert_eq!(
        result.html.matches("border-radius:50%;background:#07C160").count(),
        3
    );
}

#[test]
fn empty_doc_still_gets_header_fallback() {
    let result = render("", "moyu", None, None);
    assert!(result.html.contains("无标题"));
}

#[test]
fn header_footer_marks_and_strip() {
    let result = render("# 文章标题\n\n正文内容", "moyu", None, Some(true));
    assert!(result.html.contains("data-md2html-block=\"header\""), "头卡有标记");
    assert!(result.html.contains("data-md2html-block=\"footer\""), "页脚有标记");
    let stripped = strip_decorations(&result.html, &["header", "footer"]);
    assert!(!stripped.contains("data-md2html-block"), "标记块已剔除");
    assert!(!stripped.contains("三连"), "页脚不残留");
    assert!(stripped.contains("正文内容"), "正文保留");
    let only_header = strip_decorations(&result.html, &["header"]);
    assert!(only_header.contains("三连"), "仅剥头时页脚保留");
}

#[test]
fn no_header_renders_centered_h1() {
    let result = render("# 文章标题\n\n正文", "moyu", Some(false), Some(false));
    assert!(!result.html.contains("linear-gradient"), "无头卡");
    assert!(result.html.contains("text-align:center"), "h1 居中");
}

#[test]
fn all_themes_render_with_own_accent() {
    let accents = [
        ("moyu", "#07C160"),
        ("redwhite", "#E34D59"),
        ("deepblue", "#2A6AE9"),
        ("orange", "#F76B1C"),
        ("grape", "#7C5CFC"),
        ("peach", "#EC6E8B"),
    ];
    for (id, accent) in accents {
        let result = render("# t\n\nbody", id, None, Some(false));
        assert!(!result.html.is_empty(), "{id} 应可渲染");
        assert!(result.html.contains(accent), "{id} 应带主色 {accent}");
    }
}

#[test]
fn meta_title_priority_cli_over_frontmatter_over_h1() {
    // frontmatter title vs h1：frontmatter 优先
    let result = render_markdown(
        "---\ntitle: FM标题\n---\n# H1标题\n\n正文",
        &RenderOptions { theme: Some("moyu".into()), date: Some("22 SEP 2026".into()), ..Default::default() },
    )
    .unwrap();
    assert_eq!(result.meta.title.as_deref(), Some("FM标题"));

    // CLI title 最优先
    let result = render_markdown(
        "---\ntitle: FM标题\n---\n# H1标题\n\n正文",
        &RenderOptions {
            theme: Some("moyu".into()),
            title: Some("CLI标题".into()),
            date: Some("22 SEP 2026".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.meta.title.as_deref(), Some("CLI标题"));

    // 无 title 时 h1 兜底
    let result = render_markdown(
        "# H1标题\n\n正文",
        &RenderOptions { theme: Some("moyu".into()), date: Some("22 SEP 2026".into()), ..Default::default() },
    )
    .unwrap();
    assert_eq!(result.meta.title.as_deref(), Some("H1标题"));
}

#[test]
fn frontmatter_footer_enables_footer() {
    let result = render_markdown(
        "---\nfooter: true\n---\n正文",
        &RenderOptions { theme: Some("moyu".into()), date: Some("22 SEP 2026".into()), ..Default::default() },
    )
    .unwrap();
    assert!(result.html.contains("data-md2html-block=\"footer\""));
}

#[test]
fn frontmatter_theme_switches_colors() {
    let result = render_markdown(
        "---\ntheme: moyu\n---\n# 标题\n\n正文",
        &RenderOptions { date: Some("22 SEP 2026".into()), ..Default::default() },
    )
    .unwrap();
    assert!(result.html.contains("#07C160"), "frontmatter theme 应生效");
}

#[test]
fn mermaid_block_gets_marker_and_fallback_card() {
    let result = render("```mermaid\ngraph LR\nA-->B\n```", "moyu", None, Some(false));
    assert!(result.html.contains("data-md2html-mermaid="), "应有 base64 标记");
    assert!(result.html.contains("background:#1E1E1E"), "兜底代码卡用主题 codeBg");
}

#[test]
fn links_degrade_to_colored_text() {
    let result = render("[一个链接](https://example.com)", "moyu", Some(false), Some(false));
    assert!(!result.html.contains("<a "), "链接降级不输出 <a>");
    assert!(result.html.contains("text-decoration:underline"));
}
