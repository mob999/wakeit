import { ErrorToast } from '../components/Toast'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { t as tr, roleName } from '../i18n'
import { api } from '../api/client'
import { type AuthMeta, type User } from '../api/types'
import { Button, Field } from '../components/ui'
import { LanguageSwitch } from '../components/LanguageSwitch'
import { BrandMark } from '../components/BrandMark'

function originHost(raw?: string) {
  if (!raw) return undefined
  try {
    return new URL(raw).host
  } catch {
    return undefined
  }
}

export function InvitationPage({ user }: { user?: User }) {
  useTranslation()
  const token = useSyncExternalStore(
    (update) => {
      window.addEventListener('hashchange', update)
      return () => window.removeEventListener('hashchange', update)
    },
    () => location.hash.slice(1),
  )
  const [signup, setSignup] = useState(false)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [gitlabToken, setGitlabToken] = useState('')
  useEffect(() => {
    setPassword('')
    setConfirm('')
    setGitlabToken('')
    setSignup(false)
  }, [token])
  const invitation = useQuery<{ workspace_id: string; workspace: string; role: string }>({
    queryKey: ['invitation', token],
    queryFn: () => api('/invites/inspect', 'POST', { token }),
    retry: false,
  })
  const meta = useQuery<{ auth?: AuthMeta }>({
    queryKey: ['meta'],
    queryFn: () => api('/meta'),
  })
  const mode = meta.data?.auth?.mode
  const github = mode === 'github'
  const gitlab = mode === 'gitlab' ? meta.data?.auth?.gitlab : undefined
  const gitlabHost = originHost(gitlab?.url)
  const accept = useMutation({
    mutationFn: async () => {
      if (user) return api<{ workspace_id: string }>('/invites/accept', 'POST', { token })
      if (mode === 'gitlab') {
        await api('/auth/gitlab/token', 'POST', { token: gitlabToken.trim() })
        return api<{ workspace_id: string }>('/invites/accept', 'POST', { token })
      }
      if (signup) {
        if (password !== confirm) throw new Error(tr('两次输入的密码不一致'))
        const result = await api<{ workspace_id: string }>('/invites/accept', 'POST', {
          token,
          username: username.trim(),
          password,
        })
        await api('/session', 'POST', { username: username.trim(), password })
        return result
      }
      await api('/session', 'POST', { username: username.trim(), password })
      return api<{ workspace_id: string }>('/invites/accept', 'POST', { token })
    },
    onSuccess: (result) => {
      setPassword('')
      setConfirm('')
      setGitlabToken('')
      location.assign(`/w/${result.workspace_id}/`)
    },
  })
  const canSubmit = Boolean(user || (!github && mode !== 'gitlab') || gitlab?.allow_token)
  return (
    <div className="login-wrap">
      <LanguageSwitch className="login-language" />
      <a className="login-brand brand" href="/">
        <BrandMark />
        <span>Wakeit</span>
      </a>
      <form
        className="login-card content-card"
        onSubmit={(event) => {
          event.preventDefault()
          if (!accept.isPending) accept.mutate()
        }}
      >
        <h1 className="card-header">{tr('加入工作区')}</h1>
        <div className="card-body">
          {invitation.isPending ? (
            <p className="quiet text-muted">{tr('加载中…')}</p>
          ) : invitation.error ? (
            <ErrorToast error={invitation.error.message} />
          ) : (
            <>
              <div className="invite-target">
                <strong>{invitation.data.workspace}</strong>
                <span className="quiet text-muted">{roleName(invitation.data.role)}</span>
              </div>
              {user ? (
                <p>{user.username}</p>
              ) : github ? (
                <Button
                  variant="solid"
                  type="button"
                  onClick={() =>
                    location.assign(
                      `/api/v1/auth/github/start?return_to=${encodeURIComponent(location.pathname + location.hash)}`,
                    )
                  }
                >
                  {tr('使用 GitHub 登录')}
                </Button>
              ) : mode === 'gitlab' ? (
                <>
                  {gitlab?.oauth && (
                    <Button
                      variant={gitlab.allow_token ? undefined : 'solid'}
                      type="button"
                      onClick={() =>
                        location.assign(
                          `/api/v1/auth/gitlab/start?return_to=${encodeURIComponent(location.pathname + location.hash)}`,
                        )
                      }
                    >
                      {tr('使用 GitLab 登录')}
                    </Button>
                  )}
                  {gitlab?.allow_token && (
                    <Field
                      label={tr('GitLab 个人访问令牌')}
                      hint={gitlabHost ? `${gitlabHost} · read_user` : 'read_user'}
                    >
                      <input
                        required
                        type="password"
                        autoComplete="off"
                        value={gitlabToken}
                        disabled={accept.isPending}
                        onChange={(event) => setGitlabToken(event.target.value)}
                      />
                    </Field>
                  )}
                </>
              ) : (
                <>
                  <Field label={tr('用户名')}>
                    <input
                      required
                      autoComplete="username"
                      value={username}
                      disabled={accept.isPending}
                      onChange={(event) => setUsername(event.target.value)}
                    />
                  </Field>
                  <Field label={tr('密码')}>
                    <input
                      required
                      type="password"
                      autoComplete={signup ? 'new-password' : 'current-password'}
                      minLength={signup ? 12 : undefined}
                      value={password}
                      disabled={accept.isPending}
                      onChange={(event) => setPassword(event.target.value)}
                    />
                  </Field>
                  {signup && (
                    <Field label={tr('确认密码')}>
                      <input
                        required
                        type="password"
                        minLength={12}
                        autoComplete="new-password"
                        value={confirm}
                        disabled={accept.isPending}
                        onChange={(event) => setConfirm(event.target.value)}
                      />
                    </Field>
                  )}
                  <Button
                    type="button"
                    disabled={accept.isPending}
                    onClick={() => {
                      setSignup(!signup)
                      setPassword('')
                      setConfirm('')
                      accept.reset()
                    }}
                  >
                    {signup ? tr('登录已有账号') : tr('创建账号')}
                  </Button>
                </>
              )}
              {canSubmit && (
                <Button variant="solid" type="submit" disabled={accept.isPending}>
                  {tr('加入工作区')}
                </Button>
              )}
            </>
          )}
          {accept.error && <ErrorToast error={accept.error.message} />}
        </div>
      </form>
    </div>
  )
}
