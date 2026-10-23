import { useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { copyAction } from './copy'

export function useCopy(onError: (message: string) => void, label = t('复制')) {
  const [copied, setCopied] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      clearTimeout(timer.current)
    }
  }, [])
  async function copy(value: string) {
    if (await copyAction(value, label, onError)) {
      if (!alive.current) return true
      clearTimeout(timer.current)
      setCopied(true)
      timer.current = setTimeout(() => setCopied(false), 1500)
      return true
    }
    return false
  }
  return { copied, copy }
}
