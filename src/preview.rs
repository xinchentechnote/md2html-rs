//! 预览页壳：677px 内容列对齐公众号正文实际渲染宽度；
//! 复制按钮把正文区域的富文本（text/html）写入剪贴板。
//! 壳内 <style>/<script> 属于预览层，永远不会进入正文片段。

const HEAD: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>"##;

const MIDDLE: &str = r##"</title>
<style>
body{margin:0;background:#EDEDED;padding:36px 16px;font-family:-apple-system,'PingFang SC',sans-serif}
.wrap{max-width:677px;margin:0 auto;background:#FFFFFF;border-radius:12px;padding:40px 32px;box-sizing:border-box}
.fab{position:fixed;right:32px;bottom:32px;display:flex;flex-direction:column;align-items:center;gap:6px}
.fab button{border:none;background:#07C160;color:#FFFFFF;font-size:15px;font-weight:600;padding:13px 22px;border-radius:999px;box-shadow:0 4px 14px rgba(7,193,96,.4);cursor:pointer}
.fab button:active{transform:scale(.96)}
.fab span{font-size:12px;color:#888;background:rgba(255,255,255,.92);padding:2px 8px;border-radius:6px}
</style>
</head>
<body>
<div class="wrap" id="article">"##;

const TAIL: &str = r##"</div>
<div class="fab">
<button id="copy" type="button">复制到公众号</button>
<span>点击后到公众号编辑器 ⌘V 粘贴</span>
</div>
<script>
document.getElementById('copy').addEventListener('click', function () {
  // 克隆正文并剥离头卡（标题走后台标题栏）；页脚默认不渲染，开启时视为内容一并复制
  var article = document.getElementById('article')
  var clone = article.cloneNode(true)
  clone.querySelectorAll('[data-md2html-block="header"]').forEach(function (n) { n.remove() })
  var holder = document.createElement('div')
  holder.style.cssText = 'position:fixed;left:-9999px;top:0'
  holder.appendChild(clone)
  document.body.appendChild(holder)
  var range = document.createRange()
  range.selectNodeContents(clone)
  var sel = window.getSelection()
  sel.removeAllRanges()
  sel.addRange(range)
  document.execCommand('copy')
  sel.removeAllRanges()
  holder.remove()
  this.textContent = '已复制 ✓'
  var btn = this
  setTimeout(function () { btn.textContent = '复制到公众号' }, 2000)
})
</script>
</body>
</html>"##;

pub fn preview_page(article_html: &str, theme_name: &str) -> String {
    let title = if theme_name.is_empty() {
        "md2html 预览".to_string()
    } else {
        format!("md2html 预览 · {theme_name}")
    };
    format!("{HEAD}{title}{MIDDLE}{}{TAIL}", article_html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_contains_article_and_copy_button() {
        let page = preview_page("<p>hi</p>", "深海蓝");
        assert!(page.contains("<title>md2html 预览 · 深海蓝</title>"));
        assert!(page.contains(r#"<div class="wrap" id="article"><p>hi</p></div>"#));
        assert!(page.contains("复制到公众号"));
    }
}
