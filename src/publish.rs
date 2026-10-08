//! 发布到公众号草稿箱（不群发）。
//! 流程：渲染 → 正文图片转存微信 → 封面上传素材 → draft/add。

use std::io::Read;
use std::path::{Path, PathBuf};

use markdown::mdast::Node;
use serde_json::json;

use crate::config::load_wechat_config;
use crate::frontmatter::extract_frontmatter;
use crate::utils::{esc, node_text, strip_decorations};
use crate::wechat::{MediaFile, WechatClient};
use crate::{parse, render_markdown, RenderOptions};

use base64::Engine as _;

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Default, Clone)]
pub struct PublishOptions {
    pub theme: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub digest: Option<String>,
    pub cover: Option<String>,
    pub footer: bool,
    pub no_header: bool,
}

#[derive(Debug)]
pub struct PublishReport {
    pub config_source: String,
    pub title: String,
    pub digest: String,
    pub images: usize,
    pub skipped: Vec<String>,
    pub cover_url: Option<String>,
    pub media_id: String,
}

pub fn publish(file: &Path, options: &PublishOptions) -> Result<PublishReport, String> {
    let md_path = std::fs::canonicalize(file).map_err(|e| format!("{} 不可读: {e}", file.display()))?;
    let md = std::fs::read_to_string(&md_path).map_err(|e| format!("{} 不可读: {e}", md_path.display()))?;
    let dir = md_path.parent().unwrap_or(Path::new(".")).to_path_buf();

    let result = render_markdown(
        &md,
        &RenderOptions {
            theme: options.theme.clone(),
            header: Some(!options.no_header),
            footer: Some(options.footer),
            ..Default::default()
        },
    )?;
    // 发往微信的正文剥离头卡（标题走草稿标题字段）；页脚默认不渲染，开启时视为内容保留
    let mut content = strip_decorations(&result.html, &["header"]);
    let meta = &result.meta;
    let title = options
        .title
        .clone()
        .or_else(|| meta.title.clone())
        .unwrap_or_else(|| file_stem(&md_path));
    let author = options.author.clone().or_else(|| meta.author.clone()).unwrap_or_default();
    let digest = options
        .digest
        .clone()
        .or_else(|| meta.digest.clone())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| first_paragraph_text(&md).chars().take(120).collect());

    let config = load_wechat_config()?;
    let wx = WechatClient::new(&config.appid, &config.secret);
    let mut report = PublishReport {
        config_source: config.source,
        title: title.clone(),
        digest,
        images: 0,
        skipped: Vec::new(),
        cover_url: None,
        media_id: String::new(),
    };

    // 1) 正文图片转存（外链/本地/data URI → uploadimg → 替换为微信 URL）
    let mut first_asset: Option<Asset> = None;
    for raw_src in unique_image_sources(&result.html) {
        let src = unescape_html(&raw_src);
        if src.starts_with("https://mmbiz.qpic.cn") {
            continue; // 已是微信图
        }
        let asset = match load_asset(&src, &dir) {
            Ok(asset) => asset,
            Err(err) => {
                report.skipped.push(format!("{src}（{err}）"));
                continue;
            }
        };
        if asset.bytes.len() > MAX_IMAGE_BYTES {
            report.skipped.push(format!("{src}（超过 10MB）"));
            continue;
        }
        if first_asset.is_none() {
            first_asset = Some(asset.clone());
        }
        let wx_url = wx.upload_content_image(&MediaFile {
            field: "media",
            filename: &asset.filename,
            bytes: &asset.bytes,
        })
        .map_err(|e| e.to_string())?;
        content = content.replace(&format!("src=\"{raw_src}\""), &format!("src=\"{}\"", esc(&wx_url)));
        report.images += 1;
    }

    // 2) 封面（草稿必填 thumb_media_id）：--cover/frontmatter 指定 > 文内第一张图
    let cover_asset = if let Some(spec) = options.cover.as_deref().or(meta.cover.as_deref()) {
        load_asset(spec, &dir).map_err(|e| format!("封面图片不可读: {spec}（{e}）"))?
    } else if let Some(asset) = first_asset {
        asset
    } else {
        return Err("公众号草稿必须有封面图：用 --cover 指定（本地路径/URL），或在文中放至少一张图片".to_string());
    };
    let (media_id, cover_url) = wx
        .upload_cover(&MediaFile {
            field: "media",
            filename: &cover_asset.filename,
            bytes: &cover_asset.bytes,
        })
        .map_err(|e| e.to_string())?;
    report.cover_url = cover_url;

    // 3) 写入草稿箱
    let draft_media_id = wx
        .add_draft(json!({
            "title": title,
            "author": author,
            "digest": report.digest,
            "content": content,
            "thumb_media_id": media_id,
            "need_open_comment": 0,
            "only_fans_can_comment": 0,
        }))
        .map_err(|e| e.to_string())?;
    report.media_id = draft_media_id;
    Ok(report)
}

