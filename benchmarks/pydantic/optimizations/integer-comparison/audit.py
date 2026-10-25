import json,hashlib,pathlib,sys,subprocess,gzip
p=pathlib.Path('/workspace/shared/rydantic-integer-evidence'); root=pathlib.Path('/workspace/shared/rydantic-integer')
r=json.loads((p/'results.json').read_text());prov=json.loads((p/'provenance.json').read_text()); readme=(p/'README.md').read_text()
def sha(b):return hashlib.sha256(b).hexdigest()
def quant(v,f):
 v=sorted(v);pos=(len(v)-1)*f;i=int(pos);return v[i]+(v[min(i+1,len(v)-1)]-v[i])*(pos-i)
assert gzip.decompress((p/'results.json.gz').read_bytes())==(p/'results.json').read_bytes()
assert sha((p/'results.json').read_bytes()) in readme
assert r['source_snapshot']==prov['source_snapshot']
for path,expected in r['source_snapshot'].items():
 assert sha((root/path).read_bytes())==expected,path
 assert sha(subprocess.check_output(['git','show',f"{prov['candidate_local_commit']}:{path}"],cwd=root))==expected,path
for e,h in r['binary_sha256'].items():
 assert sha((p/e).read_bytes())==h
 rows=[json.loads(x) for x in (p/f'build-{e}.jsonl').read_text().splitlines()]
 artifacts=[x for x in rows if x.get('reason')=='compiler-artifact' and x['target']['name']=='pydantic_benchmark']
 assert len(artifacts)==1
 a=artifacts[0]; assert a['profile']['opt_level']=='3' and not a['profile']['debug_assertions'] and not a['fresh']
 assert sha(pathlib.Path(a['executable']).read_bytes())==h
assert r['binary_sha256']['baseline']!=r['binary_sha256']['candidate']
assert sha((p/'preflight-shared-target-candidate').read_bytes())==r['binary_sha256']['baseline']
sys.path.insert(0,str(root/'benchmarks/pydantic'));from run import verify,verify_engine
fixtures=json.loads((root/'benchmarks/pydantic/fixtures.json').read_text())['cases'];byname={x['name']:x for x in fixtures}
keys=[(x['name'],m) for x in fixtures for m in x['modes']]
assert len(r['measurements'])==len(keys)*6==294
for i,(case,mode) in enumerate(keys):
 group=r['measurements'][i*6:(i+1)*6]
 assert [x['engine'] for x in group]==['baseline','candidate','pydantic','pydantic','candidate','baseline']
 cells=[];med=[]
 for x in group:
  assert (x['case'],x['mode'])==(case,mode)
  req=dict(schema=byname[case]['schema'],input=byname[case]['input'],mode=mode,iterations=10000,warmup=1000,repeats=3)
  verify(byname[case],x['result'],req);verify_engine('pydantic' if x['engine']=='pydantic' else 'rydantic',x['result'])
 for engine in ['baseline','candidate','pydantic']:
  vals=[v/x['result']['iterations'] for x in group if x['engine']==engine for v in x['result']['samples_ns']]
  assert len(vals)==6
  m=quant(vals,.5);iqr=quant(vals,.75)-quant(vals,.25);med.append(m);cells.append(f'{m:.1f} ({iqr:.1f})')
 row=f'| {case} | {mode} | '+' | '.join(cells)+f' | {med[0]/med[1]:.3f} |'
 assert row in readme,row
print(f"PASS: all {len(keys)} rows manually recomputed; 294 oracle results; {len(r['source_snapshot'])} source hashes match committed candidate; binary hashes and separate optimized non-fresh build artifacts match; stale preflight distinct/excluded; gzip lossless.")
