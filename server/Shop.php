<?php
declare(strict_types=1);
require_once __DIR__.'/Core.php';
require_once __DIR__.'/PayPal.php';
require_once __DIR__.'/Confirmation.php';
final class Shop {
 const CONSENT='Ich stimme ausdrücklich zu, dass vor Ablauf der Widerrufsfrist mit der Bereitstellung des Downloads begonnen wird. Ich weiß, dass ich dadurch mit Beginn der Vertragserfüllung mein Widerrufsrecht verliere.';
 public function __construct(public Core $core, public PayPal $paypal, public ?Confirmation $confirmation=null) {$this->confirmation??=new Confirmation($core);}
 private function serial(string $id,callable $fn): mixed {
  $db=$this->core->db;$mysql=$db->getAttribute(PDO::ATTR_DRIVER_NAME)==='mysql';$name='dockodile:'.$id;
  if($mysql&&$this->core->one('SELECT GET_LOCK(?,10) AS acquired',[$name])['acquired']!=1)throw new DomainException('Die Bestellung wird bereits verarbeitet. Bitte gleich erneut prüfen.',409);
  try{return $fn();}finally{if($mysql)$this->core->one('SELECT RELEASE_LOCK(?)',[$name]);}
 }
 public function create(array $body): array {
  if(($body['consent']??null)!==true)throw new DomainException('Bitte der sofortigen Bereitstellung ausdrücklich zustimmen.',422);
  $id=$body['id']??'';$owner=$body['owner']??'';
  if(!is_string($id)||!preg_match('/^[a-f0-9]{32}$/D',$id)||!is_string($owner)||!preg_match('/^[a-f0-9]{64}$/D',$owner))throw new DomainException('Ungültige Bestellreferenz.',422);
  $buyer=$body['buyer']??[];if(!is_array($buyer))throw new DomainException('Ungültige Rechnungsanschrift.',422);Core::validateBuyer($buyer);
  $buyer=array_intersect_key($buyer,array_flip(['name','address','postal_code','city','country','email']));
  return $this->serial($id,function()use($id,$owner,$buyer){
   $c=$this->core;$o=$c->one('SELECT * FROM orders WHERE id=?',[$id]);
   if($o){$c->authorize($id,$owner);if(json_decode($o['buyer_json'],true)!=$buyer)throw new DomainException('Die Rechnungsdaten dieser Bestellung sind bereits festgelegt. Bitte verwende die ursprünglichen Angaben oder kontaktiere riccardo@lakeco.de.',409);if($o['status']==='refunded')throw new DomainException('Bestellung wurde erstattet.',410);if($o['paypal_order_id'])return ['id'=>$id,'paypal_order_id'=>$o['paypal_order_id']];}
   else {$now=Core::now();$c->db->beginTransaction();try{$c->exec('INSERT INTO orders(id,owner_hash,status,amount,currency,buyer_json,consent_text,consent_at,created_at) VALUES(?,?,?,?,?,?,?,?,?)',[$id,hash('sha256',$owner),'creating',700,'EUR',json_encode($buyer,JSON_THROW_ON_ERROR),self::CONSENT,$now,$now]);$this->confirmation->record($id);$c->db->commit();}catch(Throwable $e){if($c->db->inTransaction())$c->db->rollBack();throw $e;}}
   $r=$this->paypal->request('POST','/v2/checkout/orders',['intent'=>'CAPTURE','purchase_units'=>[['reference_id'=>$id,'custom_id'=>$id,'description'=>'Dockodile für macOS und Windows','payee'=>['merchant_id'=>$c->config['merchant_id']],'amount'=>['currency_code'=>'EUR','value'=>'7.00']]],'payment_source'=>['paypal'=>['experience_context'=>['brand_name'=>'Dockodile · Lake Code','shipping_preference'=>'NO_SHIPPING','user_action'=>'PAY_NOW']]]],md5('create:'.$id));
   if(!preg_match('/^[A-Z0-9]+$/D',$r['id']??''))throw new RuntimeException('Invalid PayPal order identifier');
   $c->exec('UPDATE orders SET paypal_order_id=?,status=? WHERE id=?',[$r['id'],'created',$id]);return ['id'=>$id,'paypal_order_id'=>$r['id']];
  });
 }
 public function capture(string $id,?string $owner): array {
  return $this->serial($id,function()use($id,$owner){
   $c=$this->core;$o=$owner===null?$c->one('SELECT * FROM orders WHERE id=?',[$id]):$c->authorize($id,$owner);
   if(!$o||!$o['paypal_order_id'])throw new DomainException('Bestellung nicht gefunden.',404);
   if($o['status']==='paid'){$this->confirmation->send($id);return $c->receipt($id);}
   if($o['status']==='refunded')throw new DomainException('Die Zahlung wurde erstattet.',410);
   $path='/v2/checkout/orders/'.$o['paypal_order_id'];$r=$this->paypal->request('GET',$path);
   if(($r['status']??'')==='APPROVED'){$this->paypal->request('POST',$path.'/capture',null,md5('capture:'.$id));$r=$this->paypal->request('GET',$path);}
   $c->paid($id,$r);$this->confirmation->send($id);return $c->receipt($id);
  });
 }
 // Answers the same whether or not the address belongs to a purchase.
 public function update(mixed $email,string $version,?callable $send=null): void {
  if(!is_string($email)||strlen($email)>160||!filter_var($email,FILTER_VALIDATE_EMAIL))throw new DomainException('Bitte eine gültige E-Mail-Adresse eingeben.',422);
  $c=$this->core;$from=$c->config['mail_from']??'';
  if(empty($c->config['mail_enabled'])||!filter_var($from,FILTER_VALIDATE_EMAIL))throw new DomainException('Der E-Mail-Versand ist gerade nicht verfügbar. Bitte schreibe an riccardo@lakeco.de.',503);
  $c->renew($email,function(string $token)use($email,$version,$from,$send): bool {
  $text="Hallo,\n\nhier ist dein Link zur aktuellen Version von Dockodile".($version!==''?" ($version)":'').":\n\nhttps://dockodile.de/update.html#".$token."\n\nAuf der Seite lädst du die Version für Mac oder Windows und schaltest mit einem Klick die Updates direkt in der App frei. Der Download-Link gilt 30 Tage und für 10 Downloads; einen neuen bekommst du jederzeit auf https://dockodile.de/update.html.\n\nDu hast diese E-Mail nicht angefordert? Dann musst du nichts tun.\n\nRiccardo Reichle · Lake Code\nriccardo@lakeco.de\n";
  $ok=$send?$send($email,$text):mail($email,'=?UTF-8?B?'.base64_encode('Dockodile: dein Update-Link').'?=',chunk_split(base64_encode($text),76,"\r\n"),['From'=>'Dockodile <'.$from.'>','Reply-To'=>$from,'MIME-Version'=>'1.0','Content-Type'=>'text/plain; charset=UTF-8','Content-Transfer-Encoding'=>'base64']);
  if(!$ok)error_log('Dockodile update mail failed');
  return $ok;
  });
 }
 public function webhook(array $e,array $headers): void {
  if(!$this->paypal->verify($e,$headers))throw new DomainException('Ungültige Webhook-Signatur.',400);
  $event=$e['id']??'';$type=$e['event_type']??'';
  if(!is_string($event)||strlen($event)>100||!preg_match('/^[A-Z0-9-]+$/D',$event))throw new DomainException('Ungültiges Ereignis.',400);
  if($this->core->one('SELECT event_id FROM webhook_events WHERE event_id=?',[$event]))return;
  $r=$e['resource']??[];
  if(in_array($type,['CHECKOUT.ORDER.APPROVED','PAYMENT.CAPTURE.COMPLETED'],true)){
   $pp=$type==='CHECKOUT.ORDER.APPROVED'?($r['id']??''):($r['supplementary_data']['related_ids']['order_id']??'');
   $o=$this->core->one('SELECT id,status FROM orders WHERE paypal_order_id=?',[$pp]);
   if($o&&$o['status']!=='refunded')$this->capture($o['id'],null);
  }elseif(in_array($type,['PAYMENT.CAPTURE.REFUNDED','PAYMENT.CAPTURE.REVERSED'],true)){
   $cap=$type==='PAYMENT.CAPTURE.REVERSED'?($r['id']??''):($r['supplementary_data']['related_ids']['capture_id']??'');
   if(!$cap)foreach($r['links']??[] as $link){$url=parse_url($link['href']??'');if(($link['rel']??'')==='up'&&preg_match('~/v2/payments/captures/([A-Z0-9]+)$~',$url['path']??'',$m))$cap=$m[1];}
   if(!$cap)throw new DomainException('Zahlungsreferenz fehlt.',409);
   $this->core->refund($cap);
  }
  try{$this->core->exec('INSERT INTO webhook_events(event_id,type,received_at) VALUES(?,?,?)',[$event,$type,Core::now()]);}catch(PDOException $x){if((string)$x->getCode()!=='23000')throw $x;}
 }
}
