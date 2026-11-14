"""Exercise the real API router with isolated releases and no payment or mail access."""
import unittest,tempfile,subprocess,socket,time,urllib.request,urllib.error,json,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class FreeDownloads(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.tmp=tempfile.TemporaryDirectory();cls.root=Path(cls.tmp.name);pub=cls.root/'httpdocs';pub.mkdir();private=cls.root/'private';(private/'app').mkdir(parents=True)
  shutil.copy2(ROOT/'public/api.php',pub/'api.php');shutil.copy2(ROOT/'Release.php',private/'app/Release.php')
  (private/'app/bootstrap.php').write_text('''<?php
class Shop {const CONSENT='test';}
function dock_ready($c){return true;}
function dock_shop(){return (object)['core'=>(object)['config'=>['free_downloads'=>true,'checkout_enabled'=>false,'paypal_env'=>'live','token_key'=>'test-only','mail_enabled'=>true,'client_id'=>'must-not-be-published']]];}
$_SERVER['HTTPS']='on';
''')
  for folder,name in [('dmg','Dockodile-0.3.2.dmg'),('win','Dockodile-0.4.1.exe')]:
   (private/folder).mkdir();(private/folder/name).write_bytes(b'current-'+folder.encode())
  with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  cls.url=f'http://127.0.0.1:{port}';cls.proc=subprocess.Popen(['php','-S',f'127.0.0.1:{port}',str(pub/'api.php')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  for _ in range(50):
   try:urllib.request.urlopen(cls.url+'/api/version',timeout=.2);break
   except Exception:time.sleep(.05)
 @classmethod
 def tearDownClass(cls):cls.proc.terminate();cls.proc.wait(timeout=5);cls.tmp.cleanup()
 def req(self,path,body=None):
  req=urllib.request.Request(self.url+path,data=body,headers={'Origin':'https://dockodile.de'})
  try:
   with urllib.request.urlopen(req) as r:return r.status,r.read()
  except urllib.error.HTTPError as e:return e.code,e.read()
 def test_public_mac_download_needs_no_account(self):self.assertEqual(self.req('/download?platform=mac'),(200,b'current-dmg'))
 def test_public_windows_download_needs_no_account(self):self.assertEqual(self.req('/download?platform=windows'),(200,b'current-win'))
 def test_app_update_needs_no_token(self):self.assertEqual(self.req('/api/app/update',b'current_version=0.3.1&min_version=0.3.2'),(200,b'current-dmg'))
 def test_stale_and_invalid_updates_are_blocked(self):
  self.assertEqual(self.req('/download?platform=mac&current_version=0.3.2')[0],409)
  self.assertEqual(self.req('/api/app/update',b'min_version=invalid')[0],422)
 def test_purchase_config_is_disabled(self):
  status,body=self.req('/api/config');self.assertEqual(status,200);c=json.loads(body);self.assertFalse(c['available']);self.assertIsNone(c['client_id']);self.assertEqual(c['price'],'0.00');self.assertTrue(c['free_downloads'])
 def test_no_new_orders_or_captures(self):
  for path in ['/api/orders','/api/orders/'+'a'*32+'/capture']:self.assertEqual(self.req(path,b'{}')[0],503)
if __name__=='__main__':unittest.main()
