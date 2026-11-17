"""Normal task-map terminal acceptance, isolated state and loopback provider only."""
import sys,json,time,threading,subprocess,hashlib
from pathlib import Path
from plan_workflow_pty import Provider,Terminal,functions,message_text,tool_result
from orc258_pty import wait

TITLES=['Inspect parser','Independent check','Write documentation','Follow checked input']
BODY='## Shared purpose and constraints\nKeep edits local. Preserve the original goal.\n'+''.join('## Task: '+title+'\n'+detail+'\n' for title,detail in zip(TITLES,[
'Read evidence.txt. Report two progress updates. PARSER_ONLY_CONTEXT',
'Read evidence.txt. Success requires ready.flag. CHECK_ONLY_CONTEXT',
'Write documentation.txt with TASK_MAP_VERIFIED. DOC_ONLY_CONTEXT',
'Read evidence.txt after Independent check passes. FOLLOW_ONLY_CONTEXT']))

def proposal():
 tasks=[]
 for title in TITLES:
  task={'task_id':title,'tools':['read_file'],'paths':['.'],'evidence':[{'kind':'closed_command','argv':['/usr/bin/grep','-q','REPOSITORY_EVIDENCE','evidence.txt']}], 'commands':['/usr/bin/grep -q REPOSITORY_EVIDENCE evidence.txt'],'max_attempts':2}
  if title==TITLES[1]:
   task['evidence']=[{'kind':'closed_command','argv':['/bin/test','-f','ready.flag']}]
   task['commands']=['/bin/test -f ready.flag']
  if title==TITLES[2]:
   task.update(depends_on=[TITLES[0]],tools=['write_file'],paths=['documentation.txt'],commands=[])
   task['evidence']=[{'kind':'artifact','path':'documentation.txt','artifact_kind':'regular_file','min_bytes':1,'must_change':True}]
  if title==TITLES[3]: task['depends_on']=[TITLES[1]]
  tasks.append(task)
 return {'title':'Automatic task map qualification','body':BODY,'tasks':TITLES,'graph':{'budget':{'allow_unpriced':True,'max_physical_requests':30},'tasks':tasks}}

class MapProvider(Provider):
 def __init__(self):
  self.second=threading.Event();self.finish=threading.Event();self.authors=0
  super().__init__()
 def hold(self,event):
  self.lock.release()
  try: assert event.wait(45),'fixture progress hold timed out'
  finally:self.lock.acquire()
 def response(self,lane,payload):
  assert lane=='orchestrator',lane
  assert payload.get('reasoning_effort')=='xhigh','selected reasoning was lost on a planner or graph worker request'
  names,text=functions(payload),message_text(payload)
  assert 'PRIVATE_CANARY' not in text,'active background filter leaked fixture canary'
  if 'report_structured_output' in names:
   assert not {'write_file','edit_file','run_command','spawn_agent'}.intersection(names)
   assert 'SKILL_CONTEXT_CANARY' in text,'requested skill was lost during planning/revision'
   assert 'REPOSITORY_EVIDENCE' in text,'@file was not resolved before planning'
   self.authors+=1
   value=proposal()
   if self.authors==1:value['graph']['tasks'][0]['produces']=[{'name':'invalid-handoff','path':'evidence.txt','kind':'regular_file'}]
   if self.authors==2:assert 'Task map preflight failed' in text,'execution policy was not checked before handoff'
   return [('map','report_structured_output',{'value':value})],''
  if '## Task: '+TITLES[0] in text:
   assert 'DOC_ONLY_CONTEXT' not in text and 'CHECK_ONLY_CONTEXT' not in text
   if tool_result(payload,'parser-first') is None:
    return [('parser-first','read_file',{'path':'evidence.txt'})],'FIRST-PENDING'
   if tool_result(payload,'parser-second') is None:
    self.hold(self.second)
    return [('parser-second','read_file',{'path':'evidence.txt'})],'SECOND-PENDING'
   self.hold(self.finish)
   return [],'Parser inspection finished with real file evidence.'
  if '## Task: '+TITLES[1] in text:
   if tool_result(payload,'check-read') is None:return [('check-read','read_file',{'path':'evidence.txt'})],''
   return [],'Independent check worker finished; host must verify ready.flag.'
  if '## Task: '+TITLES[2] in text:
   if tool_result(payload,'write-doc') is None:return [('write-doc','write_file',{'path':'documentation.txt','content':'TASK_MAP_VERIFIED\n'})],''
   return [],'Documentation written and ready for host verification.'
  if '## Task: '+TITLES[3] in text:
   if tool_result(payload,'follow-read') is None:return [('follow-read','read_file',{'path':'evidence.txt'})],''
   return [],'Follow-up completed after verified prerequisite.'
  raise AssertionError('Unexpected request without a scoped task')

