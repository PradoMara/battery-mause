// Registro PASIVO de trafico WebHID. No modifica ni envia nada por si mismo:
// solo anota lo que la pagina del fabricante envia/recibe del dispositivo.
// Pegar en la consola de Chrome (F12) ANTES de conectar el mouse en la pagina.
(() => {
  const log = (window.__hidlog = window.__hidlog || []);
  const hex = (d) => {
    const u = d instanceof DataView ? new Uint8Array(d.buffer, d.byteOffset, d.byteLength)
      : d instanceof ArrayBuffer ? new Uint8Array(d) : new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
    return [...u].map((b) => b.toString(16).padStart(2, '0')).join(' ');
  };
  const t0 = performance.now();
  const add = (s) => { const l = `[${((performance.now() - t0) / 1000).toFixed(2)}s] ${s}`; log.push(l); console.log(l); };
  const tag = (dev) => `${dev.vendorId.toString(16)}:${dev.productId.toString(16)} UP=0x${(dev.collections[0]?.usagePage ?? 0).toString(16)}`;
  const P = HIDDevice.prototype;

  for (const m of ['sendReport', 'sendFeatureReport']) {
    const orig = P[m];
    P[m] = function (id, data) { add(`TX ${m} ${tag(this)} id=${id} len=${data.byteLength}: ${hex(data)}`); return orig.apply(this, arguments); };
  }
  const origRecv = P.receiveFeatureReport;
  P.receiveFeatureReport = async function (id) {
    const r = await origRecv.apply(this, arguments);
    add(`RX receiveFeatureReport ${tag(this)} id=${id} len=${r.byteLength}: ${hex(r)}`);
    return r;
  };
  const origOpen = P.open;
  P.open = async function () {
    const r = await origOpen.apply(this, arguments);
    add(`OPEN ${tag(this)} collections=${this.collections.map((c) => '0x' + c.usagePage.toString(16)).join(',')}`);
    this.addEventListener('inputreport', (e) => add(`RX inputreport ${tag(this)} id=${e.reportId} len=${e.data.byteLength}: ${hex(e.data)}`));
    return r;
  };
  add('hook instalado. Conecta el mouse en la pagina; al terminar ejecuta: copy(__hidlog.join("\\n"))');
})();
