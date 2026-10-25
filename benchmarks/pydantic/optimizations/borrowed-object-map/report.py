import csv,gzip,hashlib,json,statistics
from pathlib import Path
p=Path(__file__).resolve().parent
r=json.loads((p/'results.json').read_text())
engines=['baseline','previous','candidate','buffered','pydantic']
out=(p/'intro.md').read_text()+'\n## Complete canonical results\n\n'
out+=f"UTC: {r['started']} to {r['finished']}. Load start/end: {r['load_start']} / {r['load_end']}. Raw JSON SHA256: `{hashlib.sha256((p/'results.json').read_bytes()).hexdigest()}`.\n\n"
out+='Executable hashes: `'+str(r['binary_sha256'])+'`. Python: `'+r['python_executable_sha256']+'`.\n\n'
out+='All values ns/op: median (inclusive IQR). Direct is the semantics-preserving single-map candidate; buffered is an experimental alternative.\n\n| Case | Mode | Original | Three fixes | Direct | Buffered | Pydantic | Three/direct |\n|---|---|---:|---:|---:|---:|---:|---:|\n'
keys=list(dict.fromkeys((x['case'],x['mode']) for x in r['measurements']))
stats={}
for case,mode in keys:
 stats[case,mode]={}
 for e in engines:
  samples=[v/x['result']['iterations'] for x in r['measurements'] if (x['case'],x['mode'],x['engine'])==(case,mode,e) for v in x['result']['samples_ns']]
  assert len(samples)==6
  q=statistics.quantiles(samples,n=4,method='inclusive');stats[case,mode][e]=(statistics.median(samples),q[2]-q[0])
 st=stats[case,mode]
 out+=f'| {case} | {mode} | '+' | '.join(f'{st[e][0]:.1f} ({st[e][1]:.1f})' for e in engines)+f" | {st['previous'][0]/st['candidate'][0]:.3f} |\n"
out+='\n## Canonical interpretation\n\n'
for name in ['flat_valid','nested_valid','collection_100']:
 st=stats[name,'json'];out+=f"- {name}: direct incremental reduction {(1-st['candidate'][0]/st['previous'][0])*100:.1f}%; direct original-to-four-fix reduction {(1-st['candidate'][0]/st['baseline'][0])*100:.1f}%; direct/Pydantic median ratio {st['candidate'][0]/st['pydantic'][0]:.3f}.\n"
slower=[f'{c}/{m}' for c,m in keys if stats[c,m]['candidate'][0]>stats[c,m]['previous'][0]]
out+='\nRows with slower direct medians than three fixes (not hidden): '+', '.join(slower)+'.\n'
out+='\nThese are same-session direct comparisons, not products of percentages from older sessions. IQR, workload semantics and shared-host caveats prevent a universal ranking or no-regression claim.\n'
for file,title in [('object-timings.tsv','Isolated object materialization'),('wide-validation-timings.tsv','Wide typed-map parse and decode'),('duplicate-validation-timings.tsv','Duplicate-key typed-map parse and decode')]:
 out+='\n## '+title+'\n\nMedian (inclusive IQR), ns/op; 10 batch samples per cell.\n\n| Entries | Reverse | Old | Direct | Buffered |\n|---|---|---:|---:|---:|\n'
 rows=list(csv.DictReader((p/file).open(),delimiter='\t'))
 for width in [3,12,32,128,1024]:
  for reverse in ['false','true']:
   vals=[]
   for mode in range(3):
    a=[float(x['ns_per_op']) for x in rows if x['width']==str(width) and x['reverse']==reverse and x['mode']==str(mode)];assert len(a)==10
    q=statistics.quantiles(a,n=4,method='inclusive');vals.append(f'{statistics.median(a):.1f} ({q[2]-q[0]:.1f})')
   out+=f'| {width} | {reverse} | '+' | '.join(vals)+' |\n'
(p/'README.md').write_text(out)
(p/'results.json.gz').write_bytes(gzip.compress((p/'results.json').read_bytes(),mtime=0))
print('canonical_rows',len(keys),'measurements',len(r['measurements']))
for name in ['flat_valid','nested_valid','collection_100']:print(name,stats[name,'json'])