#[derive(Debug, Clone)]
struct Asset {
    bytes: Vec<u8>,
    filename: String,
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// 收集渲染产物里全部 img src（去重保序）
fn unique_image_sources(html: &str) -> Vec<String> {
    const MARK: &str = "<img src=\"";
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = html[from..].find(MARK) {
        let start = from + at + MARK.len();
        let Some(end_off) = html[start..].find('"') else { break };
        let src = html[start..start + end_off].to_string();
        if seen.insert(src.clone()) {
            out.push(src);
        }
        from = start + end_off;
    }
    out
}

/// 支持 data URI / http(s) URL / 本地路径（相对 md 文件目录）
fn load_asset(src: &str, base_dir: &Path) -> Result<Asset, String> {
    if let Some(rest) = src.strip_prefix("data:image/") {
        let (mime, b64) = rest
            .split_once(";base64,")
            .ok_or_else(|| "data URI 不是 base64 图片".to_string())?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("data URI 解码失败: {e}"))?;
        let ext = mime.split('.').next().unwrap_or("jpeg");
        return Ok(Asset { bytes, filename: format!("image.{ext}") });
    }
    if src.starts_with("https://") || src.starts_with("http://") {
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(60))
            .build();
        let res = agent.get(src).call().map_err(|e| format!("下载失败: {e}"))?;
        let mut bytes = Vec::new();
        res.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("下载读取失败: {e}"))?;
        let filename = src.split('?').next().unwrap_or(src).rsplit('/').next().unwrap_or_default();
        let filename = percent_decode(filename);
        let filename = if filename.is_empty() { "image.jpg".to_string() } else { filename };
        return Ok(Asset { bytes, filename });
    }
    let path = if Path::new(src).is_absolute() { PathBuf::from(src) } else { base_dir.join(src) };
    let bytes = std::fs::read(&path).map_err(|e| format!("本地文件不可读: {e}"))?;
    let filename = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Asset { bytes, filename: if filename.is_empty() { "image.jpg".into() } else { filename } })
}

/// 摘要兜底：第一段非空纯文本（跳过纯图片段落）
fn first_paragraph_text(md: &str) -> String {
    let (_, body) = extract_frontmatter(md);
    let tree = parse::parse_markdown(body);
    let Node::Root(root) = tree else { return String::new() };
    root.children
        .iter()
        .find_map(|n| match n {
            Node::Paragraph(_) => {
                let text = node_text(n);
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
                }
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

const UNESCAPES: &[(&str, &str)] = &[("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&#39;", "'")];

fn unescape_html(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in UNESCAPES {
        out = out.replace(from, to);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_img_sources_in_order_deduped() {
        let html = r#"<img src="a.png"><p>x</p><img src="b.png"><img src="a.png">"#;
        assert_eq!(unique_image_sources(html), vec!["a.png", "b.png"]);
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(unescape_html("a&amp;b&quot;c"), "a&b\"c");
    }

    #[test]
    fn data_uri_asset() {
        // 1x1 png
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        let asset = load_asset(&format!("data:image/png;base64,{png_b64}"), Path::new(".")).unwrap();
        assert_eq!(asset.filename, "image.png");
        assert!(!asset.bytes.is_empty());
    }

    #[test]
    fn digest_falls_back_to_first_paragraph() {
        let text = first_paragraph_text("# 标题\n\n第一段\n换行内容\n\n![](x.png)");
        assert_eq!(text, "第一段 换行内容");
    }
}
