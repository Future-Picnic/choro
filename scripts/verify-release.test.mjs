import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const commands = [
  'check --locked -p ide-app --all-targets',
  'check --locked -p ide-app --all-targets --features ui-layout-tests',
  'test --locked -p ide-core',
  'test --locked -p ide-app',
  'test --locked -p ide-mcp',
  'test --locked -p ide-app --features ui-layout-tests',
  'build --locked --release --workspace',
];

// Load a Cargo stub instead of creating executables or invoking Cargo.
// --check execs Bash before the release's Git/GitHub/Apple prerequisite checks.
function runCheck(args = [], failAt = '') {
  return spawnSync('zsh', ['scripts/release-macos.sh', '--check', ...args], {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CHORO_TEST_FAIL_AT: failAt,
      BASH_ENV: fileURLToPath(new URL('./tests/mock-cargo.bash', import.meta.url)),
    },
  });
}

function cargoCommands(result) {
  return result.stdout.split('\n')
    .filter(line => line.startsWith('MOCK_CARGO '))
    .map(line => line.slice('MOCK_CARGO '.length));
}

test('--check runs default tests, UI tests and the production build without release prerequisites', () => {
  const result = runCheck();
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(cargoCommands(result), commands);
  assert.match(result.stdout, /Release code verification passed/);
  assert.doesNotMatch(result.stdout, /Choro release plan|Published Choro/);
});

test('a failing compile or test stops subsequent checks and preserves its exit code', () => {
  for (const index of [0, 1, 3, 5]) {
    const result = runCheck([], commands[index]);
    assert.equal(result.status, 42, result.stderr);
    assert.deepEqual(cargoCommands(result), commands.slice(0, index + 1));
    assert.match(result.stderr, /Release verification failed during:/);
    assert.doesNotMatch(result.stdout, /Release code verification passed/);
  }
});

test('a failed production build does not report success', () => {
  const result = runCheck([], commands.at(-1));
  assert.equal(result.status, 42, result.stderr);
  assert.match(result.stderr, /Release verification failed during: Build production workspace/);
  assert.doesNotMatch(result.stdout, /Release code verification passed/);
});

test('--check rejects release-specific options before invoking Cargo', () => {
  for (const args of [['--dry-run'], ['--version', '0.97'], ['--resume', 'v0.97'], ['--notes-file', 'notes.txt']]) {
    const result = runCheck(args);
    assert.equal(result.status, 2, result.stderr);
    assert.deepEqual(cargoCommands(result), []);
    assert.match(result.stderr, /Use --check on its own/);
  }
});
