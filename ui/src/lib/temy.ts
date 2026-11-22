export interface Tema {
  id: string
  nazov: string
  popis: string
}

export const TEMY: Tema[] = [
  { id: 'kometa', nazov: 'Kometa', popis: 'Chladná obloha a zelená žiara kométy' },
  { id: 'polarna', nazov: 'Polárna žiara', popis: 'Fialový nádych, tyrkys a ružová' },
  { id: 'sumrak', nazov: 'Súmrak', popis: 'Teplé tóny a malinová farba' },
  { id: 'les', nazov: 'Les', popis: 'Machová zelená a jemné drevo' },
  { id: 'more', nazov: 'More', popis: 'Hlboká modrá a morská pena' },
  { id: 'levandula', nazov: 'Levanduľa', popis: 'Pokojná fialová' },
  { id: 'grafit', nazov: 'Grafit', popis: 'Čisté sivé tóny a indigová' },
]
