import React from 'react'
import ReactDOM from 'react-dom/client'

// 自托管字体：不依赖运行时网络请求。
import '@fontsource/anton'
import '@fontsource/geist-mono/400.css'
import '@fontsource/geist-mono/500.css'

import './styles/tokens.css'
import './styles/global.css'
import './styles/components.css'

import App from './App'

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
