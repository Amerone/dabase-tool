import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from 'react-router-dom'
import { ConfigProvider, theme } from 'antd'
import { router } from '@/router'
import 'antd/dist/reset.css'

const queryClient = new QueryClient()

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ConfigProvider
        theme={{
          algorithm: theme.darkAlgorithm,
          token: {
            colorPrimary: '#00b96b',
            colorBgContainer: '#050505',
            colorBgElevated: '#141414',
            borderRadius: 0, // Hard edges for tech feel
            fontFamily: 'JetBrains Mono, monospace',
            colorText: '#e0e0e0',
            colorTextSecondary: '#a0a0a0',
          },
          components: {
            Layout: {
              headerBg: 'transparent',
              bodyBg: 'transparent',
              footerBg: 'transparent',
            },
            Card: {
              colorBgContainer: 'rgba(10, 10, 10, 0.6)',
              colorBorderSecondary: '#333',
              boxShadowTertiary: '0 4px 30px rgba(0, 0, 0, 0.5)',
            },
            Button: {
              borderRadius: 2,
              controlHeight: 40,
              defaultBorderColor: '#333',
              defaultBg: 'rgba(255,255,255,0.05)',
              primaryShadow: '0 0 15px rgba(0, 185, 107, 0.4)',
            },
            Input: {
              colorBgContainer: 'rgba(0,0,0,0.3)',
              activeBorderColor: '#00b96b',
              hoverBorderColor: '#00b96b',
              borderRadius: 0,
            },
            Table: {
              colorBgContainer: 'transparent',
              headerBg: 'rgba(255,255,255,0.05)',
              rowHoverBg: 'rgba(0, 185, 107, 0.1)',
            }
          }
        }}
      >
        <RouterProvider router={router} />
      </ConfigProvider>
    </QueryClientProvider>
  )
}

export default App
