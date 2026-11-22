// Spojenie s jadrom. V okne aplikácie (pywebview) cez window.pywebview.api, v okne prehliadača cez lokálny
// server s tajným kľúčom v adrese (#t=…). Neskôr v Tauri cez invoke() – zvyšok rozhrania sa nemení.

type Odpoved<T> = { ok: true; data: T } | { ok: false; chyba: string; podrobnosti?: string }

declare global {
  interface Window {
    pywebview?: { api: Record<string, (...args: unknown[]) => Promise<unknown>> }
  }
}

export class ChybaJadra extends Error {
  podrobnosti = ''
}

const kluc = new URLSearchParams(location.hash.slice(1)).get('t') ?? ''
let cesta: 'okno' | 'server' = 'server'

/** Počká, kým je jadro pripravené (pywebview vkladá svoje API až po načítaní stránky). */
export function pripojit(): Promise<void> {
  return new Promise((ok) => {
    if (window.pywebview?.api?.init) {
      cesta = 'okno'
      return ok()
    }
    if (kluc) {
      cesta = 'server'
      return ok()
    }
    const hotovo = () => {
      cesta = window.pywebview?.api ? 'okno' : 'server'
      ok()
    }
    window.addEventListener('pywebviewready', hotovo, { once: true })
    setTimeout(hotovo, 4000)
  })
}

export async function volaj<T>(metoda: string, ...args: unknown[]): Promise<T> {
  let odp: Odpoved<T>
  if (cesta === 'okno') {
    odp = (await window.pywebview!.api[metoda](...args)) as Odpoved<T>
  } else {
    let r: Response
    try {
      r = await fetch(`api/${metoda}`, {
        method: 'POST',
        headers: { 'X-Kometa': kluc, 'Content-Type': 'application/json' },
        body: JSON.stringify(args),
      })
    } catch {
      throw new ChybaJadra('Kometa neodpovedá. Ak si zatvoril jej okno v termináli, spusti ju znova.')
    }
    if (!r.ok) {
      throw new ChybaJadra(
        r.status === 403 ? 'Spojenie s Kometou sa nepodarilo overiť. Spusti Kometu znova.' : `Kometa odpovedala chybou ${r.status}.`,
      )
    }
    odp = (await r.json()) as Odpoved<T>
  }
  if (!odp.ok) {
    const e = new ChybaJadra(odp.chyba)
    e.podrobnosti = odp.podrobnosti ?? ''
    throw e
  }
  return odp.data
}
