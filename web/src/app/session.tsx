import { createContext, useContext } from 'react'
import { type User } from '../api/types'

export const UserContext = createContext<User | null>(null)
export const useUser = () => useContext(UserContext)!
