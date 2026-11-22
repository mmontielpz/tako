"""Explicit offline commands only; never invoked automatically by Desktop/adapter."""
import argparse
import json
from pathlib import Path
import sys
import time
from . import auth


def load_bundle(directory):
    root = Path(directory).absolute()
    snapshot = auth.read_local(root/'snapshot.envelope.json')
    env = auth.parse(snapshot, canonical_only=True)
    artifacts = {}
    total = len(snapshot)
    for ref in env['payload']['content']['artifacts']:
        auth.hex_bytes(ref['sha256'],32)
        auth.require(auth.token(ref['artifact_id']) and ref['artifact_id'] not in artifacts,'invalid_artifact_reference')
        raw = auth.read_local(root/(ref['sha256']+'.envelope.json'))
        total += len(raw)
        auth.require(total <= auth.LIMIT, 'bundle_size_limit')
        artifacts[ref['artifact_id']] = raw
    return snapshot,artifacts


def main(argv=None):
    parser=argparse.ArgumentParser(description='Offline signed evidence tooling; does not grant execution or acceptance')
    commands=parser.add_subparsers(dest='command',required=True)
    keygen=commands.add_parser('keygen')
    for name in ['key-file','key-id','issuer-id']: keygen.add_argument('--'+name,required=True)
    keygen.add_argument('--role',choices=sorted(auth.ROLES),required=True)
    init=commands.add_parser('init-state')
    init.add_argument('--trust',required=True); init.add_argument('--state',required=True)
    seal=commands.add_parser('seal')
    for name in ['content','key-file','output']: seal.add_argument('--'+name,required=True)
    for name in ['sequence','issued-at-ms','expires-at-ms']: seal.add_argument('--'+name,type=int,required=True)
    verify=commands.add_parser('verify')
    for name in ['bundle-dir','trust','state','backend-id','project-id','mission-id']: verify.add_argument('--'+name,required=True)
    args=parser.parse_args(argv)
    try:
        if args.command=='keygen':
            result=auth.generate_key(args.key_file,args.key_id,args.issuer_id,args.role)
        elif args.command=='init-state':
            auth.init_state(args.state,args.trust); result={'replay_state':'initialized'}
        elif args.command=='seal':
            blob=auth.seal(auth.read_local(args.content),args.key_file,args.sequence,args.issued_at_ms,args.expires_at_ms)
            auth.write_new(args.output,blob); result={'envelope':'sealed'}
        else:
            bundle_root=Path(args.bundle_dir).resolve(strict=True)
            for private_path in [args.trust,args.state]:
                auth.require(not Path(private_path).resolve(strict=True).is_relative_to(bundle_root), 'trust_or_state_inside_export')
            snapshot,artifacts=load_bundle(args.bundle_dir)
            expected={'backend_id':args.backend_id,'project_id':args.project_id,'mission_id':args.mission_id}
            result=auth.verify_bundle(snapshot,artifacts,args.trust,args.state,expected,int(time.time()*1000))
        print(json.dumps(result,sort_keys=True))
        return 0
    except auth.AuthError as exc:
        print(json.dumps({'authenticity':'unverified','error':str(exc)}))
        return 2
    except (OSError,KeyError,TypeError,ValueError,RecursionError):
        print(json.dumps({'authenticity':'unverified','error':'input_or_state_unavailable'}))
        return 2

if __name__=='__main__': sys.exit(main())
