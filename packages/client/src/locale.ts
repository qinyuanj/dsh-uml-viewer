import en from '../../../locale/en.json' with { type: 'json' }
import zh from '../../../locale/zh.json' with { type: 'json' }

export type PanelLanguage = 'zh' | 'en'
export type TextKey = keyof typeof en.panel

const DICTIONARIES: Readonly<Record<PanelLanguage, Readonly<Record<TextKey, string>>>> = { en: en.panel, zh: zh.panel }

export function panelLanguage(active: string | undefined): PanelLanguage {
  return active?.toLowerCase().split('-')[0] === 'zh' ? 'zh' : 'en'
}

export function translate(language: PanelLanguage, key: TextKey, params?: Readonly<Record<string, string>>): string {
  const template = DICTIONARIES[language][key] ?? DICTIONARIES.en[key] ?? key
  if (!params) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) => name in params ? params[name]! : match)
}

export interface PanelText {
  readonly language: PanelLanguage
  t(key: TextKey, params?: Readonly<Record<string, string>>): string
  subscribe(listener: () => void): () => void
}

interface LocaleFace {
  getLocale(): { active: string }
  subscribe(listener: () => void): () => void
}

function localeFace(value: unknown): LocaleFace | null {
  const face = value as Partial<LocaleFace> | null | undefined
  return face && typeof face.getLocale === 'function' && typeof face.subscribe === 'function' ? face as LocaleFace : null
}

export const ENGLISH_TEXT: PanelText = {
  language: 'en',
  t: (key, params) => translate('en', key, params),
  subscribe: () => () => {},
}

export function panelText(get: (name: string) => unknown): PanelText {
  const face = (): LocaleFace | null => localeFace(get('locale'))
  const language = (): PanelLanguage => panelLanguage(face()?.getLocale().active)
  return {
    get language() { return language() },
    t: (key, params) => translate(language(), key, params),
    subscribe(listener) {
      const service = face()
      if (!service) return () => {}
      let last = language()
      return service.subscribe(() => {
        const next = language()
        if (next === last) return
        last = next
        listener()
      })
    },
  }
}
