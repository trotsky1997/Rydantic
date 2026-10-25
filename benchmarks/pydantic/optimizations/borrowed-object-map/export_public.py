"""Export a path-redacted public derivative; never overwrite the private original."""
import argparse,copy,gzip,hashlib,json
from pathlib import Path

def export(raw):
    original=json.loads(raw)
    if 'public_export' in original:
        raise ValueError('expected original private results, not a public derivative')
    out=copy.deepcopy(original)
    out['python_executable']='artifact:python-interpreter'
    out['commands']={engine:([f'artifact:{engine}'] if engine!='pydantic' else ['artifact:python-interpreter','benchmarks/pydantic/python_worker.py']) for engine in original['commands']}
    # Full kernel/build identity is unrelated to reproduction of these timings.
    out['platform']='Linux x86_64 shared cloud host (detailed environment identifier redacted)'
    out['public_export']={
        'private_original_sha256':hashlib.sha256(raw).hexdigest(),
        'removed':['absolute local executable and worker paths','detailed operating-system build identifier'],
        'preserved':['all samples and outputs','source-file and executable hashes','compiler versions and flags','timing configuration and order','UTC interval and load averages'],
        'artifact_resolution':'Pass your own built binaries and source roots to compare.py; artifact labels are not executable paths.'}
    return (json.dumps(out,indent=2)+'\n').encode()

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('private_original',type=Path);p.add_argument('public_output',type=Path)
    a=p.parse_args()
    if a.private_original.resolve()==a.public_output.resolve():raise ValueError('refusing to overwrite original')
    result=export(a.private_original.read_bytes())
    a.public_output.write_bytes(gzip.compress(result,mtime=0) if a.public_output.suffix=='.gz' else result)
