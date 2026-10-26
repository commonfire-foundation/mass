#!/usr/bin/env python3
"""Repeatable synthetic Python/Rust comparison, never reads real agent stores."""
import argparse
import json
import math
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--sessions', type=int, default=200)
parser.add_argument('--runs', type=int, default=20)
args = parser.parse_args()
if args.sessions < 1 or args.runs < 1:
    parser.error('counts must be positive')
(ROOT/'target').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(dir=ROOT/'target', prefix='bench-') as tmp:
    root = Path(tmp)
    store = root/'sessions'
    store.mkdir()
    for i in range(args.sessions):
        records = [{'type':'session','id':str(i),'version':3,'cwd':str(root)}]
        for n in range(40):
            records.append({'type':'message','id':str(n),'message':{'role':'user' if n%2==0 else 'assistant','content':f'We examined the callback migration. Session {i} turn {n}. '+('Ordinary background context. '*40)}})
        (store/f'{i}.jsonl').write_text('\n'.join(map(json.dumps,records))+'\n')
    (root/'config.toml').write_text(f'[[sources]]\nid="bench"\nagent="pi"\nroot={json.dumps(str(store))}\n')
    (root/'sources.json').write_text(json.dumps([{'agent':'pi','home':str(root),'resumable':False}]))
    results = {'fixture_sessions':args.sessions, 'messages_per_session':40, 'scope':'synthetic only; timings include process startup; first query is not an OS cold-cache measurement'}
    def timed(argv):
        start=time.perf_counter()
        run=subprocess.run(argv,capture_output=True,check=True)
        return (time.perf_counter()-start)*1000,run.stdout
    for label,prefix,index_cmd,search_cmd in [
        ('rust',[str(ROOT/'target/release/mass'),'--config',str(root/'config.toml'),'--db',str(root/'rust.sqlite3')],['index'],['search','callback','--json']),
        ('python',[sys.executable,str(ROOT/'backend.py'),'--sources',str(root/'sources.json'),'--db',str(root/'python.sqlite3')],['index'],['search','--query','callback','--limit','20']),
    ]:
        initial,_=timed(prefix+index_cmd)
        incremental,_=timed(prefix+index_cmd)
        first,output=timed(prefix+search_cmd)
        samples=[timed(prefix+search_cmd)[0] for _ in range(args.runs)]
        if label=='rust':
            hits=json.loads(output)['results']; assert len(hits)==min(20,args.sessions)
            assert all('callback' in h['excerpt'].lower() for h in hits)
        else:
            assert len(output.splitlines())==min(20,args.sessions)
        results[label]={'initial_index_ms':round(initial,2),'unchanged_index_ms':round(incremental,2),'first_query_ms':round(first,2),'warm_query_p50_ms':round(statistics.median(samples),2),'warm_query_p95_ms':round(sorted(samples)[math.ceil(.95*len(samples))-1],2),'index_bytes':(root/f'{label}.sqlite3').stat().st_size}
    print(json.dumps(results,indent=2))
