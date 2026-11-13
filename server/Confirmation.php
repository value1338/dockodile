<?php
declare(strict_types=1);
require_once __DIR__.'/Invoice.php';
final class Confirmation {
 public function __construct(private Core $core,private mixed $transport=null) {}
 public function record(string $id): void {
  $c=$this->core;if($c->one('SELECT order_id FROM confirmations WHERE order_id=?',[$id]))return;
  $o=$c->one('SELECT * FROM orders WHERE id=?',[$id]);if(!$o)throw new RuntimeException('Missing order');
  $withdrawal=file_get_contents(__DIR__.'/withdrawal.txt');if(!$withdrawal)throw new RuntimeException('Missing withdrawal information');
  $text="Vertragsbestätigung Dockodile\n\nAnbieter: Riccardo Reichle · Lake Code\nMilchbänkle 12, 88079 Kressbronn am Bodensee, Deutschland\nriccardo@lakeco.de · +49 152 37157595\nUSt-IdNr.: DE463771423\n\nProdukt: Dockodile für macOS und Windows, digitaler Download. Ein Kauf gilt für beide Systeme: macOS als signierte und notarisierte App (DMG), Windows als App ohne Signatur (EXE). Die App gruppiert Fenster verschiedener Programme in einer gemeinsamen Tab-Leiste.\nVoraussetzungen: macOS 14 oder neuer, Apple Silicon oder Intel; Bedienungshilfen-Zugriff erforderlich. Frühe Version; das Verhalten kann je nach Programm abweichen.\nLizenz: GPL-3.0. Spätere Versionen enthalten, abrufbar in der App oder über https://dockodile.de/update.html. Kein Abonnement.\nGesamtpreis: 7,00 EUR, einmalig. Keine Versandkosten. Gemäß § 19 UStG wird keine Umsatzsteuer berechnet. Zahlung über PayPal.\nBereitstellung nach bestätigter Zahlung und Versand dieser Vertragsbestätigung. Downloadzugang: 30 Tage, maximal 10 Downloads; danach neuer Link über https://dockodile.de/update.html oder Support unter riccardo@lakeco.de. Gesetzliche Mängelrechte bleiben unberührt.\n\nDeine ausdrückliche Erklärung vor der Bereitstellung:\n".$o['consent_text']."\nErteilt am: ".$o['consent_at']." UTC\n\n".$withdrawal;
  $c->exec('INSERT INTO confirmations(order_id,contract_text,created_at) VALUES(?,?,?)',[$id,$text,Core::now()]);
 }
 // Caller holds the order's advisory lock. Failed transports remain retryable, without recapturing payment.
 public function send(string $id): void {
  $c=$this->core;$row=$c->one('SELECT * FROM confirmations WHERE order_id=?',[$id]);
  $o=$c->one('SELECT * FROM orders WHERE id=?',[$id]);
  if(!$row||!empty($row['accepted_at'])||!$o||$o['status']!=='paid'||empty($c->config['mail_enabled']))return;
  if(!empty($row['last_attempt_at'])&&strtotime($row['last_attempt_at'].' UTC')>time()-60)return;
  if(empty($row['payload_json'])){
   $i=$c->one('SELECT * FROM invoices WHERE order_id=?',[$id]);$snapshot=json_decode($i['snapshot_json'],true,512,JSON_THROW_ON_ERROR);$buyer=$snapshot['buyer'];$number=$c->number($i);
   $message=['to'=>$buyer['email'],'from'=>$c->config['mail_from']??'','subject'=>($snapshot['sandbox']?'[SANDBOX-TEST] ':'').'Dockodile: Vertragsbestätigung '.$number,'id'=>$id,
    'text'=>($snapshot['sandbox']?"SANDBOX-TEST – keine echte Zahlung, keine gültige Rechnung.\n\n":'').$row['contract_text']."\n\nBestellung: ".$id."\nRechnung: ".$number."\nZahlung bestätigt: ".$snapshot['date']." UTC\nRechnungsempfänger: ".$buyer['name']."\n".$buyer['address']."\n".$buyer['postal_code'].' '.$buyer['city']."\n".$buyer['country']."\n\nDie Rechnung ist als speicherbarer HTML-Anhang beigefügt. Deinen Download findest du im noch geöffneten Kauf-Tab, später jederzeit über https://dockodile.de/update.html. Bei Problemen antworte auf diese E-Mail.\n",
    'invoice'=>Invoice::render($snapshot,$number,false)];
   $c->exec('UPDATE confirmations SET payload_json=? WHERE order_id=?',[json_encode($message,JSON_THROW_ON_ERROR),$id]);
  }else $message=json_decode($row['payload_json'],true,512,JSON_THROW_ON_ERROR);
  $c->exec('UPDATE confirmations SET attempts=attempts+1,last_attempt_at=? WHERE order_id=?',[Core::now(),$id]);
  try{$ok=$this->transport?($this->transport)($message):self::deliver($message);}catch(Throwable $e){$ok=false;error_log('Dockodile confirmation transport failed');}
  if($ok)$c->exec('UPDATE confirmations SET accepted_at=? WHERE order_id=? AND accepted_at IS NULL',[Core::now(),$id]);
 }
 public static function deliver(array $m): bool {
  foreach(['to','from'] as $field)if(!filter_var($m[$field],FILTER_VALIDATE_EMAIL)||preg_match('/[\r\n]/',$m[$field]))throw new RuntimeException('Invalid mail address');
  if(!preg_match('/^[a-f0-9]{32}$/D',$m['id']))throw new RuntimeException('Invalid message ID');
  $domain=substr(strrchr($m['from'],'@'),1);$boundary='dockodile_'.bin2hex(random_bytes(16));
  $headers=['From'=>'Dockodile <'.$m['from'].'>','Reply-To'=>$m['from'],'MIME-Version'=>'1.0','Message-ID'=>'<confirmation.'.$m['id'].'@'.$domain.'>','Content-Type'=>'multipart/mixed; boundary="'.$boundary.'"'];
  $body='--'.$boundary."\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n".chunk_split(base64_encode($m['text']),76,"\r\n");
  $body.='--'.$boundary."\r\nContent-Type: text/html; charset=UTF-8; name=\"Rechnung.html\"\r\nContent-Disposition: attachment; filename=\"Rechnung.html\"\r\nContent-Transfer-Encoding: base64\r\n\r\n".chunk_split(base64_encode($m['invoice']),76,"\r\n").'--'.$boundary."--\r\n";
  return mail($m['to'],'=?UTF-8?B?'.base64_encode($m['subject']).'?=',$body,$headers);
 }
}
