import { ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { Button, Field } from '../components/ui'
import { LanguageSwitch } from '../components/LanguageSwitch'
import { useState } from 'react'
import { useQuery, useMutation } from '@tanstack/react-query'
import { ArrowUpRight } from '../components/icons'
import { api } from '../api/client'
import { useQueryClient } from '@tanstack/react-query'

export function Login() {
  useTranslation()
  const client = useQueryClient()
  const meta = useQuery<{ auth?: { mode: string; allow_registration: boolean } }>({
    queryKey: ['meta'],
    queryFn: () => api('/meta'),
  })
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [signup, setSignup] = useState(false)
  const returnTo = new URLSearchParams(location.search).get('return_to')
  const next = returnTo?.startsWith('/') && !returnTo.startsWith('//') ? returnTo : undefined
  const login = useMutation({
    mutationFn: async () => {
      if (signup) {
        if (password !== confirm) throw new Error(tr('两次输入的密码不一致'))
        await api('/register', 'POST', { username: username.trim(), password })
      }
      await api('/session', 'POST', { username: username.trim(), password })
    },
    onSuccess: () => {
      setPassword('')
      setConfirm('')
      if (next) location.assign(next)
      else client.invalidateQueries({ queryKey: ['me'] })
    },
  })
  return (
    <div className="login-wrap">
      <LanguageSwitch className="login-language" />
      <div className="login-brand">
        <span>Wakeit</span>
      </div>
      <form
        className="login-card content-card"
        onSubmit={(event) => {
          event.preventDefault()
          if (!login.isPending) login.mutate()
        }}
      >
        <h1 className="card-header">{signup ? tr('创建账号') : tr('欢迎回来')}</h1>
        <div className="card-body">
          {new URLSearchParams(location.search).get('auth_error') === 'github_denied' && (
            <ErrorToast error={tr('GitHub 授权已取消')} />
          )}
          {meta.isPending ? (
            <span className="quiet text-muted">{tr('加载中…')}</span>
          ) : meta.error ? (
            <>
              <ErrorToast error={meta.error.message} />
              <Button type="button" onClick={() => void meta.refetch()}>
                {tr('重试')}
              </Button>
            </>
          ) : meta.data?.auth?.mode === 'github' ? (
            <Button
              variant="solid"
              type="button"
              onClick={() =>
                location.assign(
                  `/api/v1/auth/github/start?return_to=${encodeURIComponent(next ?? location.pathname + location.search)}`,
                )
              }
            >
              {tr('使用 GitHub 登录')}
            </Button>
          ) : (
            <>
              <Field label={tr('用户名')}>
                <input
                  aria-label={tr('用户名')}
                  autoComplete="username"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  required
                  disabled={login.isPending}
                />
              </Field>
              <Field label={tr('密码')} hint={signup ? tr('至少 12 个字符') : undefined}>
                <input
                  aria-label={tr('密码')}
                  type="password"
                  autoComplete={signup ? 'new-password' : 'current-password'}
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  required
                  minLength={signup ? 12 : undefined}
                  disabled={login.isPending}
                />
              </Field>
              {signup && (
                <Field label={tr('确认密码')}>
                  <input
                    aria-label={tr('确认密码')}
                    type="password"
                    autoComplete="new-password"
                    value={confirm}
                    onChange={(event) => setConfirm(event.target.value)}
                    required
                    minLength={12}
                    disabled={login.isPending}
                  />
                </Field>
              )}
              {login.error && <ErrorToast error={login.error.message} />}
              <Button variant="solid" type="submit" disabled={login.isPending}>
                {login.isPending ? tr('正在连接…') : signup ? tr('创建账号') : tr('登录工作区')}
                <ArrowUpRight size={14} />
              </Button>
              {meta.data?.auth?.allow_registration && (
                <Button
                  type="button"
                  disabled={login.isPending}
                  onClick={() => {
                    setSignup(!signup)
                    setPassword('')
                    setConfirm('')
                    login.reset()
                  }}
                >
                  {signup ? tr('登录已有账号') : tr('创建账号')}
                </Button>
              )}
            </>
          )}
          {new URLSearchParams(location.search).has('login_error') && (
            <ErrorToast error={tr('GitHub 登录失败，请重试')} />
          )}
        </div>
      </form>
    </div>
  )
}
