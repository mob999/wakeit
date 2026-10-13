#!/usr/bin/env python3
"""A minimal synchronous JSONL adapter. Replace the sample work with your service."""
import datetime
import json
import sys


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def emit(kind, data):
    print(json.dumps({'version': 1, 'type': kind, 'data': data}, ensure_ascii=False), flush=True)


prompt = sys.stdin.read()
start = now()
node = {'id': 'inspect', 'kind': 'tool', 'status': 'running', 'title': '检查请求',
        'input': {'prompt': prompt}, 'output': None, 'occurred_at': start, 'started_at': start}
emit('trace', node)
emit('log', {'stream': 'stdout', 'text': '正在检查请求\n'})
finish = now()
# Each update replaces the full snapshot, including input and start time.
node.update(status='succeeded', output={'characters': len(prompt)}, occurred_at=finish, finished_at=finish)
emit('trace', node)
emit('result', '# 检查完成\n\n收到请求。')
# Exit nonzero for failure; trace statuses never set the Run lifecycle.
