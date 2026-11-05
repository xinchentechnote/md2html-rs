//! 数学公式（KaTeX）服务端渲染，公众号友好：
//!   normalize_display_math  行首 $$...$$ 原文改写为 ```math 围栏（避开 md 转义/强调解析）
//!   split_inline_math       Text 节点里 $...$ / $$...$$ 行内公式切分
//!   render_math             katex crate 出 HTML → class 布局样式内联化（公众号剥 class，必须转 style）
//!
//! 样式内联化是 katex.css（0.16）布局规则的扁平化： KaTeX 把几何信息（top/height 等）
//! 放在行内 style，布局骨架（display/position/border）放 class。本模块把后者按
//! 「自身 class + 父/祖父 class」求值合并进 style、删掉 class。查不到的 class 自然
//! no-op——只会安全降级，不会破坏结构。字体映射系统衬线栈（无 KaTeX webfont），
//! \Large 等字号命令按 KaTeX 字号表的固定比例公式还原。

use katex::{Opts, OutputType};

/// 行内公式切分结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    Literal(&'a str),
    Math { latex: &'a str, display: bool },
}

// ---------------------------------------------------------------------------
// 块级公式 → ```math 围栏
// ---------------------------------------------------------------------------

/// 把行首 `$$...$$`（单行或多行，允许内部空行）改写为 ```math 围栏。
/// 公式原文走 Code 节点后，markdown 的转义（`\\`→`\`）与 `_` 强调都不再干扰 LaTeX。
/// 只处理顶层（缩进 ≤3 空格）；跳过 ```/~~~ 围栏内部；列表/引用内留给行内兜底。
pub fn normalize_display_math(md: &str) -> String {
    if !md.contains("$$") {
        return md.to_string();
    }
    let lines: Vec<&str> = md.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut fence: Option<(char, usize)> = None; // (围栏字符, 长度)
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if let Some((ch, len)) = fence {
            out.push(line.to_string());
            if indent <= 3 && starts_fence(trimmed, ch, len) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if indent <= 3 && (starts_fence(trimmed, '`', 3) || starts_fence(trimmed, '~', 3)) {
            let ch = trimmed.chars().next().unwrap();
            fence = Some((ch, trimmed.chars().take_while(|&c| c == ch).count()));
            out.push(line.to_string());
            i += 1;
            continue;
        }
        if indent <= 3 && trimmed.starts_with("$$") {
            if let Some((content, consumed)) = match_display_block(&lines[i..]) {
                let pad = &line[..indent];
                out.push(format!("{pad}```math"));
                for cl in &content {
                    out.push(format!("{pad}{cl}"));
                }
                out.push(format!("{pad}```"));
                i += consumed;
                continue;
            }
        }
        out.push(line.to_string());
        i += 1;
    }
    let mut result = out.join("\n");
    if md.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// trimmed 是否为 ≥len 个连续 ch 字符开头（围栏开/闭判定）
fn starts_fence(trimmed: &str, ch: char, len: usize) -> bool {
    trimmed.chars().take(len).all(|c| c == ch)
        && trimmed.chars().nth(len.saturating_sub(1)) == Some(ch)
}

/// `lines[0]` 以 $$ 开头时尝试匹配整块：返回 (去定界符的内容行, 消费行数)。
/// 内容为空或找不到闭合时不改写（返回 None）。
fn match_display_block(lines: &[&str]) -> Option<(Vec<String>, usize)> {
    let rest = &lines[0].trim_start()[2..];
    let mut content: Vec<String> = Vec::new();
    // 单行：$$ x $$ → 内容取首个闭合 $$ 之前
    if let Some(close) = find_unescaped(rest, "$$") {
        if !rest[close + 2..].trim().is_empty() {
            return None;
        }
        let body = rest[..close].trim();
        if body.is_empty() {
            return None;
        }
        return Some((vec![body.to_string()], 1));
    }
    if !rest.trim().is_empty() {
        content.push(rest.trim_end().to_string());
    }
    // 多行：逐行找闭合 $$
    for (j, line) in lines.iter().enumerate().skip(1) {
        let t = line.trim_start();
        if let Some(close) = find_unescaped(t, "$$") {
            if !t[close + 2..].trim().is_empty() {
                return None;
            }
            let tail = t[..close].trim_end();
            if !tail.is_empty() {
                content.push(tail.to_string());
            }
            return Some((content, j + 1));
        }
        content.push(t.trim_end().to_string());
    }
    None
}

/// 找未被奇数个反斜杠转义的定界符，避免把 `\$$` 当作公式闭合。
fn find_unescaped(text: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = text[from..].find(needle) {
        let at = from + offset;
        let slashes = text[..at].bytes().rev().take_while(|&b| b == b'\\').count();
        if slashes % 2 == 0 {
            return Some(at);
        }
        from = at + needle.len();
    }
    None
}

// ---------------------------------------------------------------------------
// 行内公式切分
// ---------------------------------------------------------------------------

/// 切分 Text 节点里的行内公式。规则仿 remark-math：
///   - 开定界符后不能是空白；闭定界符前不能是空白、后不能紧跟数字（防「价格 $5 和 $3」）
///   - `\$` 转义跳过；`$$...$$` 优先于 `$...$`
pub fn split_inline_math(text: &str) -> Vec<Segment<'_>> {
    let mut segs = Vec::new();
    let bytes = text.as_bytes();
    let mut lit_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2, // 转义对，双双留在正文（末尾孤 \ 自然越界结束）
            b'$' => {
                let dlen = if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                    2
                } else {
                    1
                };
                match match_inline(bytes, i, dlen) {
                    Some(close) => {
                        let head = &text[lit_start..i];
                        if !head.is_empty() {
                            segs.push(Segment::Literal(head));
                        }
                        segs.push(Segment::Math {
                            latex: &text[i + dlen..close],
                            display: dlen == 2,
                        });
                        lit_start = close + dlen;
                        i = lit_start;
                    }
                    None => i += dlen,
                }
            }
            _ => i += 1,
        }
    }
    if lit_start < text.len() {
        segs.push(Segment::Literal(&text[lit_start..]));
    }
    segs
}

