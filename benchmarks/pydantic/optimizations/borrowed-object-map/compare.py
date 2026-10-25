import argparse,datetime,hashlib,json,os,platform,statistics,subprocess,sys
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--root',type=Path,required=True)
parser.add_argument('--baseline-root',type=Path,required=True)
parser.add_argument('--previous-root',type=Path,required=True)
parser.add_argument('--previous',type=Path,required=True)
parser.add_argument('--baseline',type=Path,required=True)
parser.add_argument('--candidate',type=Path,required=True)
parser.add_argument('--buffered',type=Path,required=True)
parser.add_argument('--buffered-root',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--pydantic-python',type=Path,required=True)
args=parser.parse_args()
root=args.root
fixtures=json.loads((root/'benchmarks/pydantic/fixtures.json').read_text())
bins={'baseline':args.baseline,'previous':args.previous,'candidate':args.candidate,'buffered':args.buffered}
sys.path.insert(0,str(root/'benchmarks/pydantic'))
from run import verify,verify_engine,source_snapshot,normalized_errors
commands={k:[str(v)] for k,v in bins.items()}
commands['pydantic']=[str(args.pydantic_python),str(root/'benchmarks/pydantic/python_worker.py')]
def snapshot_root(source_root):
 paths=set(source_root.glob('crates/**/*.rs')) | set(source_root.glob('crates/**/Cargo.toml'))
 paths |= {source_root/'Cargo.toml',source_root/'Cargo.lock'}
 paths |= set((source_root/'benchmarks/pydantic').glob('*.py')) | {source_root/'benchmarks/pydantic/fixtures.json',source_root/'benchmarks/pydantic/requirements.txt'}
 return {str(p.relative_to(source_root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths)}
python_executable=args.pydantic_python.resolve(strict=True)
result={'rustc':subprocess.check_output(['rustc','-Vv'],text=True),'cargo':subprocess.check_output(['cargo','-V'],text=True),'rustflags':os.environ.get('RUSTFLAGS',''),'protocol':{'iterations':10000,'warmup':1000,'repeats_per_position':3,'engine_order':['baseline','previous','candidate','buffered','pydantic','pydantic','buffered','candidate','previous','baseline'],'notes':'original / three-fix / direct-map / buffered-map / pinned Pydantic; serial; six samples per engine/case/mode; no baseline subtraction'},'python_executable':str(python_executable),'python_executable_sha256':hashlib.sha256(python_executable.read_bytes()).hexdigest(),'commands':commands,'buffered_source_snapshot':snapshot_root(args.buffered_root),'baseline_source_snapshot':snapshot_root(args.baseline_root),'previous_source_snapshot':snapshot_root(args.previous_root),'source_snapshot':source_snapshot(),'started':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'load_start':os.getloadavg(),'binary_sha256':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in bins.items()},'measurements':[]}
for case in fixtures['cases']:
 for mode in case['modes']:
  request=dict(schema=case['schema'],input=case['input'],mode=mode,iterations=10000,warmup=1000,repeats=3)
  reference=None
  for engine in ['baseline','previous','candidate','buffered','pydantic','pydantic','buffered','candidate','previous','baseline']:
   r=subprocess.run(commands[engine],input=json.dumps(request),capture_output=True,text=True,check=True)
   out=json.loads(r.stdout)
   verify(case,out,request)
   verify_engine('pydantic' if engine=='pydantic' else 'rydantic',out)
   signature={k:out[k] for k in ['accepted','output']}
   signature['errors']=normalized_errors(out['errors'])
   if reference is None:reference=signature
   assert signature==reference,(case['name'],engine,signature,reference)
   result['measurements'].append(dict(case=case['name'],mode=mode,engine=engine,result=out))
assert snapshot_root(args.buffered_root) == result['buffered_source_snapshot'], 'buffered sources changed during timing'
assert snapshot_root(args.previous_root) == result['previous_source_snapshot'], 'previous sources changed during timing'
assert snapshot_root(args.baseline_root) == result['baseline_source_snapshot'], 'baseline sources changed during timing'
assert source_snapshot() == result['source_snapshot'], 'sources changed during timing'
assert {k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in bins.items()} == result['binary_sha256'], 'binaries changed during timing'
assert hashlib.sha256(python_executable.read_bytes()).hexdigest() == result['python_executable_sha256'], 'Python executable changed during timing'
result.update(finished=datetime.datetime.now(datetime.timezone.utc).isoformat(),load_end=os.getloadavg())
args.output.write_text(json.dumps(result,indent=2)+'\n')
for name in ['flat_valid','nested_valid','collection_100']:
 samples={k:[v/10000 for r in result['measurements'] if r['case']==name and r['mode']=='json' and r['engine']==k for v in r['result']['samples_ns']] for k in commands}
 print(name,{k:round(statistics.median(v),1) for k,v in samples.items()})
print('rows',len(result['measurements']))
