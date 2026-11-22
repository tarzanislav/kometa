// Vlastné ikony Komety: mriežka 24 × 24, ťah 1,75, zaoblené konce. Každá ikona je jedna cesta SVG.

export const IKONY = {
  stiahnut: 'M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14',
  jazyky:
    'M4.5 6.5a2 2 0 0 1 2-2h11a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-6.5l-4.5 4v-4h0a2 2 0 0 1-2-2zM8.5 8.5h7M8.5 11.5h4.5',
  ucty: 'M15.5 8a3.5 3.5 0 1 1-7 0 3.5 3.5 0 0 1 7 0zM5 19.5a7 7 0 0 1 14 0',
  nastavenia:
    'M4 7h9.5M17.5 7H20M4 17h3.5M11.5 17H20M17.5 7a2 2 0 1 1-4 0 2 2 0 0 1 4 0zM11.5 17a2 2 0 1 1-4 0 2 2 0 0 1 4 0z',
  odkaz:
    'M10 14a4 4 0 0 0 5.66 0l3-3a4 4 0 0 0-5.66-5.66l-1 1M14 10a4 4 0 0 0-5.66 0l-3 3a4 4 0 0 0 5.66 5.66l1-1',
  schranka:
    'M9 4h6v3H9zM9 5.5H7A1.5 1.5 0 0 0 5.5 7v12A1.5 1.5 0 0 0 7 20.5h10a1.5 1.5 0 0 0 1.5-1.5V7A1.5 1.5 0 0 0 17 5.5h-2',
  skener:
    'M12.5 20H6.5A1.5 1.5 0 0 1 5 18.5v-13A1.5 1.5 0 0 1 6.5 4H14l4 4v2.5M14 4v4h4M20 16.5a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM19.2 18.7l1.8 1.8',
  playlist: 'M4 6h12M4 11h12M4 16h7M14.5 14v6.5l5.5-3.25z',
  play: 'M8 5.5v13l10.5-6.5z',
  pauza: 'M8.5 5.5v13M15.5 5.5v13',
  zavriet: 'M6.5 6.5l11 11M17.5 6.5l-11 11',
  kos: 'M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9l1-12.5M10 11v5M14 11v5',
  priecinok:
    'M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19H5a1.5 1.5 0 0 1-1.5-1.5z',
  fajka: 'M5 12.5l4.5 4.5L19 7.5',
  pozor: 'M12 4.5 3.2 19.5h17.6zM12 10v4.5M12 17.2v.01',
  znova: 'M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4',
  slnko:
    'M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0zM12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4',
  mesiac: 'M19.5 14.5A7.5 7.5 0 0 1 9.5 4.5a7.5 7.5 0 1 0 10 10z',
  auto: 'M20 12a8 8 0 1 1-16 0 8 8 0 0 1 16 0zM12 4v16M12 8l4.5-2.5M12 12.5l6.5-3.5M12 17l7-4',
  dole: 'M6.5 9.5l5.5 5.5 5.5-5.5',
  hore: 'M6.5 14.5 12 9l5.5 5.5',
  vpravo: 'M9.5 6.5 15 12l-5.5 5.5',
  uchop: 'M9 6.5h.01M15 6.5h.01M9 12h.01M15 12h.01M9 17.5h.01M15 17.5h.01',
  kopirovat: 'M9 9h10.5v10.5H9zM15 9V4.5H4.5V15H9',
  web: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM3 12h18M12 3c2.4 2.5 3.6 5.5 3.6 9s-1.2 6.5-3.6 9c-2.4-2.5-3.6-5.5-3.6-9S9.6 5.5 12 3z',
  film: 'M4 6.5A1.5 1.5 0 0 1 5.5 5h13A1.5 1.5 0 0 1 20 6.5v11a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 17.5zM10.5 9.5v5l4-2.5z',
  hudba: 'M9 17.5V6l10-2v11.5M9 17.5a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0zM19 15.5a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0z',
  subor: 'M14 3.5H7A1.5 1.5 0 0 0 5.5 5v14A1.5 1.5 0 0 0 7 20.5h10a1.5 1.5 0 0 0 1.5-1.5V8zM14 3.5V8h4.5',
  oblak: 'M7 18.5a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 9.5a4.5 4.5 0 0 1-.5 9z',
  zamok: 'M6.5 11h11v9h-11zM8.5 11V8a3.5 3.5 0 0 1 7 0v3',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  vonku: 'M14 4.5h5.5V10M19.5 4.5 11 13M17.5 14v4.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1H10',
  vycistit: 'M4 7h9M4 12h9M4 17h6M14.5 16l2.2 2.2 4.3-4.7',
  hladat: 'M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0zM15.3 15.3l4.7 4.7',
  info: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM12 11v5.5M12 7.8v.01',
} as const

export type MenoIkony = keyof typeof IKONY

/** Ikona podľa zdroja v fronte. */
export function ikonaZdroja(zdroj: string): MenoIkony {
  const z = zdroj.toLowerCase()
  if (z === 'webshare' || z === 'fastshare') return 'oblak'
  if (z === 'web' || z === 'stránka') return 'web'
  if (['hls', 'dash', 'mp4', 'webm', 'súbor', 'subor'].includes(z)) return 'subor'
  return 'film'
}
