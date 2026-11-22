// Slovenské formátovanie čísel, veľkostí a času.

const cislo = (n: number, des = 1) =>
  n.toLocaleString('sk-SK', { minimumFractionDigits: des, maximumFractionDigits: des })

export function velkost(b: number | null | undefined): string {
  if (b == null || !isFinite(b)) return ''
  const j = ['B', 'kB', 'MB', 'GB', 'TB']
  let i = 0
  let n = b
  while (n >= 1000 && i < j.length - 1) {
    n /= 1024
    i++
  }
  return `${cislo(n, i === 0 ? 0 : n < 10 ? 2 : n < 100 ? 1 : 0)} ${j[i]}`
}

/** Rýchlosť rozdelená na číslo a jednotku (číslo sa zobrazuje väčšie). */
export function rychlost(bps: number): { cislo: string; jednotka: string } {
  if (!bps || bps < 1) return { cislo: '0', jednotka: 'kB/s' }
  if (bps < 1024 * 1000) return { cislo: cislo(bps / 1024, 0), jednotka: 'kB/s' }
  const mb = bps / 1024 / 1024
  return { cislo: cislo(mb, mb < 10 ? 1 : 0), jednotka: 'MB/s' }
}

export function rychlostText(bps: number): string {
  const r = rychlost(bps)
  return `${r.cislo} ${r.jednotka}`
}

export function zostava(s: number | null | undefined): string {
  if (s == null || !isFinite(s)) return ''
  if (s < 60) return `${Math.max(1, Math.round(s))} s`
  const m = Math.round(s / 60)
  if (m < 60) return `${m} min`
  const h = Math.floor(m / 60)
  return `${h} h ${m % 60} min`
}

export function dlzka(s: number | null | undefined): string {
  if (s == null || !isFinite(s) || s <= 0) return ''
  const t = Math.round(s)
  const h = Math.floor(t / 3600)
  const m = Math.floor((t % 3600) / 60)
  const sec = t % 60
  const dd = (x: number) => String(x).padStart(2, '0')
  return h ? `${h}:${dd(m)}:${dd(sec)}` : `${m}:${dd(sec)}`
}

export function datum(yyyymmdd: string): string {
  if (!/^\d{8}$/.test(yyyymmdd)) return ''
  return `${+yyyymmdd.slice(6, 8)}. ${+yyyymmdd.slice(4, 6)}. ${yyyymmdd.slice(0, 4)}`
}

/** Slovenské tvary: 1 video, 2 videá, 5 videí. */
export function pocet(n: number, jeden: string, dva: string, pat: string): string {
  const t = n === 1 ? jeden : n >= 2 && n <= 4 ? dva : pat
  return `${n.toLocaleString('sk-SK')} ${t}`
}

/** Krátky kód jazyka pre poradie stôp: cs → CZ, en-US → EN-US. */
export function kratkyKod(kod: string): string {
  if (!kod) return 'ORIG'
  const k = kod.toLowerCase()
  if (k === 'cs') return 'CZ'
  return kod.toUpperCase()
}
