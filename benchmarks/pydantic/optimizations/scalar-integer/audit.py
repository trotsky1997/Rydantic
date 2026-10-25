import json,hashlib,pathlib,sys,subprocess,gzip,re
P=pathlib.Path;p=P('/workspace/shared/rydantic-scalar-final-evidence');r=json.loads((p/'results.json').read_text());readme=(p/'README.md').read_text()
def sha(b):return hashlib.sha256(b).hexdigest()
def quant(v,f):
 v=sorted(v);pos=(len(v)-1)*f;i=int(pos);return v[i]+(v[min(i+1,len(v)-1)]-v[i])*(pos-i)
assert gzip.decompress((p/'results.json.gz').read_bytes())==(p/'results.json').read_bytes()
assert sha((p/'results.json').read_bytes()) in readme
roots={'baseline':P('/workspace/shared/rydantic-combined-original'),'previous':P('/workspace/shared/rydantic-integer'),'candidate':P('/workspace/shared/rydantic-scalar')}
commits={'baseline':'862907a2','previous':'b255a8b','candidate':'ca682a51'}
keys={'baseline':'baseline_source_snapshot','previous':'previous_source_snapshot','candidate':'source_snapshot'}
receipts={'baseline':P('/workspace/shared/rydantic-combined-evidence/build-original.jsonl'),'previous':P('/workspace/shared/rydantic-integer-evidence/build-candidate.jsonl'),'candidate':P('/workspace/shared/rydantic-scalar-evidence/build-candidate.jsonl')}
for e,root in roots.items():
 for path,expected in r[keys[e]].items():
  assert sha((root/path).read_bytes())==expected,(e,path)
  assert sha(subprocess.check_output(['git','show',f'{commits[e]}:{path}'],cwd=root))==expected,(e,path)
 rows=[json.loads(x) for x in receipts[e].read_text().splitlines()];a=[x for x in rows if x.get('reason')=='compiler-artifact' and x['target']['name']=='pydantic_benchmark'];assert len(a)==1;a=a[0]
 assert a['profile']['opt_level']=='3' and not a['profile']['debug_assertions'] and not a['fresh']
 assert sha(P(a['executable']).read_bytes())==r['binary_sha256'][e]
 assert sha(P(r['commands'][e][0]).read_bytes())==r['binary_sha256'][e]
assert len(set(r['binary_sha256'].values()))==3
assert sha(P(r['python_executable']).read_bytes())==r['python_executable_sha256']
assert P(r['commands']['pydantic'][0]).resolve()==P(r['python_executable'])
for path in ['Cargo.lock','benchmarks/pydantic/fixtures.json','benchmarks/pydantic/python_worker.py','crates/rydantic/examples/pydantic_benchmark.rs']:
 assert len({r[k][path] for k in keys.values()})==1,path
root=roots['candidate'];sys.path.insert(0,str(root/'benchmarks/pydantic'));from run import verify,verify_engine
fixtures=json.loads((root/'benchmarks/pydantic/fixtures.json').read_text())['cases'];byname={x['name']:x for x in fixtures};cases=[(x['name'],m) for x in fixtures for m in x['modes']]
assert len(r['measurements'])==len(cases)*8==392
stats={}
for i,(case,mode) in enumerate(cases):
 group=r['measurements'][i*8:(i+1)*8]
 assert [x['engine'] for x in group]==['baseline','previous','candidate','pydantic','pydantic','candidate','previous','baseline']
 cells=[];med=[]
 for x in group:
  assert (x['case'],x['mode'])==(case,mode)
  req=dict(schema=byname[case]['schema'],input=byname[case]['input'],mode=mode,iterations=10000,warmup=1000,repeats=3)
  verify(byname[case],x['result'],req);verify_engine('pydantic' if x['engine']=='pydantic' else 'rydantic',x['result'])
 for e in ['baseline','previous','candidate','pydantic']:
  vals=[v/x['result']['iterations'] for x in group if x['engine']==e for v in x['result']['samples_ns']];assert len(vals)==6
  m=quant(vals,.5);iqr=quant(vals,.75)-quant(vals,.25);med.append(m);cells.append(f'{m:.1f} ({iqr:.1f})')
 row=f'| {case} | {mode} | '+' | '.join(cells)+f' | {med[1]/med[2]:.3f} | {med[0]/med[2]:.3f} |';assert row in readme,row
 stats[case,mode]=med
for case in ['flat_valid','nested_valid','collection_100']:
 m=stats[case,'json'];assert f'scalar incremental reduction {100*(1-m[2]/m[1]):.1f}%; direct original-to-three-fix reduction {100*(1-m[2]/m[0]):.1f}%; candidate/Pydantic median ratio {m[2]/m[3]:.3f}' in readme
alloc=json.loads(P('/workspace/shared/rydantic-scalar-evidence/parse-allocations.json').read_text());assert len(alloc)==6
for case in ['flat_valid','nested_valid','collection_100']:
 vals=[]
 for source in ['before','after']:
  a=next(x for x in alloc if (x['source'],x['case'])==(source,case)); assert a['accepted'] and not a['errors'];assert a['output']==byname[case]['output']
  vals.append(tuple(map(int,re.findall(r'\d+',a['alloc']))))
 row=f'| {case} | {vals[0][0]:,} | {vals[1][0]:,} | {vals[0][1]:,} | {vals[1][1]:,} |';assert row in readme,row
print('PASS49rows/two ratios/392oracle results; all3source snapshots match commits; all3optimized artifacts and4executable hashes match; same corpus/lockfile/workers; gzip/hash match; allocation6outputs/counts match tables.')
print('Slower incremental rows:',[(c,m,round(v[1],1),round(v[2],1)) for (c,m),v in stats.items() if v[2]>v[1]])
