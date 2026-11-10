/**
 * The panel's DOM text (#42, ADR 0015): every label and accessible name comes
 * from the plugin's `locale/*.json` files, in the language DSH has selected
 * (`ctx.locale`), and follows a switch without reopening the tab. Chinese and
 * English are shipped; any other language falls back to English.
 *
 * Text that the resident lays out (card rows, 「无数据」, hover rows) is chosen
 * on the Rust side; `language` is what the panel stream passes to it.
 */
import en from '../../../locale/en.json' with { type: 'json' }
import zh from '../../../locale/zh.json' with { type: 'json' }

export type PanelLanguage = 'zh' | 'en'
export type TextKey = keyof typeof en.panel

const DICTIONARIES: Readonly<Record<PanelLanguage, Readonly<Record<TextKey, string>>>> = { en: en.panel, zh: zh.panel }

/** The panel language for a DSH locale id: any Chinese variant is Chinese, the rest English. */
export function panelLanguage(active: string | undefined): PanelLanguage {
  return active?.toLowerCase().split('-')[0] === 'zh' ? 'zh' : 'en'
}

/** One entry, with `{name}` placeholders filled from `params`. */
export function translate(language: PanelLanguage, key: TextKey, params?: Readonly<Record<string, string>>): string {
  const template = DICTIONARIES[language][key] ?? DICTIONARIES.en[key] ?? key
  if (!params) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) => name in params ? params[name]! : match)
}

/** Text for one panel, read in the current language at call time. */
export interface PanelText {
  readonly language: PanelLanguage
  t(key: TextKey, params?: Readonly<Record<string, string>>): string
  /** Called after the DSH language (or its catalog) changes; returns the unsubscribe. */
  subscribe(listener: () => void): () => void
}

/** The part of DSH's `LocaleRuntime` the panel reads. */
interface LocaleFace {
  getLocale(): { active: string }
  subscribe(listener: () => void): () => void
}

function localeFace(value: unknown): LocaleFace | null {
  const face = value as Partial<LocaleFace> | null | undefined
  return face && typeof face.getLocale === 'function' && typeof face.subscribe === 'function' ? face as LocaleFace : null
}

/** English text that never changes: hosts without the locale service, and tests. */
export const ENGLISH_TEXT: PanelText = {
  language: 'en',
  t: (key, params) => translate('en', key, params),
  subscribe: () => () => {},
}

/** Follow the locale service of a client context (`ctx.get('locale')`). */
export function panelText(get: (name: string) => unknown): PanelText {
  const face = (): LocaleFace | null => localeFace(get('locale'))
  const language = (): PanelLanguage => panelLanguage(face()?.getLocale().active)
  return {
    get language() { return language() },
    t: (key, params) => translate(language(), key, params),
    subscribe(listener) {
      const service = face()
      if (!service) return () => {}
      // The service also notifies dictionary registrations; repeat only real switches.
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
