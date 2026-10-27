import { useState } from 'react'
export function AgentLogo({ name, logo }: { name: string; logo?: string | null }) {
  const [failed, setFailed] = useState<string | null>(null)
  const source =
    logo &&
    ((logo.startsWith('/') && !logo.startsWith('//') && !logo.includes('\\')) ||
      logo.startsWith('https://'))
      ? logo
      : null
  return (
    <span className="agent-logo" aria-hidden="true">
      {source && failed !== source ? (
        <img
          src={source}
          alt=""
          draggable={false}
          referrerPolicy="no-referrer"
          onError={() => setFailed(source)}
        />
      ) : (
        <span>{Array.from(name.trim())[0]?.toLocaleUpperCase() ?? '?'}</span>
      )}
    </span>
  )
}
