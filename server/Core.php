<?php
declare(strict_types=1);
final class Core {
    public function __construct(public PDO $db, public array $config) {}
    public static function now(): string { return gmdate('Y-m-d H:i:s'); }
    public static function validateBuyer(array $b): void {
        foreach (['name','address','postal_code','city','country','email'] as $k) {
            if (!isset($b[$k]) || !is_string($b[$k]) || trim($b[$k])==='' || mb_strlen($b[$k])>160 || preg_match('/[\x00-\x1f]/',$b[$k])) throw new DomainException('Bitte die Rechnungsanschrift vollständig ausfüllen.',422);
        }
        if (!filter_var($b['email'],FILTER_VALIDATE_EMAIL)) throw new DomainException('Bitte eine gültige E-Mail-Adresse eingeben.',422);
        if ($b['country']!=='DE') throw new DomainException('Der Verkauf ist zunächst auf Deutschland beschränkt.',422);
    }
    public function one(string $sql,array $args=[]): array|false {$q=$this->db->prepare($sql);$q->execute($args);return $q->fetch(PDO::FETCH_ASSOC);}
    public function exec(string $sql,array $args=[]): void {$this->db->prepare($sql)->execute($args);}
    public function lock(): string {return $this->db->getAttribute(PDO::ATTR_DRIVER_NAME)==='mysql'?' FOR UPDATE':'';}
    public function authorize(string $id,string $owner): array {
        $o=$this->one('SELECT * FROM orders WHERE id=?',[$id]);
        if (!$o || !hash_equals($o['owner_hash'],hash('sha256',$owner))) throw new DomainException('Bestellung nicht gefunden.',404);
        return $o;
    }
    public function validatePayment(array $local,array $remote): array {
        $units=$remote['purchase_units']??[];$u=$units[0]??[];$caps=$u['payments']['captures']??[];$cap=$caps[0]??[];
        if (count($units)!==1 || count($caps)!==1 || ($remote['id']??'')!==$local['paypal_order_id'] || ($remote['status']??'')!=='COMPLETED'
            || ($u['custom_id']??'')!==$local['id'] || ($u['reference_id']??'')!==$local['id'] || ($cap['status']??'')!=='COMPLETED'
            || ($u['payee']['merchant_id']??'')!==($this->config['merchant_id']??'') || empty($this->config['merchant_id'])
            || ($cap['amount']['currency_code']??'')!==$local['currency'] || ($cap['amount']['value']??'')!==number_format((int)$local['amount']/100,2,'.','')
            || !preg_match('/^[A-Z0-9]+$/D',$cap['id']??'')) throw new DomainException('Die Zahlung konnte noch nicht bestätigt werden. Bitte den Status erneut prüfen.',409);
        return $cap;
    }
    public function paid(string $id,array $remote): array {
        $this->db->beginTransaction();
        try {
            $o=$this->one('SELECT * FROM orders WHERE id=?'.$this->lock(),[$id]);
            if(!$o)throw new DomainException('Bestellung nicht gefunden.',404);
            if($o['status']==='refunded')throw new DomainException('Diese Zahlung wurde erstattet oder zurückgebucht.',410);
            $cap=$this->validatePayment($o,$remote);
            if($this->one('SELECT capture_id FROM capture_blocks WHERE capture_id=?'.$this->lock(),[$cap['id']]))throw new DomainException('Diese Zahlung wurde erstattet oder zurückgebucht.',410);
            if($o['status']!=='paid') {
                $now=self::now();$this->exec('UPDATE orders SET status=?,paypal_capture_id=?,paid_at=? WHERE id=?',['paid',$cap['id'],$now,$id]);
                $snapshot=['buyer'=>json_decode($o['buyer_json'],true),'date'=>$now,'amount'=>(int)$o['amount'],'currency'=>$o['currency'],'capture_id'=>$cap['id'],'sandbox'=>($this->config['paypal_env']??'sandbox')!=='live','product'=>'mac+windows','consent_text'=>$o['consent_text'],'consent_at'=>$o['consent_at'],'seller'=>['name'=>'Riccardo Reichle · Lake Code','address'=>'Milchbänkle 12, 88079 Kressbronn am Bodensee','vat_id'=>'DE463771423','email'=>'riccardo@lakeco.de','phone'=>'+49 152 37157595','website'=>'lakeco.de']];
                $this->exec('INSERT INTO invoices(order_id,snapshot_json,created_at) VALUES(?,?,?)',[$id,json_encode($snapshot,JSON_THROW_ON_ERROR),$now]);
                $this->exec('INSERT INTO download_tokens(order_id,token_hash,expires_at,downloads,max_downloads,created_at) VALUES(?,?,?,0,?,?)',[$id,hash('sha256',$this->token($id)),gmdate('Y-m-d H:i:s',time()+86400*(int)($this->config['token_days']??30)),(int)($this->config['token_max_downloads']??10),$now]);
            } elseif ($o['paypal_capture_id']!==$cap['id']) throw new DomainException('Abweichende Zahlungsreferenz.',409);
            $r=$this->receipt($id);$this->db->commit();return $r;
        } catch(Throwable $e) {if($this->db->inTransaction())$this->db->rollBack();throw $e;}
    }
    public function token(string $id): string {return rtrim(strtr(base64_encode(hash_hmac('sha256','download:'.$id,$this->config['token_key'],true)),'+/','-_'),'=');}
    public function receipt(string $id): array {
        $o=$this->one('SELECT * FROM orders WHERE id=?',[$id]);
        if(!$o||$o['status']!=='paid')throw new DomainException('Keine bestätigte Zahlung.',409);
        $i=$this->one('SELECT id,created_at,snapshot_json FROM invoices WHERE order_id=?',[$id]);
        $confirmed=$this->one('SELECT accepted_at FROM confirmations WHERE order_id=?',[$id]);
        $sent=!empty($confirmed['accepted_at']);
        return ['token'=>$sent?$this->token($id):null,'invoice_number'=>$this->number($i),'status'=>$sent?'paid':'awaiting_confirmation','email_sent'=>$sent];
    }
    public function number(array $i): string {return ((json_decode($i['snapshot_json'],true)['sandbox']??true)?'TEST-DOCK-':'DOCK-').substr($i['created_at'],0,4).'-'.str_pad((string)$i['id'],6,'0',STR_PAD_LEFT);}
    public function refund(string $capture): void {
        $this->db->beginTransaction();
        try {
            $block=$this->one('SELECT capture_id FROM capture_blocks WHERE capture_id=?'.$this->lock(),[$capture]);
            if(!$block)$this->exec('INSERT INTO capture_blocks(capture_id,created_at) VALUES(?,?)',[$capture,self::now()]);
            $o=$this->one('SELECT id FROM orders WHERE paypal_capture_id=?'.$this->lock(),[$capture]);
            if($o)$this->exec('UPDATE orders SET status=?,refunded_at=? WHERE id=?',['refunded',self::now(),$o['id']]);
            $this->db->commit();
        } catch(Throwable $e){if($this->db->inTransaction())$this->db->rollBack();throw $e;}
    }
    public function consume(string $token,string $version): void {
        $this->db->beginTransaction();
        try {
            $t=$this->one('SELECT t.*,o.status FROM download_tokens t JOIN orders o ON o.id=t.order_id WHERE token_hash=?'.$this->lock(),[hash('sha256',$token)]);
            if(!$t)throw new DomainException('Download-Link nicht gefunden.',404);
            if(empty($this->one('SELECT accepted_at FROM confirmations WHERE order_id=?',[$t['order_id']])['accepted_at']))throw new DomainException('Die Vertragsbestätigung wird noch versendet. Bitte den Bestellstatus erneut prüfen.',409);
            if($t['status']!=='paid'||$t['expires_at']<=self::now()||(int)$t['downloads']>=(int)$t['max_downloads'])throw new DomainException('Dieser Download-Link ist abgelaufen, verbraucht oder gesperrt. Einen neuen Link bekommst du auf https://dockodile.de/update.html, sonst hilft riccardo@lakeco.de.',410);
            $this->exec('UPDATE download_tokens SET downloads=downloads+1 WHERE id=?',[$t['id']]);
            $this->exec('INSERT INTO downloads(token_id,version,at) VALUES(?,?,?)',[$t['id'],$version,self::now()]);$this->db->commit();
        }catch(Throwable $e){if($this->db->inTransaction())$this->db->rollBack();throw $e;}
    }
    // Update link by mail: renews the download link of the newest paid order for this address.
    public function renew(string $email,?callable $deliver=null): ?string {
        // ponytail: filters paid orders in PHP so SQLite tests and MySQL share one query. Add an indexed email column at tens of thousands of orders.
        $rows=$this->db->query("SELECT o.id,o.buyer_json FROM orders o JOIN confirmations c ON c.order_id=o.id WHERE o.status='paid' AND c.accepted_at IS NOT NULL ORDER BY o.paid_at DESC")->fetchAll(PDO::FETCH_ASSOC);
        $life=86400*(int)($this->config['token_days']??30);
        foreach($rows as $o){
            if(strcasecmp((string)(json_decode($o['buyer_json'],true)['email']??''),$email)!==0)continue;
            $t=$this->one('SELECT expires_at FROM download_tokens WHERE order_id=?',[$o['id']]);
            // Throttle without a new column: a link bought or renewed in the last ten minutes is not mailed again.
            if(!$t||$t['expires_at']>gmdate('Y-m-d H:i:s',time()+$life-600))return null;
            $token=$this->token($o['id']);
            // A rejected mail handoff must leave the existing link and retry window unchanged.
            if($deliver!==null&&!$deliver($token))return null;
            $this->exec('UPDATE download_tokens SET expires_at=?,downloads=0 WHERE order_id=?',[gmdate('Y-m-d H:i:s',time()+$life),$o['id']]);
            return $token;
        }
        return null;
    }
    // Vouchers need no table: the code is signed with the token key, and every use takes one
    // order id derived from the code, so the primary key enforces the limit.
    // ponytail: a leaked code cannot be withdrawn, only used up. Keep the number of uses small; add a revocation list if that ever hurts.
    public function voucherCode(int $n,int $uses): string {return 'DOCK-'.$n.'-'.$uses.'-'.strtoupper(substr(hash_hmac('sha256',"voucher:$n:$uses",$this->config['token_key']),0,10));}
    public function voucherSlots(int $n,int $uses): array {$ids=[];for($i=0;$i<$uses;$i++)$ids[]=md5("voucher:$n:$uses:$i");return $ids;}
    public function redeem(string $code,string $email): string {
        if(!preg_match('/^DOCK-([1-9][0-9]{0,3})-([1-9][0-9]{0,2})-[0-9A-F]{10}$/D',$code,$m)||!hash_equals($this->voucherCode((int)$m[1],(int)$m[2]),$code))throw new DomainException('Diesen Gutscheincode gibt es nicht.',404);
        foreach($this->voucherSlots((int)$m[1],(int)$m[2]) as $id){
            $o=$this->one('SELECT buyer_json,status FROM orders WHERE id=?',[$id]);
            if($o){
                // The same address gets its copy again instead of using up another one.
                if($o['status']==='paid'&&strcasecmp((string)(json_decode($o['buyer_json'],true)['email']??''),$email)===0)return $this->token($id);
                continue;
            }
            $now=self::now();$this->db->beginTransaction();
            try{
                $this->exec('INSERT INTO orders(id,owner_hash,status,amount,currency,buyer_json,consent_text,consent_at,created_at,paid_at) VALUES(?,?,?,?,?,?,?,?,?,?)',[$id,hash('sha256',random_bytes(32)),'paid',0,'EUR',json_encode(['email'=>$email,'voucher'=>$code],JSON_THROW_ON_ERROR),'Gutschein '.$code,$now,$now,$now]);
                $this->exec('INSERT INTO confirmations(order_id,contract_text,accepted_at,created_at) VALUES(?,?,?,?)',[$id,'Gutschein '.$code.', kostenlose Überlassung ohne Rechnung.',$now,$now]);
                $this->exec('INSERT INTO download_tokens(order_id,token_hash,expires_at,downloads,max_downloads,created_at) VALUES(?,?,?,0,?,?)',[$id,hash('sha256',$this->token($id)),gmdate('Y-m-d H:i:s',time()+86400*(int)($this->config['token_days']??30)),(int)($this->config['token_max_downloads']??10),$now]);
                $this->db->commit();return $this->token($id);
            }catch(PDOException $e){
                if($this->db->inTransaction())$this->db->rollBack();
                if((string)$e->getCode()!=='23000')throw $e; // otherwise someone took this copy a moment ago
            }
        }
        throw new DomainException('Dieser Gutschein ist aufgebraucht.',410);
    }
    // In-app update: the token works as licence for as long as the order stays paid. No expiry, no counter.
    public function licensed(string $token,string $version): void {
        $t=$this->one('SELECT t.id,o.status FROM download_tokens t JOIN orders o ON o.id=t.order_id WHERE token_hash=?',[hash('sha256',$token)]);
        if(!$t||$t['status']!=='paid')throw new DomainException('Zu diesem Schlüssel gibt es keinen gültigen Kauf.',403);
        $this->exec('INSERT INTO downloads(token_id,version,at) VALUES(?,?,?)',[$t['id'],$version,self::now()]);
    }
    public function invoice(string $id,string $owner): string {
        $o=$this->authorize($id,$owner);$i=$this->one('SELECT * FROM invoices WHERE order_id=?',[$id]);
        if(!$i)throw new DomainException('Rechnung noch nicht verfügbar.',404);
        require_once __DIR__.'/Invoice.php';
        return Invoice::render(json_decode($i['snapshot_json'],true),$this->number($i),$o['status']==='refunded');
    }
}
