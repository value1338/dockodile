<?php
require __DIR__.'/../Release.php';
foreach([['0.2.0','0.2.0','0.3.0',409],['0.1.0','0.2.0',null,409],['0.2.0','0.1.0','0.3.0',409],['0.3.0','0.2.0','0.3.0',0],['0.4.0','0.2.0','0.3.0',0],['0.3.0',null,['invalid'],422],['0.2.0',null,null,0]] as [$v,$current,$min,$expected]){
 $code=0;try{Release::check($v,$current,$min);}catch(DomainException $e){$code=$e->getCode();}
 if($code!==$expected)throw new RuntimeException('Unexpected release guard result');
}
echo "PASS release downgrade/equal/stale/invalid-input guards and legacy compatibility\n";
