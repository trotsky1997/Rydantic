"""Redact only owned-cloud executable paths; preserve every measurement value."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('source', type=Path)
p.add_argument('destination', type=Path)
p.add_argument('manifest', type=Path)
a = p.parse_args()
raw = a.source.read_bytes()
private = json.loads(raw)
public = json.loads(raw)
public['commands'] = {'baseline': ['<baseline-worker>'], 'candidate': ['<candidate-worker>'],
                      'control': ['<same-source-candidate-control-worker>'],
                      'pydantic': ['<pinned-python>', 'benchmarks/pydantic/python_worker.py']}
assert set(private['commands']) == set(public['commands'])
assert {k: v for k, v in private.items() if k != 'commands'} == {k: v for k, v in public.items() if k != 'commands'}
data = (json.dumps(public, indent=2) + '\n').encode()
for forbidden in [b'/workspace/', b'/home/', b'/Users/', b'C:\\Users\\', b'Bearer ', b'ghp_', b'github_pat_', b'sk-proj-']:
    assert forbidden not in data, forbidden
compressed = gzip.compress(data, mtime=0)
assert gzip.decompress(compressed) == data
a.destination.parent.mkdir(parents=True, exist_ok=True)
a.destination.write_bytes(compressed)
manifest = {'format': 'gzip of UTF-8 JSON; deterministic mtime=0',
            'private_original_sha256': hashlib.sha256(raw).hexdigest(),
            'compressed_sha256': hashlib.sha256(compressed).hexdigest(),
            'decompressed_sha256': hashlib.sha256(data).hexdigest(),
            'compressed_bytes': len(compressed), 'decompressed_bytes': len(data),
            'redactions': ['commands.baseline[0]', 'commands.candidate[0]', 'commands.control[0]', 'commands.pydantic[0]', 'commands.pydantic[1]'],
            'all_other_fields_equal': True, 'top_level_fields': list(public),
            'process_records': len(public.get('measurements', public.get('records', []))),
            'scope': 'Synthetic public repository fixtures and assistant-owned cloud execution only. No user-computer observations or user data. Command paths replaced; no tokens or private environment values requested or recorded.',
            'remaining_metadata': 'Public source/binary hashes, source relative filenames/status, toolchain versions, batch/whole-child measurements and protocol. Main timing runs also contain platform release and load averages; diagnostic contains allowed CPU numbers and process CPU/context-switch receipts. These are cloud execution metadata, not a private user device.'}
a.manifest.write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(manifest, indent=2))