/// `i` 处的定界符（长度 dlen）能否闭合到某个合法位置，返回闭定界符起点索引
fn match_inline(bytes: &[u8], i: usize, dlen: usize) -> Option<usize> {
    let after_open = i + dlen;
    if after_open >= bytes.len() || bytes[after_open].is_ascii_whitespace() {
        return None;
    }
    let mut j = after_open;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            j += 2;
            continue;
        }
        if bytes[j] != b'$' {
            j += 1;
            continue;
        }
        // $$ 需要成对闭合
        if dlen == 2 && bytes.get(j + 1) != Some(&b'$') {
            j += 1;
            continue;
        }
        // 闭定界符前不能是空白、后不能紧跟数字
        if bytes[j - 1].is_ascii_whitespace() {
            j += dlen;
            continue;
        }
        if bytes.get(j + dlen).is_some_and(|b| b.is_ascii_digit()) {
            j += dlen;
            continue;
        }
        return Some(j);
    }
    None
}

// ---------------------------------------------------------------------------
// KaTeX 渲染 + 样式内联化
// ---------------------------------------------------------------------------

/// LaTeX → 全内联样式 HTML。KaTeX 解析失败返回 None（调用方兜底为文本/代码卡）。
pub fn render_math(latex: &str, display: bool) -> Option<String> {
    let opts = Opts::builder()
        .display_mode(display)
        .output_type(OutputType::Html)
        .throw_on_error(true)
        .build()
        .expect("katex Opts 是静态完整的，build 不会失败");
    let raw = katex::render_with_opts(latex, &opts).ok()?;
    Some(inline_styles(&raw))
}

const SERIF: &str = "'Times New Roman',Georgia,'Songti SC',serif";
const SANS: &str = "Helvetica,Arial,sans-serif";
const MONO: &str = "Menlo,Consolas,'Courier New',monospace";

/// KaTeX 字号表（相对 size6=1em，css reset-sizeN.sizeM 的比例 = SIZE[M]/SIZE[N]）
const KATEX_SIZES: [f64; 11] = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.2, 1.44, 1.728, 2.074, 2.488];

