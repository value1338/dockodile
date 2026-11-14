<?php
declare(strict_types=1);
ini_set('display_errors','0');
header('Cache-Control: no-store');header('Referrer-Policy: no-referrer');header('X-Content-Type-Options: nosniff');
header("Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'");
function reply(array $data,int $status=200): never {http_response_code($status);header('Content-Type: application/json; charset=utf-8');echo json_encode($data,JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES);exit;}
try {
 $private=dirname(__DIR__).'/private';
 require $private.'/app/bootstrap.php';
 require_once $private.'/app/Release.php';
 $path=parse_url($_SERVER['REQUEST_URI'],PHP_URL_PATH);$method=$_SERVER['REQUEST_METHOD'];
 // One purchase covers both systems. Without a platform the Mac is meant: Mac apps up to 0.2 send none.
 $platform=function()use($method): string {$p=$method==='GET'?($_GET['platform']??''):($_POST['platform']??'');return $p==='windows'?'windows':'mac';};
 // Highest version in the private folder is the current release. Uploading a DMG or EXE publishes the update.
 $latest=function(string $os)use($private): array {
  [$dir,$ext]=$os==='windows'?['win','.exe']:['dmg','.dmg'];
  $files=glob($private.'/'.$dir.'/Dockodile-*'.$ext);usort($files,fn($a,$b)=>version_compare(basename($b,$ext),basename($a,$ext)));
  if(!$files)throw new DomainException('Die Installationsdatei ist vorübergehend nicht verfügbar.',503);
  return [$files[0],substr(basename($files[0],$ext),10)];
 };
 $deliver=function(string $file): never {
  $handle=fopen($file,'rb');if(!$handle)throw new RuntimeException('Release unavailable');
  header('Content-Type: '.(str_ends_with($file,'.exe')?'application/vnd.microsoft.portable-executable':'application/x-apple-diskimage'));header('Content-Disposition: attachment; filename="'.basename($file).'"');header('Content-Length: '.filesize($file));header('Accept-Ranges: none');fpassthru($handle);fclose($handle);exit;
 };
 // Asked by the app once a day. No database, no identifiers.
 if($path==='/api/version'&&$method==='GET')reply(['version'=>$latest($platform())[1]]);
 $shop=dock_shop();$cfg=$shop->core->config;$secure=($_SERVER['HTTPS']??'')==='on';
 if($secure)header('Strict-Transport-Security: max-age=31536000');
 if($path==='/health'&&$method==='GET'){ $shop->core->db->query('SELECT 1');reply(['status'=>'ok']); }
 $free=!empty($cfg['free_downloads']);
 if($free&&(($path==='/download'&&in_array($method,['GET','POST'],true))||($path==='/api/app/update'&&$method==='POST'))){
  if(!$secure)reply(['error'=>'Für Downloads ist HTTPS erforderlich.'],426);
  if((int)($_SERVER['CONTENT_LENGTH']??0)>16384)reply(['error'=>'Anfrage zu groß.'],413);
  $input=$method==='GET'?$_GET:$_POST;
  [$file,$version]=$latest($platform());
  Release::check($version,$input['current_version']??null,$input['min_version']??null);
  $deliver($file);
 }
 if($free&&$method==='POST'&&($path==='/api/orders'||preg_match('~^/api/orders/[a-f0-9]{32}/capture$~D',$path)))reply(['error'=>'Dockodile ist kostenlos. Für den Download ist keine Zahlung erforderlich.'],503);
 if($path==='/api/config'&&$method==='GET')reply(['available'=>!$free&&$secure&&dock_ready($cfg),'sandbox'=>($cfg['paypal_env']??'sandbox')!=='live','client_id'=>!$free&&dock_ready($cfg)?$cfg['client_id']:null,'free_downloads'=>$free,'price'=>$free?'0.00':'7.00','currency'=>'EUR','consent_text'=>Shop::CONSENT,'email_enabled'=>!empty($cfg['mail_enabled']),'message'=>'Der Kauf ist gerade nicht verfügbar. Bitte später erneut versuchen.']);
 if($method!=='POST')reply(['error'=>'Nicht gefunden.'],404);
 if(!$secure)reply(['error'=>'Für Zahlungen und Downloads ist HTTPS erforderlich.'],426);
 if((int)($_SERVER['CONTENT_LENGTH']??0)>16384)reply(['error'=>'Anfrage zu groß.'],413);
 $origin=$_SERVER['HTTP_ORIGIN']??'';
 if($path!=='/api/paypal/webhook'&&$path!=='/api/app/update'&&!in_array($origin,['https://dockodile.de','https://dockodile.lakeco.de'],true))reply(['error'=>'Ungültige Herkunft.'],403);
 if(!in_array($path,['/download','/api/invoice','/api/paypal/webhook','/api/app/update','/api/update','/api/voucher'],true)&&!dock_ready($cfg))reply(['error'=>'Der Kauf ist vorübergehend pausiert. Es wurde keine Zahlung ausgelöst.'],503);
 // Transient HMAC bucket on the filesystem: no raw IPs, no persistent address database.
 $bucket=hash_hmac('sha256',($_SERVER['REMOTE_ADDR']??'unknown').':'.intdiv(time(),600),$cfg['token_key']);
 $dir=$private.'/rate';
 if(!is_dir($dir))mkdir($dir,0700,true);
 $f=fopen($dir.'/'.$bucket,'c+');if(!$f)throw new RuntimeException('Rate limiter unavailable');flock($f,LOCK_EX);$n=(int)stream_get_contents($f);if($n>=60){flock($f,LOCK_UN);fclose($f);header('Retry-After: 600');reply(['error'=>'Zu viele Anfragen. Bitte später erneut versuchen.'],429);}rewind($f);ftruncate($f,0);fwrite($f,(string)($n+1));flock($f,LOCK_UN);fclose($f);
 if($path==='/download'){
  $token=$_POST['token']??'';if(!is_string($token)||!preg_match('/^[A-Za-z0-9_-]{43}$/D',$token))throw new DomainException('Download-Link nicht gefunden.',404);
  $os=$platform();[$file,$version]=$latest($os);Release::check($version,$_POST['current_version']??null,$_POST['min_version']??null);$shop->core->consume($token,($os==='windows'?'win-':'').$version);$deliver($file);
 }
 // The app itself, with the token it stored through dockodile://activate. No browser, hence no Origin.
 if($path==='/api/app/update'){
  $token=$_POST['token']??'';if(!is_string($token)||!preg_match('/^[A-Za-z0-9_-]{43}$/D',$token))throw new DomainException('Zu diesem Schlüssel gibt es keinen gültigen Kauf.',403);
  $os=$platform();[$file,$version]=$latest($os);Release::check($version,$_POST['current_version']??null,$_POST['min_version']??null);$shop->core->licensed($token,($os==='windows'?'win-':'').$version);$deliver($file);
 }
 if($path==='/api/invoice'){
  $id=$_POST['id']??'';$owner=$_POST['owner']??'';if(!is_string($id)||!is_string($owner))throw new DomainException('Ungültige Anfrage.',422);
  header('Content-Type: text/html; charset=utf-8');echo $shop->core->invoice($id,$owner);exit;
 }
 $raw=file_get_contents('php://input',false,null,0,16385);if(strlen($raw)>16384)reply(['error'=>'Anfrage zu groß.'],413);
 $body=json_decode($raw,true,64,JSON_THROW_ON_ERROR);if(!is_array($body))throw new DomainException('Ungültige Anfrage.',422);
 if($path==='/api/paypal/webhook'){
  $headers=[];foreach($_SERVER as $k=>$v)if(str_starts_with($k,'HTTP_PAYPAL_'))$headers[str_replace('_','-',substr($k,5))]=$v;
  $shop->webhook($body,$headers);reply(['received'=>true]);
 }
 if($path==='/api/voucher'){
  $code=$body['code']??'';$email=$body['email']??'';
  if(!is_string($code)||!is_string($email)||strlen($email)>160||!filter_var($email,FILTER_VALIDATE_EMAIL))throw new DomainException('Bitte Gutscheincode und eine gültige E-Mail-Adresse eingeben.',422);
  reply(['token'=>$shop->core->redeem(strtoupper(trim($code)),$email)]);
 }
 if($path==='/api/update'){$shop->update($body['email']??null,$latest('mac')[1]);reply(['sent'=>true]);}
 if(!dock_ready($cfg))reply(['error'=>'Der Kauf ist vorübergehend pausiert. Es wurde keine Zahlung ausgelöst.'],503);
 if($path==='/api/orders')reply($shop->create($body));
 if(preg_match('~^/api/orders/([a-f0-9]{32})/capture$~D',$path,$m)){
  $owner=$body['owner']??'';if(!is_string($owner))throw new DomainException('Ungültige Bestellreferenz.',422);reply($shop->capture($m[1],$owner));
 }
 reply(['error'=>'Nicht gefunden.'],404);
}catch(JsonException $e){reply(['error'=>'Ungültige Anfrage.'],400);}
catch(DomainException $e){reply(['error'=>$e->getMessage()],in_array($e->getCode(),[400,403,404,409,410,413,422,429,502,503],true)?$e->getCode():400);}
catch(Throwable $e){error_log('Dockodile request failed ('.get_class($e).')');reply(['error'=>'Der Dienst ist vorübergehend nicht verfügbar. Bitte erneut versuchen.'],503);}
