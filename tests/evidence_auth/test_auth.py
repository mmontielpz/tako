"""Disposable generated Ed25519 keys only; no production credentials or services."""
import copy
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout
from io import StringIO
from types import SimpleNamespace
from tools.evidence_auth import auth

NOW = 1500
ROOT = Path(__file__).resolve().parents[2]

class Fixture:
    def __init__(self, root):
        self.root = root
        self.private = root/'private'; self.private.mkdir(mode=0o700)
        self.policy_path = self.private/'trust.json'; self.state_path = self.private/'replay.sqlite'
        self.records = json.loads((ROOT/'docs/contracts/backend-evidence/v1/conformance-example.json').read_bytes())
        self.key_paths, keys = {}, []
        for role,issuer in [('snapshot','fixture-producer'),('execution','fixture-runner'),
                            ('verification','fixture-verifier'),('isolation','fixture-attestor')]:
            path=self.private/(role+'.key'); self.key_paths[role]=path
            key=auth.generate_key(path,role+'-key',issuer,role)
            keys.append({**key,'project_ids':['fixture-project'],'not_before_ms':0,'not_after_ms':10000,'revoked':False})
        self.policy={'schema_version':auth.TRUST_VERSION,'policy_id':'fixture-policy','state_id':'fixture-state',
                     'generation':1,'backend_id':'fixture-backend','keys':keys}
        self.save_policy()
        auth.init_state(self.state_path,self.policy_path)
        self.make_documents()
    def save_policy(self):
        if self.policy_path.exists(): self.policy_path.write_bytes(auth.canonical(self.policy))
        else: auth.write_new(self.policy_path,auth.canonical(self.policy))
    def make_documents(self):
        self.docs={}
        for ref in self.records['artifacts']:
            kind=ref['kind']
            claim=({'kind':kind,'record':self.records[kind]} if kind in {'execution','verification'} else
                   {'kind':kind,'level':'l1_bubblewrap','policy_sha256':'e'*64,'runtime_id':'fixture-runtime',
                    'filesystem_restricted':True,'network_namespace':True,'process_namespace':True})
            doc={'schema_version':'tako.backend-artifact.v1','identity':self.records['identity'],
                 'revision':self.records['revision'],'execution_id':self.records['execution']['execution_id'],
                 'attempt':self.records['execution']['attempt'],'subject_sha256':self.records['execution']['subject_sha256'],
                 'issuer_id':ref['issuer_id'],'claim':claim}
            self.docs[ref['artifact_id']]=doc
            ref['sha256']=auth.sha(auth.canonical(doc))
    def bundle(self, sequence=1):
        snapshot=auth.seal(auth.canonical(self.records),self.key_paths['snapshot'],sequence,1000,2000)
        artifacts={name:auth.seal(auth.canonical(doc),self.key_paths[doc['claim']['kind']],sequence,1000,2000)
                   for name,doc in self.docs.items()}
        return snapshot,artifacts
    def verify(self, bundle=None, now=NOW, expected=None):
        snapshot,artifacts=self.bundle() if bundle is None else bundle
        return auth.verify_bundle(snapshot,artifacts,self.policy_path,self.state_path,
                                  self.records['identity'] if expected is None else expected,now)
    def sequence(self):
        with closing(sqlite3.connect(self.state_path)) as c:
            return c.execute('SELECT sequence FROM seen').fetchall()

class AuthTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.fx=Fixture(Path(self.tmp.name))
    def tearDown(self): self.tmp.cleanup()
    def test_independently_configured_ed25519_keys_authenticate_complete_bundle(self):
        result=self.fx.verify()
        self.assertEqual(result['authenticity'],'authenticated')
        self.assertEqual(result['verification'],'authenticated_claims_only')
        self.assertEqual(result['isolation'],'unverified')
        for flag in ['remote_acceptance_enabled','provider_execution_enabled','l1_badge_enabled']:
            self.assertFalse(result[flag])
        self.assertEqual(self.fx.sequence(),[(1,)])
    def test_unsigned_noncanonical_duplicate_and_embedded_key_envelopes_fail(self):
        snap,art=self.fx.bundle()
        malformed=[auth.canonical(self.fx.records),snap+b'\n',b'{"key_id":"a","key_id":"b"}']
        value=auth.parse(snap); value['public_key_hex']=self.fx.policy['keys'][0]['public_key_hex']; malformed.append(auth.canonical(value))
        for blob in malformed:
            with self.assertRaises(auth.AuthError): self.fx.verify((blob,art))
        self.assertEqual(self.fx.sequence(),[])
    def test_modified_signature_content_subject_artifact_digest_and_sequence_fail(self):
        snap,art=self.fx.bundle()
        for pointer in ['signature','subject','content','digest','sequence','key_id']:
            value=auth.parse(snap)
            if pointer=='signature': value['signature_hex']='00'*64
            elif pointer=='subject': value['payload']['subject_sha256']='f'*64
            elif pointer=='content': value['payload']['content']['lifecycle']='failed'
            elif pointer=='digest': value['payload']['content']['artifacts'][0]['sha256']='f'*64
            elif pointer=='sequence': value['payload']['sequence']=2
            else: value['key_id']='unknown-key'
            with self.assertRaises(auth.AuthError,msg=pointer): self.fx.verify((auth.canonical(value),art))
        self.assertEqual(self.fx.sequence(),[])
    def test_mission_project_backend_and_attempt_mismatches_fail(self):
        for field in ['mission_id','project_id','backend_id']:
            expected={**self.fx.records['identity'],field:'other'}
            with self.assertRaises(auth.AuthError): self.fx.verify(expected=expected)
        snap,art=self.fx.bundle()
        doc=copy.deepcopy(self.fx.docs['fixture-checks']); doc['attempt']=2
        art['fixture-checks']=auth.seal(auth.canonical(doc),self.fx.key_paths['verification'],1,1000,2000)
        with self.assertRaisesRegex(auth.AuthError,'cross_attempt'): self.fx.verify((snap,art))
        self.assertEqual(self.fx.sequence(),[])
    def test_expired_future_revoked_or_wrong_scope_credentials_fail(self):
        for now in [999,2000,10001]:
            with self.assertRaises(auth.AuthError): self.fx.verify(now=now)
        for field,value in [('revoked',True),('not_after_ms',1400),('not_before_ms',1600),
                            ('role','execution'),('project_ids',['other-project'])]:
            original=copy.deepcopy(self.fx.policy)
            self.fx.policy['keys'][0][field]=value
            self.fx.policy['generation']+=1; self.fx.save_policy()
            with self.assertRaises(auth.AuthError): self.fx.verify()
            self.fx.policy=original; self.fx.policy['generation']+=10; self.fx.save_policy()
        self.assertEqual(self.fx.sequence(),[])
    def test_revocation_applies_to_previously_issued_bundles_and_is_persistent(self):
        bundle=self.fx.bundle()
        self.fx.policy['keys'][0]['revoked']=True; self.fx.policy['generation']=2; self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'revoked'): self.fx.verify(bundle)
        self.fx.policy['generation']=1; self.fx.policy['keys'][0]['revoked']=False; self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'trust_policy_rollback'): self.fx.verify(bundle)
    def test_key_rotation_does_not_reset_sequence_and_old_key_revoked(self):
        old=self.fx.bundle(); self.fx.verify(old)
        newpath=self.fx.private/'snapshot-new.key'
        newkey=auth.generate_key(newpath,'snapshot-key-2','fixture-producer','snapshot')
        self.fx.policy['keys'][0]['revoked']=True
        self.fx.policy['keys'].append({**newkey,'project_ids':['fixture-project'],'not_before_ms':0,'not_after_ms':10000,'revoked':False})
        self.fx.policy['generation']=2; self.fx.save_policy(); self.fx.key_paths['snapshot']=newpath
        with self.assertRaisesRegex(auth.AuthError,'revoked'): self.fx.verify(old)
        with self.assertRaisesRegex(auth.AuthError,'replayed'): self.fx.verify(self.fx.bundle(1))
        self.assertEqual(self.fx.verify(self.fx.bundle(2))['sequence'],2)
    def test_repeat_and_lower_sequence_fail_after_process_restart(self):
        bundle=self.fx.bundle(3); self.fx.verify(bundle)
        # Fresh interpreter and SQLite connection; no in-memory replay cache can help.
        snap=self.fx.root/'snapshot'; snap.write_bytes(bundle[0])
        args_file=self.fx.root/'artifacts.json'; args_file.write_text(json.dumps({k:v.hex() for k,v in bundle[1].items()}))
        code='''import json,sys
from pathlib import Path
from tools.evidence_auth import auth
try:
 auth.verify_bundle(Path(sys.argv[1]).read_bytes(),{k:bytes.fromhex(v) for k,v in json.loads(Path(sys.argv[2]).read_text()).items()},sys.argv[3],sys.argv[4],json.loads(sys.argv[5]),1500)
except auth.AuthError as e:
 print(str(e));sys.exit(2)
'''
        proc=subprocess.run([sys.executable,'-c',code,str(snap),str(args_file),str(self.fx.policy_path),str(self.fx.state_path),json.dumps(self.fx.records['identity'])],cwd=ROOT,capture_output=True,text=True)
        self.assertEqual(proc.returncode,2,proc.stderr); self.assertIn('replayed',proc.stdout)
        with self.assertRaisesRegex(auth.AuthError,'replayed'): self.fx.verify(self.fx.bundle(2))
    def test_concurrent_replay_has_exactly_one_winner(self):
        bundle=self.fx.bundle()
        def attempt(_):
            try: self.fx.verify(bundle); return True
            except auth.AuthError: return False
        with ThreadPoolExecutor(max_workers=2) as pool: results=list(pool.map(attempt,range(2)))
        self.assertEqual(sorted(results),[False,True]); self.assertEqual(self.fx.sequence(),[(1,)])
    def test_missing_corrupt_or_wrong_replay_state_never_bootstraps(self):
        original=self.fx.state_path.read_bytes()
        self.fx.state_path.unlink()
        with self.assertRaises(auth.AuthError): self.fx.verify()
        self.assertFalse(self.fx.state_path.exists())
        auth.write_new(self.fx.state_path,b'corrupt')
        with self.assertRaises(auth.AuthError): self.fx.verify()
        self.fx.state_path.write_bytes(original)
        self.fx.policy['state_id']='other-state'; self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'wrong_replay_state'): self.fx.verify()
    def test_failed_bundles_do_not_consume_sequence_or_allow_revision_substitution(self):
        snap,art=self.fx.bundle(10); changed=dict(art); changed.pop('fixture-checks')
        with self.assertRaises(auth.AuthError): self.fx.verify((snap,changed))
        self.assertEqual(self.fx.sequence(),[]); self.fx.verify((snap,art))
        self.fx.records['goal']='modified signed content under same immutable revision'
        with self.assertRaisesRegex(auth.AuthError,'unexpected_or_missing_fields'): self.fx.bundle(11)
        self.fx.records.pop('goal'); self.fx.records['lifecycle']='failed'
        # A new legitimate signature still cannot substitute the immutable revision.
        with self.assertRaisesRegex(auth.AuthError,'immutable_revision'): self.fx.verify(self.fx.bundle(11))
        self.assertEqual(self.fx.sequence(),[(10,)])
    def test_same_key_material_or_issuer_cannot_fake_role_independence(self):
        self.fx.policy['keys'][2]['public_key_hex']=self.fx.policy['keys'][1]['public_key_hex']
        self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'nonindependent'): self.fx.verify()
    def test_policy_change_without_generation_and_clock_rollback_fail(self):
        self.fx.verify(self.fx.bundle())
        with self.assertRaisesRegex(auth.AuthError,'clock_rollback'): self.fx.verify(self.fx.bundle(2),now=1499)
        self.fx.policy['keys'][0]['revoked']=True; self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'without_generation'): self.fx.verify(self.fx.bundle(2))
    def test_unsafe_trust_or_private_key_files_and_reinit_fail(self):
        self.fx.policy_path.chmod(0o666)
        with self.assertRaisesRegex(auth.AuthError,'permissions'): self.fx.verify()
        self.fx.policy_path.chmod(0o600)
        with self.assertRaises(auth.AuthError): auth.init_state(self.fx.state_path,self.fx.policy_path)
        self.fx.key_paths['snapshot'].chmod(0o644)
        with self.assertRaisesRegex(auth.AuthError,'permissions'): self.fx.bundle()
    def test_valid_signature_from_unenrolled_public_key_is_rejected(self):
        attacker=self.fx.private/'unenrolled.key'
        auth.generate_key(attacker,'snapshot-key','fixture-producer','snapshot')
        snap,art=self.fx.bundle()
        forged=auth.seal(auth.canonical(self.fx.records),attacker,1,1000,2000)
        with self.assertRaisesRegex(auth.AuthError,'invalid_signature'): self.fx.verify((forged,art))
        self.assertEqual(self.fx.sequence(),[])

    def test_commit_failure_returns_no_receipt_and_rolls_back_consumption(self):
        original=auth._connect; calls=[]
        class FailedCommit:
            def __init__(self,conn): self.conn=conn
            def execute(self,*args): return self.conn.execute(*args)
            def commit(self): raise sqlite3.OperationalError('fixture durability failure')
            def close(self): self.conn.close()
        def connect(path):
            conn=original(path); calls.append(1)
            return FailedCommit(conn) if len(calls)==2 else conn
        with patch.object(auth,'_connect',side_effect=connect):
            with self.assertRaisesRegex(auth.AuthError,'state_unavailable'): self.fx.verify()
        self.assertEqual(self.fx.sequence(),[])
        self.assertEqual(self.fx.verify()['sequence'],1)

    def test_offline_cli_verifies_and_rejects_replay_and_co_located_anchor(self):
        from tools.evidence_auth import __main__ as cli
        bundle_dir=self.fx.root/'bundle'; bundle_dir.mkdir(mode=0o700)
        snapshot,artifacts=self.fx.bundle()
        (bundle_dir/'snapshot.envelope.json').write_bytes(snapshot)
        for ref in self.fx.records['artifacts']:
            (bundle_dir/(ref['sha256']+'.envelope.json')).write_bytes(artifacts[ref['artifact_id']])
        args=['verify','--bundle-dir',str(bundle_dir),'--trust',str(self.fx.policy_path),'--state',str(self.fx.state_path),
              '--backend-id','fixture-backend','--project-id','fixture-project','--mission-id','fixture-mission']
        with patch.object(cli,'time',SimpleNamespace(time=lambda:1.5)):
            out=StringIO()
            with redirect_stdout(out): self.assertEqual(cli.main(args),0)
            self.assertEqual(json.loads(out.getvalue())['authenticity'],'authenticated')
            out=StringIO()
            with redirect_stdout(out): self.assertEqual(cli.main(args),2)
            self.assertIn('replayed',out.getvalue())
            embedded=bundle_dir/'trust.json'; embedded.write_bytes(self.fx.policy_path.read_bytes()); embedded.chmod(0o600)
            args[args.index('--trust')+1]=str(embedded)
            out=StringIO()
            with redirect_stdout(out): self.assertEqual(cli.main(args),2)
            self.assertIn('trust_or_state_inside_export',out.getvalue())

    def test_symlink_or_writable_ancestor_cannot_hide_a_trust_anchor(self):
        unsafe=self.fx.root/'unsafe'; unsafe.mkdir(mode=0o777); unsafe.chmod(0o777)
        nested=unsafe/'nested'; nested.mkdir(mode=0o700)
        anchor=nested/'trust.json'; anchor.write_bytes(self.fx.policy_path.read_bytes()); anchor.chmod(0o600)
        with self.assertRaisesRegex(auth.AuthError,'unsafe_private_directory'): auth.load_policy(anchor)
        link=self.fx.root/'link'; link.symlink_to(self.fx.private,target_is_directory=True)
        with self.assertRaisesRegex(auth.AuthError,'unsafe_private_directory'): auth.load_policy(link/'trust.json')

    def test_canonical_profile_vectors_and_forbidden_values(self):
        self.assertEqual(auth.canonical({'z':1,'a':'é'}),b'{"a":"\\u00e9","z":1}')
        self.assertEqual(auth.canonical({'a':'/\n"\\','b':True,'c':None}),b'{"a":"/\\n\\"\\\\","b":true,"c":null}')
        for value in [1.0,float('nan'),2**53,'\ud800',{'é':'key'}]:
            with self.assertRaises(auth.AuthError): auth.canonical(value)


