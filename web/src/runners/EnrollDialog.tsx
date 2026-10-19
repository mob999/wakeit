import { ErrorToast } from '../Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { api } from '../api'
import { CopyValue } from '../runs/CopyValue'
import { Button, Field, Modal } from '../ui'
export function EnrollDialog({
  onClose,
  toast,
}: {
  onClose: () => void
  toast: (message: string) => void
}) {
  useTranslation()
  const [name, setName] = useState('dev-server')
  const [data, setData] = useState<{ token: string; gateway: string } | null>(null)
  const [error, setError] = useState('')
  const generate = useMutation({
    mutationFn: () => api<{ token: string; gateway: string }>('/runners/enrollment', 'POST', {}),
    onSuccess: setData,
    onError: (e) => setError(e.message),
  })
  const command = data
    ? `WAKEIT_ENROLL_TOKEN='${data.token}' wakeit --gateway '${data.gateway}' runner enroll --name '${name}' --token-env WAKEIT_ENROLL_TOKEN`
    : ''
  return (
    <Modal
      open
      onOpenChange={(v) => !v && onClose()}
      title={tr('连接 Runner')}
      description={tr('在目标设备安装 Wakeit，然后使用一次性令牌注册。')}
    >
      <div className="modal-body">
        <Field label={tr('设备名称')}>
          <input value={name} onChange={(e) => setName(e.target.value)} pattern="[A-Za-z0-9_-]+" />
        </Field>
        {data ? (
          <>
            <div className="section-heading">
              {tr('在设备上运行')}
              <CopyValue value={command} toast={toast} />
            </div>
            <pre className="code wrap">{command}</pre>
            <small className="quiet">
              {tr(
                '令牌 30 分钟内有效，使用后失效。Windows 可将令牌设置为 WAKEIT_ENROLL_TOKEN 环境变量后运行同一命令。',
              )}
            </small>
            <div className="section-heading">{tr('启动 Runner')}</div>
            <pre className="code">wakeit runner start --config .wakeit/runner/runner.yaml</pre>
            <p className="quiet">
              {tr(
                '先在设备上安装并登录 Agent，并将 /path/to/projects 换成允许执行的项目父目录。可用的 Agent 会自动上报。',
              )}
            </p>
          </>
        ) : (
          <Button
            variant="solid"
            disabled={generate.isPending || !/^[A-Za-z0-9_-]+$/.test(name)}
            onClick={() => generate.mutate()}
          >
            {tr('生成注册命令')}
          </Button>
        )}
        {error && <ErrorToast>{error}</ErrorToast>}
      </div>
      <div className="modal-footer">
        <Button onClick={onClose}>{tr('完成')}</Button>
      </div>
    </Modal>
  )
}
