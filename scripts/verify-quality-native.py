#!/usr/bin/env python3
"""Build and run isolated WebKit cancellation receipts against two source revisions."""
from __future__ import annotations

import argparse
from contextlib import ExitStack
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import wave

ROOT = Path(__file__).resolve().parents[1]

PROBE = r'''
import { invoke } from '@tauri-apps/api/core'
import { tauriDesktopApi as api } from 'API_PATH'
import { startStatusPerf } from 'PERF_PATH'
const checks = []
const timingsMs = {}
function check(name, passed) {
  checks.push({ name, passed })
  if (!passed) throw new Error(name)
}
async function until(predicate) {
  const deadline = performance.now() + 15000
  while (performance.now() < deadline) {
    const status = await api.getAppStatus()
    if (predicate(status)) return status
    await new Promise(resolve => setTimeout(resolve, 10))
  }
  throw new Error('native observation deadline exceeded')
}
async function restartCapture() {
  const deadline = performance.now() + 5000
  let retries = 0
  while (performance.now() < deadline) {
    try {
      const receipt = await api.startCapture()
      check(JSON.stringify({ restartBusyRetries: retries }), true)
      return receipt
    } catch (reason) {
      if (String(reason) !== 'Another recording is already active.') throw reason
      retries++
      await new Promise(resolve => setTimeout(resolve, 10))
    }
  }
  throw new Error('recording lease did not become available after cancellation')
}
async function verify() {
  const scenario = 'SCENARIO'
  if (scenario === 'status') {
    const status = await api.getAppStatus()
    const snapshot = Object.fromEntries(['phase', 'lastError', 'lastTranscript', 'recordingSessionId', 'recordingRevision', 'captureStopRequested', 'recordingLimitSeconds'].map(key => [key, status[key]]))
    check(JSON.stringify(snapshot), true)
  } else if (scenario === 'capture' || scenario === 'disconnect' || scenario === 'missing-device') {
    if (scenario === 'disconnect') {
      const microphones = await api.getMicrophones()
      const input = microphones.devices.find(device => device.label.includes('EchoProbe'))
      check('controlled PipeWire source is enumerated ' + JSON.stringify(microphones), input !== undefined)
      await api.setMicrophone(input.id)
    }
    const start = performance.now()
    try { await api.startCapture() } catch { }
    const failed = await until(status => status.phase === 'Failed')
    timingsMs.captureFailure = performance.now() - start
    check(JSON.stringify({ phase: failed.phase, error: failed.lastError }), true)
  } else {
    for (let warmup = 0; warmup < 5; warmup++) await api.getAppStatus()
    const started = await api.startCapture()
    check('start names a recording session', started.sessionId !== null && started.phase === 'Recording')
    await new Promise(resolve => setTimeout(resolve, 150))
    await api.stopCapture(started.sessionId)
    await until(status => status.phase === 'Transcribing')
    const cancelAt = performance.now()
    const receipt = await api.cancelTranscription(started.sessionId)
    timingsMs.cancelAcknowledgement = performance.now() - cancelAt
    check('cancellation acknowledgement names the requested session', receipt.sessionId === started.sessionId)
    const terminal = await until(status => status.phase === 'Failed')
    check('acknowledged cancellation reaches canceled terminal detail', /cancel/i.test(terminal.lastError ?? ''))
    check(JSON.stringify({ phase: terminal.phase, error: terminal.lastError }), true)
    check('canceled run adds no History row', (await api.getHistory()).length === 0)
    const replacement = await restartCapture()
    check('another recording starts with a new identity', replacement.phase === 'Recording' && replacement.sessionId !== started.sessionId)
    let rejected = false
    try { await api.cancelTranscription(started.sessionId) } catch { rejected = true }
    const afterStale = await api.getAppStatus()
    check('stale cancellation rejects without altering replacement', rejected && afterStale.recordingSessionId === replacement.sessionId && afterStale.phase === 'Recording')
    await new Promise(resolve => setTimeout(resolve, 150))
    await api.stopCapture(replacement.sessionId)
    await until(status => status.phase === 'Transcribing')
    await api.cancelTranscription(replacement.sessionId)
    await until(status => status.phase === 'Failed')
  }
  startStatusPerf({ checks, timingsMs, settingsRevisions: [] })
}
verify().catch(reason => invoke('perf_report_failed', { message: String(reason) }))
'''


