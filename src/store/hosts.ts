import { create } from 'zustand'

import { hostApi } from '../api/bridge'
import type { SavedHost } from '../api/types'

interface HostsStore {
  hosts: SavedHost[]
  loaded: boolean
  load: () => Promise<void>
  save: (host: SavedHost) => Promise<void>
  remove: (id: string) => Promise<void>
}

export const useHosts = create<HostsStore>((set) => ({
  hosts: [],
  loaded: false,

  load: async () => {
    const hosts = await hostApi.list()
    set({ hosts, loaded: true })
  },

  save: async (host) => {
    const hosts = await hostApi.save(host)
    set({ hosts })
  },

  remove: async (id) => {
    const hosts = await hostApi.remove(id)
    set({ hosts })
  },
}))
