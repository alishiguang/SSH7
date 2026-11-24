import { create } from 'zustand'

import { hostApi } from '../api/bridge'
import type { SavedHost } from '../api/types'

interface HostsStore {
  hosts: SavedHost[]
  /** 分组清单与显示顺序。可以包含空分组。 */
  groups: string[]
  loaded: boolean
  load: () => Promise<void>
  save: (host: SavedHost) => Promise<void>
  remove: (id: string) => Promise<void>
  /** 新建分组（可以是空分组）。 */
  createGroup: (name: string) => Promise<void>
  /** 重命名分组：组里的主机一起改到新名字。 */
  renameGroup: (oldName: string, newName: string) => Promise<void>
  /** 删除分组：组里的主机回到「未分组」。 */
  removeGroup: (name: string) => Promise<void>
}

export const useHosts = create<HostsStore>((set) => ({
  hosts: [],
  groups: [],
  loaded: false,

  load: async () => {
    const { hosts, groups } = await hostApi.list()
    set({ hosts, groups, loaded: true })
  },

  save: async (host) => {
    const { hosts, groups } = await hostApi.save(host)
    set({ hosts, groups })
  },

  remove: async (id) => {
    const { hosts, groups } = await hostApi.remove(id)
    set({ hosts, groups })
  },

  createGroup: async (name) => {
    const { hosts, groups } = await hostApi.createGroup(name)
    set({ hosts, groups })
  },

  renameGroup: async (oldName, newName) => {
    const { hosts, groups } = await hostApi.renameGroup(oldName, newName)
    set({ hosts, groups })
  },

  removeGroup: async (name) => {
    const { hosts, groups } = await hostApi.removeGroup(name)
    set({ hosts, groups })
  },
}))
