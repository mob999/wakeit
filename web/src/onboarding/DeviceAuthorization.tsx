import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type User } from '../api'
import { Modal, Button, Field } from '../ui'
type Request = {
  gateway: string
  state: string
  intent: { name: string; os: string; register_runner: boolean; allowed_roots: string[] }
}
export function DeviceAuthorization({ user }: { user: User }) {
  const [code, setCode] = useState(new URLSearchParams(location.search).get('user_code') ?? '')
  const [submitted, setSubmitted] = useState(code)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState('')
  const [error, setError] = useState('')
  const query = useQuery<Request>({
    queryKey: ['device-authorization', submitted],
    queryFn: () => api(`/device-authorizations/${encodeURIComponent(submitted)}`),
    enabled: !!submitted,
    retry: false,
    refetchOnWindowFocus: false,
  })
  const request = query.data
  const denied = request?.intent.register_runner && user.role === 'viewer'
  const decide = async (allow: boolean) => {
    if (busy) return
    setBusy(true)
    setError('')
    try {
      await api(
        `/device-authorizations/${encodeURIComponent(submitted)}/${allow ? 'approve' : 'deny'}`,
        'POST',
        {},
      )
      setDone(allow ? '已授权，可以返回终端' : '已拒绝授权')
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="login-wrap">
      <Modal
        open
        onOpenChange={() => {
          if (!busy) location.assign('/')
        }}
        title="连接 Wakeit CLI"
      >
        <div className="modal-body device-authorization">
          {done ? (
            <p role="status">{done}</p>
          ) : (
            <>
              {!submitted && (
                <form
                  onSubmit={(e) => {
                    e.preventDefault()
                    setSubmitted(code.replace(/[ -]/g, '').toUpperCase())
                  }}
                >
                  <Field label="验证码">
                    <input
                      autoComplete="off"
                      aria-label="验证码"
                      maxLength={10}
                      value={code}
                      onChange={(e) => setCode(e.target.value)}
                    />
                  </Field>
                  <Button
                    variant="solid"
                    type="submit"
                    disabled={code.replace(/[ -]/g, '').length !== 8}
                  >
                    继续
                  </Button>
                </form>
              )}
              {query.isFetching && <p role="status">正在读取授权请求…</p>}
              {query.error && <p role="alert">{query.error.message}</p>}
              {request && (
                <>
                  <dl className="authorization-fields">
                    <div>
                      <dt>Gateway</dt>
                      <dd>{request.gateway}</dd>
                    </div>
                    <div>
                      <dt>账号</dt>
                      <dd>
                        {user.username} <small>{user.role}</small>
                      </dd>
                    </div>
                    <div>
                      <dt>验证码</dt>
                      <dd className="authorization-code">{submitted}</dd>
                    </div>
                    <div>
                      <dt>设备</dt>
                      <dd>
                        {request.intent.name} <small>{request.intent.os}</small>
                      </dd>
                    </div>
                    <div>
                      <dt>注册 Runner</dt>
                      <dd>{request.intent.register_runner ? '是' : '否'}</dd>
                    </div>
                    {request.intent.register_runner && (
                      <div>
                        <dt>执行目录</dt>
                        <dd>
                          {request.intent.allowed_roots.map((p) => (
                            <code key={p}>{p}</code>
                          ))}
                        </dd>
                      </div>
                    )}
                  </dl>
                  {denied && <p role="alert">注册 Runner 需要 Operator 或 Admin 权限。</p>}
                  {request.state !== 'pending' && (
                    <p role="status">
                      {(
                        {
                          approved: '已批准，请返回终端',
                          denied: '已拒绝授权',
                          expired: '授权请求已过期',
                          consumed: '授权已完成',
                        } as Record<string, string>
                      )[request.state] ?? request.state}
                    </p>
                  )}
                </>
              )}
              {error && <p role="alert">{error}</p>}
            </>
          )}
        </div>
        <div className="modal-footer">
          {done || request?.state !== 'pending' ? (
            <Button onClick={() => location.assign('/')}>关闭</Button>
          ) : (
            <>
              <Button disabled={busy} onClick={() => decide(false)}>
                取消
              </Button>
              <Button
                variant="solid"
                loading={busy}
                disabled={!!denied || !request}
                onClick={() => decide(true)}
              >
                授权
              </Button>
            </>
          )}
        </div>
      </Modal>
    </div>
  )
}