def sha256(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def source_fingerprint(root):
    fingerprint = hashlib.sha256()
    for relative in sorted(subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard'], cwd=root, text=True).splitlines()):
        path = root / relative
        if path.is_file() and (relative.startswith(('crates/', 'src-tauri/', 'frontend/src/')) or relative in ('Cargo.toml', 'Cargo.lock')):
            fingerprint.update(relative.encode())
            fingerprint.update(path.read_bytes())
    return fingerprint.hexdigest()


def replace_once(source, marker, replacement):
    if source.count(marker) != 1:
        raise RuntimeError(f'native probe rewrite marker must occur once: {marker!r}')
    return source.replace(marker, replacement, 1)


def build(root, output, scenario, target, ui=False):
    version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
    fingerprint = source_fingerprint(root)
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    with tempfile.TemporaryDirectory(prefix='echo-quality-probe-') as temporary:
        probe = Path(temporary)
        (probe / 'index.html').write_text('<div id="root"></div><script type="module" src="/main.ts"></script>')
        source = PROBE.replace('API_PATH', str(root / 'frontend/src/api/tauriDesktopApi.ts')).replace('PERF_PATH', str(root / 'frontend/src/perf/statusPerf.ts')).replace('SCENARIO', scenario)
        if scenario == 'disconnect':
            source = replace_once(source, "const failed = await until(status => status.phase === 'Failed')", "let heard = false\n    for (let attempt = 0; attempt < 20 && !heard; attempt++) {\n      heard = (await api.getRecordingLevel()) > 0\n      if (!heard) await new Promise(resolve => setTimeout(resolve, 10))\n    }\n    check('controlled source delivers audio before disconnection', heard)\n    const failed = await until(status => status.phase === 'Failed')")
        if scenario == 'recovery':
            source = replace_once(source, "scenario === 'missing-device'", "scenario === 'missing-device' || scenario === 'recovery'")
            source = replace_once(source, "check(JSON.stringify({ phase: failed.phase, error: failed.lastError }), true)", """check(JSON.stringify({ phase: failed.phase, error: failed.lastError }), true)
    let input
    const deadline = performance.now() + 10000
    while (!input && performance.now() < deadline) {
      input = (await api.getMicrophones()).devices.find(device => device.label.includes('EchoProbe'))
      if (!input) await new Promise(resolve => setTimeout(resolve, 100))
    }
    check('newly connected controlled source is enumerated', input !== undefined)
    await api.setMicrophone(input.id)
    const retry = [...document.querySelectorAll('button')].find(button => button.textContent.trim() === 'Try recording again')
    check('native Home offers retry after missing device', retry !== undefined)
    retry.click()
    const recording = await until(status => status.phase === 'Recording')
    await new Promise(resolve => setTimeout(resolve, 1000))
    await api.stopCapture(recording.recordingSessionId)
    const recovered = await until(status => status.phase === 'Idle' || status.phase === 'Failed')
    check('native retry reaches Idle ' + JSON.stringify({ error: recovered.lastError }), recovered.phase === 'Idle')
    let copyButton
    const copyDeadline = performance.now() + 5000
    while (!copyButton && performance.now() < copyDeadline) {
      copyButton = document.querySelector('button[aria-label="Copy transcript"]')
      if (!copyButton) await new Promise(resolve => setTimeout(resolve, 20))
    }
    check('native Home offers the recovered transcript for copying', copyButton !== null && copyButton !== undefined && recovered.lastTranscript !== null)
    copyButton.scrollIntoView({ block: 'center' })
    copyButton.click()
    while (!document.querySelector('button[aria-label="Copied transcript"]') && performance.now() < copyDeadline) {
      await new Promise(resolve => setTimeout(resolve, 20))
    }
    check('native Home confirms transcript copied', document.querySelector('button[aria-label="Copied transcript"]') !== null)
    await new Promise(resolve => setTimeout(resolve, 3500))

""")
        if ui:
            source = ("import { createElement } from 'react'\nimport { createRoot } from 'react-dom/client'\n"
                + f"import App from {json.dumps(str(root / 'frontend/src/App.tsx'))}\n"
                + f"import {{ configureDesktopApi }} from {json.dumps(str(root / 'frontend/src/tauri.ts'))}\n"
                + f"import {json.dumps(str(root / 'frontend/src/styles/index.css'))}\n"
                + replace_once(source, "const checks = []", "configureDesktopApi(api)\nconst appRoot = createRoot(document.getElementById('root'))\nappRoot.render(createElement(App))\nconst checks = []")
            )
            source = replace_once(source, "  startStatusPerf({ checks, timingsMs, settingsRevisions: [] })", "  await new Promise(resolve => setTimeout(resolve, 1500))\n  check('native window shows terminal outcome', document.body.innerText.includes('Recording did not finish') || document.body.innerText.includes('Transcription canceled'))\n  appRoot.unmount()\n  await new Promise(resolve => setTimeout(resolve, 100))\n  startStatusPerf({ checks, timingsMs, settingsRevisions: [] })")
            if scenario == 'recovery':
                source = replace_once(source, "document.body.innerText.includes('Recording did not finish') || document.body.innerText.includes('Transcription canceled')", "document.body.innerText.includes('Ready when you are')")
            if scenario == 'cancel':
                source = replace_once(source, "document.body.innerText.includes('Recording did not finish') || document.body.innerText.includes('Transcription canceled')", "document.body.innerText.includes('Transcription canceled')")
                source = replace_once(source, 'const cancelAt = performance.now()', 'let cancelAt = 0')
                source = replace_once(source, "const receipt = await api.cancelTranscription(started.sessionId)", "let button\n    const deadline = performance.now() + 5000\n    while (!button && performance.now() < deadline) {\n      button = [...document.querySelectorAll('button')].find(node => node.textContent.trim() === 'Cancel transcription')\n      if (!button) await new Promise(resolve => setTimeout(resolve, 20))\n    }\n    check('native Home exposes cancellation', button !== undefined)\n    await new Promise(resolve => setTimeout(resolve, 1200))\n    cancelAt = performance.now()\n    button.click()\n    await until(status => status.phase === 'Failed')\n    const receipt = { sessionId: (await api.getAppStatus()).recordingSessionId }")
                source = replace_once(source, "'cancellation acknowledgement names the requested session'", "'Home cancellation targets the active session'")
                source = replace_once(source, 'timingsMs.cancelAcknowledgement', 'timingsMs.homeCancelTerminalObservation')
                source = replace_once(source, 'const replacement = await restartCapture()', """let retry
    const retryDeadline = performance.now() + 5000
    while (!retry && performance.now() < retryDeadline) {
      retry = [...document.querySelectorAll('button')].find(node => node.textContent.trim() === 'Try recording again' && !node.disabled)
      if (!retry) await new Promise(resolve => setTimeout(resolve, 20))
    }
    check('native Home offers retry after cancellation', retry !== undefined)
    retry.click()
    const nextRecording = await until(status => status.phase === 'Recording')
    const replacement = { phase: nextRecording.phase, sessionId: nextRecording.recordingSessionId }""")
        (probe / 'main.ts').write_text(source)
        (probe / 'node_modules').symlink_to(root / 'frontend/node_modules', target_is_directory=True)
        (probe / 'vite.config.mjs').write_text('export default ' + json.dumps({'root': str(probe), 'define': {'__APP_VERSION__': json.dumps(version)}, 'build': {'outDir': str(probe / 'dist')}}))
        subprocess.run([str(root / 'frontend/node_modules/.bin/vite'), 'build', '--config', str(probe / 'vite.config.mjs')], cwd=root, check=True)
        environment = {**os.environ, 'CARGO_TARGET_DIR': str(target), 'ECHO_BUILD_SHA': commit, 'TAURI_CONFIG': json.dumps({'build': {'frontendDist': str(probe / 'dist')}})}
        subprocess.run(['cargo', 'build', '-p', 'echo-desktop', '--features', 'status-perf-probe'], cwd=root, env=environment, check=True)
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(target / 'debug/echo-desktop', output)
    if source_fingerprint(root) != fingerprint:
        raise RuntimeError('application source changed during native probe build')
    metadata = {'commit': commit, 'sourceFingerprint': fingerprint, 'probeSha256': hashlib.sha256(source.replace(str(root), '<source-root>').encode()).hexdigest(), 'binarySha256': sha256(output), 'scenario': scenario, 'ui': ui, 'sourceStatus': subprocess.check_output(['git', 'status', '--porcelain'], cwd=root, text=True)}
    output.with_suffix('.json').write_text(json.dumps(metadata, indent=2) + '\n')
    return metadata


def stop_process(process):
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def run_binary(binary, artifact, scenario, status=None, lease=None, expected_error=None):
    metadata = json.loads(binary.with_suffix('.json').read_text())
    if metadata['scenario'] != scenario or metadata['binarySha256'] != sha256(binary):
        raise RuntimeError('native binary identity or scenario does not match')
    artifact.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='echo-quality-runtime-') as temporary, ExitStack() as cleanup:
        runtime = Path(temporary)
        for name in ('data', 'config', 'models', 'bin', 'xdg-data', 'xdg-config', 'xdg-cache'):
            (runtime / name).mkdir()
        audio = runtime / 'fixture.wav'
        with wave.open(str(audio), 'wb') as wav:
            wav.setnchannels(1)
            wav.setsampwidth(2)
            wav.setframerate(16000)
            wav.writeframes(b'\x40\x1f' * 48000)
        (runtime / 'models/ggml-small.bin').write_bytes(b'')
        engine = runtime / 'bin/whisper-cli'
        engine.write_text('#!/bin/sh\nwhile :; do /bin/sleep 0.1; done\n')
        engine.chmod(0o755)
        if status is not None:
            (runtime / 'data/status').write_text(status)
        if lease is not None:
            (runtime / 'data/recording.lock').write_text(lease)
        environment = {**os.environ, 'PATH': str(runtime / 'bin') + ':' + os.environ['PATH'], 'ECHO_DATA_DIR': str(runtime / 'data'), 'ECHO_CONFIG_DIR': str(runtime / 'config'), 'ECHO_MODEL_DIR': str(runtime / 'models'), 'ECHO_AUDIO_FIXTURE': str(audio if scenario != 'capture' else runtime / 'missing.wav'), 'ECHO_ENGINE': 'whisper', 'ECHO_WHISPER_MODEL': 'small', 'ECHO_SKIP_INJECT': '1', 'ECHO_HUD': '0', 'XDG_DATA_HOME': str(runtime / 'xdg-data'), 'XDG_CONFIG_HOME': str(runtime / 'xdg-config'), 'XDG_CACHE_HOME': str(runtime / 'xdg-cache'), 'GDK_BACKEND': 'x11', 'XDG_SESSION_TYPE': 'x11'}
        bus_config = runtime / 'dbus.conf'
        bus_config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><policy context="default"><allow send_destination="*"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>')
        audio_servers = []
        if scenario in ('disconnect', 'missing-device', 'recovery'):
            environment.pop('ECHO_AUDIO_FIXTURE', None)
            if scenario == 'recovery':
                environment['ECHO_ENGINE'] = 'fake'
            if scenario in ('recovery', 'disconnect'):
                spa_config = runtime / 'xdg-config/pipewire/pipewire.conf.d'
                spa_config.mkdir(parents=True)
                (spa_config / 'quality.conf').write_text('context.spa-libs = { audiotestsrc = audiotestsrc/libspa-audiotestsrc }\n')
            environment.update({'XDG_RUNTIME_DIR': str(runtime), 'PIPEWIRE_RUNTIME_DIR': str(runtime), 'PULSE_SERVER': 'unix:' + str(runtime / 'pulse/native')})
            for server in ('pipewire', 'pipewire-pulse'):
                log = cleanup.enter_context((artifact / f'{server}.log').open('w'))
                server_process = subprocess.Popen([server], env=environment, stdout=log, stderr=log)
                audio_servers.append(server_process)
                cleanup.callback(stop_process, server_process)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if subprocess.run(['pactl', 'info'], env=environment, capture_output=True).returncode == 0:
                    break
                time.sleep(.05)
            if scenario in ('recovery', 'disconnect'):
                log = cleanup.enter_context((artifact / 'wireplumber.log').open('w'))
                policy = subprocess.Popen(['wireplumber', '-c', 'policy.conf'], env={**environment, 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=' + str(runtime / 'no-session-bus')}, stdout=log, stderr=log)
                cleanup.callback(stop_process, policy)
            if scenario == 'disconnect':
                subprocess.run(['pw-cli', 'create-node', 'adapter', '{ factory.name=audiotestsrc node.name=echo_probe node.description=EchoProbe media.class=Audio/Source audio.position=[MONO] audio.rate=48000 node.driver=true object.linger=true adapter.auto-port-config={ mode=dsp position=preserve } }'], env=environment, check=True, capture_output=True)
            def recover():
                status_path = runtime / 'data/status'
                deadline = time.monotonic() + 30
                while time.monotonic() < deadline:
                    if status_path.exists() and 'state=Failed' in status_path.read_text():
                        shutil.copy2(status_path, artifact / 'initial-status')
                        time.sleep(1)
                        completed = subprocess.run(['pw-cli', 'create-node', 'adapter', '{ factory.name=audiotestsrc node.name=echo_probe node.description=EchoProbe media.class=Audio/Source audio.position=[MONO] audio.rate=48000 node.driver=true object.linger=true adapter.auto-port-config={ mode=dsp position=preserve } }'], env=environment, capture_output=True, text=True)
                        (artifact / 'source-added.json').write_text(json.dumps({'exitCode': completed.returncode, 'stderr': completed.stderr}))
                        return
                    time.sleep(.01)
            if scenario == 'recovery':
                threading.Thread(target=recover, daemon=True).start()
            def disconnect():
                deadline = time.monotonic() + 30
                status_path = runtime / 'data/status'
                while time.monotonic() < deadline:
                    if status_path.exists() and 'state=Recording\n' in status_path.read_text():
                        time.sleep(.5)
                        if 'state=Recording\n' not in status_path.read_text():
                            return
                        audio_servers[0].terminate()
                        (artifact / 'disconnect.json').write_text(json.dumps({'trigger': 'Recording observed, then 500 ms', 'device': 'private PipeWire EchoProbe Audio/Source (virtual adapter)', 'action': 'SIGTERM private pipewire server'}))
                        return
                    time.sleep(.01)
            if scenario == 'disconnect':
                threading.Thread(target=disconnect, daemon=True).start()
        metadata_path = binary.with_suffix('.json')
        metadata = json.loads(metadata_path.read_text()) if metadata_path.exists() else {}
        application = [sys.executable, str(Path(__file__).resolve()), '--capture-child', str(binary), str(artifact.resolve())] if metadata.get('ui') else [str(binary)]
        command = ['dbus-run-session', '--config-file', str(bus_config), '--', 'xvfb-run', '-a', *application]
        process = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=90)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
        (artifact / 'stdout.log').write_text(stdout)
        (artifact / 'stderr.log').write_text(stderr)
        for name in ('status', 'history.json'):
            path = runtime / 'data' / name
            if path.exists():
                shutil.copy2(path, artifact / name)
        payloads = [json.loads(line.removeprefix('STATUS_PERF_JSON ')) for line in stdout.splitlines() if line.startswith('STATUS_PERF_JSON ')]
        if process.returncode != 0 or len(payloads) != 1:
            raise RuntimeError(f'native probe failed with exit {process.returncode}; see {artifact}')
        result = payloads[0]
        if expected_error is not None:
            snapshot = next((json.loads(check['name']) for check in result['report']['verification']['checks'] if check['name'].startswith('{"phase"')), {})
            observed_error = snapshot.get('error')
            persisted_error = next((line.removeprefix('error=') for line in (artifact / ('initial-status' if scenario == 'recovery' else 'status')).read_text().splitlines() if line.startswith('error=')), None)
            if not observed_error or expected_error not in observed_error or persisted_error != observed_error:
                raise RuntimeError(f'expected retained native error containing {expected_error!r}; observed {observed_error!r}, persisted {persisted_error!r}')
        if scenario == 'recovery':
            expected_clipboard = next(line.removeprefix('last=') for line in (artifact / 'status').read_text().splitlines() if line.startswith('last='))
            if not (artifact / 'clipboard.txt').exists() or (artifact / 'clipboard.txt').read_text() != expected_clipboard:
                raise RuntimeError('native clipboard does not match the recovered transcript')
        if scenario == 'disconnect' and not (artifact / 'disconnect.json').exists():
            raise RuntimeError('capture failed before the controlled server disconnect')
        result['report'].pop('userAgent', None)
        result['binarySha256'] = sha256(binary)
        metadata_path = binary.with_suffix('.json')
        metadata = json.loads(metadata_path.read_text()) if metadata_path.exists() else {}
        result['build'] = metadata
        result['limitations'] = [
            'Home controls are driven through DOM for UI cancellation and recovery scenarios.',
            'Audio is controlled. Disconnect and recovery use private PipeWire; missing-device uses a private empty server.',
            'Synthetic PCM is used for cancellation. Engines are controlled stand-ins; insertion is disabled.',
        ]
        (artifact / 'receipt.json').write_text(json.dumps(result, indent=2) + '\n')
        return result


def percentile(values):
    ordered = sorted(values)
    rank = (len(ordered) - 1) * .95
    lower = int(rank)
    return ordered[lower] + (ordered[min(lower + 1, len(ordered) - 1)] - ordered[lower]) * (rank - lower)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--build', type=Path)
    parser.add_argument('--ui', action='store_true')
    parser.add_argument('--target', type=Path, default=ROOT / 'target/recording-native-probe')
    parser.add_argument('--scenario', choices=['cancel', 'capture', 'status', 'disconnect', 'missing-device', 'recovery'], default='cancel')
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--head', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=30)
    parser.add_argument('--expect-error', help='require this substring in matching native and persisted capture error')
    parser.add_argument('--status-file', type=Path)
    parser.add_argument('--status-dir', type=Path)
    args = parser.parse_args()
    if args.scenario == 'recovery' and args.build and not args.ui:
        parser.error('recovery requires --ui')
    if args.build:
        print(json.dumps(build(args.root.resolve(), args.build.resolve(), args.scenario, args.target.resolve(), args.ui)))
        return
    if args.samples <= 0:
        parser.error('--samples must be positive')
    if args.head is None:
        parser.error('--head or --build is required')
    if args.status_dir:
        if args.scenario != 'status' or args.baseline is None:
            parser.error('--status-dir requires --scenario status and both binaries')
        fixtures = {path.stem: path.read_text() for path in sorted(args.status_dir.glob('*.status'))}
        ticks = Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()[19]
        for phase in ('Recording', 'Transcribing', 'Injecting', 'FuturePhase'):
            live = f'state={phase}\npid={os.getpid()}\npid_start_ticks={ticks}\n'
            fixtures['live-legacy-' + phase] = live
            fixtures['live-scoped-' + phase] = live + 'session_id=live-fixture\nsession_revision=13\n'
        comparisons = []
        for name, raw in fixtures.items():
            snapshots = {}
            for label, binary in [('baseline', args.baseline), ('head', args.head)]:
                artifact = args.output / name / label
                lease = f'{os.getpid()}\nlive-fixture\n{ticks}\nscoped-intents-v1' if name.startswith('live-scoped-') else None
                receipt = run_binary(binary.resolve(), artifact, 'status', raw, lease)
                if lease:
                    (artifact / 'input.recording.lock').write_text(lease)
                (artifact / 'input.status').write_text(raw)
                snapshots[label] = json.loads(receipt['report']['verification']['checks'][0]['name'])
            expected = name.rsplit('-', 1)[-1] if name.startswith('live-') else None
            if expected == 'FuturePhase':
                expected = 'Failed'
            valid_phase = expected is None or all(snapshot['phase'] == expected for snapshot in snapshots.values())
            comparisons.append({'fixture': name, 'snapshots': snapshots, 'expectedPhase': expected, 'equivalent': snapshots['baseline'] == snapshots['head'] and valid_phase})
            print(json.dumps(comparisons[-1]), flush=True)
        (args.output / 'compatibility.json').write_text(json.dumps(comparisons, indent=2) + '\n')
        if not comparisons or not all(item['equivalent'] for item in comparisons):
            raise RuntimeError('native status snapshots differ')
        return
    if args.baseline and args.scenario == 'cancel':
        baseline_build = json.loads(args.baseline.with_suffix('.json').read_text())
        head_build = json.loads(args.head.with_suffix('.json').read_text())
        if baseline_build.get('ui') or head_build.get('ui') or baseline_build.get('probeSha256') != head_build.get('probeSha256'):
            parser.error('latency comparison requires the same non-UI probe in both binaries')
    if args.baseline and args.scenario != 'cancel':
        parser.error('latency comparison requires --scenario cancel; use --status-dir for fixtures')
    records = {'baseline': [], 'head': []}
    pairs = [('baseline', args.baseline), ('head', args.head)] if args.baseline else [('head', args.head)]
    count = args.samples + 5 if args.baseline else 1
    for iteration in range(count):
        for label, binary in (pairs if iteration % 2 == 0 else list(reversed(pairs))):
            artifact = args.output / f'{iteration:02}-{label}'
            receipt = run_binary(binary.resolve(), artifact, args.scenario, args.status_file.read_text() if args.status_file else None, expected_error=args.expect_error)
            if iteration >= 5 or not args.baseline:
                records[label].append(receipt)
            print(json.dumps({'iteration': iteration, 'label': label, 'passed': True}), flush=True)
    summary = {'samples': {label: len(receipts) for label, receipts in records.items()}, 'warmups': 5 if args.baseline else 0, 'interleaved': bool(args.baseline), 'metrics': {}, 'performanceComparison': bool(args.baseline)}
    if args.baseline:
        for metric in ('cancelAcknowledgement', 'current-status'):
            values = {}
            for label, receipts in records.items():
                values[label] = [r['report']['verification']['timingsMs'][metric] if metric == 'cancelAcknowledgement' else next(lane for lane in r['report']['lanes'] if lane['name'] == metric)['samplesMs'][0] for r in receipts]
            before, after = percentile(values['baseline']), percentile(values['head'])
            summary['metrics'][metric] = {'rawSamplesMs': values, 'baselineP95Ms': before, 'headP95Ms': after, 'passed': after <= before + max(before * .2, 10)}
        summary['statusMetricDefinition'] = 'One warm current-status read per retained launch; each receipt also contains a 40-read diagnostic lane.'
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    if args.baseline and not all(metric['passed'] for metric in summary['metrics'].values()):
        raise RuntimeError('native latency exceeded its acceptance budget')


def capture_child(binary, artifact):
    process = subprocess.Popen([binary])
    last = None
    while process.poll() is None:
        status_path = Path(os.environ['ECHO_DATA_DIR']) / 'status'
        if status_path.exists():
            state = status_path.read_text().splitlines()[0].split('=', 1)[-1].split()[0]
            if state != last:
                time.sleep(.7)
                subprocess.run(['import', '-window', 'root', str(Path(artifact) / f'{state}.png')], capture_output=True)
                last = state
            if state == 'Idle' and not (Path(artifact) / 'clipboard.txt').exists():
                expected = next((line.removeprefix('last=') for line in status_path.read_text().splitlines() if line.startswith('last=')), None)
                if expected:
                    try:
                        clipboard = subprocess.run(['xclip', '-selection', 'clipboard', '-o'], capture_output=True, text=True, timeout=1)
                    except subprocess.TimeoutExpired:
                        clipboard = None
                    if clipboard and clipboard.returncode == 0 and clipboard.stdout == expected:
                        (Path(artifact) / 'clipboard.txt').write_text(clipboard.stdout)
                        time.sleep(.1)
                        subprocess.run(['import', '-window', 'root', str(Path(artifact) / 'copied-transcript.png')], capture_output=True)
        time.sleep(.1)
    return process.returncode


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--capture-child':
        raise SystemExit(capture_child(sys.argv[2], sys.argv[3]))
    main()
