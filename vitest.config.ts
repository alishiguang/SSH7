import { defineConfig } from 'vitest/config'

// 单元测试独立于 Vite 的 dev server 配置，避免 `npm test` 与 `npm run dev` 互相干扰。
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    reporters: 'default',
  },
})
