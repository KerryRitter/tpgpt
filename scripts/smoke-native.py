#!/usr/bin/env python3
"""Exercise the actual release executable over stdio with an empty temporary DB."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
help_result = subprocess.run([binary, '--help'], capture_output=True, text=True, timeout=30, check=True)
assert 'TPGPT' in help_result.stdout
with tempfile.TemporaryDirectory(prefix='tpgpt-smoke-') as directory:
    database = Path(directory) / 'empty.sqlite'
    database.touch()  # An existing empty SQLite file is migrated by the native server.
    requests = [
        {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'release-smoke', 'version': '1'}}},
        {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
        {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'},
        {'jsonrpc': '2.0', 'id': 3, 'method': 'tools/call', 'params': {'name': 'get_database_overview', 'arguments': {}}},
    ]
    result = subprocess.run([binary, '--mcp', '--database', str(database)],
                            input=''.join(json.dumps(r) + '\n' for r in requests), capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, result.stderr
    replies = {r['id']: r for r in map(json.loads, result.stdout.splitlines()) if 'id' in r}
    assert replies[1]['result']['serverInfo']['name'] == 'tpgpt', replies[1]
    tools = replies[2]['result']['tools']
    assert len(tools) >= 13 and any(t['name'] == 'get_database_overview' for t in tools)
    assert 'error' not in replies[3] and not replies[3]['result'].get('isError'), replies[3]
print('Release executable: help, MCP initialization, tool inventory, and empty-database query passed.')
