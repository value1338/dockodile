<?php
declare(strict_types=1);
final class Invoice {
 public static function render(array $s,string $number,bool $refunded=false): string {
  $e=fn($v)=>htmlspecialchars((string)$v,ENT_QUOTES|ENT_SUBSTITUTE,'UTF-8');
  $seller=$s['seller'];$sellerName=$e($seller['name']);$sellerAddress=$e($seller['address']);$sellerVat=$e($seller['vat_id']);$sellerEmail=$e($seller['email']??'riccardo@lakeco.de');$sellerPhone=$e($seller['phone']??'+49 152 37157595');$sellerWebsite=$e($seller['website']??'lakeco.de');
  $b=$s['buyer'];$date=gmdate('d.m.Y',strtotime($s['date'].' UTC'));$total=number_format($s['amount']/100,2,',','.').' €';
  $css=file_get_contents(__DIR__.'/invoice.css');
  $consent=$e($s['consent_text']??'');$consentAt=$e($s['consent_at']??'');
  $name=$e($b['name']);$address=$e($b['address']);$city=$e($b['postal_code'].' '.$b['city']);$email=$e($b['email']);$nr=$e($number);
  $sandbox=!empty($s['sandbox'])?'<p><strong>TESTRECHNUNG – Sandbox, keine echte Zahlung.</strong></p>':'';
  // Orders since the Windows version include both systems; older invoices keep their wording.
  $both=($s['product']??'')==='mac+windows';
  $product=$both?'Dockodile für macOS und Windows':'Dockodile für macOS';
  $lead=$both?'Bereitstellung der App als digitaler Download für macOS und Windows.':'Bereitstellung der signierten App als digitaler Download.';
  $line=$both?'macOS: signierte und notarisierte App<br>Windows: App für Windows 11':'Signierte und notarisierte App';
  $refund=$refunded?'<p>Diese Zahlung wurde erstattet oder zurückgebucht.</p>':'';
  return <<<HTML
<!doctype html><html lang="de"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Rechnung $nr – Lake Code</title><style>$css</style></head><body>
<div class="print-tools">Rechnung $nr · Über „Drucken“ im Browser als PDF speichern.</div>
<main class="paper">
<div class="din-fold-marks" aria-hidden="true"><span class="din-fold-mark"></span><span class="din-fold-mark"></span><span class="din-fold-mark"></span></div>
<div class="paper-letterhead"><div class="paper-head-right"><div class="paper-logo-wrap"><svg class="paper-logo" width="54" height="36" viewBox="0 0 30 20" fill="none"><rect x="1" y="1" width="28" height="18" stroke="currentColor" stroke-width="1.2"/><text x="15" y="14" text-anchor="middle" font-family="monospace" font-size="9" font-weight="700" fill="currentColor">LC</text></svg><span class="paper-logo-word">Lake Code</span></div><div class="meta"><b>Rechnung</b><br>Nr. $nr<br>Datum: $date<br>Leistungsdatum: $date</div></div></div>
<div class="address-window"><div class="sender-line">$sellerName · $sellerAddress</div><div class="recipient-address"><b>$name</b><br>$address<br>$city<br>Deutschland</div></div><div class="address-meta">E-Mail: $email</div>
<div class="paper-body"><h2>Rechnung</h2>$sandbox$refund<p class="paper-project muted-paper">$product</p><p>$lead</p>
<div class="paper-positions"><table class="invoice-table"><colgroup><col class="col-pos"><col class="col-date"><col class="col-desc"><col class="col-qty"><col class="col-rate"><col class="col-amount"></colgroup><thead><tr><th>Pos.</th><th>Datum</th><th>Leistungsbeschreibung</th><th class="right">Menge</th><th class="right">Einzelpreis</th><th class="right">Betrag</th></tr></thead><tbody><tr><td>1</td><td>$date</td><td>$product<br>$line</td><td class="right">1</td><td class="right">$total</td><td class="right">$total</td></tr></tbody></table></div>
<div class="paper-closing"><div class="totals"><div class="total"><span>Gesamtbetrag</span><span>$total</span></div></div><div class="note"><b>Hinweis zur Umsatzsteuer:</b><br>Für die Leistung gilt die Steuerbefreiung für Kleinunternehmer gemäß § 19 UStG. Es wird keine Umsatzsteuer ausgewiesen.</div><p class="paper-payment">Bereits per PayPal bezahlt. Vielen Dank für deinen Kauf.</p><p class="small">Vertragsbestätigung: $product als digitaler Download, einmalig $total, spätere Versionen enthalten. Bereitstellung nach bestätigter Zahlung und Versand der Vertragsbestätigung. Deine Zustimmung am $consentAt UTC: $consent</p><div class="footer"><div><b>$sellerName</b><br>$sellerAddress<br>Deutschland</div><div><b>Kontakt</b><br>$sellerEmail<br>$sellerPhone<br>USt-IdNr.: $sellerVat<br>$sellerWebsite</div></div></div></div></main></body></html>
HTML;
 }
}