/// KaTeX HTML → class 布局样式内联化。只重写 `<span>`（KaTeX 产出全部是 span），
/// 其余标签（svg 等）原样保留；class/aria-hidden 删除，行内 style 置于 class 规则之后。
fn inline_styles(html: &str) -> String {
    let mut out = String::with_capacity(html.len() * 3);
    // 打开的 span 的 class 列表栈（供父子选择器求值）
    let mut stack: Vec<Vec<&str>> = Vec::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        if rest.starts_with("</") {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            if rest.starts_with("</span") {
                stack.pop();
            }
            rest = &rest[end..];
        } else if rest.starts_with("<span") && rest[5..].starts_with(['>', ' ', '/']) {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            let tag = &rest[..end];
            let classes: Vec<&str> = attr_of(tag, "class")
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let style = attr_of(tag, "style");

            let mut merged = style_decls(&classes, stack.last(), stack.iter().rev().nth(1));
            if let Some(s) = style {
                if !merged.is_empty() {
                    merged.push(';');
                }
                merged.push_str(s);
            }
            if merged.is_empty() {
                out.push_str("<span>");
            } else {
                out.push_str(&format!("<span style=\"{merged}\">"));
            }
            stack.push(classes);
            rest = &rest[end..];
        } else if rest.starts_with("<svg") {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            out.push_str(&merge_tag_style(
                &rest[..end],
                "display:block;position:absolute;width:100%;height:inherit;fill:currentColor;stroke:currentColor;fill-rule:nonzero;fill-opacity:1;stroke-width:1;stroke-linecap:butt;stroke-linejoin:miter;stroke-miterlimit:4;stroke-dasharray:none;stroke-dashoffset:0;stroke-opacity:1",
            ));
            rest = &rest[end..];
        } else if rest.starts_with("<path") {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            out.push_str(&merge_tag_style(&rest[..end], "stroke:none"));
            rest = &rest[end..];
        } else if rest.starts_with("<img") {
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            out.push_str(&merge_tag_style(
                &rest[..end],
                "border-style:none;min-width:0;min-height:0;max-width:none;max-height:none",
            ));
            rest = &rest[end..];
        } else {
            // KaTeX 其余标签整体原样保留。
            let end = rest.find('>').map(|i| i + 1).unwrap_or(rest.len());
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

/// 在保留 svg/viewBox 等必要属性的前提下合并行内样式。
fn merge_tag_style(tag: &str, extra: &str) -> String {
    if let Some(style) = attr_of(tag, "style") {
        let start = style.as_ptr() as usize - tag.as_ptr() as usize;
        let mut out = String::with_capacity(tag.len() + extra.len() + 1);
        out.push_str(&tag[..start]);
        out.push_str(style);
        if !style.is_empty() && !style.ends_with(';') {
            out.push(';');
        }
        out.push_str(extra);
        out.push_str(&tag[start + style.len()..]);
        out
    } else {
        let insert = tag
            .rfind("/>")
            .or_else(|| tag.rfind('>'))
            .unwrap_or(tag.len());
        format!("{} style=\"{extra}\"{}", &tag[..insert], &tag[insert..])
    }
}

fn attr_of<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let i = tag.find(&pat)? + pat.len();
    let j = i + tag[i..].find('"')?;
    Some(&tag[i..j])
}

fn decls(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// 对一个 span 求值全部适用规则。`classes` 自身 class，`parent`/`grand` 为栈上父/祖父。
fn style_decls(classes: &[&str], parent: Option<&Vec<&str>>, grand: Option<&Vec<&str>>) -> String {
    let mut d: Vec<(&str, String)> = Vec::new();
    let has = |c: &str| classes.contains(&c);
    let p = |c: &str| parent.is_some_and(|ps| ps.contains(&c));
    let gp = |c: &str| grand.is_some_and(|ps| ps.contains(&c));
    let mut v = |k: &'static str, val: &str| d.push((k, val.to_string()));

    // 根：KaTeX 的 font 简写还原为系统衬线栈（保持正常字重/样式，行高按 KaTeX 设计取 1.2）
    if has("katex") {
        v("display", "inline-block");
        // KaTeX 默认是 1.21em；此前漏掉后公式明显小于正文。
        v("font-size", "1.21em");
        v("font-weight", "400");
        v("font-style", "normal");
        v("font-family", SERIF);
        v("line-height", "1.2");
        v("text-indent", "0");
        v("text-rendering", "auto");
    }
    if has("katex-display") {
        v("display", "block");
        v("text-align", "center");
        v("text-indent", "0");
        v("margin", "0"); // 外层包装 section 负责间距
    }
    if has("katex") && p("katex-display") {
        v("display", "block");
        v("text-align", "center");
        v("white-space", "nowrap");
    }
    if has("katex-html") && p("katex") && gp("katex-display") {
        v("display", "block");
        v("position", "relative");
    }
    if has("newline") {
        v("display", "block");
    }

    // 基础盒
    if has("base") {
        v("display", "inline-block");
        v("position", "relative");
        v("white-space", "nowrap");
        v("width", "min-content");
    }
    if has("strut") {
        v("display", "inline-block");
    }
    if has("overlay") {
        v("display", "block");
    }
    if has("nulldelimiter") {
        v("display", "inline-block");
        v("width", ".12em");
    }

    // vlist 垂直堆叠体系（分式/上下标/根号内容物的骨架）
    if has("vlist-t") {
        v("display", "inline-table");
        v("table-layout", "fixed");
        v("border-collapse", "collapse");
        if p("accent") || p("op-limits") {
            v("text-align", "center");
        }
        if p("col-align-c") {
            v("text-align", "center");
        }
        if p("col-align-l") {
            v("text-align", "left");
        }
        if p("col-align-r") {
            v("text-align", "right");
        }
    }
    if has("vlist-t2") {
        v("margin-right", "-2px");
    }
    if has("vlist-r") {
        v("display", "table-row");
        // .mfrac > span > span（即 vlist-r）：text-align 靠继承传到全宽行块，分子/分母才居中
        if gp("mfrac") {
            v("text-align", "center");
        }
    }
    if has("vlist") {
        v("display", "table-cell");
        v("vertical-align", "bottom");
        v("position", "relative");
    }
    if has("vlist-s") {
        v("display", "table-cell");
        v("font-size", "1px");
        v("min-width", "2px");
        v("width", "2px");
        v("vertical-align", "bottom");
    }
    if p("vlist") {
        // .vlist > span：行
        v("display", "block");
        v("height", "0");
        v("position", "relative");
    } else if has("pstrut") {
        // .vlist > span > .pstrut（pstrut 只出现在 vlist 行内；须为 inline-block，高度才撑得起行盒锚点）
        v("display", "inline-block");
        v("overflow", "hidden");
        v("width", "0");
    } else if parent.is_none_or(|ps| ps.is_empty()) && gp("vlist") {
        // .vlist > span > span：行内容
        v("display", "inline-block");
    }
    if has("msupsub") {
        v("text-align", "left");
    }

    // 水平盒子
    if has("vbox") {
        v("display", "inline-flex");
        v("flex-direction", "column");
        v("align-items", "baseline");
    }
    if has("hbox") || has("thinbox") {
        v("display", "inline-flex");
        v("flex-direction", "row");
        if has("hbox") {
            v("width", "100%");
        }
        if has("thinbox") {
            v("max-width", "0");
            v("width", "0");
        }
    }
    if has("mspace") {
        v("display", "inline-block");
    }

    // 线条（分式线/上下划线/规则线）——行内 style 已带 border 宽度与颜色链到 currentColor
    if has("frac-line") {
        v("display", "inline-block");
        v("width", "100%");
        v("border-bottom-style", "solid");
        v("min-height", "1px");
        v("border-color", "currentColor");
    }
    if has("hline") || has("overline-line") || has("underline-line") {
        v("display", "inline-block");
        v("width", "100%");
        v("border-bottom-style", "solid");
        v("min-height", "1px");
        v("border-color", "currentColor");
    }
    if has("hdashline") {
        v("display", "inline-block");
        v("width", "100%");
        v("border-bottom-style", "dashed");
        v("min-height", "1px");
        v("border-color", "currentColor");
    }
    if has("rule") {
        v("display", "inline-block");
        v("position", "relative");
        v("border", "0 solid");
        v("min-height", "1px");
        v("border-color", "currentColor");
    }

    // lap 系（\llap/\rlap/\clap、拉伸箭头占位）
    if has("llap") || has("rlap") || has("clap") {
        v("position", "relative");
        v("width", "0");
    }
    if has("inner")
        && parent.is_some_and(|ps| ps.iter().any(|c| ["llap", "rlap", "clap"].contains(c)))
    {
        v("position", "absolute");
        if p("llap") {
            v("right", "0");
        }
        if p("rlap") || p("clap") {
            v("left", "0");
        }
    }
    if has("fix")
        && parent.is_some_and(|ps| ps.iter().any(|c| ["llap", "rlap", "clap"].contains(c)))
    {
        v("display", "inline-block");
    }
    if p("inner") && gp("clap") {
        v("margin-left", "-50%");
        v("margin-right", "50%");
    }

    // 定界符 / 大算符（KaTeX_Size* 高字形字体不可得，退到衬线栈，形高略降级）
    if has("delimsizing") {
        v("font-family", SERIF);
    }
    if has("delimcenter") || has("op-symbol") {
        v("position", "relative");
    }
    if has("op-symbol") && (has("small-op") || has("large-op")) {
        v("font-family", SERIF);
    }
    if has("delim-size1") || has("delim-size4") {
        // .delimsizing.mult .delim-size{1,4} > span
        if parent.is_some_and(|ps| {
            ps.iter()
                .any(|c| ["delim-size1", "delim-size4"].contains(c))
        }) {
            v("font-family", SERIF);
        }
    }
    if has("root") && p("sqrt") {
        v("margin-left", ".27778em");
        v("margin-right", "-.55556em");
    }
    if has("accent-body") {
        v("position", "relative");
        if !has("accent-full") {
            v("width", "0");
        }
    }

    // mtable
    if has("vertical-separator") {
        v("display", "inline-block");
        v("min-width", "1px");
        v("border-color", "currentColor");
    }
    if has("arraycolsep") {
        v("display", "inline-block");
    }

    // 拉伸装饰（svg/伪元素类构造，公众号场景降级保留骨架）
    if has("stretchy") {
        v("display", "block");
        v("overflow", "hidden");
        v("position", "relative");
        v("width", "100%");
    }
    if has("hide-tail") {
        v("overflow", "hidden");
        v("position", "relative");
        v("width", "100%");
    }
    if has("svg-align") {
        v("text-align", "left");
    }
    if has("halfarrow-left") {
        v("position", "absolute");
        v("overflow", "hidden");
        v("left", "0");
        v("width", "50.2%");
    }
    if has("halfarrow-right") {
        v("position", "absolute");
        v("overflow", "hidden");
        v("right", "0");
        v("width", "50.2%");
    }
    if has("brace-left") {
        v("position", "absolute");
        v("overflow", "hidden");
        v("left", "0");
        v("width", "25.1%");
    }
    if has("brace-center") {
        v("position", "absolute");
        v("overflow", "hidden");
        v("left", "25%");
        v("width", "50%");
    }
    if has("brace-right") {
        v("position", "absolute");
        v("overflow", "hidden");
        v("right", "0");
        v("width", "25.1%");
    }
    if has("mover") || has("munder") || has("x-arrow") {
        v("text-align", "center");
    }
    if has("x-arrow-pad") {
        v("padding", "0 .5em");
    }
    if has("boxpad") {
        v("padding", "0 .3em");
    }
    if has("cancel-pad") {
        v("padding", "0 .2em");
    }
    if has("cancel-lap") {
        v("margin-left", "-.2em");
        v("margin-right", "-.2em");
    }
    if has("fbox") || has("fcolorbox") {
        v("border", ".04em solid");
        v("border-color", "currentColor");
        v("box-sizing", "border-box");
    }
    if has("sout") {
        v("border-bottom-style", "solid");
        v("border-bottom-width", ".08em");
        v("border-color", "currentColor");
    }
    if has("angl") {
        v("border-right", ".049em solid");
        v("border-top", ".049em solid");
        v("border-color", "currentColor");
        v("box-sizing", "border-box");
        v("margin-right", ".03889em");
    }
    if has("anglpad") {
        v("padding", "0 .03889em");
    }
    if has("mtr-glue") {
        v("width", "50%");
    }

    // 字体族（KaTeX webfont → 系统栈）
    if has("mathnormal") {
        v("font-family", SERIF);
        v("font-style", "italic");
    }
    if has("mathit") {
        v("font-style", "italic");
    }
    if has("mathrm") || has("textrm") {
        v("font-family", SERIF);
        v("font-style", "normal");
    }
    if has("mathbf") || has("textbf") || has("boldsymbol") {
        v("font-weight", "700");
    }
    if has("boldsymbol") {
        v("font-style", "italic");
    }
    if has("textit") {
        v("font-style", "italic");
    }
    if has("mathcal")
        || has("mathscr")
        || has("textscr")
        || has("mathfrak")
        || has("textfrak")
        || has("mathboldfrak")
        || has("textboldfrak")
    {
        v("font-family", SERIF);
        if has("mathcal") || has("mathscr") || has("textscr") {
            v("font-style", "italic");
        }
        if has("mathboldfrak") || has("textboldfrak") {
            v("font-weight", "700");
        }
    }
    if has("amsrm") || has("mathbb") || has("textbb") {
        v("font-family", SERIF);
    }
    if has("mathtt") || has("texttt") {
        v("font-family", MONO);
    }
    if has("mathsf") || has("textsf") {
        v("font-family", SANS);
    }
    if has("mathboldsf") || has("textboldsf") {
        v("font-family", SANS);
        v("font-weight", "700");
    }
    if has("mathitsf") || has("textitsf") {
        v("font-family", SANS);
        v("font-style", "italic");
    }
    if has("mainrm") {
        v("font-family", SERIF);
        v("font-style", "normal");
    }

    // 字号命令（\small/\Large…）：reset-sizeN sizeM → 比例公式
    if has("sizing") || has("fontsize-ensurer") {
        let from = classes
            .iter()
            .find_map(|c| c.strip_prefix("reset-size"))
            .and_then(|n| n.parse::<usize>().ok());
        let to = classes
            .iter()
            .find_map(|c| c.strip_prefix("size").and_then(|n| n.parse::<usize>().ok()));
        if let (Some(n), Some(m)) = (from, to) {
            if (1..=11).contains(&n) && (1..=11).contains(&m) {
                v(
                    "font-size",
                    &em_ratio(KATEX_SIZES[m - 1] / KATEX_SIZES[n - 1]),
                );
            }
        }
    }

    decls(&d)
}

fn em_ratio(ratio: f64) -> String {
    let s = format!("{ratio:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}em")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_display_math ---

    #[test]
    fn normalize_single_line_display() {
        let md = "前文\n\n$$ Q_{net}=Q_{rt}+B_p-S_p $$\n\n后文";
        let out = normalize_display_math(md);
        assert!(out.contains("```math\nQ_{net}=Q_{rt}+B_p-S_p\n```"));
        assert!(out.starts_with("前文\n\n"));
        assert!(out.ends_with("\n\n后文"));
    }

    #[test]
    fn normalize_multiline_display_with_blank_lines() {
        let md = "$$\nQ_{net}=Q_{rt}+B_p-S_p\n\n=115{,}000\\text{ 股}\n$$";
        let out = normalize_display_math(md);
        assert!(out.contains("```math\nQ_{net}=Q_{rt}+B_p-S_p\n\n=115{,}000\\text{ 股}\n```"));
    }

    #[test]
    fn normalize_inside_code_fence_untouched() {
        let md = "```text\n$$ x $$\n```";
        assert_eq!(normalize_display_math(md), md);
    }

    #[test]
    fn normalize_non_line_start_untouched() {
        let md = "正文里 $$x$$ 的行内写法不动";
        assert_eq!(normalize_display_math(md), md);
    }

    #[test]
    fn normalize_indented_code_block_untouched() {
        let md = "    $$ x $$";
        assert_eq!(normalize_display_math(md), md);
    }

    #[test]
    fn normalize_list_indent_preserved() {
        let md = "- 列表\n  $$ y=1 $$";
        let out = normalize_display_math(md);
        assert!(out.contains("  ```math\n  y=1\n  ```"));
    }

    #[test]
    fn normalize_unclosed_untouched() {
        let md = "$$\nx=1\n没闭合";
        assert_eq!(normalize_display_math(md), md);
    }

    #[test]
    fn normalize_preserves_trailing_newline() {
        assert!(normalize_display_math("$$ x $$\n").ends_with("```\n"));
        assert!(!normalize_display_math("$$ x $$").ends_with('\n'));
    }

    #[test]
    fn normalize_closing_on_content_line() {
        let md = "$$\na + b\n= c$$";
        let out = normalize_display_math(md);
        assert!(out.contains("```math\na + b\n= c\n```"));
    }

    #[test]
    fn normalize_does_not_drop_text_after_closing_delimiter() {
        let single = "$$ x $$ 这段不能丢";
        assert_eq!(normalize_display_math(single), single);

        let multiline = "$$\nx=1\n$$ 后文";
        assert_eq!(normalize_display_math(multiline), multiline);
    }

    #[test]
    fn normalize_tab_indented_code_untouched() {
        let md = "\t$$ x $$";
        assert_eq!(normalize_display_math(md), md);
    }

    #[test]
    fn normalize_escaped_closing_delimiter_untouched() {
        let md = "$$ x \\$$";
        assert_eq!(normalize_display_math(md), md);
    }

    // --- split_inline_math ---

    #[test]
    fn split_basic_inline() {
        let segs = split_inline_math("持仓 $Q_{net}\\ge 0$ 恒成立");
        assert_eq!(
            segs,
            vec![
                Segment::Literal("持仓 "),
                Segment::Math {
                    latex: "Q_{net}\\ge 0",
                    display: false
                },
                Segment::Literal(" 恒成立"),
            ]
        );
    }

    #[test]
    fn split_display_inline() {
        let segs = split_inline_math("a $$x=1$$ b");
        assert!(matches!(
            segs.as_slice(),
            [
                Segment::Literal(_),
                Segment::Math { display: true, .. },
                Segment::Literal(_)
            ]
        ));
    }

    #[test]
    fn split_currency_not_math() {
        let segs = split_inline_math("价格 $5 和 $3 元，共 $8");
        assert!(segs.iter().all(|s| matches!(s, Segment::Literal(_))));
    }

    #[test]
    fn split_closing_digit_guard() {
        // $a$5 的闭合后紧跟数字 → 不当公式
        let segs = split_inline_math("$a$5");
        assert!(segs.iter().all(|s| matches!(s, Segment::Literal(_))));
    }

    #[test]
    fn split_escaped_dollar() {
        let segs = split_inline_math("价格 \\$5，只剩 \\$3");
        assert_eq!(segs, vec![Segment::Literal("价格 \\$5，只剩 \\$3")]);
    }

    #[test]
    fn split_unclosed_literal() {
        let segs = split_inline_math("只有 $ 一个");
        assert!(segs.iter().all(|s| matches!(s, Segment::Literal(_))));
    }

    #[test]
    fn split_multiple_formulas() {
        let segs = split_inline_math("$a$ 与 $b+c$");
        assert_eq!(
            segs,
            vec![
                Segment::Math {
                    latex: "a",
                    display: false
                },
                Segment::Literal(" 与 "),
                Segment::Math {
                    latex: "b+c",
                    display: false
                },
            ]
        );
    }

    // --- render_math ---

    fn assert_no_banned(html: &str) {
        for bad in ["class=", "id=", "<style", "<script", "aria-hidden"] {
            assert!(!html.contains(bad), "产出含 {bad}: {html}");
        }
    }

    #[test]
    fn render_display_user_formula() {
        let html = render_math(
            "Q_{net}=110{,}000+10{,}000-5{,}000=115{,}000\\text{ 股}",
            true,
        )
        .expect("合法公式应渲染成功");
        assert_no_banned(&html);
        assert!(html.contains("股"));
        assert!(
            html.contains("display:inline-table"),
            "应含 vlist 布局的内联样式: {html}"
        );
        assert!(
            html.contains("font-size:1.21em"),
            "公式字号应与 KaTeX 默认一致: {html}"
        );
    }

    #[test]
    fn render_inline_fraction() {
        let html = render_math("\\frac{Q_{rt}}{2}+B_p", false).expect("分式应渲染成功");
        assert_no_banned(&html);
        assert!(
            html.contains("border-bottom-style:solid"),
            "分式线应有内联边框样式: {html}"
        );
    }

    #[test]
    fn render_inline_mathnormal_italic() {
        let html = render_math("Q_{net}", false).expect("应渲染成功");
        assert!(
            html.contains("font-style:italic"),
            "数学变量应有衬线斜体: {html}"
        );
    }

    #[test]
    fn render_sizing_downsize_subscript() {
        // 下标走 sizing reset-size6 size3 → font-size:0.7em
        let html = render_math("Q_{net}", false).expect("应渲染成功");
        assert!(html.contains("font-size:0.7em"), "下标应缩至 0.7em: {html}");
    }

    #[test]
    fn render_sqrt_and_display_centered() {
        let html = render_math("\\sqrt{Q_{net}+1}", true).expect("应渲染成功");
        assert_no_banned(&html);
        assert!(html.contains("text-align:center"), "块级公式应居中");
        assert!(
            html.contains("fill:currentColor"),
            "根号 SVG 应带完整内联样式: {html}"
        );
        assert!(html.contains("stroke:none"), "SVG path 应禁用描边: {html}");
    }

    #[test]
    fn render_invalid_latex_returns_none() {
        assert_eq!(render_math("\\frac{", false), None);
        assert_eq!(render_math("\\unknownmacro{x}", false), None);
    }
}
