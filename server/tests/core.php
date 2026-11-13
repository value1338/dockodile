<?php
declare(strict_types=1);
require __DIR__.'/../Core.php';
function check(bool $v,string $m):void {if(!$v)throw new RuntimeException($m);echo "PASS $m\n";}
function denied(callable $fn,string $m):void {try{$fn();}catch(DomainException $e){check(true,$m);return;}throw new RuntimeException('Not rejected: '.$m);}
$db=new PDO('sqlite::memory:',null,null,[PDO::ATTR_ERRMODE=>PDO::ERRMODE_EXCEPTION]);
$db->exec('CREATE TABLE orders(id TEXT PRIMARY KEY, owner_hash TEXT, paypal_order_id TEXT UNIQUE, paypal_capture_id TEXT UNIQUE, status TEXT, amount INT, currency TEXT, buyer_json TEXT, consent_text TEXT, consent_at TEXT, created_at TEXT, paid_at TEXT, refunded_at TEXT)');
$db->exec('CREATE TABLE invoices(id INTEGER PRIMARY KEY AUTOINCREMENT,order_id TEXT UNIQUE,snapshot_json TEXT,created_at TEXT)');
$db->exec('CREATE TABLE download_tokens(id INTEGER PRIMARY KEY AUTOINCREMENT,order_id TEXT UNIQUE,token_hash TEXT UNIQUE,expires_at TEXT,downloads INT,max_downloads INT,created_at TEXT)');
$db->exec('CREATE TABLE downloads(id INTEGER PRIMARY KEY AUTOINCREMENT,token_id INT,version TEXT,at TEXT)');
$db->exec('CREATE TABLE capture_blocks(capture_id TEXT PRIMARY KEY,created_at TEXT)');
$db->exec('CREATE TABLE confirmations(order_id TEXT PRIMARY KEY,contract_text TEXT,payload_json TEXT,accepted_at TEXT,last_attempt_at TEXT,attempts INT DEFAULT 0,created_at TEXT)');
$c=new Core($db,['token_key'=>str_repeat('a',64),'merchant_id'=>'MERCHANT','token_days'=>30,'token_max_downloads'=>10]);
$buyer=['name'=>'<script>alert(1)</script>','address'=>'Testweg 1','postal_code'=>'12345','city'=>'Berlin','country'=>'DE','email'=>'test@example.invalid'];
denied(fn()=>Core::validateBuyer([]),'Missing billing details rejected');
Core::validateBuyer($buyer);
denied(fn()=>Core::validateBuyer(array_merge($buyer,['country'=>'US'])),'Foreign sales blocked pending tax review');
$id='abc123';$owner='owner-test';
$db->prepare('INSERT INTO orders VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)')->execute([$id,hash('sha256',$owner),'PP1',null,'created',700,'EUR',json_encode($buyer),'consent','2026-11-11 00:00:00','2026-11-11 00:00:00',null,null]);
$o=['id'=>'PP1','status'=>'COMPLETED','purchase_units'=>[['reference_id'=>$id,'custom_id'=>$id,'payee'=>['merchant_id'=>'MERCHANT'],'payments'=>['captures'=>[['id'=>'CAP1','status'=>'COMPLETED','amount'=>['value'=>'7.00','currency_code'=>'EUR'],'final_capture'=>true]]]]]];
denied(fn()=>$c->authorize($id,'wrong'),'Foreign owner rejected');
check($c->authorize($id,$owner)['id']===$id,'Correct owner accepted');
foreach(['amount','currency','merchant','reference','status'] as $bad){$x=$o;switch($bad){case 'amount':$x['purchase_units'][0]['payments']['captures'][0]['amount']['value']='0.01';break;case 'currency':$x['purchase_units'][0]['payments']['captures'][0]['amount']['currency_code']='USD';break;case 'merchant':$x['purchase_units'][0]['payee']['merchant_id']='OTHER';break;case 'reference':$x['purchase_units'][0]['custom_id']='other';break;case 'status':$x['status']='APPROVED';}denied(fn()=>$c->paid($id,$x),"Reject $bad mismatch");}
$c->exec('INSERT INTO confirmations(order_id,accepted_at) VALUES(?,?)',[$id,Core::now()]);
$r=$c->paid($id,$o);$r2=$c->paid($id,$o);
check($r===$r2,'Repeated capture gives same token and invoice');
check((int)$db->query('SELECT count(*) FROM invoices')->fetchColumn()===1,'Exactly one invoice');
check((int)$db->query('SELECT count(*) FROM download_tokens')->fetchColumn()===1,'Exactly one token');
check($db->query('SELECT token_hash FROM download_tokens')->fetchColumn()!==$r['token'],'Only token hash persisted');
$html=$c->invoice($id,$owner);
check(!str_contains($html,'<script>')&&str_contains($html,'&lt;script&gt;'),'Invoice escapes buyer input');
check(str_contains($html,'DE463771423')&&str_contains($html,'§ 19 UStG')&&str_contains($html,'7,00'),'Invoice seller, tax exemption and total');
for($i=0;$i<10;$i++)$c->consume($r['token'],'0.1.0');
denied(fn()=>$c->consume($r['token'],'0.1.0'),'Download limit enforced');
check((int)$db->query('SELECT count(*) FROM downloads')->fetchColumn()===10,'Downloads recorded');
$db->exec("UPDATE download_tokens SET downloads=0,expires_at='2000-01-01 00:00:00'");
denied(fn()=>$c->consume($r['token'],'0.1.0'),'Expired token rejected');
denied(fn()=>$c->consume(str_repeat('z',43),'0.1.0'),'Unknown token rejected');
$c->refund('CAP1');
denied(fn()=>$c->paid($id,$o),'Refund cannot be undone by delayed completed event');
denied(fn()=>$c->consume($r['token'],'0.1.0'),'Refund blocks download');
echo "ALL CORE TESTS PASSED\n";
require_once __DIR__.'/../Shop.php';
$db->exec('CREATE TABLE webhook_events(event_id TEXT PRIMARY KEY,type TEXT,received_at TEXT)');
$state=['created'=>0,'captured'=>0,'get_status'=>'APPROVED'];$newId=str_repeat('b',32);$newOwner=str_repeat('c',64);
$fake=new PayPal(['paypal_env'=>'sandbox','webhook_id'=>'WH1'],function($method,$path,$body,$req)use(&$state,$newId){
 if($path==='/v1/notifications/verify-webhook-signature')return ['verification_status'=>'SUCCESS'];
 if($method==='POST'&&$path==='/v2/checkout/orders'){$state['created']++;check($body['purchase_units'][0]['amount']['value']==='7.00','Browser price ignored');check(strlen($req)<=38,'PayPal idempotency identifier length');return ['id'=>'PP2'];}
 if(str_ends_with($path,'/capture')){$state['captured']++;$state['get_status']='COMPLETED';return ['id'=>'PP2','status'=>'COMPLETED'];}
 return ['id'=>'PP2','status'=>$state['get_status'],'purchase_units'=>[['reference_id'=>$newId,'custom_id'=>$newId,'payee'=>['merchant_id'=>'MERCHANT'],'payments'=>['captures'=>[['id'=>'CAP2','status'=>'COMPLETED','amount'=>['value'=>'7.00','currency_code'=>'EUR']]]]]]];
});
$shop=new Shop($c,$fake);$body=['id'=>$newId,'owner'=>$newOwner,'buyer'=>$buyer,'consent'=>true,'amount'=>'0.01'];
denied(fn()=>$shop->create(array_merge($body,['consent'=>false])),'Direct API requires consent');
$shop->create($body);$shop->create($body);check($state['created']===1,'Creation retry creates one PayPal order');
denied(fn()=>$shop->create(array_replace($body,['buyer'=>array_replace($buyer,['email'=>'changed@example.invalid'])])),'Changed billing email on existing order is rejected');
$shop->capture($newId,$newOwner);$shop->capture($newId,$newOwner);check($state['captured']===1,'Capture retry does not recapture payment');
denied(fn()=>$shop->webhook(['id'=>'EV1','event_type'=>'PAYMENT.CAPTURE.REFUNDED'],[]),'Webhook missing signature headers rejected');
$headers=['PAYPAL-AUTH-ALGO'=>'SHA256withRSA','PAYPAL-CERT-URL'=>'https://api.sandbox.paypal.com/v1/notifications/certs/CERT','PAYPAL-TRANSMISSION-ID'=>'T1','PAYPAL-TRANSMISSION-SIG'=>'SIG','PAYPAL-TRANSMISSION-TIME'=>gmdate('c')];
$event=['id'=>'EV1','event_type'=>'PAYMENT.CAPTURE.REFUNDED','resource'=>['supplementary_data'=>['related_ids'=>['capture_id'=>'CAP2']]]];
$shop->webhook($event,$headers);$shop->webhook($event,$headers);
check((int)$db->query('SELECT COUNT(*) FROM webhook_events')->fetchColumn()===1,'Webhook replay deduplicated');
denied(fn()=>$shop->capture($newId,$newOwner),'Refunded payment cannot be recaptured');
echo "ALL SHOP TESTS PASSED\n";

$c->config['paypal_env']='live';
check(str_starts_with($c->number($c->one('SELECT * FROM invoices WHERE order_id=?',['abc123'])), 'TEST-DOCK-'),'Old sandbox invoice keeps sandbox prefix');

$c->refund('CAP3');
$lateId=str_repeat('d',32);
$db->prepare('INSERT INTO orders VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)')->execute([$lateId,hash('sha256',$owner),'PP3',null,'created',700,'EUR',json_encode($buyer),'consent',Core::now(),Core::now(),null,null]);
$late=$o;$late['id']='PP3';$late['purchase_units'][0]['reference_id']=$lateId;$late['purchase_units'][0]['custom_id']=$lateId;$late['purchase_units'][0]['payments']['captures'][0]['id']='CAP3';
denied(fn()=>$c->paid($lateId,$late),'Early refund blocks later capture confirmation');
