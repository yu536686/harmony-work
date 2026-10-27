import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';

const source = await readFile(new URL('../installer/light/update.ps1', import.meta.url), 'utf8');
const main = source.slice(source.indexOf('$changed=@()'));
const recovery = source.slice(source.indexOf('function RecoverTransactions('), source.indexOf('$changed=@()'));

test('source contract: snapshot after exit, before journaled app mutation and launch', () => {
  const exit = main.indexOf('WaitForProductExit $exe');
  const snapshot = main.indexOf('NewSettingsSnapshot $transaction');
  const pending = main.indexOf("WriteJournal 'pending'");
  const mutation = main.indexOf('foreach($f in $manifest.files){Log "Installing');
  assert.ok(exit >= 0 && snapshot > exit && pending > snapshot && mutation > pending);
  assert.ok(main.indexOf('$child=Start-Process') > mutation);
  assert.match(source, /settingsSnapshot=\$settingsSnapshot/);
  assert.match(source, /schemaVersion=2;installDirectory/);
  assert.ok(main.indexOf('RecoverTransactions $actual $key') < snapshot);
  assert.ok(main.indexOf('if($Check){exit 0}') < snapshot);
});

test('source contract: DPAPI CurrentUser, fixed destination, bounded bytes, private ACL', () => {
  assert.match(source, /ProtectedData\]::Protect/);
  assert.match(source, /ProtectedData\]::Unprotect/);
  assert.match(source, /DataProtectionScope\]::CurrentUser/);
  assert.doesNotMatch(source, /DataProtectionScope\]::LocalMachine/);
  assert.match(source, /Join-Path \$env:APPDATA 'com\.harmony\.xiaoheng-workbuddy\\config\.json'/);
  assert.match(source, /4MB/);
  assert.match(source, /SetAccessRuleProtection\(\$true,\$false\)/);
  assert.match(source, /WindowsIdentity\]::GetCurrent\(\)\.User/);
  assert.match(source, /GetFolderPath\('UserProfile'\)/);
  assert.match(source, /ReparsePoint/);
  assert.doesNotMatch(source, /Copy-Item[^\n]*(?:config\.json|\$config)/);
});

test('source contract: both rollback paths restore settings, failures block restart', () => {
  assert.match(recovery, /RestoreSettingsSnapshot \$dir\.FullName \$j\.settingsSnapshot/);
  assert.ok(recovery.indexOf('RestoreSettingsSnapshot') < recovery.indexOf('Remove-Item -LiteralPath $dir.FullName -Recurse'));
  const failure = main.slice(main.indexOf('} catch {\n $failure='));
  assert.ok(failure.indexOf('WaitForProductExit $originalExe') < failure.indexOf('RestoreSettingsSnapshot $transaction'));
  assert.ok(failure.indexOf('RestoreSettingsSnapshot $transaction') < failure.indexOf('$safeRestart='));
  assert.ok(failure.indexOf("WriteJournal 'rolledBack'") > failure.indexOf('RestoreSettingsSnapshot $transaction'));
  assert.ok(recovery.indexOf('-ValidateOnly') < recovery.indexOf('foreach($entry in $entries)'));
  assert.match(recovery, /if\(\$j.state -eq 'pending'\)/);
  assert.match(main, /RecoverTransactions \$actual \$key \} catch \{ \$recoveryFailed=\$true;throw/);
  assert.match(failure, /\$rollbackErrors\+=\$_\.Exception\.Message/);
  assert.match(failure, /\$safeRestart=\(!\$Check -and \$canRestart -and !\$rollbackErrors.Count -and !\$recoveryFailed\)/);
  assert.doesNotMatch(failure, /Remove-Item -LiteralPath \$transaction/);
  assert.ok(main.indexOf("WriteJournal 'committed'") < main.indexOf('Remove-Item -LiteralPath $transaction -Recurse'));
});

test('source contract: absent snapshot never deletes config, hash and metadata are validated', () => {
  const restore = source.slice(source.indexOf('function RestoreSettingsSnapshot('), source.indexOf('function WriteJournal('));
  assert.match(restore, /if\(!\$metadata\.existed\)\{return\}/);
  assert.match(restore, /SettingsHash \$protected/);
  assert.match(restore, /Invalid settings snapshot metadata/);
  assert.match(restore, /\[IO\.File\]::Replace/);
  assert.doesNotMatch(restore, /Remove-Item -LiteralPath \$config/);
  assert.doesNotMatch(restore, /\$metadata\.(?:configPath|destination)/);
});

test('source contract: atomic config overwrite passes CLR null explicitly for Windows PowerShell 5.1', () => {
  const restore = source.slice(source.indexOf('function RestoreSettingsSnapshot('), source.indexOf('function WriteJournal('));
  assert.match(restore, /\[IO\.File\]::Replace\(\$temp,\$config,\[NullString\]::Value\)/);
  assert.doesNotMatch(restore, /\[IO\.File\]::Replace\(\$temp,\$config,\$null\)/);
  assert.doesNotMatch(restore, /Remove-Item -LiteralPath \$config|Copy-Item/);
});

test('Windows behavior harness uses actual extracted updater functions, not a DPAPI mock', async () => {
  const script = await readFile(new URL('./test-light-settings.ps1', import.meta.url), 'utf8');
  for (const contract of ['ScriptBlock]::Create', 'NewSettingsSnapshot', 'RestoreSettingsSnapshot', 'RecoverTransactions', 'APPDATA', 'wrong hash', 'plaintext', 'absent']) {
    assert.ok(script.includes(contract), contract);
  }
  assert.match(script, /PlatformID\]::Win32NT/);
});
