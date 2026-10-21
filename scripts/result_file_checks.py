"""Result-scoped file reads on a disposable Runner; all paths and bodies are synthetic."""
import base64
import json
import os
from pathlib import Path
from integration import call, expect_status, wait


def verify(api, a, b, check, context):
    root=Path(a['cwd']);outside=root.parent/'outside-preview';outside.mkdir(exist_ok=True)
    (root/'report.md').write_text('# Current file\n\n[Nested](./other.md)')
    (root/'你好 file.md').write_text('Unicode file')
    (root/'large.txt').write_text('x'*700000)
    (root/'large.md').write_text('![Image](./pixel.png)\n'+'x'*700000)
    (outside/'private.txt').write_text('PRIVATE_FIXTURE')
    png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a5X8AAAAASUVORK5CYII=')
    for name in ['pixel.png','image-only.png']:(root/name).write_bytes(png)
    (root/'docs').mkdir()
    for name in ['pixel.png','你好 图.png']:(root/'docs'/name).write_bytes(png)
    document='![Relative](./pixel.png)\n\n![Encoded](./%E4%BD%A0%E5%A5%BD%20%E5%9B%BE.png)\n\n![Absolute]('+str(root/'pixel.png')+')\n\n![Parent](../pixel.png)\n\n![Nonimage](../report.md)\n\n![Escape](../../outside-preview/private.txt)\n\n`![Hidden](./secret.png)`\n\n```md\n![Hidden](./code.png)\n```'
    (root/'docs/report.md').write_text(document)
    (root/'unlinked.md').write_text('![Image](./pixel.png)')
    links=['./report.md','./%E4%BD%A0%E5%A5%BD%20file.md','./large.txt','./pixel.png','../outside-preview/private.txt','file://remote/private.txt','./docs/report.md','./large.md']
    if os.name!='nt':
        os.symlink(outside/'private.txt',root/'escape')
        os.mkfifo(root/'pipe')
        links+=['./escape','./pipe']
    report='\n\n'.join('[Artifact '+str(i)+']('+link+')' for i,link in enumerate(links))+'\n\n`[Not a link](./secret)`\n\n![Inline](./image-only.png)\n\n![Nonimage](./large.txt)'
    c=api('/tasks/same-task')['config'];c.update(id='file-preview',name='File preview fixture',prompt=json.dumps(dict(text=report)))
    api('/tasks/file-preview','PUT',dict(config=c,expected_revision=None))
    id=api('/tasks/file-preview/runs','POST',{})['id']
    final=wait(lambda:(r if (r:=api('/runs/'+id))['finished_at'] else None),'file source finishes')
    def read(href,document=None,**kwargs):return api('/runs/'+id+'/files/preview','POST',dict(href=href,**({'document':document} if document else {})),**kwargs)
    value=read('./report.md',gateway=context['other'])
    assert value['format']=='markdown' and value['content'].startswith('# Current file') and value['runner_id']==a['id'],value
    assert read(links[1])['content']=='Unicode file'
    assert read('./large.txt')['truncated'] and len(read('./large.txt')['content'])<=512*1024
    assert base64.b64decode(read('./pixel.png')['content'])==png
    assert base64.b64decode(read('./image-only.png')['content'])==png
    value=read('./pixel.png',document='./docs/report.md',gateway=context['other'])
    assert value['format']=='image' and Path(value['path'])==root/'docs/pixel.png' and base64.b64decode(value['content'])==png,value
    assert Path(read('./%E4%BD%A0%E5%A5%BD%20%E5%9B%BE.png',document='./docs/report.md')['path'])==root/'docs/你好 图.png'
    assert Path(read(str(root/'pixel.png'),document='./docs/report.md')['path'])==root/'pixel.png'
    assert Path(read('../pixel.png',document='./docs/report.md')['path'])==root/'pixel.png'
    for href in ['../report.md','../../outside-preview/private.txt','./secret.png','./code.png','./forged.png']:
        expect_status(lambda href=href:read(href,document='./docs/report.md'),400)
    for document in ['./unlinked.md','./large.txt','./large.md']:
        expect_status(lambda document=document:read('./pixel.png',document=document),400)
    for href in ['../outside-preview/private.txt','file://remote/private.txt','./secret','./other.md']+(['./escape','./pipe'] if os.name!='nt' else []):
        expect_status(lambda href=href:read(href),400)
    expect_status(lambda:read('./pixel.png',document='./docs/report.md',workspace=b['workspace'],token=b['token']),404)
    expect_status(lambda:read('./report.md',workspace=b['workspace'],token=b['token']),404)
    expect_status(lambda:api('/runs/'+b['run']+'/files/preview','POST',dict(href='./report.md'),workspace=b['workspace'],token=a['token']),403)
    invite=api('/invites','POST',{'role':'viewer'})['token']
    call(context['other'],'/api/v1/invites/accept','POST',{'token':invite},token=b['token'])
    expect_status(lambda:read('./image-only.png',token=b['token']),403)
    expect_status(lambda:read('./pixel.png',document='./docs/report.md',token=b['token']),403)
    (root/'docs/report.md').write_text('# Reference removed')
    expect_status(lambda:read('./pixel.png',document='./docs/report.md'),400)
    (root/'report.md').write_text('# Changed current file')
    c['cwd']=b['cwd'];task=api('/tasks/file-preview')
    api('/tasks/file-preview','PUT',dict(config=c,expected_revision=task['revision']))
    assert read('./report.md')['content']=='# Changed current file'
    assert api('/runs/'+id)['result']==final['result']
    check('File and inline-image previews use authored references and original execution cwd, including Markdown-relative, absolute and encoded paths')
    check('File preview rejects forged/nested links, incomplete/non-Markdown sources, removed image references, other workspaces, Viewer, remote hosts, outside roots, symlink escape and special files')
    return id