def seed_start(original):
 def launch(args,**kwargs):
  args=list(args)
  if '--new-session-id' in args:
   idx=args.index('--new-session-id');sid=args[idx+1];args[idx]='--session'
   runtime=Path(kwargs['env']['HOME']);workspace=Path(kwargs['cwd'])
   directory=runtime/'.local/share/ca/sessions';directory.mkdir(parents=True,exist_ok=True)
   stamp='2026-11-08T12:00:00Z'
   messages=[{'role':'User' if n%2==0 else 'Assistant','content':f'History {n}: representative retained previous work with a long explanatory sentence.','tool_calls':None,'thinking':None,'timestamp':stamp} for n in range(192)]
   (directory/(sid+'.json')).write_text(json.dumps({'version':2,'id':sid,'title':'Task map history fixture','workspace':str(workspace),'created_at':stamp,'updated_at':stamp,'provider':'orchestrator','model':'workflow-orchestrator','reasoning_effort':'xhigh','parent':None,'messages':messages,'context_checkpoint':None}))
   config=runtime/'.config/ca/config.toml'
   config.write_text(config.read_text()+"\n[provider_filter]\nenabled=true\n[provider_filter.hide]\nwords=['PRIVATE_CANARY']\n[agent]\nreview_changes=false\nproactive_delegation=false\n")
   config.write_text(config.read_text()+"\n[model_profiles.\"orchestrator/workflow-orchestrator\"]\nreasoning_efforts=['xhigh']\ndefault_reasoning_effort='xhigh'\nreasoning_request_format='reasoning_effort'\n")
   skill=workspace/'.ca/skills/map-fixture';skill.mkdir(parents=True)
   (skill/'SKILL.md').write_text('---\nname: map-fixture\ndescription: Local task map acceptance fixture\n---\nKeep this workflow local. SKILL_CONTEXT_CANARY\n')
   (workspace/'evidence.txt').write_text('REPOSITORY_EVIDENCE PRIVATE_CANARY\n')
  return original(args,**kwargs)
 return launch

