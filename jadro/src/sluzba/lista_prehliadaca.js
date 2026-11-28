// Lišta prehliadača kurzov: Späť, Dopredu, Obnoviť a adresa stránky (dá sa skopírovať aj
// prepísať). Beží v stránke ako jej vlastný skript – nemá žiadny prístup ku Komete. Je v tieňovom
// DOM, takže štýly stránky ju nerozbijú a ona nerozbije stránku. V textových súboroch
// (.txt) urobí z adries klikateľné odkazy.
(() => {
  /* kometaLista */
  if (window.top !== window) return; // len hlavná stránka, nie vložené rámce
  const KLUC = 'kometa-lista-zbalena';
  const zbalenaNaZaciatku = (() => {
    try {
      return localStorage.getItem(KLUC) === '1';
    } catch (e) {
      return false;
    }
  })();

  const STYL = `
    :host { all: initial; }
    .lista {
      display: flex; align-items: center; gap: 4px;
      padding: 5px; border-radius: 12px;
      background: rgba(22, 28, 40, 0.94); color: #eef2f7;
      box-shadow: 0 1px 2px rgba(0,0,0,.35), 0 10px 30px -8px rgba(0,0,0,.55);
      font: 13px/1.2 'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, sans-serif;
      backdrop-filter: blur(6px);
    }
    button {
      all: unset; box-sizing: border-box; cursor: pointer;
      display: inline-grid; place-items: center;
      min-width: 30px; height: 30px; padding: 0 8px; border-radius: 8px;
      color: inherit; font: inherit;
    }
    button:hover { background: rgba(255,255,255,.12); }
    button:focus-visible, input:focus-visible { outline: 2px solid #3fd1b5; outline-offset: 1px; }
    svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
    input {
      all: unset; box-sizing: border-box;
      width: min(420px, 46vw); height: 30px; padding: 0 10px; border-radius: 8px;
      background: rgba(255,255,255,.09); color: #eef2f7; font: inherit;
      white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    input:focus { background: rgba(255,255,255,.16); }
    .znak { font-weight: 700; color: #3fd1b5; }
    .plna { display: contents; }
    .zbalena .plna { display: none; }
    .lista:not(.zbalena) .mala { display: none; }
    .ok { color: #3fd1b5; }
  `;
  const ikona = (d) => `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="${d}"/></svg>`;

  function pripoj() {
    if (!document.documentElement || document.querySelector('kometa-lista')) return;
    const host = document.createElement('kometa-lista');
    host.style.cssText = 'all: initial; position: fixed; left: 12px; bottom: 12px; z-index: 2147483647;';
    const koren = host.attachShadow({ mode: 'open' });
    koren.innerHTML = `<style>${STYL}</style>
      <div class="lista" role="toolbar" aria-label="Prehliadač kurzov – Kometa">
        <button class="mala znak" data-a="rozbal" title="Ukázať lištu Komety (Späť, adresa)" aria-label="Ukázať lištu Komety">K</button>
        <span class="plna">
          <button data-a="spat" title="Späť (Alt + ←)" aria-label="Späť">${ikona('M15 18l-6-6 6-6')}</button>
          <button data-a="dopredu" title="Dopredu (Alt + →)" aria-label="Dopredu">${ikona('M9 18l6-6-6-6')}</button>
          <button data-a="obnovit" title="Obnoviť stránku" aria-label="Obnoviť stránku">${ikona('M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7')}</button>
          <input type="text" spellcheck="false" autocomplete="off" aria-label="Adresa stránky" title="Adresa stránky – Enter otvorí zadanú adresu">
          <button data-a="kopirovat" title="Kopírovať adresu" aria-label="Kopírovať adresu">${ikona('M9 9h10v10H9zM5 15V5h10')}</button>
          <button data-a="zbal" title="Skryť lištu" aria-label="Skryť lištu">${ikona('M6 12h12')}</button>
        </span>
      </div>`;
    const lista = koren.querySelector('.lista');
    const pole = koren.querySelector('input');
    const zbal = (ano) => {
      lista.classList.toggle('zbalena', ano);
      try {
        localStorage.setItem(KLUC, ano ? '1' : '0');
      } catch (e) {
        /* stránka nepovolila úložisko */
      }
    };
    zbal(zbalenaNaZaciatku);
    const obnovAdresu = () => {
      if (koren.activeElement !== pole) pole.value = location.href;
    };
    obnovAdresu();
    lista.addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b) return;
      const a = b.dataset.a;
      if (a === 'spat') history.back();
      else if (a === 'dopredu') history.forward();
      else if (a === 'obnovit') location.reload();
      else if (a === 'zbal') zbal(true);
      else if (a === 'rozbal') zbal(false);
      else if (a === 'kopirovat') {
        const hotovo = () => {
          b.classList.add('ok');
          setTimeout(() => b.classList.remove('ok'), 1200);
        };
        const zalozne = () => {
          pole.value = location.href;
          pole.select();
          try {
            if (document.execCommand('copy')) hotovo();
          } catch (e) {
            /* nič */
          }
        };
        if (navigator.clipboard && window.isSecureContext) navigator.clipboard.writeText(location.href).then(hotovo, zalozne);
        else zalozne();
      }
    });
    pole.addEventListener('focus', () => pole.select());
    pole.addEventListener('keydown', (e) => {
      e.stopPropagation(); // klávesové skratky stránky nech pri písaní adresy nereagujú
      if (e.key === 'Escape') {
        pole.value = location.href;
        pole.blur();
      } else if (e.key === 'Enter') {
        let a = pole.value.trim();
        if (!a) return;
        if (!/^https?:\/\//i.test(a)) a = 'https://' + a;
        try {
          const u = new URL(a);
          if (u.protocol === 'http:' || u.protocol === 'https:') location.href = u.href;
        } catch (err) {
          pole.value = location.href;
        }
      }
    });
    ['keyup', 'keypress'].forEach((t) => pole.addEventListener(t, (e) => e.stopPropagation()));
    window.addEventListener('popstate', obnovAdresu);
    window.addEventListener('hashchange', obnovAdresu);
    setInterval(obnovAdresu, 1000); // stránky, ktoré menia adresu bez udalosti (SPA)
    document.documentElement.appendChild(host);
  }

  // textový súbor (napr. materiál „odkaz vo vnútri súboru“): adresy ako odkazy
  function odkazy() {
    if (document.contentType !== 'text/plain') return;
    const pre = document.querySelector('pre');
    if (!pre || pre.dataset.kometa) return;
    pre.dataset.kometa = '1';
    const text = pre.textContent || '';
    const casti = text.split(/(https?:\/\/[^\s<>"']+)/g);
    if (casti.length < 2) return;
    pre.textContent = '';
    casti.forEach((c, i) => {
      if (i % 2 === 0) {
        pre.append(c);
        return;
      }
      const cista = c.replace(/[.,;)\]]+$/, '');
      const a = document.createElement('a');
      a.href = cista;
      a.textContent = cista;
      pre.append(a, c.slice(cista.length));
    });
  }

  const pripravene = () => {
    pripoj();
    odkazy();
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', pripravene);
  else pripravene();
  window.addEventListener('load', pripravene);
})();
