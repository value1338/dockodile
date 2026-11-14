"""Source contracts for native update authentication and retained replacement guards.
Native compilation and app installation are separate release checks.
"""
from pathlib import Path
import unittest
ROOT=Path(__file__).resolve().parents[1]
class PublicUpdaterContracts(unittest.TestCase):
 def test_mac_update_gate_and_request_do_not_depend_on_purchase(self):
  s=(ROOT/'src/Updater.swift').read_text();gate=s.split('private var canInstall: Bool {',1)[1].split('private func install()',1)[0];request=s.split('private func install()',1)[1].split('private func swap',1)[0]
  self.assertNotIn('token',gate);self.assertNotIn('let token',request);self.assertNotIn('token=',request)
  for safety in ['installFolders.contains(parent)','Updater.team','AppTranslocation','isWritableFile']:self.assertIn(safety,gate)
  for safety in ['freshBundle?.bundleIdentifier','freshVersion.compare(current','freshVersion.compare(expected','Updater.signed']:self.assertIn(safety,s)
 def test_native_menus_do_not_request_activation(self):
  self.assertNotIn('Updates freischalten',(ROOT/'src/App.swift').read_text());self.assertNotIn('Updates freischalten',(ROOT/'windows/src/main.rs').read_text())
 def test_windows_update_request_does_not_depend_on_purchase(self):
  s=(ROOT/'windows/src/main.rs').read_text();gate=s.split('fn can_install() -> bool {',1)[1].split('unsafe fn schedule_updates()',1)[0];request=s.split('unsafe fn install_update()',1)[1].split('unsafe fn update_downloaded',1)[0]
  self.assertNotIn('store.token',gate);self.assertNotIn('store.token',request);self.assertNotIn('token=',request)
  self.assertIn('current_version=',request);self.assertIn('min_version=',request);self.assertIn('microsoft-store',gate)
if __name__=='__main__':unittest.main()
