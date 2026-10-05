"""Installer integration checks with local release fixtures; no editor or network needed."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import shutil
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / 'target/debug/xmd-installer'
BOOTSTRAP = ROOT / 'install.sh'

class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.config = self.root / 'config'
        self.data = self.root / 'Zed data'
        self.backups = self.root / 'backups'
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        tools = self.root / 'tools'
        tools.mkdir()
        gh = tools / 'gh'
        gh.write_text('''#!/bin/sh
if [ "$1" = api ]; then echo v0.2.0; exit; fi
while [ "$#" -gt 0 ]; do
 case "$1" in
 --pattern) asset=$2; shift 2;;
 --dir) destination=$2; shift 2;;
 *) shift;;
 esac
done
printf "%s\\n" "$asset" >> "$DOWNLOAD_LOG"
cp "$FIXTURE_ASSETS/$asset" "$destination/$asset"
''')
        gh.chmod(0o755)
        code = tools / 'code'
        code.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$CODE_ARGS"\nexit "${CODE_EXIT:-0}"\n')
        code.chmod(0o755)
        self.env = dict(os.environ, PATH=str(tools)+os.pathsep+os.environ['PATH'],
            FIXTURE_ASSETS=str(self.assets), XMD_BIN_DIR=str(self.bin),
            XMD_ZED_CONFIG_DIR=str(self.config), XMD_ZED_DATA_DIR=str(self.data),
            XMD_BACKUP_DIR=str(self.backups), XMD_VERSION='latest',
            XMD_INSTALLER_CACHE_DIR=str(self.root/'cache with spaces'), DOWNLOAD_LOG=str(self.root/'downloads'),
            XMD_CODE_BIN=str(code), XMD_VSCODE_USER_DATA_DIR=str(self.root/'VS Code data'),
            XMD_VSCODE_EXTENSIONS_DIR=str(self.root/'VS Code extensions'), CODE_ARGS=str(self.root/'code-args'))
        payload = self.root / 'payload'
        payload.mkdir()
        (payload/'xmd').write_text('#!/bin/sh\necho "xmd 0.2.0"\n')
        (payload/'xmd').chmod(0o755)
        self.archive(f'xmd-{os.uname().sysname}-{os.uname().machine}.tar.gz', payload, ['xmd'])
        (payload/'extension.toml').write_text('id = "xmd"\nversion = "0.2.0"\n')
        (payload/'extension.wasm').write_bytes(b'\0asm')
        self.archive('xmd-zed.tar.gz', payload, ['extension.toml','extension.wasm'])
        vsix = self.assets/'xmd.vsix'
        with zipfile.ZipFile(vsix, 'w') as archive:
            archive.writestr('extension/package.json', json.dumps({'name':'xmd','publisher':'drbh','version':'0.2.0'}))
        (self.assets/'xmd.vsix.sha256').write_text(hashlib.sha256(vsix.read_bytes()).hexdigest()+'  xmd.vsix\n')

    def archive(self, name, root, members):
        path = self.assets/name
        with tarfile.open(path,'w:gz') as archive:
            for member in members: archive.add(root/member,arcname=member)
        (self.assets/(name+'.sha256')).write_text(hashlib.sha256(path.read_bytes()).hexdigest()+'  '+name+'\n')

    def run_install(self, editor="zed"):
        return subprocess.run([str(INSTALLER),'--editor',editor,'--github-auth'], env=self.env,capture_output=True,text=True)

    def test_clean_install_and_repeat(self):
        for _ in range(2):
            result=self.run_install();self.assertEqual(result.returncode,0,result.stderr)
        settings=json.loads((self.config/'settings.json').read_text())
        self.assertEqual(settings['languages']['XMD']['semantic_tokens'],'full')
        self.assertEqual(settings['lsp']['xmd']['binary']['path'],str((self.bin/'xmd').resolve()))
        self.assertTrue((self.data/'extensions/installed/xmd/extension.wasm').is_file())
        self.assertEqual(len(list(self.backups.iterdir())),2)

    def test_jsonc_and_dev_symlink_are_preserved(self):
        self.config.mkdir()
        settings='''{
  // Keep this comment and unrelated values.
  "languages": {"Python": {"tab_size": 4}, "XMD": {"semantic_tokens": "combined",},},
  "lsp": {"xmd": {"binary": {"path": "/custom/xmd", "arguments": ["lsp"],},},},
  "literal": "https://example.com/*literal*/",
}'''
        (self.config/'settings.json').write_text(settings)
        source=self.root/'dev-source';source.mkdir();(source/'keep').write_text('untouched')
        installed=self.data/'extensions/installed';installed.mkdir(parents=True)
        (installed/'xmd').symlink_to(source,target_is_directory=True)
        result=self.run_install();self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual((self.config/'settings.json').read_text(),settings)
        self.assertFalse((installed/'xmd').is_symlink())
        backup=next(self.backups.iterdir())
        self.assertTrue((backup/'zed-extension').is_symlink())
        self.assertEqual((source/'keep').read_text(),'untouched')

    def test_invalid_settings_leave_install_untouched(self):
        self.config.mkdir();(self.config/'settings.json').write_text('{"languages": broken}')
        result=self.run_install();self.assertNotEqual(result.returncode,0)
        self.assertFalse((self.bin/'xmd').exists())
        self.assertFalse(self.backups.exists())

    def test_bad_checksum_leaves_install_untouched(self):
        (self.assets/'xmd-zed.tar.gz.sha256').write_text('invalid\n')
        result=self.run_install();self.assertNotEqual(result.returncode,0)
        self.assertIn('checksum mismatch',result.stderr)
        self.assertFalse((self.bin/'xmd').exists())

    def test_vscode_install_and_repeat(self):
        for _ in range(2):
            result=self.run_install('vscode');self.assertEqual(result.returncode,0,result.stderr)
        settings=json.loads((Path(self.env['XMD_VSCODE_USER_DATA_DIR'])/'User/settings.json').read_text())
        self.assertEqual(settings['xmd.serverPath'],str((self.bin/'xmd').resolve()))
        self.assertTrue(settings['[xmd]']['editor.semanticHighlighting.enabled'])
        args=(self.root/'code-args').read_text().splitlines()
        self.assertEqual(args[:4],['--user-data-dir',self.env['XMD_VSCODE_USER_DATA_DIR'],'--extensions-dir',self.env['XMD_VSCODE_EXTENSIONS_DIR']])
        self.assertEqual(args[-3],'--install-extension')
        self.assertEqual(args[-1],'--force')

    def test_vscode_preserves_existing_jsonc_preferences(self):
        settings=Path(self.env['XMD_VSCODE_USER_DATA_DIR'])/'User/settings.json'
        settings.parent.mkdir(parents=True)
        original='{// keep me\n"xmd.serverPath":"custom-xmd","[xmd]":{"editor.semanticHighlighting.enabled":false,},}'
        settings.write_text(original)
        result=self.run_install('vscode');self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(settings.read_text(),original)

    def test_vscode_repairs_missing_server_path_and_backs_up_settings(self):
        settings=Path(self.env['XMD_VSCODE_USER_DATA_DIR'])/'User/settings.json'
        settings.parent.mkdir(parents=True)
        original='{// old checkout\n"xmd.serverPath":'+json.dumps(str(self.root/'removed-checkout/xmd'))+',}'
        settings.write_text(original)
        result=self.run_install('vscode')
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('replacing missing VS Code server path',result.stdout)
        self.assertIn('// old checkout',settings.read_text())
        self.assertIn(str((self.bin/'xmd').resolve()),settings.read_text())
        self.assertEqual(next(self.backups.glob('*/vscode-settings.json')).read_text(),original)

    def test_vscode_keeps_existing_absolute_server_path(self):
        settings=Path(self.env['XMD_VSCODE_USER_DATA_DIR'])/'User/settings.json'
        settings.parent.mkdir(parents=True)
        custom=self.root/'custom-xmd'
        custom.write_text('#!/bin/sh\nexit 0\n')
        custom.chmod(0o755)
        settings.write_text(json.dumps({'xmd.serverPath':str(custom)}))
        result=self.run_install('vscode')
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(json.loads(settings.read_text())['xmd.serverPath'],str(custom))

    def test_vscode_cli_failure_is_reported(self):
        self.env['CODE_EXIT']='1'
        result=self.run_install('vscode');self.assertNotEqual(result.returncode,0)
        self.assertNotIn('configured vscode highlighting',result.stdout)

    def test_vscode_bad_checksum_prevents_install(self):
        (self.assets/'xmd.vsix.sha256').write_text('invalid\n')
        result=self.run_install('vscode');self.assertNotEqual(result.returncode,0)
        self.assertFalse((self.root/'code-args').exists())
        self.assertFalse((self.bin/'xmd').exists())

    def bootstrap_asset(self, content=None):
        name=f'xmd-installer-{os.uname().sysname}-{os.uname().machine}'
        if content is None:
            shutil.copy2(INSTALLER,self.assets/name)
        else:
            (self.assets/name).write_bytes(content)
        (self.assets/(name+'.sha256')).write_text(hashlib.sha256((self.assets/name).read_bytes()).hexdigest()+'  '+name+'\n')
        return name

    def test_shell_downloads_and_runs_native_installer(self):
        self.bootstrap_asset()
        result=subprocess.run(['sh',str(BOOTSTRAP),'--editor','vscode','--github-auth'],env=self.env,capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)
        settings=json.loads((Path(self.env['XMD_VSCODE_USER_DATA_DIR'])/'User/settings.json').read_text())
        self.assertEqual(settings['xmd.serverPath'],str((self.bin/'xmd').resolve()))
        self.assertTrue((self.root/'code-args').exists())

    def test_shell_rejects_installer_checksum_mismatch(self):
        name=self.bootstrap_asset()
        (self.assets/(name+'.sha256')).write_text('invalid\n')
        result=subprocess.run(['sh',str(BOOTSTRAP),'--editor','zed','--github-auth'],env=self.env,capture_output=True,text=True)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('installer checksum mismatch',result.stderr)
        self.assertFalse((self.bin/'xmd').exists())

    def run_bootstrap(self, *args):
        return subprocess.run(['sh', str(BOOTSTRAP), '--github-auth', *args], env=self.env, capture_output=True, text=True)

    def test_shell_reuses_and_repairs_cached_installer(self):
        name=self.bootstrap_asset()
        for _ in range(2):
            result=self.run_bootstrap()
            self.assertEqual(result.returncode,0,result.stderr)
        downloads=(self.root/'downloads').read_text().splitlines()
        self.assertEqual(downloads.count(name),1)
        self.assertEqual(downloads.count(name+'.sha256'),2)
        cached=next(Path(self.env['XMD_INSTALLER_CACHE_DIR']).glob('v*/*/*'))
        cached.write_bytes(b'corrupted')
        result=self.run_bootstrap()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(cached.read_bytes(),(self.assets/name).read_bytes())
        self.assertEqual((self.root/'downloads').read_text().splitlines().count(name),2)

    def test_shell_separates_release_versions(self):
        name=self.bootstrap_asset(b'#!/bin/sh\necho "$XMD_VERSION"\n')
        for version in ['0.2.0','0.3.0','0.2.0']:
            result=self.run_bootstrap('--release',version)
            self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual((self.root/'downloads').read_text().splitlines().count(name),2)

    def test_shell_concurrent_cache_population(self):
        self.bootstrap_asset(b'#!/bin/sh\necho ready\n')
        processes=[subprocess.Popen(['sh',str(BOOTSTRAP),'--github-auth'],env=self.env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(3)]
        for process in processes:
            stdout,stderr=process.communicate(timeout=30)
            self.assertEqual(process.returncode,0,stderr)
            self.assertEqual(stdout.strip(),'ready')
        self.assertEqual(len(list(Path(self.env['XMD_INSTALLER_CACHE_DIR']).glob('v*/*/*'))),1)

    def test_shell_rejects_corrupt_download_without_caching_it(self):
        name=self.bootstrap_asset()
        (self.assets/name).write_bytes(b'corrupted')
        result=self.run_bootstrap()
        self.assertNotEqual(result.returncode,0)
        self.assertIn('installer checksum mismatch',result.stderr)
        self.assertEqual(list(Path(self.env['XMD_INSTALLER_CACHE_DIR']).glob('v*/*/*')),[])

    def test_shell_refreshes_replaced_release_asset(self):
        name=self.bootstrap_asset()
        result=self.run_bootstrap()
        self.assertEqual(result.returncode,0,result.stderr)
        replacement=b'#!/bin/sh\necho replacement-installer\n'
        (self.assets/name).write_bytes(replacement)
        (self.assets/(name+'.sha256')).write_text(hashlib.sha256(replacement).hexdigest()+'  '+name+'\n')
        result=self.run_bootstrap()
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertIn('replacement-installer',result.stdout)
        self.assertEqual((self.root/'downloads').read_text().splitlines().count(name),2)

    def test_public_shell_download_path(self):
        self.bootstrap_asset()
        curl=self.root/'tools/curl'
        curl.write_text('#!/bin/sh\nfor arg in "$@"; do\n if [ "$arg" = \'%{url_effective}\' ]; then echo https://github.com/drbh/xmd/releases/tag/v0.2.0; exit; fi\ndone\nwhile [ "$#" -gt 0 ]; do\n case "$1" in\n -o) destination=$2; shift 2;;\n https://*) url=$1; shift;;\n *) shift;;\n esac\ndone\ncp "$FIXTURE_ASSETS/${url##*/}" "$destination"\n')
        curl.chmod(0o755)
        result=subprocess.run(['sh','-s','--','--editor','zed'],input=BOOTSTRAP.read_text(),env=self.env,capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)
        settings=json.loads((self.config/'settings.json').read_text())
        self.assertEqual(settings['languages']['XMD']['semantic_tokens'],'full')

if __name__ == '__main__': unittest.main()
