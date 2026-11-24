// Spoločné pre jazykové stopy: karta Jazyky aj výber pre celý playlist či kanál – názvy, poradie, vkladanie.

export type Zvuk = 'oboje' | 'aac' | 'opus'

export const ZVUKY: { id: Zvuk; text: string }[] = [
  { id: 'oboje', text: 'AAC + Opus' },
  { id: 'aac', text: 'Len AAC' },
  { id: 'opus', text: 'Len Opus' },
]

export const KVALITY = ['Najlepšia', '2160p', '1440p', '1080p', '720p', '480p']

/** Poradie jazykov: CZ, SK, EN, PL, HU, RU – ostatné za nimi. */
export const PRIORITA = ['cs', 'sk', 'en', 'pl', 'hu', 'ru']

/** Jazyky, v ktorých YouTube najčastejšie dabuje (a naše). */
export const BEZNE: [string, string][] = [
  ['cs', 'Čeština'],
  ['sk', 'Slovenčina'],
  ['en', 'Angličtina'],
  ['en-US', 'Angličtina (USA)'],
  ['en-GB', 'Angličtina (Británia)'],
  ['pl', 'Poľština'],
  ['hu', 'Maďarčina'],
  ['ru', 'Ruština'],
  ['uk', 'Ukrajinčina'],
  ['de', 'Nemčina'],
  ['fr', 'Francúzština'],
  ['es', 'Španielčina'],
  ['it', 'Taliančina'],
  ['pt', 'Portugalčina'],
  ['pt-BR', 'Portugalčina (Brazília)'],
  ['nl', 'Holandčina'],
  ['ro', 'Rumunčina'],
  ['hr', 'Chorvátčina'],
  ['sr', 'Srbčina'],
  ['bg', 'Bulharčina'],
  ['el', 'Gréčtina'],
  ['tr', 'Turečtina'],
  ['ar', 'Arabčina'],
  ['he', 'Hebrejčina'],
  ['hi', 'Hindčina'],
  ['id', 'Indonézština'],
  ['vi', 'Vietnamčina'],
  ['th', 'Thajčina'],
  ['ja', 'Japončina'],
  ['ko', 'Kórejčina'],
  ['zh', 'Čínština'],
  ['sv', 'Švédčina'],
  ['da', 'Dánčina'],
  ['no', 'Nórčina'],
  ['fi', 'Fínčina'],
]

export function nazovJazyka(kod: string): string {
  return BEZNE.find(([k]) => k.toLowerCase() === kod.toLowerCase())?.[1] ?? kod.toUpperCase()
}

export function priorita(kod: string): number {
  const i = PRIORITA.indexOf(kod.toLowerCase().split('-')[0])
  return i < 0 ? Infinity : i
}

/** Vloží jazyk na jeho miesto: CZ, SK, EN, PL, HU, RU podľa poradia, ostatné na koniec. */
export function vlozJazyk(poradie: string[], kod: string): string[] {
  if (poradie.includes(kod)) return poradie
  const p = priorita(kod)
  const j = p === Infinity ? -1 : poradie.findIndex((k) => priorita(k) > p)
  return j < 0 ? [...poradie, kod] : [...poradie.slice(0, j), kod, ...poradie.slice(j)]
}

/** Voľby jazykov pre položky frontu – rovnaké kľúče ako jadro (Python aj Rust). */
export interface VolbyJazykov {
  poradie: string[]
  original: boolean
  zvuk: Zvuk
  zvuky: Record<string, Zvuk>
  kontajner: 'mkv' | 'mp4'
  kvalita: string
  rezim: 'video' | 'audio'
  bez_jazykov: 'povodny' | 'preskocit'
}
