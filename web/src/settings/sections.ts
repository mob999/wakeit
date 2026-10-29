import { t as tr } from '../i18n'
import { type User } from '../api/types'

export const settingSections = (role: User['role'], demo = false) => [
  {
    id: 'workspace',
    name: tr('工作区'),
    terms: 'workspace 时区 日志 保留 创建 退出 timezone retention create leave',
  },
  ...(!demo && role === 'admin'
    ? [
        {
          id: 'members',
          name: tr('成员'),
          terms: 'members admin operator viewer 管理员 读写 只读 邀请 角色',
        },
      ]
    : []),
  ...(!demo ? [{ id: 'tokens', name: tr('个人 API Key'), terms: 'key token api 凭据 cli' }] : []),
  ...(!demo && role !== 'viewer'
    ? [{ id: 'archived', name: tr('归档任务'), terms: 'archive archived task 归档 恢复' }]
    : []),
]
