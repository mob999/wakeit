import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { api, type Member } from '../api'
import { Button, Field, Modal } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
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
  const client = useQueryClient()
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [role, setRole] = useState('operator')
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
      setError('两次输入的密码不一致')
      return
    }
    busy.current = true
    setPending(true)
    try {
      if (member) await api(`/users/${member.id}/password`, 'POST', { password })
      else await api('/users', 'POST', { username: username.trim(), password, role })
      if (!alive.current) return
      setPassword('')
      setConfirm('')
      if (member && self) {
        client.clear()
        location.assign('/')
        return
      }
      client.invalidateQueries({ queryKey: ['members'] })
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
      title={member ? '重置密码' : '新建账号'}
      description={
        member
          ? `重置「${member.username}」的密码后，全部登录和 API Key 将失效。${self ? '你将退出登录。' : ''}`
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
            <Field label="用户名">
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
          <Field label={member ? '新密码' : '密码'}>
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
          <Field label="确认密码">
            <input
              type="password"
              required
              value={confirm}
              disabled={pending}
              autoComplete="new-password"
              onChange={(e) => setConfirm(e.target.value)}
            />
          </Field>
          {!member && (
            <Field label="角色">
              <ChoicePicker
                label="角色"
                value={role}
                disabled={pending}
                onChange={setRole}
                options={['admin', 'operator', 'viewer'].map((value) => ({
                  value,
                  label: value[0].toUpperCase() + value.slice(1),
                }))}
              />
            </Field>
          )}
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button type="button" disabled={pending} onClick={onClose}>
            取消
          </Button>
          <Button
            type="submit"
            variant="solid"
            disabled={pending || !password || !confirm || (!member && !username.trim())}
          >
            {pending ? '正在保存…' : member ? '重置密码' : '创建'}
          </Button>
        </div>
      </form>
    </Modal>
  )
}
