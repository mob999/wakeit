import { ErrorToast } from '../components/Toast'
import { t as tr, roleName } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../api/client'
import { type User } from '../api/types'
import { Modal, Button, Field } from '../components/ui'
import { LanguageSwitch } from '../components/LanguageSwitch'
type Request = {
  gateway: string
  state: string
  workspace: string
  workspace_id: string
  role: User['role']
  intent: { name: string; os: string; register_runner: boolean; allowed_roots: string[] }
}
export function DeviceAuthorization({ user }: { user: User }) {
  useTranslation()
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
  const denied = request?.intent.register_runner && request.role === 'viewer'
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
      setDone(allow ? tr('已授权，可以返回终端') : tr('已拒绝授权'))
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
        title={tr('连接 Wakeit CLI')}
        actions={<LanguageSwitch />}
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
                  <Field label={tr('验证码')}>
                    <input
                      autoComplete="off"
                      aria-label={tr('验证码')}
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
                    {tr('继续')}
                  </Button>
                </form>
              )}
              {query.isFetching && <p role="status">{tr('正在读取授权请求…')}</p>}
              {query.error && <ErrorToast error={query.error.message} />}
              {request && (
                <>
                  <dl className="authorization-fields">
                    <div>
                      <dt>{tr('工作区')}</dt>
                      <dd>{request.workspace}</dd>
                    </div>
                    <div>
                      <dt>Gateway</dt>
                      <dd>{request.gateway}</dd>
                    </div>
                    <div>
                      <dt>{tr('账号')}</dt>
                      <dd>
                        {user.username} <small>{roleName(request.role)}</small>
                      </dd>
                    </div>
                    <div>
                      <dt>{tr('验证码')}</dt>
                      <dd className="authorization-code">{submitted}</dd>
                    </div>
                    <div>
                      <dt>{tr('设备')}</dt>
                      <dd>
                        {request.intent.name} <small>{request.intent.os}</small>
                      </dd>
                    </div>
                    <div>
                      <dt>{tr('注册 Runner')}</dt>
                      <dd>{request.intent.register_runner ? tr('是') : tr('否')}</dd>
                    </div>
                    {request.intent.register_runner && (
                      <div>
                        <dt>{tr('执行目录')}</dt>
                        <dd>
                          {request.intent.allowed_roots.map((p) => (
                            <code key={p}>{p}</code>
                          ))}
                        </dd>
                      </div>
                    )}
                  </dl>
                  {denied && <ErrorToast error={tr('注册 Runner 需要 Operator 或 Admin 权限。')} />}
                  {request.state !== 'pending' && (
                    <p role="status">
                      {(
                        {
                          approved: tr('已批准，请返回终端'),
                          denied: tr('已拒绝授权'),
                          expired: tr('授权请求已过期'),
                          consumed: tr('授权已完成'),
                        } as Record<string, string>
                      )[request.state] ?? request.state}
                    </p>
                  )}
                </>
              )}
              {error && <ErrorToast error={error} />}
            </>
          )}
        </div>
        <div className="modal-footer">
          {done || request?.state !== 'pending' ? (
            <Button onClick={() => location.assign('/')}>{tr('关闭')}</Button>
          ) : (
            <>
              <Button disabled={busy} onClick={() => decide(false)}>
                {tr('取消')}
              </Button>
              <Button
                variant="solid"
                loading={busy}
                disabled={!!denied || !request}
                onClick={() => decide(true)}
              >
                {tr('授权')}
              </Button>
            </>
          )}
        </div>
      </Modal>
    </div>
  )
}
