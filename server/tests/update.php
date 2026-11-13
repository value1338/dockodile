<?php
require __DIR__.'/core.php';
// Updates for buyers: link by mail (renewed token) and in-app download (token as licence).
$id=str_repeat('7',32);$mailBuyer=array_replace($buyer,['email'=>'Kaeufer@example.invalid']);
$c->exec('INSERT INTO orders(id,owner_hash,paypal_order_id,status,amount,currency,buyer_json,consent_text,consent_at,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)',[$id,hash('sha256','x'),'PP7','created',700,'EUR',json_encode($mailBuyer),'consent',Core::now(),Core::now()]);
$remote=$o;$remote['id']='PP7';$remote['purchase_units'][0]['reference_id']=$id;$remote['purchase_units'][0]['custom_id']=$id;$remote['purchase_units'][0]['payments']['captures'][0]['id']='CAP7';
$c->exec('INSERT INTO confirmations(order_id,accepted_at) VALUES(?,?)',[$id,Core::now()]);
$token=$c->paid($id,$remote)['token'];
$c->config['mail_enabled']=true;$c->config['mail_from']='riccardo@lakeco.de';
$sent=[];$send=function($to,$text)use(&$sent){$sent[]=[$to,$text];return true;};
$shop=new Shop($c,new PayPal([],fn()=>[]));
denied(fn()=>$shop->update('kein-at',''),'Invalid address rejected');
denied(fn()=>$shop->update(['a'],''),'Non-string address rejected');
$shop->update('fremd@example.invalid','0.2.0',$send);check($sent===[],'Unknown address gets no mail and no error');
$shop->update('kaeufer@example.invalid','0.2.0',$send);check($sent===[],'Fresh purchase is throttled');
// Old, used-up link: expired a year ago, all downloads spent.
$c->exec('UPDATE download_tokens SET expires_at=?,downloads=max_downloads WHERE order_id=?',['2025-01-01 00:00:00',$id]);
denied(fn()=>$c->consume($token,'0.2.0'),'Expired link is refused');
$shop->update('kaeufer@example.invalid','0.2.0',$send);
check(count($sent)===1&&$sent[0][0]==='kaeufer@example.invalid'&&str_contains($sent[0][1],'https://dockodile.de/update.html#'.$token)&&str_contains($sent[0][1],'0.2.0'),'Buyer gets the update link, address matched case-insensitively');
// A rejected handoff must not block the next real attempt for ten minutes.
$c->exec('UPDATE download_tokens SET expires_at=?,downloads=max_downloads WHERE order_id=?',['2025-01-01 00:00:00',$id]);
$failedAttempts=0;
$shop->update('kaeufer@example.invalid','0.2.0',function()use(&$failedAttempts){$failedAttempts++;return false;});
check($c->one('SELECT expires_at FROM download_tokens WHERE order_id=?',[$id])['expires_at']==='2025-01-01 00:00:00','Rejected mail does not renew the token or start throttling');
$shop->update('kaeufer@example.invalid','0.2.0',$send);
check($failedAttempts===1&&count($sent)===2,'Mail can be retried immediately after rejected handoff');
$sent=array_slice($sent,-1);
$c->consume($token,'0.2.0');check(true,'Renewed link downloads again');
$shop->update('kaeufer@example.invalid','0.2.0',$send);check(count($sent)===1,'Second request within ten minutes sends nothing');
$c->licensed($token,'0.2.0');check(true,'App licence accepted for paid order');
$c->exec('UPDATE download_tokens SET expires_at=? WHERE order_id=?',['2025-01-01 00:00:00',$id]);
$c->licensed($token,'0.2.1');check(true,'App licence does not expire');
denied(fn()=>$c->licensed(str_repeat('A',43),'0.2.1'),'Unknown licence refused');
$c->refund('CAP7');
denied(fn()=>$c->licensed($token,'0.2.1'),'Refund ends in-app updates');
$shop->update('kaeufer@example.invalid','0.2.1',$send);check(count($sent)===1,'Refunded order gets no update link');
$c->config['mail_enabled']=false;denied(fn()=>$shop->update('kaeufer@example.invalid','',$send),'Disabled mail is reported');
// Vouchers: signed code, limited copies, no invoice.
$code=$c->voucherCode(1,2);check((bool)preg_match('/^DOCK-1-2-[0-9A-F]{10}$/',$code),'Voucher code has the expected shape');
denied(fn()=>$c->redeem('DOCK-1-2-0000000000','a@example.invalid'),'Forged voucher refused');
denied(fn()=>$c->redeem(str_replace('DOCK-1-2-','DOCK-1-9-',$code),'a@example.invalid'),'Changed number of copies refused');
$t1=$c->redeem($code,'a@example.invalid');$c->consume($t1,'0.2.0');$c->licensed($t1,'0.2.0');check(true,'Voucher copy downloads and updates');
check($c->redeem($code,'A@example.invalid')===$t1,'Same address gets the same copy again');
$t2=$c->redeem($code,'b@example.invalid');check($t2!==$t1,'Second friend gets an own copy');
denied(fn()=>$c->redeem($code,'c@example.invalid'),'Used-up voucher refused');
check((int)$db->query("SELECT count(*) FROM invoices i JOIN orders o ON o.id=i.order_id WHERE o.amount=0")->fetchColumn()===0,'Voucher creates no invoice');
$c->config['mail_enabled']=true;$c->exec('UPDATE download_tokens SET expires_at=? WHERE token_hash=?',['2025-01-01 00:00:00',hash('sha256',$t2)]);
$shop->update('b@example.invalid','0.2.1',$send);check(str_contains(end($sent)[1],$t2),'Voucher holder gets update links by mail');
echo "ALL UPDATE TESTS PASSED\n";
