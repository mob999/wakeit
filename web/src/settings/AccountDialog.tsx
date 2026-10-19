import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { api, type Member } from '../api'
import { Button, Field, Modal } from '../ui'
export function AccountDialog({
  member,
  self,
  onClose,
  onCreated,
}: {
  member?: Member
  self?: boolean
  onClose: () => void
  onCreated?: () => void
}) {
  useTranslation()
  const client = useQueryClient()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  const busy = useRef(false)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])
  const submit = async () => {
    if (busy.current) return
    setError('')
    if (password !== confirm) {
      setError(tr('两次输入的密码不一致'))
      return
    }
    busy.current = true
    setPending(true)
    try {
      if (member) await api(`/instance/users/${member.id}/password`, 'POST', { password })
      else await api('/instance/users', 'POST', { username: username.trim(), password })
      if (!alive.current) return
      setPassword('')
      setConfirm('')
      if (member && self) {
        client.clear()
        location.assign('/')
        return
      }
      client.invalidateQueries({ queryKey: ['instance-users'] })
      if (!member) onCreated?.()
      onClose()
    } catch (e) {
      if (alive.current) setError((e as Error).message)
    } finally {
      busy.current = false
      if (alive.current) setPending(false)
    }
  }
  return (
    <Modal
      open
      title={member ? tr('重置密码') : tr('新建账号')}
      description={
        member
          ? tr('重置「{{v0}}」的密码后，全部登录和 API Key 将失效。{{v1}}', {
              v0: member.username,
              v1: self ? tr('你将退出登录。') : '',
            })
          : undefined
      }
      onOpenChange={(v) => {
        if (!v && !busy.current) onClose()
      }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <div className="modal-body">
          {!member && (
            <Field label={tr('用户名')}>
              <input
                autoFocus
                required
                value={username}
                disabled={pending}
                autoComplete="off"
                onChange={(e) => setUsername(e.target.value)}
              />
            </Field>
          )}
          <Field label={member ? tr('新密码') : tr('密码')}>
            <input
              type="password"
              autoFocus={!!member}
              required
              value={password}
              disabled={pending}
              autoComplete="new-password"
              onChange={(e) => setPassword(e.target.value)}
            />
          </Field>
          <Field label={tr('确认密码')}>
            <input
              type="password"
              required
              value={confirm}
              disabled={pending}
              autoComplete="new-password"
              onChange={(e) => setConfirm(e.target.value)}
            />
          </Field>
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button type="button" disabled={pending} onClick={onClose}>
            {tr('取消')}
          </Button>
          <Button
            type="submit"
            variant="solid"
            disabled={pending || !password || !confirm || (!member && !username.trim())}
          >
            {pending ? tr('正在保存…') : member ? tr('重置密码') : tr('创建')}
          </Button>
        </div>
      </form>
    </Modal>
  )
}
