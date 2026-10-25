#!/usr/bin/env node
/**
 * website 文档页生成器：把仓库 markdown 教程渲染为 website/docs/** 静态 HTML。
 *
 * 源（唯一真源）→ 输出：
 *   docs/USER_GUIDE.md          → website/docs/user-guide/index.html
 *   docs/en/USER_GUIDE.md       → website/docs/en/user-guide/index.html
 *   docs/MIGRATION.md           → website/docs/migration/index.html
 *   docs/en/MIGRATION.md        → website/docs/en/migration/index.html
 *   docs/AGENT.md               → website/docs/agent/index.html
 *   docs/en/AGENT.md            → website/docs/en/agent/index.html
 *   CHANGELOG.md                → website/docs/changelog/index.html（仅中文）
 *
 * 链接改写规则（渲染前对 markdown 源做相对路径解析）：
 *   - 属于上表的 md      → 站内相对目录链接（尾斜杠，GitHub Pages 目录 URL）
 *   - docs/imags 图片    → website/assets 同名图（home/api-home 已存在）
 *   - 其余相对 md/资源   → GitHub blob 绝对地址
 *   - http(s)/#锚点/mailto 保持不变
 *
 * 输出目录不入库（.gitignore），由 CI 在上传 Pages artifact 前生成；
 * 本地预览：npm ci --prefix scripts && node scripts/build_site_docs.mjs
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { marked } from 'marked'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const SITE = path.join(ROOT, 'website')
const SITE_BASE = 'https://weihubeats.github.io/Miyo'
const GH_BLOB = 'https://github.com/weihubeats/Miyo/blob/main/'

/** 站内文档页注册表：src 为相对仓库根的 markdown 路径。 */
const PAGES = [
  { src: 'docs/USER_GUIDE.md', slug: 'user-guide', lang: 'zh', title: '用户手册', pair: 'docs/en/USER_GUIDE.md', desc: '安装、界面、请求、环境、测试、Mock、备份——Miyo 完整使用手册。' },
  { src: 'docs/en/USER_GUIDE.md', slug: 'user-guide', lang: 'en', title: 'User Guide', pair: 'docs/USER_GUIDE.md', desc: 'Install, UI, requests, environments, testing, mock, backup — the complete Miyo user guide.' },
  { src: 'docs/MIGRATION.md', slug: 'migration', lang: 'zh', title: '迁移手册', pair: 'docs/en/MIGRATION.md', desc: '从 Apifox / Postman 迁移到 Miyo：导出、导入、环境变量与认证对照。' },
  { src: 'docs/en/MIGRATION.md', slug: 'migration', lang: 'en', title: 'Migration Guide', pair: 'docs/MIGRATION.md', desc: 'Migrate from Apifox / Postman to Miyo: export, import, environments and auth mapping.' },
  { src: 'docs/AGENT.md', slug: 'agent', lang: 'zh', title: 'Agent 集成', pair: 'docs/en/AGENT.md', desc: 'MCP Server 与本机 HTTP 控制面：一句话把 cURL 或后端代码保存为接口。' },
  { src: 'docs/en/AGENT.md', slug: 'agent', lang: 'en', title: 'Agent Integration', pair: 'docs/AGENT.md', desc: 'MCP server and local HTTP control plane: save a cURL or backend snippet as an endpoint.' },
  { src: 'docs/CLI.md', slug: 'cli', lang: 'zh', title: 'CLI 测试运行器', pair: 'docs/en/CLI.md', desc: 'miyo-cli：在终端 / CI 无头运行测试集合，输出 JUnit 报告，失败即流水线红。' },
  { src: 'docs/en/CLI.md', slug: 'cli', lang: 'en', title: 'CLI Test Runner', pair: 'docs/CLI.md', desc: 'miyo-cli: run API collections headlessly in CI with JUnit reports — fail the pipeline on regressions.' },
  { src: 'CHANGELOG.md', slug: 'changelog', lang: 'zh', title: '更新日志', pair: null, desc: 'Miyo 各版本变化速览。' },
]

const bySrc = new Map(PAGES.map((p) => [p.src, p]))

/** 输出页相对 website/ 的目录路径（不带文件名）。 */
function outDirOf(page) {
  return page.lang === 'en' ? path.posix.join('docs/en', page.slug) : path.posix.join('docs', page.slug)
}

/** 输出页相对 website/ 的 index.html 路径。 */
function outFileOf(page) {
  return path.posix.join(outDirOf(page), 'index.html')
}

function encodePath(p) {
  return p.split('/').map(encodeURIComponent).join('/')
}

