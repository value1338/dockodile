'use strict';
// Shows the Mac or the Windows version of the texts. Detected from the browser; the switch on the
// page overrides it for this visit. Everything else (phones, Linux) sees the Mac version.
(() => {
 const root = document.documentElement;
 const detect = () => {
  const platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
  if (/win/i.test(platform) || /Windows NT/.test(navigator.userAgent)) return 'win';
  return 'mac';
 };
 let chosen = null;
 try { chosen = sessionStorage.getItem('dockodile-os'); } catch (e) {}
 // Links can name the system: ?os=windows or ?os=mac, e.g. in a voucher link.
 const asked = new URLSearchParams(location.search).get('os');
 if (asked) {
  chosen = /^win/i.test(asked) ? 'win' : 'mac';
  try { sessionStorage.setItem('dockodile-os', chosen); } catch (e) {}
 }
 const set = os => {
  root.dataset.os = os === 'win' ? 'win' : 'mac';
  document.querySelectorAll('[data-set-os]').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.setOs === root.dataset.os)));
 };
 set(chosen || detect());
 window.dockodileOS = () => root.dataset.os;
 document.addEventListener('DOMContentLoaded', () => {
  set(root.dataset.os);
  document.querySelectorAll('[data-set-os]').forEach(b => b.addEventListener('click', () => {
   set(b.dataset.setOs);
   try { sessionStorage.setItem('dockodile-os', b.dataset.setOs); } catch (e) {}
  }));
 });
})();
