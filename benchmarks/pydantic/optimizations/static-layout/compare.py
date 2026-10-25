import argparse,datetime,hashlib,json,os,platform,statistics,subprocess,sys
from pathlib import Path
parser=argparse.ArgumentParser()
parser.add_argument('--root',type=Path,required=True)
parser.add_argument('--baseline',type=Path,required=True)
parser.add_argument('--candidate',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--pydantic-python',type=Path,required=True)
args=parser.parse_args()
root=args.root
fixtures=json.loads((root/'benchmarks/pydantic/fixtures.json').read_text())
bins={'baseline':args.baseline,'candidate':args.candidate}
sys.path.insert(0,str(root/'benchmarks/pydantic'))
from run import verify,verify_engine,source_snapshot,normalized_errors
commands={k:[str(v)] for k,v in bins.items()}
commands['pydantic']=[str(args.pydantic_python),str(root/'benchmarks/pydantic/python_worker.py')]
result={'source_snapshot':source_snapshot(),'started':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'load_start':os.getloadavg(),'binary_sha256':{k:hashlib.sha256(v.read_bytes()).hexdigest() for k,v in bins.items()},'measurements':[]}
for case in fixtures['cases']:
 for mode in case['modes']:
  request=dict(schema=case['schema'],input=case['input'],mode=mode,iterations=10000,warmup=1000,repeats=3)
  reference=None
  for engine in ['baseline','candidate','pydantic','pydantic','candidate','baseline']:
   r=subprocess.run(commands[engine],input=json.dumps(request),capture_output=True,text=True,check=True)
   out=json.loads(r.stdout)
   verify(case,out,request)
   verify_engine('pydantic' if engine=='pydantic' else 'rydantic',out)
   signature={k:out[k] for k in ['accepted','output']}
   signature['errors']=normalized_errors(out['errors'])
   if reference is None:reference=signature
   assert signature==reference,(case['name'],engine,signature,reference)
   result['measurements'].append(dict(case=case['name'],mode=mode,engine=engine,result=out))
result.update(finished=datetime.datetime.now(datetime.timezone.utc).isoformat(),load_end=os.getloadavg())
args.output.write_text(json.dumps(result,indent=2)+'\n')
for name in ['flat_valid','nested_valid','collection_100']:
 samples={k:[v/10000 for r in result['measurements'] if r['case']==name and r['mode']=='json' and r['engine']==k for v in r['result']['samples_ns']] for k in commands}
 print(name,{k:round(statistics.median(v),1) for k,v in samples.items()})
print('rows',len(result['measurements']))