/**
 * 改写 markdown 源里的相对链接/图片。
 * @param {string} md markdown 源
 * @param {string} srcPath 该 md 相对仓库根路径（posix）
 */
function rewriteLinks(md, srcPath) {
  const srcDir = path.posix.dirname(srcPath)
  const outFile = outFileOf(bySrc.get(srcPath))
  const fromDir = path.posix.dirname(outFile)

  return md.replace(/(!?)\[(.*?)\]\(([^)\s]+)(\s+"[^"]*")?\)/g, (whole, bang, text, url, title = '') => {
    if (/^(https?:|mailto:|#)/i.test(url)) return whole
    const [pathPart, anchor = ''] = url.split('#')
    const resolved = pathPosixNormalize(pathPosixJoin(srcDir, pathPart))
    const suffix = anchor ? `#${anchor}` : ''

    const target = bySrc.get(resolved)
    if (target) {
      const rel = siteRel(fromDir, path.posix.dirname(outFileOf(target)))
      return `${bang}[${text}](${rel}/${suffix}${title})`.replace('/#', '/#')
    }

    // docs/imags/*.png → website/assets 同名文件（存在才改写）
    const assetMatch = resolved.match(/^(?:docs\/imags|imags)\/(.+)$/)
    if (assetMatch) {
      const assetRel = path.posix.join('assets', assetMatch[1])
      if (existsSync(path.join(SITE, assetRel))) {
        const rel = siteRel(fromDir, 'assets')
        return `${bang}[${text}](${rel}/${assetMatch[1]}${suffix}${title})`
      }
    }

    // 其余相对引用 → GitHub blob
    const abs = /^README|CHANGELOG|docs\//.test(resolved) || !resolved.includes('/')
      ? GH_BLOB + encodePath(resolved)
      : GH_BLOB + encodePath(resolved)
    return `${bang}[${text}](${abs}${suffix}${title})`
  })
}

function pathPosixJoin(dir, rel) {
  return path.posix.normalize(path.posix.join(dir === '.' ? '' : dir, rel))
}
function pathPosixNormalize(p) {
  return path.posix.normalize(p).replace(/^\.\//, '')
}

/** 从 fromDir（website 内目录）到 toDir（website 内目录）的相对路径。 */
function siteRel(fromDir, toDir) {
  const rel = path.posix.relative(fromDir === '.' ? '' : fromDir, toDir === '.' ? '' : toDir)
  return rel === '' ? '.' : rel
}

const slugCounts = new Map()
function slugify(text) {
  const base = text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '')
  const n = slugCounts.get(base) ?? 0
  slugCounts.set(base, n + 1)
  return n === 0 ? base : `${base}-${n + 1}`
}

function decodeEntities(s) {
  return s
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
}

function stripInline(s) {
  return decodeEntities(s.replace(/<[^>]*>/g, '').replace(/[`*_~]/g, ''))
}

/** 渲染单页，返回 { html, toc }。 */
function render(md, ctx) {
  const toc = []
  slugCounts.clear()
  marked.use({
    renderer: {
      heading(token) {
        const inner = this.parser.parseInline(token.tokens)
        const id = slugify(stripInline(inner))
        toc.push({ depth: token.depth, id, text: stripInline(inner) })
        return `<h${token.depth} id="${id}">${inner}</h${token.depth}>\n`
      },
    },
  })
  const body = marked.parse(md, { async: false, gfm: true })
  return { html: body, toc }
}

const CSS = `
:root {
  --bg: #0d1117; --bg-soft: #161b22; --border: #30363d;
  --text: #e6edf3; --muted: #8b949e;
  --accent: #f78166; --accent-strong: #ff7b29; --green: #3fb950;
}
* { margin: 0; padding: 0; box-sizing: border-box; }
html { scroll-behavior: smooth; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", Helvetica, Arial, sans-serif; background: var(--bg); color: var(--text); line-height: 1.7; }
a { color: var(--accent); text-decoration: none; }
a:hover { text-decoration: underline; }
.container { max-width: 1080px; margin: 0 auto; padding: 0 24px; }
nav { position: sticky; top: 0; z-index: 10; background: rgba(13,17,23,.85); backdrop-filter: blur(8px); border-bottom: 1px solid var(--border); }
.nav-inner { display: flex; align-items: center; justify-content: space-between; height: 60px; }
.brand { display: flex; align-items: center; gap: 8px; font-weight: 700; font-size: 18px; color: var(--text); }
.brand:hover { text-decoration: none; }
.brand img { width: 22px; height: 22px; }
.nav-links { display: flex; gap: 24px; font-size: 14px; align-items: center; }
.nav-links a { color: var(--muted); }
.nav-links a:hover { color: var(--text); text-decoration: none; }
.nav-links a.active { color: var(--accent); }
.crumbs { font-size: 13px; color: var(--muted); padding: 20px 0 0; }
.crumbs a { color: var(--muted); }
.crumbs a:hover { color: var(--text); }
.doc-layout { display: grid; grid-template-columns: 250px minmax(0, 1fr); gap: 40px; max-width: 1120px; margin: 0 auto; padding: 16px 24px 72px; }
.doc-toc { position: sticky; top: 76px; align-self: start; max-height: calc(100vh - 100px); overflow-y: auto; font-size: 13px; line-height: 1.5; border-right: 1px solid var(--border); padding-right: 14px; scrollbar-width: thin; scrollbar-color: transparent transparent; }
.doc-toc:hover { scrollbar-color: var(--border) transparent; }
.doc-toc::-webkit-scrollbar { width: 6px; }
.doc-toc::-webkit-scrollbar-track { background: transparent; }
.doc-toc::-webkit-scrollbar-thumb { background: transparent; border-radius: 3px; }
.doc-toc:hover::-webkit-scrollbar-thumb { background: var(--border); }
.doc-toc .toc-title { font-weight: 700; color: var(--text); margin-bottom: 10px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; }
.doc-toc ul { list-style: none; }
.doc-toc li { margin: 1px 0; }
.doc-toc a { color: var(--muted); display: block; padding: 4px 8px; border-radius: 6px; border-left: 2px solid transparent; }
.doc-toc a:hover { color: var(--text); background: var(--bg-soft); text-decoration: none; }
.doc-toc a.active { color: var(--accent); background: rgba(247,129,102,.08); border-left-color: var(--accent); }
.doc-toc .lv3 { padding-left: 18px; font-size: 12.5px; }
.doc-content { min-width: 0; max-width: 820px; }
.doc-content h1 { font-size: 34px; line-height: 1.25; letter-spacing: -.4px; margin: 8px 0 18px; scroll-margin-top: 76px; }
.doc-content h2 { font-size: 23px; line-height: 1.35; margin: 44px 0 14px; padding-top: 18px; border-top: 1px solid var(--border); scroll-margin-top: 76px; }
.doc-content > hr + h2 { border-top: none; padding-top: 0; margin-top: 8px; }
.doc-content h3 { font-size: 17.5px; margin: 28px 0 10px; scroll-margin-top: 76px; }
.doc-content p { margin: 12px 0; color: var(--text); font-size: 15px; }
.doc-content ul, .doc-content ol { margin: 12px 0 12px 24px; font-size: 15px; }
.doc-content li { margin: 6px 0; }
.doc-content li > p { margin: 2px 0; }
.doc-content strong { color: #fff; }
.doc-content hr { border: none; border-top: 1px solid var(--border); margin: 32px 0; }
.doc-content blockquote { margin: 16px 0; padding: 12px 16px; border-left: 3px solid var(--accent); background: var(--bg-soft); border-radius: 0 8px 8px 0; color: var(--muted); }
.doc-content blockquote p { color: var(--muted); margin: 6px 0; font-size: 14px; }
.doc-content code { background: rgba(110,118,129,.24); padding: 1px 6px; border-radius: 6px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 13px; }
.doc-content pre { background: #0a0d12; border: 1px solid var(--border); border-radius: 12px; padding: 16px 18px; overflow-x: auto; margin: 16px 0; }
.doc-content pre code { background: none; padding: 0; font-size: 13px; color: #c9d1d9; }
.table-wrap { overflow-x: auto; border: 1px solid var(--border); border-radius: 12px; margin: 16px 0; }
.doc-content table { width: 100%; border-collapse: collapse; font-size: 14px; min-width: 480px; }
.doc-content th, .doc-content td { padding: 10px 14px; text-align: left; border-bottom: 1px solid var(--border); vertical-align: top; }
.doc-content th { background: var(--bg-soft); font-weight: 600; color: #fff; }
.doc-content tr:last-child td { border-bottom: none; }
.doc-content img { max-width: 100%; border-radius: 12px; border: 1px solid var(--border); margin: 8px 0; }
.doc-content details { background: var(--bg-soft); border: 1px solid var(--border); border-radius: 12px; padding: 12px 16px; margin: 16px 0; }
.doc-content summary { cursor: pointer; font-weight: 600; }
.doc-meta { font-size: 13px; color: var(--muted); margin-top: 48px; padding-top: 16px; border-top: 1px solid var(--border); }
footer { border-top: 1px solid var(--border); padding: 32px 0; text-align: center; color: var(--muted); font-size: 14px; }
footer a { color: var(--muted); }
footer a:hover { color: var(--text); }
@media (max-width: 900px) {
  .doc-layout { grid-template-columns: 1fr; gap: 8px; }
  .doc-toc { position: static; max-height: none; border-right: none; border-bottom: 1px solid var(--border); padding: 0 0 12px; }
  .doc-toc a.active { border-left-color: transparent; }
  .doc-content { max-width: none; }
  .doc-content h1 { font-size: 28px; }
}
`.trim()

function escapeHtml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

function renderToc(toc, lang) {
  const items = toc
    .filter((h) => h.depth >= 2 && h.depth <= 3)
    .map((h) => `<li class="lv${h.depth}"><a href="#${h.id}">${escapeHtml(h.text)}</a></li>`)
    .join('\n')
  const label = lang === 'zh' ? '目录' : 'On this page'
  return `<nav class="doc-toc" aria-label="Table of contents">\n<div class="toc-title">${label}</div>\n<ul>\n${items}\n</ul>\n</nav>`
}

function langLabel(page) {
  return page.lang === 'zh' ? 'zh-CN' : 'en'
}

function buildPage(page, md) {
  slugCounts.clear()
  const rewritten = rewriteLinks(md, page.src)
  const { html, toc } = render(rewritten, page)
  const outRel = outFileOf(page)
  const fromDir = path.posix.dirname(outRel)

  // 首页（按语言）与配对语言页
  const homeRel = siteRel(fromDir, page.lang === 'zh' ? 'zh' : '.')
  const homeHref = `${homeRel === '.' ? './' : homeRel + '/'}`
  const pairPage = page.pair ? bySrc.get(page.pair) : null
  const pairHref = pairPage ? `${siteRel(fromDir, path.posix.dirname(outFileOf(pairPage)))}/` : null

  const rootPrefix = page.lang === 'zh' ? '' : '' // favicon/assets 相对路径单独算
  const assetRel = siteRel(fromDir, 'assets')

  const canonical = `${SITE_BASE}/${outRel.replace(/index\.html$/, '')}`
  const pairCanonical = pairPage ? `${SITE_BASE}/${outFileOf(pairPage).replace(/index\.html$/, '')}` : null
  // x-default 统一指向英文版（站点根为英文；与 sitemap 条目对齐）。
  const xDefault = page.lang === 'zh' && pairCanonical ? pairCanonical : canonical
  const selfLang = page.lang === 'zh' ? 'zh-Hans' : 'en'
  const pairLang = page.lang === 'zh' ? 'en' : 'zh-Hans'
  const hreflangs =
    `\n<link rel="alternate" hreflang="${selfLang}" href="${canonical}">` +
    (pairCanonical ? `\n<link rel="alternate" hreflang="${pairLang}" href="${pairCanonical}">` : '') +
    `\n<link rel="alternate" hreflang="x-default" href="${xDefault}">`

  const desc = page.desc
  const ogImage = `${SITE_BASE}/assets/home.png`
  const ogLocale = page.lang === 'zh' ? 'zh_CN' : 'en_US'
  const pageLangAttr = langLabel(page)

  const homeUrl = page.lang === 'zh' ? `${SITE_BASE}/zh/` : `${SITE_BASE}/`
  const docsUrl = page.lang === 'zh' ? `${SITE_BASE}/zh/#docs` : `${SITE_BASE}/#docs`
  const jsonLd = JSON.stringify({
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'TechArticle',
        headline: `${page.title} — Miyo`,
        description: desc,
        image: ogImage,
        inLanguage: pageLangAttr,
        url: canonical,
        dateModified: new Date().toISOString().slice(0, 10),
        publisher: {
          '@type': 'Organization',
          name: 'Miyo',
          url: 'https://github.com/weihubeats/Miyo',
        },
      },
      {
        '@type': 'BreadcrumbList',
        itemListElement: [
          { '@type': 'ListItem', position: 1, name: page.lang === 'zh' ? '首页' : 'Home', item: homeUrl },
          { '@type': 'ListItem', position: 2, name: page.lang === 'zh' ? '文档' : 'Docs', item: docsUrl },
          { '@type': 'ListItem', position: 3, name: page.title, item: canonical },
        ],
      },
    ],
  })

  const homeZh = `${siteRel(fromDir, 'zh')}/`
  const homeEn = `${siteRel(fromDir, '.')}/`
  const docsAnchor = `${homeHref}#docs`

  const htmlOut = `<!DOCTYPE html>
<html lang="${pageLangAttr}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>${escapeHtml(page.title)} — Miyo</title>
<meta name="description" content="${escapeHtml(desc)}">
<meta name="robots" content="index,follow">
<link rel="canonical" href="${canonical}">${hreflangs}
<link rel="icon" type="image/png" href="${assetRel}/favicon.png">
<meta name="theme-color" content="#0d1117">
<meta property="og:type" content="article">
<meta property="og:site_name" content="Miyo">
<meta property="og:locale" content="${ogLocale}">
<meta property="og:url" content="${canonical}">
<meta property="og:title" content="${escapeHtml(page.title)} — Miyo">
<meta property="og:description" content="${escapeHtml(desc)}">
<meta property="og:image" content="${ogImage}">
<meta property="og:image:width" content="2880">
<meta property="og:image:height" content="1600">
<meta property="og:image:alt" content="Miyo — lightweight cross-platform API client">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="${escapeHtml(page.title)} — Miyo">
<meta name="twitter:description" content="${escapeHtml(desc)}">
<meta name="twitter:image" content="${ogImage}">
<script type="application/ld+json">
${jsonLd}
</script>
<style>
${CSS}
</style>
</head>
<body>

<nav>
  <div class="container nav-inner">
    <a class="brand" href="${homeHref}"><img src="${assetRel}/logo.png" alt=""> Miyo</a>
    <div class="nav-links">
      <a class="active" href="${docsAnchor}">${page.lang === 'zh' ? '文档' : 'Docs'}</a>
      ${pairHref ? `<a href="${pairHref}">${page.lang === 'zh' ? 'English' : '中文'}</a>` : `<a href="${page.lang === 'en' ? homeZh : homeEn}">${page.lang === 'zh' ? 'English' : '中文'}</a>`}
      <a href="https://github.com/weihubeats/Miyo">GitHub</a>
    </div>
  </div>
</nav>

<div class="container crumbs">
  <a href="${homeHref}">${page.lang === 'zh' ? '首页' : 'Home'}</a> / <a href="${docsAnchor}">${page.lang === 'zh' ? '文档' : 'Docs'}</a> / ${escapeHtml(page.title)}
</div>

<div class="doc-layout">
${renderToc(toc, page.lang)}
<article class="doc-content">
${html}
<p class="doc-meta"><a href="${GH_BLOB}${page.src}">${page.lang === 'zh' ? '在 GitHub 上查看本页源文' : 'View this page\'s source on GitHub'}</a></p>
</article>
</div>

<footer>
  <div class="container">
    <p><a href="https://github.com/weihubeats/Miyo">GitHub</a> · <a href="https://github.com/weihubeats/Miyo/releases">Releases</a> · <a href="https://github.com/weihubeats/Miyo/blob/main/LICENSE">Apache-2.0 License</a></p>
    <p>Built with 🦊 &amp; Rust</p>
  </div>
</footer>

<script>
(function () {
  var links = Array.prototype.slice.call(document.querySelectorAll('.doc-toc a[href^="#"]'));
  if (!links.length || !('IntersectionObserver' in window)) return;
  var byId = new Map();
  links.forEach(function (a) {
    var el = document.getElementById(decodeURIComponent(a.getAttribute('href').slice(1)));
    if (el) byId.set(el, a);
  });
  var current = null;
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (!e.isIntersecting) return;
      if (current) current.classList.remove('active');
      current = byId.get(e.target) || null;
      if (current) current.classList.add('active');
    });
  }, { rootMargin: '-80px 0px -65% 0px', threshold: 0 });
  byId.forEach(function (_, el) { io.observe(el); });
})();
</script>

</body>
</html>
`

  const outFile = path.join(SITE, outRel)
  mkdirSync(path.dirname(outFile), { recursive: true })
  writeFileSync(outFile, htmlOut)
  return outRel
}

function main() {
  if (!existsSync(path.join(ROOT, 'scripts/node_modules/marked'))) {
    console.error('missing dependency: run `npm ci --prefix scripts` first')
    process.exit(1)
  }
  const outputs = []
  for (const page of PAGES) {
    const src = path.join(ROOT, page.src)
    if (!existsSync(src)) {
      console.error(`missing source: ${page.src}`)
      process.exit(1)
    }
    const md = readFileSync(src, 'utf8')
    outputs.push(buildPage(page, md))
  }
  console.log(`generated ${outputs.length} pages:`)
  for (const o of outputs) console.log(`  website/${o}`)
}

main()
