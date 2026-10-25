// «Controlla l'ortografia mentre scrivo» (CONTRATTO-2J lavoro 3). Seguono l'impostazione la barra di
// scrittura e ogni textarea senza un `spellcheck` scritto dal codice (istruzioni, profilo, regole,
// routine, abilità, risposte alle domande) e i campi marcati `data-ortografia`. I campi tecnici
// (comandi, URL, token, segreti, variabili) hanno `spellcheck="false"` nel codice e restano così.

import { bus, CHIAVI, impostazione } from './stato.js';

export const ortografiaAccesa = () => impostazione(CHIAVI.ortografia, true) !== false;

function segna(radice) {
  const campi = radice.matches?.('textarea, [data-ortografia]') ? [radice] : [];
  campi.push(...(radice.querySelectorAll?.('textarea, [data-ortografia]') || []));
  const si = String(ortografiaAccesa());
  for (const c of campi) {
    if (c.dataset.ortografia === undefined) {
      if (c.hasAttribute('spellcheck')) continue; // tecnico: deciso dal codice
      c.dataset.ortografia = '';
    }
    if (c.getAttribute('spellcheck') !== si) c.setAttribute('spellcheck', si);
  }
}

let avviata = false;
export function avviaOrtografia() {
  if (avviata) return;
  avviata = true;
  segna(document.body);
  new MutationObserver((mut) => { for (const m of mut) for (const n of m.addedNodes) if (n.nodeType === 1) segna(n); })
    .observe(document.body, { childList: true, subtree: true });
  bus.su('impostazioni', () => segna(document.body));
}
