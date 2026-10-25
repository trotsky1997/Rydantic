#!/usr/bin/env python3
"""Select exact registry version or materialized source without editing test source.
Usage: select-dependency.py registry 0.8.0-beta.1 | source /absolute/core/path
"""
import pathlib,sys,re,json
mode, value = sys.argv[1:]
p = pathlib.Path(__file__).parent / 'Cargo.toml'
s = p.read_text()
if mode == 'registry':
    if not re.fullmatch(r'0\.8\.0(?:-beta\.[0-9]+)?',value): raise SystemExit('exact 0.8 version required')
    dep = f'rydantic = {{ version = "={value}", features = ["axum", "sqlite"] }}'
elif mode == 'source':
    dep = f'rydantic = {{ path = {json.dumps(str(pathlib.Path(value).resolve()))}, features = ["axum", "sqlite"] }}'
else: raise SystemExit('source or registry required')
s = re.sub(r'^rydantic = .*$',dep,s,flags=re.M)
# beta.1 has no OpenAPI feature. Keep common app identical and feature disabled.
s = re.sub(r'^openapi-acceptance = .*$', 'openapi-acceptance = []' if mode == 'registry' and value == '0.8.0-beta.1' else 'openapi-acceptance = ["rydantic/openapi"]',s,flags=re.M)
p.write_text(s)
