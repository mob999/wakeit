"""Result-scoped file reads on a disposable Runner; all paths and bodies are synthetic."""
import base64
import json
import os
from pathlib import Path
from integration import expect_status, wait


def verify(api, a, b, check, context):
    root=Path(a['cwd']);outside=root.parent/'outside-preview';outside.mkdir(exist_ok=True)
    (root/'report.md').write_text('# Current file\n\n[Nested](./other.md)')
    (root/'你好 file.md').write_text('Unicode file')
    (root/'large.txt').write_text('x'*700000)
    (outside/'private.txt').write_text('PRIVATE_FIXTURE')
    png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a5X8AAAAASUVORK5CYII=')
    (root/'pixel.png').write_bytes(png)
    links=['./report.md','./%E4%BD%A0%E5%A5%BD%20file.md','./large.txt','./pixel.png','../outside-preview/private.txt','file://remote/private.txt']
    if os.name!='nt':
        os.symlink(outside/'private.txt',root/'escape')
        os.mkfifo(root/'pipe')
        links+=['./escape','./pipe']
    report='\n\n'.join('[Artifact '+str(i)+']('+link+')' for i,link in enumerate(links))+'\n\n`[Not a link](./secret)`'
    c=api('/tasks/same-task')['config'];c.update(id='file-preview',name='File preview fixture',prompt=json.dumps(dict(text=report)))
    api('/tasks/file-preview','PUT',dict(config=c,expected_revision=None))
    id=api('/tasks/file-preview/runs','POST',{})['id']
    final=wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'file source finishes')
    def read(href,**kwargs):return api('/runs/'+id+'/files/preview','POST',dict(href=href),**kwargs)
    value=read('./report.md',gateway=context['other'])
    assert value['format']=='markdown' and value['content'].startswith('# Current file') and value['runner_id']==a['id'],value
    assert read(links[1])['content']=='Unicode file'
    assert read('./large.txt')['truncated'] and len(read('./large.txt')['content'])<=512*1024
    assert base64.b64decode(read('./pixel.png')['content'])==png
    for href in ['../outside-preview/private.txt','file://remote/private.txt','./secret','./other.md']+(['./escape','./pipe'] if os.name!='nt' else []):
        expect_status(lambda href=href:read(href),400)
    expect_status(lambda:read('./report.md',workspace=b['workspace'],token=b['token']),404)
    expect_status(lambda:api('/runs/'+b['run']+'/files/preview','POST',dict(href='./report.md'),workspace=b['workspace'],token=a['token']),403)
    (root/'report.md').write_text('# Changed current file')
    c['cwd']=b['cwd'];task=api('/tasks/file-preview')
    api('/tasks/file-preview','PUT',dict(config=c,expected_revision=task['revision']))
    assert read('./report.md')['content']=='# Changed current file'
    assert api('/runs/'+id)['result']==final['result']
    check('File preview uses persisted exact links and original execution cwd, returns current bounded content and leaves results unchanged')
    check('File preview rejects forged/nested links, other workspaces, Viewer, remote hosts, outside roots, symlink escape and special files')
