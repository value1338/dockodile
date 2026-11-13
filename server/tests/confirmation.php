<?php
require __DIR__.'/core.php';
// A paid order must not expose a download before its contract confirmation was accepted for mailing.
$id=str_repeat('e',32);$owner=str_repeat('f',64);
$c->config['paypal_env']='sandbox';
$c->exec('INSERT INTO orders(id,owner_hash,paypal_order_id,status,amount,currency,buyer_json,consent_text,consent_at,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)',[$id,hash('sha256',$owner),'PP4','created',700,'EUR',json_encode($buyer),Shop::CONSENT,Core::now(),Core::now()]);
$remote=$o;$remote['id']='PP4';$remote['purchase_units'][0]['reference_id']=$id;$remote['purchase_units'][0]['custom_id']=$id;$remote['purchase_units'][0]['payments']['captures'][0]['id']='CAP4';
$r=$c->paid($id,$remote);
check(empty($r['token']),'Paid order without confirmation exposes no download token');
denied(fn()=>$c->consume($c->token($id),'test'),'Direct download blocked before confirmation');
require_once __DIR__.'/../Confirmation.php';
$c->config['mail_enabled']=true;$c->config['mail_from']='riccardo@lakeco.de';
$mail=new Confirmation($c,fn($message)=>false);$mail->record($id);
$mail->send($id);
check(empty($c->receipt($id)['token']),'Mail transport failure keeps download blocked');
$c->exec('UPDATE confirmations SET last_attempt_at=NULL WHERE order_id=?',[$id]);
$messages=[];$mail=new Confirmation($c,function($m)use(&$messages){$messages[]=$m;return true;});
$mail->send($id);$mail->send($id);
$r=$c->receipt($id);check(!empty($r['token'])&&$r['email_sent']===true,'Accepted confirmation unlocks download');
check(count($messages)===1,'Capture retry does not send a second confirmation');
check(str_contains($messages[0]['text'],Shop::CONSENT)&&str_contains($messages[0]['text'],'Muster-Widerrufsformular')&&str_contains($messages[0]['text'],'7,00 EUR'),'Confirmation includes consent, withdrawal form and price');
check(str_contains($messages[0]['invoice'],'&lt;script&gt;'),'Invoice attachment escapes buyer input');
$c->consume($r['token'],'test');
$c->refund('CAP4');denied(fn()=>$c->consume($r['token'],'test'),'Refund still blocks confirmed download');
echo "ALL CONFIRMATION TESTS PASSED\n";
// The real Shop retry path must preserve payment and immutable contract contents.
$newId=str_repeat('1',32);$newOwner=str_repeat('2',64);$captures=0;
$pp=new PayPal([],function($method,$path,$body,$request)use($newId,&$captures){
 if($path==='/v2/checkout/orders')return ['id'=>'PP5'];
 if(str_ends_with($path,'/capture'))$captures++;
 return ['id'=>'PP5','status'=>$captures?'COMPLETED':'APPROVED','purchase_units'=>[['reference_id'=>$newId,'custom_id'=>$newId,'payee'=>['merchant_id'=>'MERCHANT'],'payments'=>['captures'=>[['id'=>'CAP5','status'=>'COMPLETED','amount'=>['value'=>'7.00','currency_code'=>'EUR']]]]]]];
});
$failedShop=new Shop($c,$pp,new Confirmation($c,fn($m)=>false));
$failedShop->create(['id'=>$newId,'owner'=>$newOwner,'buyer'=>$buyer,'consent'=>true]);
$r=$failedShop->capture($newId,$newOwner);check($r['status']==='awaiting_confirmation'&&empty($r['token']),'Capture reports pending confirmation on mail failure');
$original=$c->one('SELECT payload_json FROM confirmations WHERE order_id=?',[$newId])['payload_json'];
$c->exec('UPDATE confirmations SET last_attempt_at=NULL WHERE order_id=?',[$newId]);
$c->config['mail_from']='changed@example.invalid';$resent=null;
$retryShop=new Shop($c,$pp,new Confirmation($c,function($m)use(&$resent){$resent=$m;return true;}));
$r=$retryShop->capture($newId,$newOwner);
check($captures===1&&$r['status']==='paid','Confirmation retry releases download without recapturing');
check($resent===json_decode($original,true),'Retry uses the original immutable confirmation');
check((int)$c->one('SELECT COUNT(*) AS n FROM invoices WHERE order_id=?',[$newId])['n']===1,'Confirmation retry preserves single invoice');
echo "ALL SHOP CONFIRMATION RETRY TESTS PASSED\n";
