import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { panelLanguage, translate } from '../src/locale.js'

const file = (name: string) => JSON.parse(readFileSync(new URL(`../../../locale/${name}.json`, import.meta.url), 'utf8')) as { panel: Record<string, string> }

describe('panel language files (#42)', () => {
  it('ship the same panel keys in Chinese and English', () => {
    expect(Object.keys(file('zh').panel).sort()).toEqual(Object.keys(file('en').panel).sort())
  })

  it('keep every placeholder in both languages', () => {
    const en = file('en').panel
    const zh = file('zh').panel
    const holes = (text: string) => (text.match(/\{\w+\}/g) ?? []).sort()
    for (const key of Object.keys(en)) expect(holes(zh[key]!), key).toEqual(holes(en[key]!))
  })

  it('follows DSH Chinese variants and falls back to English otherwise', () => {
    expect(panelLanguage('zh')).toBe('zh')
    expect(panelLanguage('zh-CN')).toBe('zh')
    expect(panelLanguage('ZH-tw')).toBe('zh')
    expect(panelLanguage('en')).toBe('en')
    expect(panelLanguage('fr')).toBe('en')
    expect(panelLanguage('ja')).toBe('en')
    expect(panelLanguage(undefined)).toBe('en')
  })

  it('uses the glossary terms in Chinese', () => {
    expect(translate('zh', 'menu.hide')).toBe('隐藏')
    expect(translate('zh', 'card.label', { title: 'Catalog' })).toBe('类卡片 Catalog')
    expect(translate('zh', 'source.label', { title: 'a.ts' })).toBe('源码窗口 a.ts')
    expect(translate('zh', 'summary.source')).toBe('源码图')
    expect(translate('en', 'card.label', { title: 'Catalog' })).toBe('Class card Catalog')
    // #45: the breadcrumb path and the canvas status codes.
    expect(translate('zh', 'breadcrumbs.label')).toBe('面包屑路径')
    expect(translate('zh', 'breadcrumb.go', { name: 'core' })).toBe('前往 core')
    expect(translate('zh', 'status.missingFile', { path: 'diagram.edn' })).toBe('找不到 IR 文件：diagram.edn')
    expect(translate('en', 'status.waitingDisplay')).toContain(':display')
  })
})
