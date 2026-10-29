import { useQuery } from '@tanstack/react-query'
import { api } from '../api/client'
import { type Agent } from '../api/types'

export function useAgents() {
  return useQuery<Agent[]>({
    queryKey: ['agents'],
    queryFn: () => api('/agents'),
    refetchInterval: 3000,
  })
}
