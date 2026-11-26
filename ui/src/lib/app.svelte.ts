import type { VolbyJazykov } from './jazyky'
// Spoločný stav rozhrania: údaje z jadra, nastavenia, téma, upozornenia a otvorené okná.

import { volaj, ChybaJadra } from './api'
import type { Init, Nastavenia, Stav } from './typy'

export type Stranka = 'stahovanie' | 'schranka' | 'kurzy' | 'jazyky' | 'ucty' | 'nastavenia'
export type Dialog =
  | { druh: 'playlist'; url: string; jazyky?: VolbyJazykov }
  | { druh: 'skener'; url: string }
  | { druh: 'zmazat'; co: 'front' | 'subory'; ids?: string[] | null }
  | null

export interface Toast {
  id: number
  text: string
  druh: 'info' | 'ok' | 'chyba'
  akcia?: { text: string; spusti: () => void }
}

const nocSystem = matchMedia('(prefers-color-scheme: dark)')

export const app = $state({
  pripravene: false,
  init: null as Init | null,
  nastavenia: null as Nastavenia | null,
  stav: null as Stav | null,
  stranka: 'stahovanie' as Stranka,
  dialog: null as Dialog,
  prikaz: '',
  toasty: [] as Toast[],
  systemNoc: nocSystem.matches,
  spojenie: true,
  /** práve zachytené odkazy zo schránky (karta ich zvýrazní) */
  noveZoSchranky: [] as string[],
})

nocSystem.addEventListener('change', (e) => (app.systemNoc = e.matches))

/** Skutočný režim (auto → podľa systému). */
export function noc(): boolean {
  const r = app.nastavenia?.rezim ?? 'auto'
  return r === 'noc' || (r === 'auto' && app.systemNoc)
}

let dalsi = 1
export function toast(text: string, druh: Toast['druh'] = 'info', akcia?: Toast['akcia']): void {
  const t: Toast = { id: dalsi++, text, druh, akcia }
  app.toasty = [...app.toasty.slice(-3), t]
  setTimeout(() => zavrietToast(t.id), druh === 'chyba' ? 9000 : 4500)
}

export function zavrietToast(id: number): void {
  app.toasty = app.toasty.filter((t) => t.id !== id)
}

/** Zavolá jadro; chybu ukáže ako upozornenie a vráti null. */
export async function skus<T>(metoda: string, ...args: unknown[]): Promise<T | null> {
  try {
    return await volaj<T>(metoda, ...args)
  } catch (e) {
    toast(e instanceof ChybaJadra ? e.message : String(e), 'chyba')
    return null
  }
}

export async function nastav(zmeny: Partial<Nastavenia>): Promise<void> {
  if (!app.nastavenia) return
  Object.assign(app.nastavenia, zmeny)
  const ulozene = await skus<Nastavenia>('ulozit_nastavenia', zmeny)
  if (ulozene) app.nastavenia = ulozene
}

// Odkazy zo schránky: keď pribudnú nové, okno sa prepne na kartu Zo schránky priamo k nim
// (dá sa vypnúť). Nie počas otvoreného dialógu ani keď človek práve píše do poľa v Komete.
let znameZachytene: Set<string> | null = null
function sledujSchranku(s: Stav): void {
  const ids = (s.zachytene ?? []).map((z) => z.id)
  if (znameZachytene === null) {
    znameZachytene = new Set(ids) // prvé načítanie: to, čo už bolo zachytené, nie je nové
    return
  }
  const nove = ids.filter((id) => !znameZachytene!.has(id))
  znameZachytene = new Set(ids)
  if (!nove.length) return
  app.noveZoSchranky = nove
  const a = document.activeElement
  const pise = a instanceof HTMLElement && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName))
  if ((app.nastavenia?.strazca_ukazat ?? true) && !app.dialog && !(pise && document.hasFocus())) {
    app.stranka = 'schranka'
  }
}

let casovac = 0
export async function obnovovat(): Promise<void> {
  clearTimeout(casovac)
  try {
    app.stav = await volaj<Stav>('stav')
    sledujSchranku(app.stav)
    app.spojenie = true
  } catch {
    app.spojenie = false
  }
  casovac = window.setTimeout(obnovovat, document.hidden ? 2000 : 450)
}

export async function spustit(): Promise<void> {
  const init = await volaj<Init>('init')
  app.init = init
  app.nastavenia = init.nastavenia
  app.pripravene = true
  obnovovat()
}