def main():
 binary,output=Path(sys.argv[1]).resolve(),Path(sys.argv[2]).resolve();output.mkdir(parents=True,exist_ok=False)
 provider=MapProvider();terminal=None;result={'status':'failed','binary':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
 original=subprocess.Popen
 try:
  subprocess.Popen=seed_start(original)
  terminal=Terminal(binary,output,provider)
  subprocess.Popen=original
  terminal.command('Do this task: inspect @evidence.txt. Perform the independent check, write documentation, and follow the checked input. $map-fixture')
  def reviewed():
   assert not provider.errors,provider.errors
   record=terminal.saved()
   workflow=record[1].get('work_state',{}).get('plan_workflow') if record else None
   assert not workflow or workflow.get('status')!='failed',workflow
   return record if record and record[1].get('work_state',{}).get('plan_workflow',{}).get('status')=='completed' else None
  wait(reviewed,terminal,timeout=50);terminal.wait_text('Approve and start',10)
  assert len(provider.requests)==2,'invalid graph was not corrected once before review, or execution happened before approval'
  assert any('SKILL_CONTEXT_CANARY' in message_text(entry['body']) for entry in provider.requests),'explicit skill was dropped'
  terminal.command('Revise the task map: keep the original constraints and all four tasks, with clear completion checks.')
  wait(lambda: reviewed() and provider.authors==3,terminal,timeout=40)
  terminal.wait_text('Approve and start',10)
  assert len(provider.requests)==3,'revision started execution before approval'
  (output/'review-wide.txt').write_text(terminal.screen.text())
  terminal.resize(70,36);terminal.settle(.2);(output/'review-narrow.txt').write_text(terminal.screen.text())
  terminal.write(b'\r')
  terminal.wait_text('FIRST-PENDING',45)
  wait(lambda: terminal.saved()[1]['work_state']['plan']['tasks'][1]['agent_runtime']['state']=='failed',terminal,timeout=25)
  first=terminal.screen.text();assert 'TO PROVIDER' not in first and 'Review filters' not in first
  assert 'Failed' in first,'worker completion hid failed host verification'
  (output/'first-pending.txt').write_text(first)
  work=terminal.saved()[1]['work_state'];assert work['agent_graph']['status']=='running'
  assert work['plan']['tasks'][3]['agent_runtime']['state']=='dependency_blocked'
  provider.second.set();terminal.wait_text('SECOND-PENDING',25)
  second=terminal.screen.text();assert 'FIRST-PENDING' not in second
  (output/'second-pending.txt').write_text(second)
  terminal.resize(120,36);terminal.settle(.2);(output/'second-pending-wide.txt').write_text(terminal.screen.text())
  terminal.write(b'\x18t');terminal.settle(.2);assert 'SECOND-PENDING' not in terminal.screen.text()
  terminal.write(b'\x18t');terminal.wait_text('SECOND-PENDING',5)
  provider.finish.set()
  wait(lambda:(terminal.workspace/'documentation.txt').exists(),terminal,timeout=30)
  wait(lambda:terminal.saved()[1]['work_state']['agent_graph']['status']=='paused',terminal,timeout=30)
  assert (terminal.workspace/'documentation.txt').read_text()=='TASK_MAP_VERIFIED\n'
  (output/'partial-completion.txt').write_text(terminal.screen.text())
  # The same verifier passes only after the missing external input is supplied.
  (terminal.workspace/'ready.flag').write_text('ready\n')
  terminal.command('/tasks');terminal.write(b'\x1b[B');terminal.write(b'R');terminal.settle(.2)
  terminal.write(b'\x1b[C\r');terminal.settle(.3);terminal.write(b'\x1b')
  wait(lambda:terminal.saved()[1]['work_state']['agent_graph']['status']=='completed',terminal,timeout=45)
  terminal.wait_text('Completed:',10)
  path,record=terminal.saved();envelope=json.loads(path.read_text())
  assert envelope['message_count']>=194,'long canonical history was lost'
  assert len(record['work_state']['plan']['tasks'][1]['agent_runtime']['attempts'])==2
  (output/'completed.txt').write_text(terminal.screen.text())
  (output/'saved-work.json').write_text(json.dumps(record['work_state'],indent=2))
  result.update(status='passed',requests=len(provider.requests),history_messages=envelope['message_count'],two_pending_updates=True,parallel_failure_isolation=True,explicit_retry=True,filter_closed=True,background_filter=True,semantic_graph_repair=True)
 finally:
  subprocess.Popen=original;provider.second.set();provider.finish.set()
  if terminal:
   terminal.close();result['terminal_restored']=terminal.restored_termios;(output/'terminal.raw').write_bytes(terminal.raw)
  (output/'requests.json').write_text(json.dumps(provider.requests,indent=2));provider.close()
  (output/'receipt.json').write_text(json.dumps(result,indent=2))
 print(json.dumps(result))
if __name__=='__main__':main()