class ProductionReadinessQualificationTests(unittest.TestCase):
    """Explicit production-gate probes, not a claim that offline crypto is ready.

    Expected failures identify missing external rollback protection. These tests
    must become ordinary passing security tests before production admission.
    Only disposable fixture policy/state files are restored or reinitialized.
    """
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.fx=Fixture(Path(self.tmp.name))

    @unittest.expectedFailure
    def test_restored_replay_database_must_not_accept_a_consumed_bundle(self):
        checkpoint=self.fx.state_path.read_bytes()
        bundle=self.fx.bundle()
        self.fx.verify(bundle)
        self.fx.state_path.write_bytes(checkpoint)
        with self.assertRaises(auth.AuthError):
            self.fx.verify(bundle)

    @unittest.expectedFailure
    def test_restored_policy_and_database_must_not_resurrect_a_revoked_key(self):
        old_policy=self.fx.policy_path.read_bytes()
        old_state=self.fx.state_path.read_bytes()
        bundle=self.fx.bundle()
        self.fx.policy['generation']=2
        self.fx.policy['keys'][0]['revoked']=True
        self.fx.save_policy()
        with self.assertRaisesRegex(auth.AuthError,'revoked'):
            self.fx.verify(bundle)
        self.fx.policy_path.write_bytes(old_policy)
        self.fx.state_path.write_bytes(old_state)
        with self.assertRaises(auth.AuthError):
            self.fx.verify(bundle)

    @unittest.expectedFailure
    def test_reinitializing_lost_state_requires_an_external_historical_floor(self):
        bundle=self.fx.bundle()
        self.fx.verify(bundle)
        self.fx.state_path.unlink()
        auth.init_state(self.fx.state_path,self.fx.policy_path)
        with self.assertRaises(auth.AuthError):
            self.fx.verify(bundle)

    def test_separately_issued_sequence_collision_is_refused_not_reallocated(self):
        first=self.fx.bundle(1)
        self.fx.records['revision']='fixture-revision-2'
        self.fx.make_documents()
        second=self.fx.bundle(1)
        self.fx.verify(first)
        with self.assertRaisesRegex(auth.AuthError,'replayed'):
            self.fx.verify(second)
        # Consumer rejection is not a producer allocation or retry protocol.
        self.assertEqual(self.fx.sequence(),[(1,)])

    def test_signed_self_report_is_only_authenticated_claims_never_verified_outcome(self):
        # All role signatures are genuine, but the record contradicts itself:
        # a claimed pass with a failed check. This belongs at the full v1 semantic
        # validator; the offline authenticity receipt must not become that verdict.
        self.fx.records['verification']['checks'][0]['status']='failed'
        self.fx.make_documents()
        result=self.fx.verify()
        self.assertEqual(result['authenticity'],'authenticated')
        self.assertEqual(result['verification'],'authenticated_claims_only')
        self.assertEqual(result['isolation'],'unverified')
        self.assertFalse(result['remote_acceptance_enabled'])
        self.assertFalse(result['provider_execution_enabled'])
        self.assertFalse(result['l1_badge_enabled'])

    def test_revoked_signer_can_still_issue_offline_but_verifier_refuses_it(self):
        self.fx.policy['generation']=2
        self.fx.policy['keys'][0]['revoked']=True
        self.fx.save_policy()
        issued_after_revocation=self.fx.bundle()
        with self.assertRaisesRegex(auth.AuthError,'revoked'):
            self.fx.verify(issued_after_revocation)

if __name__=='__main__': unittest.main()
