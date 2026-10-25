#!/usr/bin/env python3
"""Check documented 0.7 -> 0.8 context migration boundaries, not app adoption."""
import argparse, json, pathlib, subprocess, tempfile
p = argparse.ArgumentParser()
g = p.add_mutually_exclusive_group(required=True)
g.add_argument('--dependency-path', type=pathlib.Path)
g.add_argument('--version')
p.add_argument('--output', type=pathlib.Path, required=True)
a = p.parse_args()
dep = '{ path = ' + json.dumps(str(a.dependency_path.resolve())) + ' }' if a.dependency_path else json.dumps('=' + a.version)
cases = {
    'safe_panic_boundary': (True, None, '''use rydantic::Context; use std::sync::Arc;
fn main() { let result = std::panic::catch_unwind(|| { let context = Context::with_data(Arc::new(42u32)); assert_eq!(*context.require_data::<u32>().unwrap(), 42); }); assert!(result.is_ok()); }'''),
    'context_outside_panic_boundary': (False, 'RefUnwindSafe', '''use rydantic::Context;
fn main() { let context = Context::new(); let _ = std::panic::catch_unwind(|| context.error_count()); }'''),
    'rc_is_not_thread_safe': (False, 'cannot be sent between threads safely', '''use rydantic::Context; use std::{rc::Rc, sync::Arc};
fn main() { let _ = Context::with_data(Arc::new(Rc::new(42u32))); }'''),
    'borrowed_data_is_not_owned': (False, 'does not live long enough', '''use rydantic::Context; use std::sync::Arc;
fn main() { let owned = String::from("synthetic"); let _ = Context::with_data(Arc::new(&owned)); }'''),
}
a.output.mkdir(parents=True, exist_ok=True)
results = []
with tempfile.TemporaryDirectory(prefix='rydantic-migration-') as root:
    root = pathlib.Path(root)
    (root / 'src').mkdir()
    (root / 'Cargo.toml').write_text('[package]\nname="rydantic-context-migration-probe"\nversion="0.0.0"\nedition="2024"\npublish=false\n[workspace]\n[dependencies]\nrydantic=' + dep + '\n')
    for name, (success, marker, code) in cases.items():
        (root / 'src/main.rs').write_text(code)
        command = ['cargo', 'run' if success else 'check', '--manifest-path', str(root / 'Cargo.toml')]
        proc = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (a.output / (name + '.log')).write_text(proc.stdout)
        ok = (proc.returncode == 0) if success else (proc.returncode != 0 and marker in proc.stdout)
        results.append({'case': name, 'expected_compile_success': success, 'required_diagnostic': marker, 'returncode': proc.returncode, 'passed': ok})
        print(name + ': ' + ('PASS' if ok else 'FAIL'), flush=True)
    (a.output / 'Cargo.lock').write_bytes((root / 'Cargo.lock').read_bytes())
(a.output / 'results.json').write_text(json.dumps({'dependency': dep, 'cases': results}, indent=2) + '\n')
raise SystemExit(0 if all(r['passed'] for r in results) else 1)
