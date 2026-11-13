<?php
declare(strict_types=1);
final class PayPal {
 private string $access='';
 public function __construct(private array $config,private mixed $transport=null) {}
 public function request(string $method,string $path,?array $data=null,string $requestId=''): array {
  if($this->transport)return ($this->transport)($method,$path,$data,$requestId);
  if(!$this->access){$r=$this->http('POST','/v1/oauth2/token',null,'',true);$this->access=$r['access_token']??'';if(!$this->access)throw new RuntimeException('PayPal authentication failed');}
  return $this->http($method,$path,$data,$requestId);
 }
 private function http(string $method,string $path,?array $data,string $requestId,bool $auth=false): array {
  $base=($this->config['paypal_env']??'sandbox')==='live'?'https://api-m.paypal.com':'https://api-m.sandbox.paypal.com';
  $ch=curl_init($base.$path);$headers=['Accept: application/json'];
  $opts=[CURLOPT_RETURNTRANSFER=>true,CURLOPT_CONNECTTIMEOUT=>10,CURLOPT_TIMEOUT=>35,CURLOPT_CUSTOMREQUEST=>$method,CURLOPT_FOLLOWLOCATION=>false,CURLOPT_PROTOCOLS=>CURLPROTO_HTTPS];
  if($auth){$opts[CURLOPT_USERPWD]=$this->config['client_id'].':'.$this->config['client_secret'];$opts[CURLOPT_POSTFIELDS]='grant_type=client_credentials';$headers[]='Content-Type: application/x-www-form-urlencoded';}
  else {$headers[]='Authorization: Bearer '.$this->access;$headers[]='Content-Type: application/json';if($data!==null)$opts[CURLOPT_POSTFIELDS]=json_encode($data,JSON_THROW_ON_ERROR);}
  if($requestId)$headers[]='PayPal-Request-Id: '.$requestId;
  $opts[CURLOPT_HTTPHEADER]=$headers;curl_setopt_array($ch,$opts);$body=curl_exec($ch);$status=curl_getinfo($ch,CURLINFO_RESPONSE_CODE);curl_close($ch);
  if($body===false||$status<200||$status>=300)throw new DomainException('PayPal ist gerade nicht erreichbar oder konnte die Zahlung nicht ausführen. Bitte prüfe den Bestellstatus erneut.',502);
  return json_decode($body,true,512,JSON_THROW_ON_ERROR);
 }
 public function verify(array $event,array $headers): bool {
  foreach(['PAYPAL-AUTH-ALGO','PAYPAL-CERT-URL','PAYPAL-TRANSMISSION-ID','PAYPAL-TRANSMISSION-SIG','PAYPAL-TRANSMISSION-TIME'] as $h)if(empty($headers[$h]))return false;
  $url=parse_url($headers['PAYPAL-CERT-URL']);$allowed=($this->config['paypal_env']??'sandbox')==='live'?'api.paypal.com':'api.sandbox.paypal.com';
  if(($url['scheme']??'')!=='https'||($url['host']??'')!==$allowed||!str_starts_with($url['path']??'','/v1/notifications/certs/'))return false;
  if(empty($this->config['webhook_id']))return false;
  $r=$this->request('POST','/v1/notifications/verify-webhook-signature',['auth_algo'=>$headers['PAYPAL-AUTH-ALGO'],'cert_url'=>$headers['PAYPAL-CERT-URL'],'transmission_id'=>$headers['PAYPAL-TRANSMISSION-ID'],'transmission_sig'=>$headers['PAYPAL-TRANSMISSION-SIG'],'transmission_time'=>$headers['PAYPAL-TRANSMISSION-TIME'],'webhook_id'=>$this->config['webhook_id'],'webhook_event'=>$event]);
  return ($r['verification_status']??'')==='SUCCESS';
 }
}
