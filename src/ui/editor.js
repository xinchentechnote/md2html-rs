// 浏览器端编辑器逻辑（Rust 版适配）：渲染走本地服务 POST /api/render（core 在 Rust 侧），
// mermaid 兜底形态异步换成 /api/mermaid 渲染的图片，与 CLI 产出一致。
(function () {
  'use strict'

  var state = {
    theme: localStorage.getItem('md2html-theme') || '',
    mermaid: 'auto',
    file: null,
  }

  var mermaidCache = new Map()
  var renderSeq = 0
  var renderTimer = null

  function $(id) { return document.getElementById(id) }

  function init() {
    var select = $('theme')
    select.addEventListener('change', function () {
      state.theme = select.value
      localStorage.setItem('md2html-theme', state.theme)
      render()
    })

    fetch('/api/state')
      .then(function (r) { return r.json() })
      .then(function (data) {
        state.file = data.file
        state.mermaid = data.mermaid || 'auto'
        for (var t of data.themes || []) {
          var opt = document.createElement('option')
          opt.value = t.id
          opt.textContent = t.name
          select.appendChild(opt)
        }
        var saved = localStorage.getItem('md2html-theme')
        // 优先级：CLI -t > 上次选择 > 服务端默认主题（与 CLI 不传 -t 时一致）
        state.theme = data.theme || saved || data.defaultTheme
        if (!saved && data.theme) localStorage.setItem('md2html-theme', state.theme)
        select.value = state.theme
        $('editor').value = data.content || ''
        $('filename').textContent = data.file ? data.file.split(/[\\/]/).pop() : '未命名（用 md2html ui 文件.md 打开可编辑保存）'
        if (!data.file) $('save').disabled = true
        updateCount()
        render()
      })
      .catch(function () { $('filename').textContent = '加载失败' })

    $('editor').addEventListener('input', function () {
      markDirty()
      updateCount()
      clearTimeout(renderTimer)
      renderTimer = setTimeout(render, 300)
    })

    // Tab 缩进（textarea 默认会跳出焦点）
    $('editor').addEventListener('keydown', function (e) {
      if (e.key !== 'Tab') return
      e.preventDefault()
      var el = e.target
      var a = el.selectionStart, b = el.selectionEnd, value = el.value
      el.value = value.slice(0, a) + '  ' + value.slice(b)
      el.selectionStart = el.selectionEnd = a + 2
      markDirty()
      render()
    })

    document.addEventListener('keydown', function (e) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        save()
      }
    })

    $('save').addEventListener('click', save)
    $('copy').addEventListener('click', copyArticle)
  }

  function render() {
    var seq = ++renderSeq
    fetch('/api/render', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ markdown: $('editor').value, theme: state.theme }),
    })
      .then(function (r) { return r.json() })
      .then(function (data) {
        if (seq !== renderSeq) return // 已被更新的渲染取代，丢弃过期结果
        if (data.error) throw new Error(data.error)
        $('preview').innerHTML = data.html
        $('error').style.display = 'none'
        hydrateMermaid(seq)
      })
      .catch(function (err) {
        $('error').textContent = '渲染出错: ' + err.message
        $('error').style.display = 'block'
      })
  }

  /** 把预览里的 mermaid 兜底形态（代码卡+标记）异步换成服务端渲染的图片，与 CLI 产出一致 */
  function hydrateMermaid(seq) {
    if (state.mermaid === 'off') return
    var holders = Array.prototype.slice.call(document.querySelectorAll('#preview [data-md2html-mermaid]'))
    holders.reduce(function (chain, el) {
      return chain.then(function () {
        var b64 = el.getAttribute('data-md2html-mermaid')
        return fetchMermaidPng(b64ToUtf8(b64)).then(function (res) {
          if (seq !== renderSeq) return // 已被更新的渲染取代，丢弃过期结果
          if (res.png) {
            var card = document.createElement('section')
            card.style.cssText = 'background:#FFFFFF;border-radius:12px;padding:12px 8px;margin:16px 0'
            var img = new Image()
            img.src = 'data:image/png;base64,' + res.png
            img.alt = 'mermaid 图表'
            img.style.cssText = 'max-width:100%;display:block;margin:0 auto'
            card.appendChild(img)
            el.replaceWith(card)
          }
          // res.error：保留代码卡兜底
        })
      })
    }, Promise.resolve())
  }

  function fetchMermaidPng(code) {
    var key = state.theme + '::' + code
    if (!mermaidCache.has(key)) {
      var entry = fetch('/api/mermaid', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ code: code, theme: state.theme }),
      })
        .then(function (r) {
          return r.json().then(function (j) {
            if (!r.ok) throw new Error(j.error || 'HTTP ' + r.status)
            return { png: j.png }
          })
        })
        .catch(function (err) { return { error: err.message } })
      mermaidCache.set(key, entry)
    }
    return mermaidCache.get(key)
  }

  function b64ToUtf8(b64) {
    var bin = atob(b64)
    var bytes = Uint8Array.from(bin, function (c) { return c.charCodeAt(0) })
    return new TextDecoder().decode(bytes)
  }

  function markDirty() {
    $('status').textContent = '● 未保存'
  }

  function save() {
    if (!state.file) return
    fetch('/api/file', { method: 'PUT', body: $('editor').value })
      .then(function () {
        $('status').textContent = '✓ 已保存 ' + new Date().toLocaleTimeString()
      })
      .catch(function (err) {
        $('status').textContent = '保存失败: ' + err.message
      })
  }

  function copyArticle() {
    // 与预览页/publish 一致：剥离头卡（标题走公众号后台标题栏）；页脚开启时视为内容一并复制
    var clone = $('preview').cloneNode(true)
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
    var btn = $('copy')
    btn.textContent = '已复制 ✓'
    setTimeout(function () { btn.textContent = '复制到公众号' }, 2000)
  }

  function updateCount() {
    $('count').textContent = $('editor').value.length + ' 字'
  }

  init()
})()
