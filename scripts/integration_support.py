"""Shared HTTP and process helpers for disposable integration stacks."""
import datetime
import json
import pathlib
import re
import socket
import subprocess
import time
import urllib.request
import urllib.error
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = ROOT / 'target/debug/wakeit'
GATEWAY = ROOT / 'target/debug/wakeit-gateway'
MOCK = ROOT / 'target/debug/examples/mock_agent'
PROBE = ROOT / 'target/debug/examples/bus_probe'

def timestamp(value):
    value = re.sub(r'\.(\d+)(?=[+-]|Z|$)', lambda m: '.' + m.group(1).ljust(6, '0')[:6], value)
    return datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))

class APIError(Exception):
    def __init__(self, status, body): self.status, self.body = status, body; super().__init__(f'{status}: {body}')

def port():
    with socket.socket() as sock: sock.bind(('127.0.0.1', 0)); return sock.getsockname()[1]

def call(base, path, method='GET', body=None, token=None, key=None, headers=None):
    headers = dict(headers) if headers is not None else {'X-Wakeit-Request': '1'}
    if token: headers['Authorization'] = f'Bearer {token}'
    if method == 'POST': headers['Idempotency-Key'] = key or str(uuid.uuid4())
    data = json.dumps(body).encode() if body is not None else None
    if data: headers['Content-Type'] = 'application/json'
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as response: return json.load(response)
    except urllib.error.HTTPError as error:
        raise APIError(error.code, json.load(error)) from None

def wait(fn, description, timeout=25):
    deadline = time.monotonic() + timeout
    error = None
    while time.monotonic() < deadline:
        try:
            value = fn()
            if value: return value
        except (APIError, OSError) as e: error = str(e)
        time.sleep(.2)
    raise AssertionError(f'timed out: {description}; {error or "condition not reached"}')

def expect_status(fn, status):
    try: fn()
    except APIError as error: assert error.status == status, error; return
    raise AssertionError(f'expected HTTP {status}')

def command(args, env=None):
    result=subprocess.run([str(a) for a in args], cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode:
        raise RuntimeError(f'command failed: {args[0]}\n{result.stdout[-4000:]}\n{result.stderr[-4000:]}')
    return result.stdout

class WorkspaceAPI:
    """Explicit workspace paths; global requests continue to use call()."""
    def __init__(self, workspace):
        self.workspace = workspace

    def path(self, path):
        return f'/api/v1/workspaces/{self.workspace}{path}'

    def call(self, base, path, *args, **kwargs):
        return call(base, self.path(path), *args, **kwargs)

