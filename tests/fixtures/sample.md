# 让技术写作回归写作本身

> **效率**不是写得更快，而是不为排版消耗第二次时间。

## WHY 为什么需要它

把 Markdown 变成**公众号可发布的排版**，中间隔着三道坎：字体被编辑器重置、代码样式丢失、图片排版错乱。md2html 用一条命令闭合这条链路，样式全部内联，粘贴不丢。

## HOW 它是怎么工作的

整条链路是纯规则转换：`parse` → `transform` → `render`，不调用任何模型，结果完全可复现。

### 三步管线

1. 解析成标准 AST
2. 结构性预处理（章节自动编号、图集分组）
3. 组件渲染并内联样式

### 我们不做什么

- 不调用任何 LLM API
- 不依赖公众号登录态
- 不做云端存储，文件不出本机

### 待办清单

- [x] 代码高亮内联
- [x] 快照测试
- [ ] 数学公式（二期）
- [ ] UI 编辑器（二期）

## 纯中文标题的编号验证

没有英文标签的章节，只保留序号与标题，行内元素依旧生效：*斜体*、~~删除线~~、`inline_code`、[一个链接](https://example.com)。

### 多级清单

- 一级要点：渲染链路
  - 二级补充：解析与变换
    - 三级细节：章节编号与图集分组
- 另一个一级要点：主题系统

### 宽表格

| 功能 | 平台 | 状态 | 负责 | 预计 |
| :--- | :--- | :--- | :--- | :--- |
| 主题系统 | 全平台 | ✅ | 前端 | 26Q3 |
| UI 编辑器 | Web | 🔜 | 前端 | 26Q4 |

## WHAT 你会得到什么

### 能力一览

| 能力 | 状态 | 说明 |
| :--- | :---: | --- |
| 多主题 | ✅ | designVars 色板，加主题不碰渲染器 |
| 代码高亮 | ✅ | 跨行 token 颜色栈，行结构不丢 |
| 快照测试 | ✅ | node:test，回归零成本 |
| UI 预览 | 🔜 | 二期，双击即用的单文件编辑器 |

---

### 代码高亮示例

```js
// 跨行注释验证颜色栈：
// 第二行依然是注释色
function highlight(node, stack = []) {
  if (node.type === 'text') return splitLines(node.value, stack.at(-1))
  const color = styleOf(node) ?? stack.at(-1)
  return node.children.flatMap((child) => highlight(child, [...stack, color]))
}

const RESULT = highlight(tree).join('\n') // → 每行一个 <span>
```

### 图片与图集

![单图示例：封面配图](https://images.example.com/cover.png)

![图集一](https://images.example.com/a.png)
![图集二](https://images.example.com/b.png)
![图集三](https://images.example.com/c.png)

## END 开始使用

```bash
npm link
md2html article.md -t moyu --open
```

> 少一次排版，多一次创作。
