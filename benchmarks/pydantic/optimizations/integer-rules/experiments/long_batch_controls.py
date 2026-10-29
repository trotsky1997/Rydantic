#!/usr/bin/env python3
"""Longer-batch confirmation for observed syntax/missing-field regressions, not a replacement benchmark."""
import argparse, datetime, hashlib, json, os, platform, random, statistics, subprocess, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from common import canonical_harness, digest, load_json, snapshot
p=argparse.ArgumentParser(description=__doc__)
for engine in ['baseline','control','candidate']:p.add_argument('--'+engine+'-build',type=Path,required=True)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args()
if a.output.exists():raise ValueError('refusing to overwrite evidence')
engines=['baseline','control','candidate'];builds={e:load_json(getattr(a,e+'_build')) for e in engines}
for e,b in builds.items():
 assert snapshot(b['root'])==b['source_snapshot'],e
 assert digest(b['executable'])==b['executable_sha256'],e
assert builds['baseline']['source_snapshot']==builds['control']['source_snapshot']
root=Path(builds['candidate']['root']);h=canonical_harness(root)
fixtures=json.loads((root/'benchmarks/pydantic/fixtures.json').read_text());cases={c['name']:c for c in fixtures['cases']}
names=['malformed_json','flat_missing','flat_valid'];orders=[engines+engines[::-1],engines[::-1]+engines]
commands={e:[b['executable']] for e,b in builds.items()}
r={'purpose':'targeted longer-batch diagnostic; all canonical results retained separately','builds':builds,'commands':commands,'python_executable':sys.executable,'python_executable_sha256':digest(sys.executable),'platform':platform.platform(),'source_script_sha256':digest(__file__),'fixtures_sha256':digest(root/'benchmarks/pydantic/fixtures.json'),'started':datetime.datetime.now(datetime.timezone.utc).isoformat(),'load_start':os.getloadavg(),'protocol':{'cases':names,'mode':'json','iterations':250000,'warmup':10000,'repeats':5,'rounds':2,'engine_orders':orders,'case_shuffle_seed':125},'measurements':[]}
for rd,order in enumerate(orders):
 jobs=names.copy();random.Random(125+rd).shuffle(jobs)
 for name in jobs:
  case=cases[name];req=dict(schema=case['schema'],input=case['input'],mode='json',iterations=250000,warmup=10000,repeats=5)
  for position,engine in enumerate(order):
   result=json.loads(subprocess.run(commands[engine],input=json.dumps(req),text=True,capture_output=True,check=True,timeout=600).stdout)
   h.verify(case,result,req);h.verify_engine('rydantic',result)
   r['measurements'].append(dict(round=rd,position=position,case=name,engine=engine,request=req,result=result))
for e,b in builds.items():
 assert snapshot(b['root'])==b['source_snapshot'],e
 assert digest(b['executable'])==b['executable_sha256'],e
r.update(finished=datetime.datetime.now(datetime.timezone.utc).isoformat(),load_end=os.getloadavg())
a.output.write_text(json.dumps(r,indent=2)+'\n')
for name in names:
 vals={e:statistics.median([v/x['result']['iterations'] for x in r['measurements'] if(x['case'],x['engine'])==(name,e) for v in x['result']['samples_ns']]) for e in engines}
 print(name,vals,'candidate_change_percent',100*(vals['candidate']/vals['baseline']-1),flush=True)
