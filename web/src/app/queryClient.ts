import { QueryClient } from '@tanstack/react-query'
import { ApiError, workspaceId } from '../api/client'

export const client = new QueryClient({
  defaultOptions: {
    queries: {
      queryKeyHashFn: (key) => JSON.stringify([workspaceId ?? 'account', key]),
      retry: (n, e) => !(e instanceof ApiError && [401, 403].includes(e.status)) && n < 2,
      refetchOnWindowFocus: true,
    },
  },
})
