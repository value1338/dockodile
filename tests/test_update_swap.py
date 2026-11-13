"""Run the actual embedded replacement helper against isolated fake app bundles."""
import os,re,subprocess,tempfile,textwrap,unittest
from pathlib import Path
SOURCE=Path(__file__).resolve().parents[1]/'src/Updater.swift'
SCRIPT=textwrap.dedent(re.search(r'let script = """\n(.*?)\n        """',SOURCE.read_text(),re.S)[1])
class SwapTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory(prefix='dockodile-swap-');self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
  self.target=self.root/'Programme mit Leerzeichen'/'Dockodile.app';self.target.mkdir(parents=True);(self.target/'version').write_text('old')
  self.stage=self.target.parent/'.Dockodile-update-test.app';self.stage.mkdir();(self.stage/'version').write_text('new')
  self.backup=self.target.parent/'.Dockodile-rollback-test.app';self.bin=self.root/'bin';self.bin.mkdir()
  open_script='''#!/bin/sh
printf '%s\\n' "$1" >> "$OPEN_LOG"
if [ "${FAIL_OPEN_ONCE:-}" = 1 ] && [ ! -e "$FAIL_FLAG" ]; then touch "$FAIL_FLAG"; exit 1; fi
'''
  (self.bin/'open').write_text(open_script);(self.bin/'open').chmod(0o755)
  self.env={**os.environ,'PATH':str(self.bin)+':/usr/bin:/bin','OLDPID':'99999999','TARGET':str(self.target),'STAGED':str(self.stage),'BACKUP':str(self.backup),'OPEN_LOG':str(self.root/'opened'),'FAIL_FLAG':str(self.root/'failed-once')}
 def run_helper(self):return subprocess.run(['/bin/sh','-c',SCRIPT],env=self.env,capture_output=True,timeout=5)
 def assert_single(self,version):
  self.assertEqual((self.target/'version').read_text(),version);self.assertFalse(self.stage.exists());self.assertFalse(self.backup.exists())
 def test_success_replaces_at_same_path_and_removes_old_copy(self):
  self.assertEqual(self.run_helper().returncode,0);self.assert_single('new');self.assertEqual((self.root/'opened').read_text().strip(),str(self.target))
 def test_launch_failure_restores_old_app_without_duplicate(self):
  self.env['FAIL_OPEN_ONCE']='1';self.assertEqual(self.run_helper().returncode,1);self.assert_single('old')
 def test_failed_stage_move_restores_old_app(self):
  (self.bin/'mv').write_text('#!/bin/sh\nif [ "$1" = "$STAGED" ]; then exit 1; fi\nexec /bin/mv "$@"\n');(self.bin/'mv').chmod(0o755)
  self.assertEqual(self.run_helper().returncode,1);self.assert_single('old')
 def test_failed_old_move_leaves_old_app_and_removes_stage(self):
  (self.bin/'mv').write_text('#!/bin/sh\nif [ "$1" = "$TARGET" ]; then exit 1; fi\nexec /bin/mv "$@"\n');(self.bin/'mv').chmod(0o755)
  self.assertEqual(self.run_helper().returncode,1);self.assert_single('old')
if __name__=='__main__':unittest.main()
