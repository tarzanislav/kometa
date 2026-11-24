// Tvary dát, ktoré posiela jadro: kometa/webapp.py (Python) aj jadro/src/sluzba (Rust).

export type StavPolozky = 'caka' | 'stahuje' | 'pauza' | 'hotovo' | 'chyba' | 'rozbalene'

export interface Polozka {
  id: string
  url: string
  zdroj: string
  format: string
  nazov: string
  stav: StavPolozky
  hotovo: number
  celkom: number | null
  rychlost: number
  zostava: number | null
  subor: string
  chyba: string
  podrobnosti: string
  nahlad: string
  dlzka: number | null
  autor: string
  pridane: number
  /** podiel hotovej práce, keď sa nedá rátať v bajtoch (video s jazykmi) */
  podiel?: number | null
  /** čo sa práve robí, napr. „Sťahujem: Čeština (2/3)“ */
  krok?: string
  /** voľby jazykov, napr. „CZ, SK, EN · AAC + Opus · MKV“ */
  jazyky?: string | null
}

export interface StavUctu {
  stav: 'neprihlaseny' | 'overujem' | 'ok' | 'bez_vip' | 'chyba'
  text: string
  user: string
}

export interface StavJazykov {
  busy: boolean
  frac: number
  text: string
  log: string[]
  file: string
  error: string
  rev: number
}

export interface Stav {
  polozky: Polozka[]
  rychlost: number
  graf: number[]
  jazyky: StavJazykov
  ucty: Record<'webshare' | 'fastshare', StavUctu>
  nastroje?: StavNastrojov
}

export interface Nastavenia {
  outdir: string
  dl_preset: string
  parallel: number
  connections: number
  autoresume: boolean
  cookies_browser: string
  audio_mode: 'oboje' | 'aac' | 'opus'
  tema: string
  rezim: 'auto' | 'den' | 'noc'
  include_original: boolean
  keep_separate: boolean
  mode: 'video' | 'existing' | 'audio'
  kontajner: 'mkv' | 'mp4'
  max_height: string
}

export interface Init {
  verzia: string
  platforma: string
  nastavenia: Nastavenia
  jazyky_poradie: string[]
  formaty: { id: string; nazov: string }[]
  prehliadace: string[]
  zvuk: Record<string, string>
  chyba_ffmpeg: boolean
  chyba_ytdlp: boolean
  /** jadro v Ruste (aplikácia alebo `kometa okno`); v Pythone chýba */
  jadro?: 'rust'
  okno?: 'aplikacia' | 'prehliadac'
  funkcie?: Funkcie
}

/** Čo vie toto jadro (v Pythone všetko). */
export interface Funkcie {
  jazyky: boolean
  ucty: boolean
  kolekcie: boolean
  doinstalovat_ytdlp: boolean
  /** vlastné spájanie stôp – FFmpeg treba len na MP3 a zriedkavé formáty */
  ffmpeg_volitelny: boolean
}

export interface StavNastrojov {
  ytdlp: boolean
  ffmpeg: boolean
  instaluje: boolean
  text: string
  podiel: number
}

export interface Rozpoznany {
  url: string
  zdroj: string
  kolekcia: boolean
}

export interface VideoKolekcie {
  url: string
  title: string
  duration: number | null
  id: string
  date: string
  views: number | null
  nahlad: string
}

export interface Kolekcia {
  title: string
  uploader: string
  url: string
  entries: VideoKolekcie[]
  kanal: boolean
}

export interface Nalez {
  url: string
  druh: string
  druh_text: string
  poskytovatel: string
  nazov: string
  poznamka: string
  skore: number
  referer: string
  odporucane: boolean
  da_sa: boolean
}

export interface Stopa {
  kod: string
  nazov: string
  original: boolean
  aac: boolean
  kbps: number
}

export interface VideoJazyky {
  nazov: string
  autor: string
  dlzka: number | null
  nahlad: string
  original: string
  stopy: Stopa[]
  vybrane: string[]
}
